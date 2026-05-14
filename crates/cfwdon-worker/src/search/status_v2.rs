use std::cmp::Reverse;

use super::status_store::{search_local_status_rows, search_remote_status_rows};
use super::statuses::{
    ParsedStatusSearchQuery, earlier_status_search_bound, later_status_search_bound,
    parse_status_search_query, status_is_searchable_by_scope, status_matches_search_metadata,
    status_matches_search_syntax, status_matches_search_timestamp, status_search_query_terms,
    status_search_rank, text_mentions_search_library_viewer,
};
use crate::{
    AccountReference, MastodonStatusResponse, build_local_status_response,
    build_remote_status_card_value, build_remote_status_response, build_status_card_value,
    can_view_local_status, find_account_by_id, find_account_by_username,
    find_media_attachments_by_status_id, find_remote_actor_by_username_domain,
    find_remote_status_attachments_by_status_id, find_remote_status_by_id,
    find_remote_status_poll_by_status_id, find_status_by_id, find_status_poll_by_status_id,
    is_local_status_bookmarked_by, is_local_status_favourited_by, is_local_status_reblogged_by,
    is_public_activitypub_visibility, is_remote_status_bookmarked_by,
    is_remote_status_favourited_by, is_remote_status_reblogged_by, load_in_reply_to_account_id,
    parse_lookup_handle, resolve_account_reference, strip_html_tags,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{AccountHandle, LocalAccount};
use worker::{D1Database, Result};

type SearchStatusSortKey = ((u8, u8, u8), Reverse<String>, Reverse<String>);
type SearchStatusEntry = (SearchStatusSortKey, MastodonStatusResponse);

fn status_search_self_reference(viewer: &LocalAccount, value: &str) -> Option<AccountReference> {
    value
        .eq_ignore_ascii_case("me")
        .then(|| AccountReference::Local(viewer.clone()))
}

async fn resolve_status_search_handle_reference(
    db: &D1Database,
    config: &AppConfig,
    handle: &AccountHandle,
) -> Result<Option<AccountReference>> {
    if handle.is_local_to(&config.instance_domain) {
        return Ok(find_account_by_username(db, &handle.username)
            .await?
            .map(AccountReference::Local));
    }

    Ok(find_remote_actor_by_username_domain(
        db,
        &handle.username,
        handle.domain.as_deref().unwrap_or_default(),
    )
    .await?
    .map(AccountReference::Remote))
}

async fn resolve_status_search_from_reference(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    value: &str,
) -> Result<Option<AccountReference>> {
    if let Some(reference) = status_search_self_reference(viewer, value) {
        return Ok(Some(reference));
    }

    let handle = match parse_lookup_handle(value, config) {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };

    resolve_status_search_handle_reference(db, config, &handle).await
}

fn account_reference_identity(reference: &AccountReference) -> &str {
    match reference {
        AccountReference::Local(account) => &account.id,
        AccountReference::Remote(actor) => &actor.actor_uri,
    }
}

fn merge_status_search_account_reference(
    current: Option<AccountReference>,
    syntax: Option<AccountReference>,
) -> Option<AccountReference> {
    match (current, syntax) {
        (Some(current), Some(syntax))
            if account_reference_identity(&current) == account_reference_identity(&syntax) =>
        {
            Some(current)
        }
        (Some(_), Some(_)) => None,
        (Some(current), None) => Some(current),
        (None, Some(syntax)) => Some(syntax),
        (None, None) => None,
    }
}

fn account_reference_matches_owner(
    reference: &AccountReference,
    local_account_id: Option<&str>,
    remote_actor_uri: Option<&str>,
) -> bool {
    match reference {
        AccountReference::Local(account) => Some(account.id.as_str()) == local_account_id,
        AccountReference::Remote(actor) => Some(actor.actor_uri.as_str()) == remote_actor_uri,
    }
}

async fn resolve_search_status_bound_timestamp(
    db: &D1Database,
    status_id: Option<&str>,
) -> Result<Option<String>> {
    let Some(status_id) = status_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if let Some(status) = find_status_by_id(db, status_id).await? {
        return Ok(Some(status.created_at));
    }
    if let Some(status) = find_remote_status_by_id(db, status_id).await? {
        return Ok(Some(status.published_at));
    }
    Ok(None)
}

fn search_status_query_limit(limit: u32, offset: u32) -> u32 {
    limit
        .saturating_add(offset)
        .saturating_mul(4)
        .min(200)
        .max(limit)
}

async fn local_status_is_in_search_library(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    status: &crate::StatusRow,
) -> Result<bool> {
    if status.account_id == viewer.id {
        return Ok(true);
    }
    if is_local_status_favourited_by(db, &viewer.id, status).await? {
        return Ok(true);
    }
    if is_local_status_bookmarked_by(db, &viewer.id, status).await? {
        return Ok(true);
    }
    if is_local_status_reblogged_by(db, &viewer.id, status).await? {
        return Ok(true);
    }
    if text_mentions_search_library_viewer(config, &status._text_content, &viewer.username) {
        return Ok(true);
    }
    Ok(false)
}

async fn remote_status_is_in_search_library(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    status: &crate::RemoteStatusRow,
) -> Result<bool> {
    if is_remote_status_favourited_by(db, &viewer.id, &status.id).await? {
        return Ok(true);
    }
    if is_remote_status_bookmarked_by(db, &viewer.id, &status.id).await? {
        return Ok(true);
    }
    if is_remote_status_reblogged_by(db, &viewer.id, &status.id).await? {
        return Ok(true);
    }
    if text_mentions_search_library_viewer(
        config,
        &strip_html_tags(&status.content_html),
        &viewer.username,
    ) {
        return Ok(true);
    }
    Ok(false)
}

async fn collect_local_search_status_entries(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    parsed_query: &ParsedStatusSearchQuery,
    account_reference: Option<&AccountReference>,
    excluded_account_reference: Option<&AccountReference>,
    search_terms: &[String],
    query_limit: u32,
    max_id: Option<&str>,
    max_timestamp: Option<&str>,
    min_id: Option<&str>,
    min_timestamp: Option<&str>,
) -> Result<Vec<SearchStatusEntry>> {
    let local_account_filter = match account_reference {
        Some(AccountReference::Local(account)) => Some(account.id.as_str()),
        _ => None,
    };
    let mut entries = Vec::new();

    for status in search_local_status_rows(
        db,
        search_terms,
        query_limit,
        local_account_filter,
        max_id,
        max_timestamp,
        min_id,
        min_timestamp,
    )
    .await?
    {
        let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
            continue;
        };
        if excluded_account_reference.is_some_and(|reference| {
            account_reference_matches_owner(reference, Some(&owner.id), None)
        }) {
            continue;
        }
        if !can_view_local_status(db, &status, Some(viewer), &owner).await? {
            continue;
        }
        let is_public = status.visibility == "public";
        let is_library = local_status_is_in_search_library(db, config, viewer, &status).await?;
        if !status_is_searchable_by_scope(parsed_query, is_public, is_library) {
            continue;
        }
        if !status_matches_search_syntax(
            parsed_query,
            &status._text_content,
            &status.spoiler_text,
            status.in_reply_to_id.is_some(),
            status.sensitive != 0,
            status.boost_of_uri.is_some(),
            status.quote_of_uri.is_some(),
            status.language.as_deref(),
        ) {
            continue;
        }
        if !status_matches_search_timestamp(parsed_query, &status.created_at) {
            continue;
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        if !status_matches_search_metadata(
            parsed_query,
            !media.is_empty(),
            find_status_poll_by_status_id(db, &status.id)
                .await?
                .is_some(),
            build_status_card_value(&status._text_content).is_some(),
        ) {
            continue;
        }
        let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
        entries.push((
            (
                status_search_rank(parsed_query, &status._text_content, &status.spoiler_text),
                Reverse(status.created_at.clone()),
                Reverse(status.id.clone()),
            ),
            build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &owner,
                in_reply_to_account_id,
                media,
            )
            .await?,
        ));
    }

    Ok(entries)
}

