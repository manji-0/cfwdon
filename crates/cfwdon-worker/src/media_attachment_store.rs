use crate::{
    D1Database, Error, LocalAccount, MediaAttachmentRow, OrphanMediaRow, Result,
    UpdateMediaRequest, parse_media_focus,
};
use serde::Deserialize;
use std::collections::HashSet;
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

        if row.account_id != account.id {
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
    let status_binding = D1Type::Text(status_id);
    db.prepare(
        "UPDATE media_attachments
         SET status_id = NULL
         WHERE status_id = ?1",
    )
    .bind_refs(&status_binding)?
    .run()
    .await?;

    for attachment in media {
        let bindings = [
            D1Type::Text(status_id),
            D1Type::Text(attachment.id.as_str()),
        ];
        db.prepare(
            "UPDATE media_attachments
             SET status_id = ?1
             WHERE id = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}
