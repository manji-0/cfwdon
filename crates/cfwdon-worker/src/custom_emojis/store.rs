use super::custom_emoji_to_json;
use super::gif_static::gif_static_bytes;
use crate::{
    Result, generate_entity_id, log_r2_operation, media_object_url, observability_started_at_ms,
};
use cfwdon_core::{AppConfig, CustomEmoji, is_custom_emoji_shortcode};
use serde::Deserialize;
use worker::{Bucket, D1Database, HttpMetadata, d1::D1Type};

const CUSTOM_EMOJI_MAX_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CustomEmojiRow {
    pub(crate) id: String,
    pub(crate) shortcode: String,
    pub(crate) object_key: String,
    pub(crate) static_object_key: String,
    #[allow(dead_code)]
    pub(crate) content_type: String,
    pub(crate) visible_in_picker: i64,
    pub(crate) category: Option<String>,
}

#[derive(Debug)]
pub(crate) struct CustomEmojiUploadDraft {
    pub(crate) shortcode: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: String,
    pub(crate) extension: String,
    pub(crate) static_bytes: Vec<u8>,
    pub(crate) static_content_type: String,
    pub(crate) static_extension: String,
    pub(crate) visible_in_picker: bool,
    pub(crate) category: Option<String>,
}

pub(crate) async fn resolve_custom_emojis(
    db: &D1Database,
    config: &AppConfig,
) -> Result<Vec<CustomEmoji>> {
    let db_emojis = list_custom_emojis_from_db(db, config).await?;
    Ok(merge_custom_emojis(&config.custom_emojis, db_emojis))
}

pub(crate) async fn config_with_resolved_custom_emojis(
    db: &D1Database,
    config: &AppConfig,
) -> Result<AppConfig> {
    let mut resolved = config.clone();
    resolved.custom_emojis = resolve_custom_emojis(db, config).await?;
    Ok(resolved)
}

pub(crate) async fn list_custom_emojis_from_db(
    db: &D1Database,
    config: &AppConfig,
) -> Result<Vec<CustomEmoji>> {
    let result = db
        .prepare(
            "SELECT id, shortcode, object_key, static_object_key, content_type, visible_in_picker, category
             FROM custom_emojis
             ORDER BY shortcode ASC",
        )
        .all()
        .await?;
    let rows = result.results::<CustomEmojiRow>()?;

    Ok(rows
        .into_iter()
        .map(|row| row_to_custom_emoji(config, &row))
        .collect())
}

pub(crate) async fn list_admin_custom_emojis(
    db: &D1Database,
    config: &AppConfig,
) -> Result<Vec<serde_json::Value>> {
    let result = db
        .prepare(
            "SELECT id, shortcode, object_key, static_object_key, content_type, visible_in_picker, category
             FROM custom_emojis
             ORDER BY shortcode ASC",
        )
        .all()
        .await?;
    let rows = result.results::<CustomEmojiRow>()?;

    Ok(rows
        .into_iter()
        .map(|row| admin_custom_emoji_json(config, &row))
        .collect())
}

pub(crate) async fn find_custom_emoji_row_by_id(
    db: &D1Database,
    emoji_id: &str,
) -> Result<Option<CustomEmojiRow>> {
    let bindings = [D1Type::Text(emoji_id)];
    db.prepare(
        "SELECT id, shortcode, object_key, static_object_key, content_type, visible_in_picker, category
         FROM custom_emojis
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .first::<CustomEmojiRow>(None)
    .await
}

pub(crate) async fn shortcode_taken(db: &D1Database, shortcode: &str) -> Result<bool> {
    let bindings = [D1Type::Text(shortcode)];
    let row = db
        .prepare("SELECT id FROM custom_emojis WHERE shortcode = ?1 LIMIT 1")
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.is_some())
}

