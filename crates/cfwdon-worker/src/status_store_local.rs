use crate::{
    AppConfig, D1Database, Error, Result, StatusRow, find_account_by_id,
    local_status_identity_from_uri,
};
use worker::d1::D1Type;

pub(crate) async fn require_status_by_id(db: &D1Database, status_id: &str) -> Result<StatusRow> {
    find_status_by_id(db, status_id)
        .await?
        .ok_or_else(|| Error::RustError("status not found".to_owned()))
}

pub(crate) async fn find_status_by_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<StatusRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
         FROM statuses
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<StatusRow>(None)
    .await
}

pub(crate) async fn find_status_by_ap_id(
    db: &D1Database,
    ap_id: &str,
) -> Result<Option<StatusRow>> {
    let ap_id = D1Type::Text(ap_id);
    db.prepare(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
         FROM statuses
         WHERE ap_id = ?1
         LIMIT 1",
    )
    .bind_refs(&ap_id)?
    .first::<StatusRow>(None)
    .await
}

pub(crate) async fn load_in_reply_to_account_id(
    db: &D1Database,
    status: &StatusRow,
) -> Result<Option<String>> {
    match status.in_reply_to_id.as_deref() {
        Some(reply_id) => Ok(find_status_by_id(db, reply_id)
            .await?
            .map(|reply| reply.account_id)),
        None => Ok(None),
    }
}

pub(crate) async fn status_is_reply_to_other_account(
    db: &D1Database,
    status: &StatusRow,
    account_id: &str,
) -> Result<bool> {
    let Some(reply_id) = status.in_reply_to_id.as_deref() else {
        return Ok(false);
    };

    Ok(find_status_by_id(db, reply_id)
        .await?
        .map(|reply| reply.account_id != account_id)
        .unwrap_or(false))
}

pub(crate) async fn list_public_outbox_statuses(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let account_id = D1Type::Text(account_id);
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id = ?1
               AND visibility IN ('public', 'unlisted')
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(&[account_id, limit])?
        .all()
        .await?;

    result.results::<StatusRow>()
}

pub(crate) async fn list_account_statuses(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let account_id = D1Type::Text(account_id);
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(&[account_id, limit])?
        .all()
        .await?;

    result.results::<StatusRow>()
}

pub(crate) async fn list_direct_local_replies(
    db: &D1Database,
    status_id: &str,
) -> Result<Vec<StatusRow>> {
    let status_id = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE in_reply_to_id = ?1
             ORDER BY created_at ASC",
        )
        .bind_refs(&status_id)?
        .all()
        .await?;

    result.results::<StatusRow>()
}

pub(crate) fn local_status_target_uri(status: &StatusRow) -> String {
    status
        .ap_id
        .clone()
        .unwrap_or_else(|| format!("local:{}", status.id))
}

pub(crate) async fn find_local_status_by_object_uri(
    db: &D1Database,
    config: &AppConfig,
    object_uri: &str,
) -> Result<Option<StatusRow>> {
    if let Some(status) = find_status_by_ap_id(db, object_uri).await? {
        return Ok(Some(status));
    }
    let Some((username, status_id)) = local_status_identity_from_uri(config, object_uri) else {
        return Ok(None);
    };
    let Some(status) = find_status_by_id(db, &status_id).await? else {
        return Ok(None);
    };
    let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(None);
    };
    if owner.username.eq_ignore_ascii_case(&username) {
        Ok(Some(status))
    } else {
        Ok(None)
    }
}
