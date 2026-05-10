use crate::{
    D1Database, LocalAccount, MediaAttachmentRow, MediaKind, MediaUploadDraft, OrphanMediaRow,
    Result, delete_media_attachment_row, generate_entity_id, require_media_attachment_by_id,
};
use serde::Deserialize;
use worker::{Bucket, HttpMetadata, d1::D1Type};

#[derive(Debug, Deserialize)]
struct QueuedMediaDeletionRow {
    object_key: String,
}

pub(crate) async fn store_media_attachment(
    db: &D1Database,
    bucket: &Bucket,
    account: &LocalAccount,
    draft: &MediaUploadDraft,
) -> Result<MediaAttachmentRow> {
    let media_id = generate_entity_id(16)?;
    let object_key = format!(
        "media/{}/{}/{}",
        account.id,
        media_kind_label(draft.kind),
        media_id
    );

    bucket
        .put(&object_key, draft.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(draft.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await?;

    let bindings = [
        D1Type::Text(media_id.as_str()),
        D1Type::Text(account.id.as_str()),
        D1Type::Text(object_key.as_str()),
        D1Type::Text(draft.content_type.as_str()),
        D1Type::Text(draft.description.as_str()),
    ];

    let insert_result = db
        .prepare(
            "INSERT INTO media_attachments (
                id,
                account_id,
                status_id,
                object_key,
                content_type,
                description,
                created_at
            ) VALUES (
                ?1,
                ?2,
                NULL,
                ?3,
                ?4,
                ?5,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await;

    if let Err(error) = insert_result {
        let _ = bucket.delete(&object_key).await;
        return Err(error);
    }

    require_media_attachment_by_id(db, &media_id).await
}
pub(crate) async fn delete_media_attachments(
    db: &D1Database,
    bucket: &Bucket,
    attachments: &[MediaAttachmentRow],
) -> Result<()> {
    for attachment in attachments {
        delete_media_attachment_row(db, &attachment.id).await?;
        if let Err(error) = bucket.delete(&attachment.object_key).await {
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
        match bucket.delete(&orphan.object_key).await {
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
        match bucket.delete(&row.object_key).await {
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
