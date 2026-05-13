use crate::{
    AppConfig, D1Database, Error, Result, StatusRow, find_account_by_id,
    local_status_identity_from_uri, sql_placeholders, unique_ordered_refs,
};
use std::collections::HashMap;
use worker::d1::D1Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccountStatusVisibilityScope {
    All,
    Public,
    PublicUnlistedPrivate,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AccountStatusListOptions<'a> {
    pub(crate) max_id: Option<&'a str>,
    pub(crate) min_id: Option<&'a str>,
    pub(crate) limit: u32,
    pub(crate) visibility: AccountStatusVisibilityScope,
    pub(crate) only_media: bool,
    pub(crate) exclude_replies: bool,
    pub(crate) exclude_reblogs: bool,
    pub(crate) tagged: Option<&'a str>,
}

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

pub(crate) async fn find_statuses_by_ids(
    db: &D1Database,
    status_ids: &[String],
) -> Result<Vec<StatusRow>> {
    let ids = unique_ordered_refs(status_ids);
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = sql_placeholders(1, ids.len());
    let sql = format!(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
         FROM statuses
         WHERE id IN ({placeholders})"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result.results::<StatusRow>()
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
    options: AccountStatusListOptions<'_>,
) -> Result<Vec<StatusRow>> {
    let tagged_pattern = options
        .tagged
        .map(|tag| tag.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|tag| !tag.is_empty())
        .map(|tag| format!("%#{tag}%"));
    let mut bindings = vec![
        D1Type::Text(account_id),
        options.max_id.map_or(D1Type::Null, D1Type::Text),
        options.min_id.map_or(D1Type::Null, D1Type::Text),
    ];
    let mut next_binding = 4;
    let mut predicates = vec![
        "account_id = ?1".to_owned(),
        "(
            ?2 IS NULL
            OR NOT EXISTS (SELECT 1 FROM max_cursor)
            OR EXISTS (
                SELECT 1 FROM max_cursor
                WHERE statuses.created_at < max_cursor.created_at
                   OR (statuses.created_at = max_cursor.created_at AND statuses.id < max_cursor.id)
            )
        )"
        .to_owned(),
        "(
            ?3 IS NULL
            OR NOT EXISTS (SELECT 1 FROM min_cursor)
            OR EXISTS (
                SELECT 1 FROM min_cursor
                WHERE statuses.created_at > min_cursor.created_at
                   OR (statuses.created_at = min_cursor.created_at AND statuses.id > min_cursor.id)
            )
        )"
        .to_owned(),
    ];

    match options.visibility {
        AccountStatusVisibilityScope::All => {}
        AccountStatusVisibilityScope::Public => {
            predicates.push("visibility IN ('public', 'unlisted')".to_owned());
        }
        AccountStatusVisibilityScope::PublicUnlistedPrivate => {
            predicates.push("visibility IN ('public', 'unlisted', 'private')".to_owned());
        }
    }
    if options.exclude_reblogs {
        predicates.push("boost_of_uri IS NULL".to_owned());
    }
    if options.exclude_replies {
        predicates.push(
            "(in_reply_to_id IS NULL
              OR NOT EXISTS (SELECT 1 FROM statuses reply WHERE reply.id = statuses.in_reply_to_id)
              OR EXISTS (
                  SELECT 1 FROM statuses reply
                  WHERE reply.id = statuses.in_reply_to_id
                    AND reply.account_id = ?1
              ))"
            .to_owned(),
        );
    }
    if options.only_media {
        predicates.push(
            "EXISTS (
                SELECT 1 FROM media_attachments media
                WHERE media.status_id = statuses.id
            )"
            .to_owned(),
        );
    }
    if let Some(pattern) = tagged_pattern.as_deref() {
        predicates.push(format!("lower(text_content) LIKE ?{next_binding}"));
        bindings.push(D1Type::Text(pattern));
        next_binding += 1;
    }

    let limit_binding = next_binding;
    bindings.push(D1Type::Integer(options.limit as i32));
    let sql = format!(
        "WITH max_cursor AS (
            SELECT id, created_at FROM statuses WHERE id = ?2 LIMIT 1
         ),
         min_cursor AS (
            SELECT id, created_at FROM statuses WHERE id = ?3 LIMIT 1
         )
         SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
         FROM statuses
         WHERE {}
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_binding}",
        predicates.join("\n           AND ")
    );
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result.results::<StatusRow>()
}

pub(crate) async fn list_public_account_statuses(
    db: &D1Database,
    account_id: &str,
    max_id: Option<&str>,
    min_id: Option<&str>,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [
        D1Type::Text(account_id),
        max_id.map_or(D1Type::Null, D1Type::Text),
        min_id.map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "WITH max_cursor AS (
                SELECT id, created_at FROM statuses WHERE id = ?2 LIMIT 1
             ),
             min_cursor AS (
                SELECT id, created_at FROM statuses WHERE id = ?3 LIMIT 1
             )
             SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
             FROM statuses
             WHERE account_id = ?1
               AND visibility IN ('public', 'unlisted')
               AND (
                    ?2 IS NULL
                    OR NOT EXISTS (SELECT 1 FROM max_cursor)
                    OR EXISTS (
                        SELECT 1 FROM max_cursor
                        WHERE statuses.created_at < max_cursor.created_at
                           OR (statuses.created_at = max_cursor.created_at AND statuses.id < max_cursor.id)
                    )
               )
               AND (
                    ?3 IS NULL
                    OR NOT EXISTS (SELECT 1 FROM min_cursor)
                    OR EXISTS (
                        SELECT 1 FROM min_cursor
                        WHERE statuses.created_at > min_cursor.created_at
                           OR (statuses.created_at = min_cursor.created_at AND statuses.id > min_cursor.id)
                    )
               )
             ORDER BY created_at DESC, id DESC
             LIMIT ?4",
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