pub(crate) async fn create_custom_emoji(
    db: &D1Database,
    bucket: &Bucket,
    config: &AppConfig,
    draft: CustomEmojiUploadDraft,
) -> Result<serde_json::Value> {
    let emoji_id = generate_entity_id(16)?;
    let object_key = format!("custom_emojis/{emoji_id}/original.{}", draft.extension);
    let static_object_key = format!("custom_emojis/{emoji_id}/static.{}", draft.static_extension);

    put_custom_emoji_object(bucket, &object_key, &draft.bytes, &draft.content_type).await?;
    put_custom_emoji_object(
        bucket,
        &static_object_key,
        &draft.static_bytes,
        &draft.static_content_type,
    )
    .await?;

    let bindings = [
        D1Type::Text(&emoji_id),
        D1Type::Text(&draft.shortcode),
        D1Type::Text(&object_key),
        D1Type::Text(&static_object_key),
        D1Type::Text(&draft.content_type),
        D1Type::Integer(i32::from(draft.visible_in_picker)),
        match draft.category.as_deref() {
            Some(category) => D1Type::Text(category),
            None => D1Type::Null,
        },
    ];
    let insert_result = db
        .prepare(
            "INSERT INTO custom_emojis (
                id,
                shortcode,
                object_key,
                static_object_key,
                content_type,
                visible_in_picker,
                category
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await;

    if insert_result.is_err() {
        let _ = delete_r2_object(bucket, &object_key).await;
        let _ = delete_r2_object(bucket, &static_object_key).await;
        return Err(insert_result.err().unwrap());
    }

    let row = find_custom_emoji_row_by_id(db, &emoji_id)
        .await?
        .ok_or_else(|| {
            worker::Error::RustError("failed to reload created custom emoji".to_owned())
        })?;
    Ok(admin_custom_emoji_json(config, &row))
}

pub(crate) async fn update_custom_emoji(
    db: &D1Database,
    bucket: Option<&Bucket>,
    config: &AppConfig,
    emoji_id: &str,
    visible_in_picker: Option<bool>,
    category: Option<Option<String>>,
    image: Option<CustomEmojiUploadDraft>,
) -> Result<Option<serde_json::Value>> {
    let Some(row) = find_custom_emoji_row_by_id(db, emoji_id).await? else {
        return Ok(None);
    };

    let next_visible = visible_in_picker.unwrap_or(row.visible_in_picker != 0);
    let next_category = match category {
        Some(value) => value,
        None => row.category.clone(),
    };

    let (object_key, static_object_key, content_type, previous_object_keys) =
        if let Some(draft) = image {
            let bucket = bucket.ok_or_else(|| {
                worker::Error::RustError(
                    "media bucket binding is required to replace emoji image".to_owned(),
                )
            })?;
            let object_key = format!("custom_emojis/{emoji_id}/original.{}", draft.extension);
            let static_object_key =
                format!("custom_emojis/{emoji_id}/static.{}", draft.static_extension);

            put_custom_emoji_object(bucket, &object_key, &draft.bytes, &draft.content_type).await?;
            put_custom_emoji_object(
                bucket,
                &static_object_key,
                &draft.static_bytes,
                &draft.static_content_type,
            )
            .await?;

            (
                object_key,
                static_object_key,
                draft.content_type,
                Some((row.object_key.clone(), row.static_object_key.clone())),
            )
        } else {
            (
                row.object_key.clone(),
                row.static_object_key.clone(),
                row.content_type.clone(),
                None,
            )
        };

    let bindings = [
        D1Type::Text(emoji_id),
        D1Type::Text(&object_key),
        D1Type::Text(&static_object_key),
        D1Type::Text(&content_type),
        D1Type::Integer(i32::from(next_visible)),
        match next_category.as_deref() {
            Some(category) => D1Type::Text(category),
            None => D1Type::Null,
        },
    ];
    let update_result = db
        .prepare(
            "UPDATE custom_emojis
         SET object_key = ?2,
             static_object_key = ?3,
             content_type = ?4,
             visible_in_picker = ?5,
             category = ?6,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await;

    if let Err(error) = update_result {
        if let (Some(bucket), Some((previous_object_key, previous_static_object_key))) =
            (bucket.as_ref(), previous_object_keys.as_ref())
        {
            rollback_replaced_custom_emoji_objects(
                bucket,
                &object_key,
                &static_object_key,
                previous_object_key,
                previous_static_object_key,
            )
            .await;
        }
        return Err(error);
    }

    if let (Some(bucket), Some((previous_object_key, previous_static_object_key))) =
        (bucket.as_ref(), previous_object_keys.as_ref())
    {
        delete_replaced_custom_emoji_objects(
            bucket,
            &object_key,
            &static_object_key,
            previous_object_key,
            previous_static_object_key,
        )
        .await;
    }

    let updated = find_custom_emoji_row_by_id(db, emoji_id)
        .await?
        .ok_or_else(|| {
            worker::Error::RustError("failed to reload updated custom emoji".to_owned())
        })?;
    Ok(Some(admin_custom_emoji_json(config, &updated)))
}

pub(crate) async fn delete_custom_emoji(
    db: &D1Database,
    bucket: &Bucket,
    emoji_id: &str,
) -> Result<bool> {
    let Some(row) = find_custom_emoji_row_by_id(db, emoji_id).await? else {
        return Ok(false);
    };

    let bindings = [D1Type::Text(emoji_id)];
    db.prepare("DELETE FROM custom_emojis WHERE id = ?1")
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    let _ = delete_r2_object(bucket, &row.object_key).await;
    if row.static_object_key != row.object_key {
        let _ = delete_r2_object(bucket, &row.static_object_key).await;
    }

    Ok(true)
}

pub(crate) fn normalize_custom_emoji_shortcode(
    shortcode: Option<String>,
) -> std::result::Result<String, String> {
    let shortcode = shortcode
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "shortcode is required".to_owned())?;
    if !is_custom_emoji_shortcode(&shortcode) {
        return Err(
            "shortcode must contain only ASCII letters, digits, and underscores".to_owned(),
        );
    }
    Ok(shortcode)
}

pub(crate) fn normalize_custom_emoji_upload(
    shortcode: String,
    bytes: Vec<u8>,
    content_type: String,
    visible_in_picker: bool,
    category: Option<String>,
    static_image: Option<(Vec<u8>, String)>,
) -> std::result::Result<CustomEmojiUploadDraft, String> {
    if bytes.is_empty() {
        return Err("image must not be empty".to_owned());
    }
    if bytes.len() > CUSTOM_EMOJI_MAX_BYTES {
        return Err(format!(
            "image exceeds the {CUSTOM_EMOJI_MAX_BYTES} byte custom emoji limit"
        ));
    }

    let content_type = content_type.trim().to_ascii_lowercase();
    let extension = match content_type.as_str() {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        other => return Err(format!("unsupported custom emoji content type: {other}")),
    };

    let category = category
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());

    let (static_bytes, static_content_type, static_extension) =
        resolve_static_emoji_upload(&bytes, &content_type, extension, static_image)?;

    Ok(CustomEmojiUploadDraft {
        shortcode,
        bytes,
        content_type,
        extension: extension.to_owned(),
        static_bytes,
        static_content_type,
        static_extension,
        visible_in_picker,
        category,
    })
}

