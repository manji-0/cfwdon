use crate::{
    D1Database, Error, LocalAccount, MediaAttachmentRow, MediaKind, MediaUploadDraft,
    OrphanMediaRow, Result, delete_media_attachment_row, generate_entity_id,
    require_media_attachment_by_id,
};
use worker::{Bucket, HttpMetadata, d1::D1Type};

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

    let upload = bucket
        .put(&object_key, draft.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(draft.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await?;
    if upload.is_none() {
        return Err(Error::RustError(
            "failed to persist media object to R2".to_owned(),
        ));
    }

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
        bucket.delete(&attachment.object_key).await?;
        delete_media_attachment_row(db, &attachment.id).await?;
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
        bucket.delete(&orphan.object_key).await?;
        delete_media_attachment_row(db, &orphan.id).await?;
        deleted += 1;
    }

    Ok(deleted)
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
