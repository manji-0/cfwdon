use crate::{
    D1Database, Error, LocalAccount, MediaAttachmentRow, OrphanMediaRow, Result,
    UpdateMediaRequest, parse_media_focus,
};
use worker::d1::D1Type;

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
        "SELECT id, account_id, status_id, object_key, content_type, description, focus_x, focus_y, created_at
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
            "SELECT id, account_id, status_id, object_key, content_type, description, focus_x, focus_y, created_at
             FROM media_attachments
             WHERE status_id = ?1
             ORDER BY created_at ASC",
        )
        .bind_refs(&status_id)?
        .all()
        .await?;

    result.results::<MediaAttachmentRow>()
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

    for media_id in media_ids {
        let row = find_media_attachment_by_id(db, media_id)
            .await
            .map_err(|error| format!("failed to load media attachment {media_id}: {error}"))?
            .ok_or_else(|| format!("media attachment {media_id} was not found"))?;

        if row.account_id != account.id {
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

pub(crate) async fn attach_media_to_status(
    db: &D1Database,
    status_id: &str,
    media: &[MediaAttachmentRow],
) -> Result<()> {
    for attachment in media {
        let bindings = [
            D1Type::Text(status_id),
            D1Type::Text(attachment.id.as_str()),
        ];
        db.prepare(
            "UPDATE media_attachments
             SET status_id = ?1
             WHERE id = ?2 AND status_id IS NULL",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}
