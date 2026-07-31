use crate::{
    AppConfig, D1Database, Error, LocalAccount, MediaAttachmentRow, OrphanMediaRow, Result,
    UpdateMediaRequest, d1_in_value_chunk_size, enqueue_addressed_create_activity,
    enqueue_direct_create_activity, outbox_create_insert_statement_with_attachments,
    parse_media_focus, sql_placeholders,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::d1::D1Type;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct RemoteStatusAttachmentRow {
    pub(crate) id: String,
    pub(crate) status_id: String,
    pub(crate) remote_url: String,
    pub(crate) preview_url: Option<String>,
    pub(crate) content_type: String,
    pub(crate) description: Option<String>,
    pub(crate) blurhash: Option<String>,
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) created_at: String,
}

pub(crate) async fn require_media_attachment_by_id(
    db: &D1Database,
    media_id: &str,
) -> Result<MediaAttachmentRow> {
    find_media_attachment_by_id(db, media_id)
        .await?
        .ok_or_else(|| Error::RustError("media attachment not found".to_owned()))
}

pub(crate) async fn find_media_attachment_by_id(
    db: &D1Database,
    media_id: &str,
) -> Result<Option<MediaAttachmentRow>> {
    let media_id = D1Type::Text(media_id);
    db.prepare(
        "SELECT id, account_id, status_id, object_key, content_type, description, focus_x, focus_y, width, height, created_at
         FROM media_attachments
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&media_id)?
    .first::<MediaAttachmentRow>(None)
    .await
}

