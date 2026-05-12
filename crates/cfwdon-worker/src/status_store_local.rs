use crate::{
    AppConfig, D1Database, Error, ResolvedTimelineCursor, Result, StatusRow, find_account_by_id,
    local_status_identity_from_uri, sql_placeholders, unique_ordered_refs,
};
use std::collections::HashMap;
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
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
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
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
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

pub(crate) async fn load_in_reply_to_account_ids(
    db: &D1Database,
    statuses: &[StatusRow],
) -> Result<HashMap<String, String>> {
    let reply_ids = statuses
        .iter()
        .filter_map(|status| status.in_reply_to_id.clone())
        .collect::<Vec<_>>();
    let reply_ids = unique_ordered_refs(&reply_ids);
    if reply_ids.is_empty() {
        return Ok(HashMap::new());
    }

    #[derive(Debug, serde::Deserialize)]
    struct ReplyAccountIdRow {
        id: String,
        account_id: String,
    }

    let placeholders = sql_placeholders(1, reply_ids.len());
    let sql = format!(
        "SELECT id, account_id
         FROM statuses
         WHERE id IN ({placeholders})"
    );
    let bindings = reply_ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    let reply_accounts_by_status_id = result
        .results::<ReplyAccountIdRow>()?
        .into_iter()
        .map(|row| (row.id, row.account_id))
        .collect::<HashMap<_, _>>();

    Ok(statuses
        .iter()
        .filter_map(|status| {
            status
                .in_reply_to_id
                .as_ref()
                .and_then(|reply_id| reply_accounts_by_status_id.get(reply_id))
                .map(|account_id| (status.id.clone(), account_id.clone()))
        })
        .collect())
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
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
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
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
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

pub(crate) async fn list_public_account_statuses(
    db: &D1Database,
    account_id: &str,
    cursor: &ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [
        D1Type::Text(account_id),
        cursor
            .max_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.max_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        cursor
            .min_timestamp
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        cursor.min_id.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
             FROM statuses
             WHERE account_id = ?1
               AND visibility IN ('public', 'unlisted')
               AND (
                    ?2 IS NULL
                    OR created_at < ?2
                    OR (created_at = ?2 AND id < ?3)
               )
               AND (
                    ?4 IS NULL
                    OR created_at > ?4
                    OR (created_at = ?4 AND id > ?5)
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
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
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, created_at, updated_at
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
