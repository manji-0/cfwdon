use super::{
    AppConfig, Error, Result, StatusRecord, StatusRow, find_account_by_id,
    find_remote_statuses_with_actors_by_ids, json_string_array, local_status_identity_from_uri,
    remote_account_rest_id, sql_in_json_each, status_from_record, statuses_from_records,
    unique_ordered_refs,
};
use crate::{D1Database, append_local_status_id_cursor_parts, format_with_clauses};
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
    .first::<StatusRecord>(None)
    .await
    .and_then(|row| row.map(status_from_record).transpose())
}

pub(crate) async fn find_statuses_by_ids(
    db: &D1Database,
    status_ids: &[String],
) -> Result<Vec<StatusRow>> {
    let ids = unique_ordered_refs(status_ids);
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let ids_json = json_string_array(&ids);
    let sql = format!(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
         FROM statuses
         WHERE id {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(ids_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;

    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
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
    .first::<StatusRecord>(None)
    .await
    .and_then(|row| row.map(status_from_record).transpose())
}

pub(crate) async fn find_statuses_by_ap_ids(
    db: &D1Database,
    ap_ids: &[String],
) -> Result<Vec<StatusRow>> {
    let ap_ids = unique_ordered_refs(ap_ids);
    if ap_ids.is_empty() {
        return Ok(Vec::new());
    }

    let ap_ids_json = json_string_array(&ap_ids);
    let sql = format!(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
         FROM statuses
         WHERE ap_id {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(ap_ids_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;

    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
}

pub(crate) async fn load_in_reply_to_account_id(
    db: &D1Database,
    status: &StatusRow,
) -> Result<Option<String>> {
    match status.in_reply_to_id.as_deref() {
        Some(reply_id) => super::resolve_in_reply_to_account_id(db, reply_id).await,
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

    let reply_ids_json = json_string_array(&reply_ids);
    let sql = format!(
        "SELECT id, account_id
         FROM statuses
         WHERE id {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(reply_ids_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;
    let mut reply_accounts_by_status_id = result
        .results::<ReplyAccountIdRow>()?
        .into_iter()
        .map(|row| (row.id, row.account_id))
        .collect::<HashMap<_, _>>();

    let unresolved_reply_ids = reply_ids
        .iter()
        .filter(|reply_id| !reply_accounts_by_status_id.contains_key(**reply_id))
        .map(|reply_id| (*reply_id).to_owned())
        .collect::<Vec<_>>();
    if !unresolved_reply_ids.is_empty() {
        for (remote_status, actor) in
            find_remote_statuses_with_actors_by_ids(db, &unresolved_reply_ids).await?
        {
            reply_accounts_by_status_id
                .insert(remote_status.id, remote_account_rest_id(&actor.actor_uri));
        }
    }

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
    list_public_outbox_statuses_page(db, account_id, limit, 0).await
}

pub(crate) async fn list_public_outbox_statuses_page(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<StatusRow>> {
    let account_id = D1Type::Text(account_id);
    let limit = D1Type::Integer(limit as i32);
    let offset = D1Type::Integer(offset as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
             FROM statuses
             WHERE account_id = ?1
               AND visibility IN ('public', 'unlisted')
             ORDER BY created_at DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind_refs(&[account_id, limit, offset])?
        .all()
        .await?;

    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
}

pub(crate) async fn count_public_outbox_statuses(db: &D1Database, account_id: &str) -> Result<u64> {
    #[derive(serde::Deserialize)]
    struct CountRow {
        count: i64,
    }

    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM statuses
             WHERE account_id = ?1
               AND visibility IN ('public', 'unlisted')",
        )
        .bind_refs(&[account_id])?
        .first::<CountRow>(None)
        .await?;

    Ok(row.map(|row| row.count.max(0) as u64).unwrap_or(0))
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
    let mut bindings = vec![D1Type::Text(account_id)];
    let cursor_parts = append_local_status_id_cursor_parts(
        &mut bindings,
        "statuses",
        options.max_id,
        options.min_id,
    );
    let mut next_binding = bindings.len() + 1;
    let mut predicates = vec!["account_id = ?1".to_owned()];
    predicates.extend(cursor_parts.predicates);

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
    let with_clause = format_with_clauses(&cursor_parts.with_clauses);
    let sql = format!(
        "{with_clause}SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
         FROM statuses
         WHERE {}
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_binding}",
        predicates.join("\n           AND ")
    );
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
}

pub(crate) async fn list_public_account_statuses(
    db: &D1Database,
    account_id: &str,
    max_id: Option<&str>,
    min_id: Option<&str>,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let (sql, bindings) = public_account_statuses_sql(account_id, max_id, min_id, limit);
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
}

fn public_account_statuses_sql<'a>(
    account_id: &'a str,
    max_id: Option<&'a str>,
    min_id: Option<&'a str>,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![D1Type::Text(account_id)];
    let cursor_parts =
        append_local_status_id_cursor_parts(&mut bindings, "statuses", max_id, min_id);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_binding = bindings.len();
    let with_clause = format_with_clauses(&cursor_parts.with_clauses);
    let mut predicates = vec![
        "account_id = ?1".to_owned(),
        "visibility IN ('public', 'unlisted')".to_owned(),
    ];
    predicates.extend(cursor_parts.predicates);
    let sql = format!(
        "{with_clause}SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_approval_policy, quote_state, application_id, created_at, updated_at
         FROM statuses
         WHERE {}
         ORDER BY created_at DESC, id DESC
         LIMIT ?{limit_binding}",
        predicates.join("\n               AND ")
    );
    (sql, bindings)
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

    result
        .results::<StatusRecord>()
        .and_then(statuses_from_records)
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
    if owner.username().eq_ignore_ascii_case(&username) {
        Ok(Some(status))
    } else {
        Ok(None)
    }
}