fn resolve_static_emoji_upload(
    bytes: &[u8],
    content_type: &str,
    extension: &str,
    static_image: Option<(Vec<u8>, String)>,
) -> std::result::Result<(Vec<u8>, String, String), String> {
    if let Some((static_bytes, static_content_type)) = static_image {
        if static_bytes.is_empty() {
            return Err("static_image must not be empty".to_owned());
        }
        if static_bytes.len() > CUSTOM_EMOJI_MAX_BYTES {
            return Err(format!(
                "static_image exceeds the {CUSTOM_EMOJI_MAX_BYTES} byte custom emoji limit"
            ));
        }
        let static_content_type = static_content_type.trim().to_ascii_lowercase();
        let static_extension = match static_content_type.as_str() {
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            other => {
                return Err(format!(
                    "unsupported custom emoji static_image content type: {other}"
                ));
            }
        };
        return Ok((
            static_bytes,
            static_content_type,
            static_extension.to_owned(),
        ));
    }

    if content_type == "image/gif" {
        let static_bytes = gif_static_bytes(bytes);
        return Ok((static_bytes, "image/gif".to_owned(), "gif".to_owned()));
    }

    Ok((
        bytes.to_vec(),
        content_type.to_owned(),
        extension.to_owned(),
    ))
}

fn row_to_custom_emoji(config: &AppConfig, row: &CustomEmojiRow) -> CustomEmoji {
    CustomEmoji {
        shortcode: row.shortcode.clone(),
        url: media_object_url(config, &row.object_key),
        static_url: media_object_url(config, &row.static_object_key),
        visible_in_picker: row.visible_in_picker != 0,
        category: row.category.clone(),
    }
}

