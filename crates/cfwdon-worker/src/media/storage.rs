use crate::{
    D1Database, LocalAccount, MediaAttachmentRow, MediaKind, MediaUploadDraft, OrphanMediaRow,
    Result, delete_media_attachment_row, generate_entity_id, log_observed_operation,
    observability_started_at_ms, require_media_attachment_by_id,
};
use cfwdon_domain::StoredMediaAttachmentIntent;
use serde::Deserialize;
use worker::{Bucket, HttpMetadata, d1::D1Type};

#[derive(Debug, Deserialize)]
struct QueuedMediaDeletionRow {
    object_key: String,
}

fn stored_media_attachment_intent(
    account: &LocalAccount,
    draft: &MediaUploadDraft,
    media_id: String,
) -> StoredMediaAttachmentIntent {
    let object_key = media_attachment_object_key(account.id(), draft.kind, &media_id);
    StoredMediaAttachmentIntent::new(
        media_id,
        account.id(),
        object_key,
        draft.content_type.clone(),
        draft.description.clone(),
        draft.width,
        draft.height,
    )
}

pub(crate) async fn store_media_attachment(
    db: &D1Database,
    bucket: &Bucket,
    account: &LocalAccount,
    draft: &MediaUploadDraft,
) -> Result<MediaAttachmentRow> {
    let media_id = generate_entity_id(16)?;
    let intent = stored_media_attachment_intent(account, draft, media_id);

    let put_started_at_ms = observability_started_at_ms();
    let put_result = bucket
        .put(&intent.object_key, draft.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(draft.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await;
    let put_outcome = if put_result.is_ok() { "ok" } else { "error" };
    log_r2_operation(
        "put",
        put_outcome,
        put_started_at_ms,
        &intent.object_key,
        Some(draft.bytes.len()),
    );
    put_result?;

    if let Err(error) = insert_media_attachment_row(db, &intent).await {
        let _ = delete_r2_object(bucket, &intent.object_key, "rollback_delete").await;
        return Err(error);
    }

    require_media_attachment_by_id(db, &intent.media_id).await
}

async fn insert_media_attachment_row(
    db: &D1Database,
    intent: &StoredMediaAttachmentIntent,
) -> Result<()> {
    let bindings = media_attachment_insert_bindings(intent);
    db.prepare(
        "INSERT INTO media_attachments (
            id,
            account_id,
            status_id,
            object_key,
            content_type,
            description,
            width,
            height,
            created_at
        ) VALUES (
            ?1,
            ?2,
            NULL,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

fn media_attachment_object_key(account_id: &str, kind: MediaKind, media_id: &str) -> String {
    format!(
        "media/{}/{}/{}",
        account_id,
        media_kind_label(kind),
        media_id
    )
}

fn media_attachment_insert_bindings(intent: &StoredMediaAttachmentIntent) -> [D1Type<'_>; 7] {
    [
        D1Type::Text(intent.media_id.as_str()),
        D1Type::Text(intent.account_id.as_str()),
        D1Type::Text(intent.object_key.as_str()),
        D1Type::Text(intent.content_type.as_str()),
        D1Type::Text(intent.description.as_str()),
        intent
            .width
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
        intent
            .height
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
    ]
}

pub(crate) async fn delete_media_attachments(
    db: &D1Database,
    bucket: &Bucket,
    attachments: &[MediaAttachmentRow],
) -> Result<()> {
    for attachment in attachments {
        delete_media_attachment_row(db, &attachment.id).await?;
        if let Err(error) = delete_r2_object(bucket, &attachment.object_key, "delete").await {
            queue_media_deletion(db, &attachment.object_key, &error.to_string()).await?;
        }
    }

    Ok(())
}

pub(crate) async fn delete_orphan_media(
    db: &D1Database,
    bucket: &Bucket,
    orphans: &[OrphanMediaRow],
) -> Result<u32> {
    let mut deleted = 0;

    for orphan in orphans {
        delete_media_attachment_row(db, &orphan.id).await?;
        match delete_r2_object(bucket, &orphan.object_key, "delete_orphan").await {
            Ok(()) => {
                deleted += 1;
            }
            Err(error) => {
                queue_media_deletion(db, &orphan.object_key, &error.to_string()).await?;
            }
        }
    }

    Ok(deleted)
}

pub(crate) async fn delete_queued_media(
    db: &D1Database,
    bucket: &Bucket,
    limit: u32,
) -> Result<u32> {
    let queued = list_queued_media_deletions(db, limit).await?;
    let mut deleted = 0;

    for row in queued {
        match delete_r2_object(bucket, &row.object_key, "delete_queued").await {
            Ok(()) => {
                delete_queued_media_deletion(db, &row.object_key).await?;
                deleted += 1;
            }
            Err(error) => {
                queue_media_deletion(db, &row.object_key, &error.to_string()).await?;
            }
        }
    }

    Ok(deleted)
}

async fn list_queued_media_deletions(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<QueuedMediaDeletionRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT object_key
             FROM media_deletion_queue
             ORDER BY updated_at ASC, object_key ASC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<QueuedMediaDeletionRow>()
}

async fn queue_media_deletion(db: &D1Database, object_key: &str, error: &str) -> Result<()> {
    let bindings = [D1Type::Text(object_key), D1Type::Text(error)];
    db.prepare(
        "INSERT INTO media_deletion_queue (object_key, attempts, last_error, created_at, updated_at)
         VALUES (?1, 1, ?2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT(object_key) DO UPDATE SET
             attempts = attempts + 1,
             last_error = excluded.last_error,
             updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_queued_media_deletion(db: &D1Database, object_key: &str) -> Result<()> {
    let object_key = D1Type::Text(object_key);
    db.prepare(
        "DELETE FROM media_deletion_queue
         WHERE object_key = ?1",
    )
    .bind_refs(&object_key)?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_r2_object(
    bucket: &Bucket,
    object_key: &str,
    operation: &str,
) -> Result<()> {
    let started_at_ms = observability_started_at_ms();
    let result = bucket.delete(object_key).await;
    let outcome = if result.is_ok() { "ok" } else { "error" };
    log_r2_operation(operation, outcome, started_at_ms, object_key, None);
    result
}

pub(crate) fn log_r2_operation(
    operation: &str,
    outcome: &str,
    started_at_ms: f64,
    object_key: &str,
    bytes: Option<usize>,
) {
    let mut details = serde_json::json!({
        "object_family": r2_object_family(object_key),
    });
    if let (Some(details), Some(bytes)) = (details.as_object_mut(), bytes) {
        details.insert("bytes".to_owned(), serde_json::json!(bytes));
    }
    log_observed_operation("r2", operation, outcome, started_at_ms, details);
}

fn r2_object_family(object_key: &str) -> &'static str {
    if object_key.starts_with("media/") {
        "media"
    } else if object_key.starts_with("profiles/") {
        "profile"
    } else {
        "unknown"
    }
}
pub(crate) fn classify_media_kind(content_type: &str) -> Option<MediaKind> {
    if content_type.starts_with("image/") {
        Some(MediaKind::Image)
    } else if content_type.starts_with("video/") {
        Some(MediaKind::Video)
    } else if content_type.starts_with("audio/") {
        Some(MediaKind::Audio)
    } else {
        None
    }
}

pub(crate) const fn media_kind_label(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => "image",
        MediaKind::Video => "video",
        MediaKind::Audio => "audio",
    }
}