pub(crate) async fn apply_media_update(
    db: &D1Database,
    media: &MediaAttachmentRow,
    update: UpdateMediaRequest,
) -> Result<MediaAttachmentRow> {
    let description = update
        .description
        .as_deref()
        .map(str::trim)
        .unwrap_or(&media.description)
        .to_owned();
    let focus = parse_media_focus(update.focus.as_deref()).map_err(Error::RustError)?;
    let focus_x = focus.map(|(x, _)| x).or(media.focus_x);
    let focus_y = focus.map(|(_, y)| y).or(media.focus_y);

    let bindings = [
        D1Type::Text(description.as_str()),
        match focus_x {
            Some(value) => D1Type::Real(value),
            None => D1Type::Null,
        },
        match focus_y {
            Some(value) => D1Type::Real(value),
            None => D1Type::Null,
        },
        D1Type::Text(media.id.as_str()),
    ];
    db.prepare(
        "UPDATE media_attachments
         SET description = ?1,
             focus_x = ?2,
             focus_y = ?3
         WHERE id = ?4",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_media_attachment_by_id(db, &media.id).await
}

pub(crate) async fn find_media_attachments_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Vec<MediaAttachmentRow>> {
    let status_id = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT id, account_id, status_id, object_key, content_type, description, focus_x, focus_y, width, height, created_at
             FROM media_attachments
             WHERE status_id = ?1
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(&status_id)?
        .all()
        .await?;

    result.results::<MediaAttachmentRow>()
}

pub(crate) async fn find_media_attachments_by_status_ids(
    db: &D1Database,
    status_ids: &[String],
) -> Result<HashMap<String, Vec<MediaAttachmentRow>>> {
    let mut seen = HashSet::new();
    let ids = status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut by_status_id = HashMap::new();
    for chunk in ids.chunks(d1_in_value_chunk_size(0)) {
        let placeholders = sql_placeholders(1, chunk.len());
        let sql = format!(
            "SELECT id, account_id, status_id, object_key, content_type, description, focus_x, focus_y, width, height, created_at
             FROM media_attachments
             WHERE status_id IN ({placeholders})
             ORDER BY status_id ASC, created_at ASC, id ASC"
        );
        let bindings = chunk
            .iter()
            .map(|id| D1Type::Text(id.as_str()))
            .collect::<Vec<_>>();
        let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
        for row in result.results::<MediaAttachmentRow>()? {
            by_status_id
                .entry(row.status_id.clone().unwrap_or_default())
                .or_insert_with(Vec::new)
                .push(row);
        }
    }

    Ok(by_status_id)
}

pub(crate) async fn find_remote_status_attachments_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Vec<RemoteStatusAttachmentRow>> {
    let status_id = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT id, status_id, remote_url, preview_url, content_type, description, blurhash, width, height, created_at
             FROM remote_status_attachments
             WHERE status_id = ?1
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(&status_id)?
        .all()
        .await?;

    result.results::<RemoteStatusAttachmentRow>()
}

pub(crate) async fn find_remote_status_attachments_by_status_ids(
    db: &D1Database,
    status_ids: &[String],
) -> Result<HashMap<String, Vec<RemoteStatusAttachmentRow>>> {
    let mut seen = HashSet::new();
    let ids = status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut by_status_id = HashMap::new();
    for chunk in ids.chunks(d1_in_value_chunk_size(0)) {
        let placeholders = sql_placeholders(1, chunk.len());
        let sql = format!(
            "SELECT id, status_id, remote_url, preview_url, content_type, description, blurhash, width, height, created_at
             FROM remote_status_attachments
             WHERE status_id IN ({placeholders})
             ORDER BY status_id ASC, created_at ASC, id ASC"
        );
        let bindings = chunk
            .iter()
            .map(|id| D1Type::Text(id.as_str()))
            .collect::<Vec<_>>();
        let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
        for row in result.results::<RemoteStatusAttachmentRow>()? {
            by_status_id
                .entry(row.status_id.clone())
                .or_insert_with(Vec::new)
                .push(row);
        }
    }

    Ok(by_status_id)
}

pub(crate) async fn replace_remote_status_attachments(
    db: &D1Database,
    status_id: &str,
    attachments: &[RemoteStatusAttachmentRow],
) -> Result<()> {
    let status_id_binding = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM remote_status_attachments
         WHERE status_id = ?1",
    )
    .bind_refs(&status_id_binding)?
    .run()
    .await?;

    for attachment in attachments {
        let bindings = [
            D1Type::Text(attachment.id.as_str()),
            D1Type::Text(attachment.status_id.as_str()),
            D1Type::Text(attachment.remote_url.as_str()),
            attachment
                .preview_url
                .as_deref()
                .map_or(D1Type::Null, D1Type::Text),
            D1Type::Text(attachment.content_type.as_str()),
            attachment
                .description
                .as_deref()
                .map_or(D1Type::Null, D1Type::Text),
            attachment
                .blurhash
                .as_deref()
                .map_or(D1Type::Null, D1Type::Text),
            attachment
                .width
                .map_or(D1Type::Null, |value| D1Type::Integer(value as i32)),
            attachment
                .height
                .map_or(D1Type::Null, |value| D1Type::Integer(value as i32)),
            D1Type::Text(attachment.created_at.as_str()),
        ];
        db.prepare(
            "INSERT INTO remote_status_attachments (
                id,
                status_id,
                remote_url,
                preview_url,
                content_type,
                description,
                blurhash,
                width,
                height,
                created_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

pub(crate) async fn remote_status_has_media(db: &D1Database, status_id: &str) -> Result<bool> {
    let status_id = D1Type::Text(status_id);
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM remote_status_attachments
             WHERE status_id = ?1
             LIMIT 1",
        )
        .bind_refs(&status_id)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

#[derive(Debug, Deserialize)]
struct RemoteMediaStatusIdRow {
    status_id: String,
}

pub(crate) async fn find_remote_status_ids_with_media(
    db: &D1Database,
    status_ids: &[String],
) -> Result<HashSet<String>> {
    let mut seen = HashSet::new();
    let ids = status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT DISTINCT status_id
         FROM remote_status_attachments
         WHERE status_id IN ({placeholders})"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<RemoteMediaStatusIdRow>()?
        .into_iter()
        .map(|row| row.status_id)
        .collect())
}

pub(crate) async fn list_orphan_media(
    db: &D1Database,
    older_than_hours: u32,
    limit: u32,
) -> Result<Vec<OrphanMediaRow>> {
    let older_than_modifier = format!("-{} hours", older_than_hours);
    let older_than = D1Type::Text(older_than_modifier.as_str());
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, object_key
             FROM media_attachments
             WHERE status_id IS NULL
               AND created_at <= datetime(CURRENT_TIMESTAMP, ?1)
             ORDER BY created_at ASC
             LIMIT ?2",
        )
        .bind_refs(&[older_than, limit])?
        .all()
        .await?;

    result.results::<OrphanMediaRow>()
}

pub(crate) async fn delete_media_attachment_row(db: &D1Database, media_id: &str) -> Result<()> {
    let media_id = D1Type::Text(media_id);
    db.prepare(
        "DELETE FROM media_attachments
         WHERE id = ?1",
    )
    .bind_refs(&media_id)?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn resolve_attachable_media(
    db: &D1Database,
    account: &LocalAccount,
    media_ids: &[String],
) -> std::result::Result<Vec<MediaAttachmentRow>, String> {
    let mut media = Vec::with_capacity(media_ids.len());
    let mut seen = HashSet::new();

    for media_id in media_ids {
        if !seen.insert(media_id.clone()) {
            continue;
        }
        let row = find_media_attachment_by_id(db, media_id)
            .await
            .map_err(|error| format!("failed to load media attachment {media_id}: {error}"))?
            .ok_or_else(|| format!("media attachment {media_id} was not found"))?;

        if row.account_id != account.id() {
            return Err(format!(
                "media attachment {media_id} does not belong to the authenticated account"
            ));
        }
        if row.status_id.is_some() {
            return Err(format!("media attachment {media_id} is already attached"));
        }

        media.push(row);
    }

    Ok(media)
}

pub(crate) async fn attach_media_and_enqueue_outbox(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &crate::StatusRow,
    media: &[MediaAttachmentRow],
) -> Result<()> {
    if media.is_empty() {
        return Ok(());
    }

    let values_sql = requested_media_values_sql(media.len());
    let expected_count_param = media.len() + 2;
    let sql = format!(
        "WITH requested(id) AS (VALUES {values_sql})
         UPDATE media_attachments
         SET status_id = ?1
         WHERE id IN (SELECT id FROM requested)
           AND status_id IS NULL
           AND (
                SELECT COUNT(*)
                FROM media_attachments
                WHERE id IN (SELECT id FROM requested)
                  AND status_id IS NULL
           ) = ?{expected_count_param}"
    );
    let mut bindings = media_status_bindings(&status.id, media);
    bindings.push(D1Type::Integer(media.len() as i32));
    let attach_statement = db.prepare(sql).bind_refs(bindings.iter())?;

    let outbox_statement =
        outbox_create_insert_statement_with_attachments(db, config, account, status, Some(media))
            .await?;

    let attach_result = attach_statement.run().await?;
    if !d1_result_did_change(&attach_result)? {
        return Err(Error::RustError(
            "one or more media attachments are no longer attachable".to_owned(),
        ));
    }

    if let Some(outbox_statement) = outbox_statement {
        outbox_statement.run().await?;
        enqueue_addressed_create_activity(db, config, account, status, Some(media)).await?;
    } else {
        enqueue_direct_create_activity(db, config, account, status, Some(media)).await?;
    }

    Ok(())
}

pub(crate) async fn resolve_editable_media(
    db: &D1Database,
    account: &LocalAccount,
    status_id: &str,
    media_ids: &[String],
) -> std::result::Result<Vec<MediaAttachmentRow>, String> {
    let mut media = Vec::with_capacity(media_ids.len());
    let mut seen = HashSet::new();

    for media_id in media_ids {
        if !seen.insert(media_id.clone()) {
            continue;
        }
        let row = find_media_attachment_by_id(db, media_id)
            .await
            .map_err(|error| format!("failed to load media attachment {media_id}: {error}"))?
            .ok_or_else(|| format!("media attachment {media_id} was not found"))?;

        if row.account_id != account.id() {
            return Err(format!(
                "media attachment {media_id} does not belong to the authenticated account"
            ));
        }
        if row.status_id.as_deref() != Some(status_id) && row.status_id.is_some() {
            return Err(format!("media attachment {media_id} is already attached"));
        }

        media.push(row);
    }

    Ok(media)
}

pub(crate) async fn replace_status_media(
    db: &D1Database,
    status_id: &str,
    media: &[MediaAttachmentRow],
) -> Result<()> {
    if media.is_empty() {
        let status_binding = D1Type::Text(status_id);
        db.prepare(
            "UPDATE media_attachments
             SET status_id = NULL
             WHERE status_id = ?1",
        )
        .bind_refs(&status_binding)?
        .run()
        .await?;
        return Ok(());
    }

    let values_sql = requested_media_values_sql(media.len());
    let expected_count_param = media.len() + 2;
    let sql = format!(
        "WITH requested(id) AS (VALUES {values_sql})
         UPDATE media_attachments
         SET status_id = CASE
             WHEN id IN (SELECT id FROM requested) THEN ?1
             ELSE NULL
         END
         WHERE (status_id = ?1 OR id IN (SELECT id FROM requested))
           AND (
                SELECT COUNT(*)
                FROM media_attachments
                WHERE id IN (SELECT id FROM requested)
                  AND (status_id IS NULL OR status_id = ?1)
           ) = ?{expected_count_param}"
    );
    let mut bindings = media_status_bindings(status_id, media);
    bindings.push(D1Type::Integer(media.len() as i32));
    let result = db.prepare(sql).bind_refs(bindings.iter())?.run().await?;

    if !d1_result_did_change(&result)? {
        return Err(Error::RustError(
            "one or more media attachments are no longer editable".to_owned(),
        ));
    }

    Ok(())
}

fn requested_media_values_sql(count: usize) -> String {
    (0..count)
        .map(|index| format!("(?{})", index + 2))
        .collect::<Vec<_>>()
        .join(", ")
}

fn media_status_bindings<'a>(
    status_id: &'a str,
    media: &'a [MediaAttachmentRow],
) -> Vec<D1Type<'a>> {
    let mut bindings = Vec::with_capacity(media.len() + 2);
    bindings.push(D1Type::Text(status_id));
    for attachment in media {
        bindings.push(D1Type::Text(attachment.id.as_str()));
    }
    bindings
}

fn d1_result_did_change(result: &worker::d1::D1Result) -> Result<bool> {
    Ok(result
        .meta()?
        .and_then(|meta| {
            meta.changed_db
                .or_else(|| meta.changes.map(|changes| changes > 0))
                .or_else(|| meta.rows_written.map(|rows_written| rows_written > 0))
        })
        .unwrap_or(false))
}