async fn collect_remote_search_status_entries(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    parsed_query: &ParsedStatusSearchQuery,
    account_reference: Option<&AccountReference>,
    excluded_account_reference: Option<&AccountReference>,
    search_terms: &[String],
    query_limit: u32,
    max_id: Option<&str>,
    max_timestamp: Option<&str>,
    min_id: Option<&str>,
    min_timestamp: Option<&str>,
) -> Result<Vec<SearchStatusEntry>> {
    let remote_actor_filter = match account_reference {
        Some(AccountReference::Remote(actor)) => Some(actor.actor_uri.as_str()),
        _ => None,
    };
    let mut entries = Vec::new();

    for (status, actor) in search_remote_status_rows(
        db,
        search_terms,
        query_limit,
        remote_actor_filter,
        max_id,
        max_timestamp,
        min_id,
        min_timestamp,
    )
    .await?
    {
        let is_public = is_public_activitypub_visibility(&status.visibility);
        if excluded_account_reference.is_some_and(|reference| {
            account_reference_matches_owner(reference, None, Some(&status.actor_uri))
        }) {
            continue;
        }
        let is_library = remote_status_is_in_search_library(db, config, viewer, &status).await?;
        if !status_is_searchable_by_scope(parsed_query, is_public, is_library) {
            continue;
        }
        let text_content = strip_html_tags(&status.content_html);
        if !status_matches_search_syntax(
            parsed_query,
            &text_content,
            &status.spoiler_text,
            status.in_reply_to_uri.is_some(),
            status.sensitive != 0,
            status.boost_of_uri.is_some(),
            status.quote_of_uri.is_some(),
            status.language.as_deref(),
        ) {
            continue;
        }
        if !status_matches_search_timestamp(parsed_query, &status.published_at) {
            continue;
        }
        let remote_attachments =
            find_remote_status_attachments_by_status_id(db, &status.id).await?;
        if !status_matches_search_metadata(
            parsed_query,
            !remote_attachments.is_empty(),
            find_remote_status_poll_by_status_id(db, &status.id)
                .await?
                .is_some(),
            build_remote_status_card_value(&text_content, &remote_attachments).is_some(),
        ) {
            continue;
        }
        entries.push((
            (
                status_search_rank(parsed_query, &text_content, &status.spoiler_text),
                Reverse(status.published_at.clone()),
                Reverse(status.id.clone()),
            ),
            build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
        ));
    }

    Ok(entries)
}

