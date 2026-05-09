use super::{
    ProfileMediaUpload, UpdateCredentialsRequest, enqueue_profile_update_activities,
    find_account_by_id, generate_entity_id, profile_field_from_update, render_status_html,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::d1::D1Type;
use worker::{Bucket, D1Database, Error, HttpMetadata, Result};

struct AccountSourceDefaults {
    post_visibility: String,
    quote_policy: String,
    sensitive: bool,
    language: Option<String>,
}

fn account_source_defaults(
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> AccountSourceDefaults {
    AccountSourceDefaults {
        post_visibility: update
            .source
            .as_ref()
            .and_then(|source| source.privacy.as_deref())
            .unwrap_or(account.default_post_visibility.as_str())
            .to_owned(),
        quote_policy: update
            .source
            .as_ref()
            .and_then(|source| source.quote_policy.as_deref())
            .unwrap_or(account.default_quote_policy.as_str())
            .to_owned(),
        sensitive: update
            .source
            .as_ref()
            .and_then(|source| source.sensitive)
            .unwrap_or(account.default_sensitive),
        language: update
            .source
            .as_ref()
            .and_then(|source| source.language.clone())
            .or_else(|| account.default_language.clone()),
    }
}

pub(crate) async fn apply_account_credentials_update(
    db: &D1Database,
    bucket: &Bucket,
    config: &AppConfig,
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> Result<LocalAccount> {
    let display_name = update
        .display_name
        .as_deref()
        .unwrap_or(account.display_name.as_str())
        .to_owned();
    let bio_text = update
        .note
        .as_deref()
        .unwrap_or(account.bio_text.as_str())
        .to_owned();
    let bio_html = render_status_html(&bio_text);
    let fields = update
        .fields_attributes
        .as_ref()
        .map(|fields| {
            fields
                .iter()
                .filter_map(profile_field_from_update)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| account.fields.clone());
    let fields_json = serde_json::to_string(&fields).map_err(|error| {
        Error::RustError(format!("failed to serialize account fields: {error}"))
    })?;
    let locked = update.locked.unwrap_or(account.locked);
    let bot = update.bot.unwrap_or(account.bot);
    let discoverable = update.discoverable.unwrap_or(account.discoverable);
    let source_defaults = account_source_defaults(account, update);
    let avatar_profile = match update.avatar.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };
    let header_profile = match update.header.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };
    if let Some(previous) = account.avatar_object_key.as_deref()
        && avatar_profile.is_some()
        && avatar_profile
            .as_ref()
            .map(|profile| profile.0.as_str() != previous)
            .unwrap_or(false)
    {
        bucket.delete(previous).await?;
    }
    if let Some(previous) = account.header_object_key.as_deref()
        && header_profile.is_some()
        && header_profile
            .as_ref()
            .map(|profile| profile.0.as_str() != previous)
            .unwrap_or(false)
    {
        bucket.delete(previous).await?;
    }

    let bindings = [
        D1Type::Text(display_name.as_str()),
        D1Type::Text(bio_html.as_str()),
        D1Type::Text(bio_text.as_str()),
        D1Type::Text(fields_json.as_str()),
        D1Type::Integer(if locked { 1 } else { 0 }),
        D1Type::Integer(if bot { 1 } else { 0 }),
        D1Type::Integer(if discoverable { 1 } else { 0 }),
        D1Type::Text(source_defaults.post_visibility.as_str()),
        D1Type::Text(source_defaults.quote_policy.as_str()),
        D1Type::Integer(if source_defaults.sensitive { 1 } else { 0 }),
        match source_defaults.language.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match avatar_profile.as_ref().map(|value| value.0.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.avatar_object_key.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        match avatar_profile.as_ref().map(|value| value.1.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.avatar_content_type.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        match header_profile.as_ref().map(|value| value.0.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.header_object_key.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        match header_profile.as_ref().map(|value| value.1.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.header_content_type.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        D1Type::Text(account.id.as_str()),
    ];

    db.prepare(
        "UPDATE accounts
         SET display_name = ?1,
             bio_html = ?2,
             bio_text = ?3,
             fields_json = ?4,
             locked = ?5,
             bot = ?6,
             discoverable = ?7,
             default_post_visibility = ?8,
             default_quote_policy = ?9,
             default_sensitive = ?10,
             default_language = ?11,
             avatar_object_key = ?12,
             avatar_content_type = ?13,
             header_object_key = ?14,
             header_content_type = ?15,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?16",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let updated = find_account_by_id(db, &account.id)
        .await?
        .ok_or_else(|| Error::RustError("failed to reload updated account".to_owned()))?;
    enqueue_profile_update_activities(db, config, &updated).await?;
    Ok(updated)
}

async fn store_profile_media(
    bucket: &Bucket,
    account: &LocalAccount,
    upload: &ProfileMediaUpload,
) -> Result<(String, String)> {
    let media_id = generate_entity_id(16)?;
    let object_key = format!(
        "profiles/{}/{}/{}",
        account.id, upload.object_kind, media_id
    );
    bucket
        .put(&object_key, upload.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(upload.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await?;
    Ok((object_key, upload.content_type.clone()))
}