fn admin_custom_emoji_json(config: &AppConfig, row: &CustomEmojiRow) -> serde_json::Value {
    let mut value = custom_emoji_to_json(&row_to_custom_emoji(config, row));
    if let serde_json::Value::Object(map) = &mut value {
        map.insert("id".to_owned(), serde_json::json!(row.id));
    }
    value
}

async fn put_custom_emoji_object(
    bucket: &Bucket,
    object_key: &str,
    bytes: &[u8],
    content_type: &str,
) -> Result<()> {
    let started_at_ms = observability_started_at_ms();
    let result = bucket
        .put(object_key, bytes.to_vec())
        .http_metadata(HttpMetadata {
            content_type: Some(content_type.to_owned()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await;
    let outcome = if result.is_ok() { "ok" } else { "error" };
    log_r2_operation(
        "put_custom_emoji",
        outcome,
        started_at_ms,
        object_key,
        Some(bytes.len()),
    );
    result?;
    Ok(())
}

async fn delete_r2_object(bucket: &Bucket, object_key: &str) -> Result<()> {
    let started_at_ms = observability_started_at_ms();
    let result = bucket.delete(object_key).await;
    let outcome = if result.is_ok() { "ok" } else { "error" };
    log_r2_operation(
        "delete_custom_emoji",
        outcome,
        started_at_ms,
        object_key,
        None,
    );
    result?;
    Ok(())
}

async fn delete_replaced_custom_emoji_objects(
    bucket: &Bucket,
    object_key: &str,
    static_object_key: &str,
    previous_object_key: &str,
    previous_static_object_key: &str,
) {
    if previous_object_key != object_key {
        let _ = delete_r2_object(bucket, previous_object_key).await;
    }
    if previous_static_object_key != static_object_key
        && previous_static_object_key != previous_object_key
    {
        let _ = delete_r2_object(bucket, previous_static_object_key).await;
    }
}

async fn rollback_replaced_custom_emoji_objects(
    bucket: &Bucket,
    object_key: &str,
    static_object_key: &str,
    previous_object_key: &str,
    previous_static_object_key: &str,
) {
    if object_key != previous_object_key {
        let _ = delete_r2_object(bucket, object_key).await;
    }
    if static_object_key != previous_static_object_key
        && static_object_key != object_key
        && static_object_key != previous_object_key
    {
        let _ = delete_r2_object(bucket, static_object_key).await;
    }
}

pub(super) fn merge_custom_emojis(
    configured: &[CustomEmoji],
    stored: Vec<CustomEmoji>,
) -> Vec<CustomEmoji> {
    let mut merged = configured
        .iter()
        .map(|emoji| (emoji.shortcode.clone(), emoji.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    for emoji in stored {
        merged.insert(emoji.shortcode.clone(), emoji);
    }
    let mut emojis = merged.into_values().collect::<Vec<_>>();
    emojis.sort_by(|left, right| left.shortcode.cmp(&right.shortcode));
    emojis
}