pub(crate) async fn search_statuses_for_v2(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &str,
    limit: u32,
    offset: u32,
    account_id: Option<&str>,
    max_id: Option<&str>,
    min_id: Option<&str>,
) -> Result<Vec<MastodonStatusResponse>> {
    let parsed_query = parse_status_search_query(query);
    if parsed_query.unsatisfiable {
        return Ok(Vec::new());
    }

    let account_reference = match account_id {
        Some(account_id) => resolve_account_reference(db, account_id).await?,
        None => None,
    };
    if account_id.is_some() && account_reference.is_none() {
        return Ok(Vec::new());
    }
    let syntax_account_reference = match parsed_query.from.as_deref() {
        Some(value) => resolve_status_search_from_reference(db, config, viewer, value).await?,
        None => None,
    };
    let excluded_account_reference = match parsed_query.not_from.as_deref() {
        Some(value) => resolve_status_search_from_reference(db, config, viewer, value).await?,
        None => None,
    };
    let account_reference =
        merge_status_search_account_reference(account_reference, syntax_account_reference);
    if parsed_query.from.is_some() && account_reference.is_none() {
        return Ok(Vec::new());
    }

    let query_limit = search_status_query_limit(limit, offset);
    let cursor_max_timestamp = resolve_search_status_bound_timestamp(db, max_id).await?;
    let cursor_min_timestamp = resolve_search_status_bound_timestamp(db, min_id).await?;
    let max_timestamp =
        earlier_status_search_bound(cursor_max_timestamp.clone(), parsed_query.before.clone());
    let min_timestamp =
        later_status_search_bound(cursor_min_timestamp.clone(), parsed_query.after.clone());
    let search_terms = status_search_query_terms(&parsed_query);
    let max_id = (max_timestamp == cursor_max_timestamp)
        .then_some(max_id)
        .flatten();
    let min_id = (min_timestamp == cursor_min_timestamp)
        .then_some(min_id)
        .flatten();
    let mut entries = Vec::new();

    if !matches!(
        account_reference.as_ref(),
        Some(AccountReference::Remote(_))
    ) {
        entries.extend(
            collect_local_search_status_entries(
                db,
                config,
                viewer,
                &parsed_query,
                account_reference.as_ref(),
                excluded_account_reference.as_ref(),
                &search_terms,
                query_limit,
                max_id,
                max_timestamp.as_deref(),
                min_id,
                min_timestamp.as_deref(),
            )
            .await?,
        );
    }

    if !matches!(account_reference.as_ref(), Some(AccountReference::Local(_))) {
        entries.extend(
            collect_remote_search_status_entries(
                db,
                config,
                viewer,
                &parsed_query,
                account_reference.as_ref(),
                excluded_account_reference.as_ref(),
                &search_terms,
                query_limit,
                max_id,
                max_timestamp.as_deref(),
                min_id,
                min_timestamp.as_deref(),
            )
            .await?,
        );
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries
        .into_iter()
        .skip(offset as usize)
        .map(|(_, value)| value)
        .take(limit as usize)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::search_status_query_limit;

    #[test]
    fn search_status_query_limit_oversamples_with_cap() {
        assert_eq!(search_status_query_limit(20, 0), 80);
        assert_eq!(search_status_query_limit(40, 20), 200);
        assert_eq!(search_status_query_limit(250, 0), 250);
    }
}
