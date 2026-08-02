use std::cmp::Reverse;

use super::status_store::{search_local_status_rows, search_remote_status_rows};
use super::statuses::{
    ParsedStatusSearchQuery, earlier_status_search_bound, later_status_search_bound,
    parse_status_search_query, status_is_searchable_by_scope, status_matches_search_metadata,
    status_matches_search_syntax, status_matches_search_timestamp, status_search_query_terms,
    status_search_rank, text_mentions_search_library_viewer,
};
use crate::{
    AccountReference, D1Database, MastodonStatusResponse,
    build_local_status_response_with_quote_count_preloads, build_remote_status_card_value,
    build_remote_status_response, build_status_card_value, can_view_local_status,
    find_account_by_id, find_account_by_username, find_media_attachments_by_status_id,
    find_media_attachments_by_status_ids, find_remote_actor_by_username_domain,
    find_remote_status_attachments_by_status_id, find_remote_status_by_id,
    find_remote_status_poll_by_status_id, find_status_by_id, find_status_poll_by_status_id,
    is_local_status_bookmarked_by, is_local_status_favourited_by, is_local_status_reblogged_by,
    is_public_activitypub_visibility, is_remote_status_bookmarked_by,
    is_remote_status_favourited_by, is_remote_status_reblogged_by, load_account_filter_matcher,
    load_in_reply_to_account_id, local_status_ap_id, parse_lookup_handle,
    preload_local_status_viewer_state, preload_mastodon_poll_responses,
    preload_status_applications, preload_status_counts, preload_status_quote_counts,
    resolve_account_reference, strip_html_tags,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{AccountHandle, LocalAccount};
use worker::Result;

type SearchStatusSortKey = ((u8, u8, u8), Reverse<String>, Reverse<String>);

enum SearchStatusCandidate {
    Local {
        status: crate::StatusRow,
        owner: LocalAccount,
        in_reply_to_account_id: Option<String>,
    },
    Remote {
        status: crate::RemoteStatusRow,
        actor: crate::RemoteActorRow,
    },
}

type SearchStatusEntry = (SearchStatusSortKey, SearchStatusCandidate);

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
        AccountReference::Local(account) => account.id(),
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
        AccountReference::Local(account) => Some(account.id()) == local_account_id,
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

fn search_status_query_limit(limit: u32, offset: u32, has_text_terms: bool) -> u32 {
    let needed = limit.saturating_add(offset).max(limit);
    if has_text_terms {
        needed.saturating_mul(4).min(200).max(needed)
    } else {
        // Filter-only queries (from:/has:/is:) keep SQL date order as the sort key, so
        // oversampling only multiplies expensive status hydration work.
        needed.min(200)
    }
}

fn status_search_needs_media_metadata(parsed_query: &ParsedStatusSearchQuery) -> bool {
    parsed_query.has_media.is_some()
}

fn status_search_needs_poll_metadata(parsed_query: &ParsedStatusSearchQuery) -> bool {
    parsed_query.has_poll.is_some()
}

fn status_search_needs_embed_metadata(parsed_query: &ParsedStatusSearchQuery) -> bool {
    parsed_query.has_embed.is_some()
}

async fn local_status_is_in_search_library(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    status: &crate::StatusRow,
) -> Result<bool> {
    if status.account_id == viewer.id() {
        return Ok(true);
    }
    if is_local_status_favourited_by(db, viewer.id(), status).await? {
        return Ok(true);
    }
    if is_local_status_bookmarked_by(db, viewer.id(), status).await? {
        return Ok(true);
    }
    if is_local_status_reblogged_by(db, viewer.id(), status).await? {
        return Ok(true);
    }
    if text_mentions_search_library_viewer(config, &status.text, viewer.username()) {
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
    if is_remote_status_favourited_by(db, viewer.id(), &status.id).await? {
        return Ok(true);
    }
    if is_remote_status_bookmarked_by(db, viewer.id(), &status.id).await? {
        return Ok(true);
    }
    if is_remote_status_reblogged_by(db, viewer.id(), &status.id).await? {
        return Ok(true);
    }
    if text_mentions_search_library_viewer(
        config,
        &strip_html_tags(&status.content_html),
        viewer.username(),
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
        Some(AccountReference::Local(account)) => Some(account.id()),
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
            account_reference_matches_owner(reference, Some(owner.id()), None)
        }) {
            continue;
        }
        if !can_view_local_status(db, &status, Some(viewer), &owner).await? {
            continue;
        }
        let is_public = status.visibility == cfwdon_domain::Visibility::Public;
        let is_library = if is_public && parsed_query.in_library.is_none() {
            false
        } else {
            local_status_is_in_search_library(db, config, viewer, &status).await?
        };
        if !status_is_searchable_by_scope(parsed_query, is_public, is_library) {
            continue;
        }
        if !status_matches_search_syntax(
            parsed_query,
            &status.text,
            &status.spoiler_text,
            status.in_reply_to_id.is_some(),
            status.sensitive,
            status.boost_of_uri.is_some(),
            status.quote_of_uri.is_some(),
            status.language.as_deref(),
        ) {
            continue;
        }
        if !status_matches_search_timestamp(parsed_query, &status.created_at) {
            continue;
        }
        if status_search_needs_media_metadata(parsed_query)
            || status_search_needs_poll_metadata(parsed_query)
            || status_search_needs_embed_metadata(parsed_query)
        {
            let media = if status_search_needs_media_metadata(parsed_query) {
                find_media_attachments_by_status_id(db, &status.id).await?
            } else {
                Vec::new()
            };
            if !status_matches_search_metadata(
                parsed_query,
                !media.is_empty(),
                if status_search_needs_poll_metadata(parsed_query) {
                    find_status_poll_by_status_id(db, &status.id)
                        .await?
                        .is_some()
                } else {
                    false
                },
                if status_search_needs_embed_metadata(parsed_query) {
                    build_status_card_value(&status.text).is_some()
                } else {
                    false
                },
            ) {
                continue;
            }
        }
        let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
        entries.push((
            (
                status_search_rank(parsed_query, &status.text, &status.spoiler_text),
                Reverse(status.created_at.clone()),
                Reverse(status.id.clone()),
            ),
            SearchStatusCandidate::Local {
                status,
                owner,
                in_reply_to_account_id,
            },
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
        let is_public = is_public_activitypub_visibility(status.visibility.as_str());
        if excluded_account_reference.is_some_and(|reference| {
            account_reference_matches_owner(reference, None, Some(&status.actor_uri))
        }) {
            continue;
        }
        let is_library = if is_public && parsed_query.in_library.is_none() {
            false
        } else {
            remote_status_is_in_search_library(db, config, viewer, &status).await?
        };
        if !status_is_searchable_by_scope(parsed_query, is_public, is_library) {
            continue;
        }
        let text_content = strip_html_tags(&status.content_html);
        if !status_matches_search_syntax(
            parsed_query,
            &text_content,
            &status.spoiler_text,
            status.in_reply_to_uri.is_some(),
            status.sensitive,
            status.boost_of_uri.is_some(),
            status.quote_of_uri.is_some(),
            status.language.as_deref(),
        ) {
            continue;
        }
        if !status_matches_search_timestamp(parsed_query, &status.published_at) {
            continue;
        }
        let remote_attachments = if status_search_needs_media_metadata(parsed_query)
            || status_search_needs_embed_metadata(parsed_query)
        {
            find_remote_status_attachments_by_status_id(db, &status.id).await?
        } else {
            Vec::new()
        };
        if !status_matches_search_metadata(
            parsed_query,
            !remote_attachments.is_empty(),
            if status_search_needs_poll_metadata(parsed_query) {
                find_remote_status_poll_by_status_id(db, &status.id)
                    .await?
                    .is_some()
            } else {
                false
            },
            if status_search_needs_embed_metadata(parsed_query) {
                build_remote_status_card_value(&text_content, &remote_attachments).is_some()
            } else {
                false
            },
        ) {
            continue;
        }
        entries.push((
            (
                status_search_rank(parsed_query, &text_content, &status.spoiler_text),
                Reverse(status.published_at.clone()),
                Reverse(status.id.clone()),
            ),
            SearchStatusCandidate::Remote { status, actor },
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

    let search_terms = status_search_query_terms(&parsed_query);
    let query_limit = search_status_query_limit(limit, offset, !search_terms.is_empty());
    let cursor_max_timestamp = resolve_search_status_bound_timestamp(db, max_id).await?;
    let cursor_min_timestamp = resolve_search_status_bound_timestamp(db, min_id).await?;
    let max_timestamp =
        earlier_status_search_bound(cursor_max_timestamp.clone(), parsed_query.before.clone());
    let min_timestamp =
        later_status_search_bound(cursor_min_timestamp.clone(), parsed_query.after.clone());
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
    let selected = entries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|(_, candidate)| candidate)
        .collect::<Vec<_>>();
    build_search_status_candidates(db, config, viewer, selected).await
}

async fn build_search_status_candidates(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    candidates: Vec<SearchStatusCandidate>,
) -> Result<Vec<MastodonStatusResponse>> {
    let local_statuses = candidates
        .iter()
        .filter_map(|candidate| match candidate {
            SearchStatusCandidate::Local { status, owner, .. } => Some((status, owner)),
            SearchStatusCandidate::Remote { .. } => None,
        })
        .collect::<Vec<_>>();
    let local_status_ids = local_statuses
        .iter()
        .map(|(status, _)| status.id.clone())
        .collect::<Vec<_>>();
    let local_status_refs = local_statuses
        .iter()
        .map(|(status, _)| *status)
        .collect::<Vec<_>>();
    let quote_uris = local_statuses
        .iter()
        .map(|(status, owner)| local_status_ap_id(config, owner, status))
        .collect::<Vec<_>>();

    let (
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        mut media_by_status_id,
        filter_matcher,
    ) = futures_util::try_join!(
        preload_status_counts(db, &local_status_ids, &[]),
        preload_status_quote_counts(db, &quote_uris),
        preload_mastodon_poll_responses(db, &local_status_ids, Some(viewer)),
        preload_local_status_viewer_state(db, viewer.id(), &local_status_refs, None),
        preload_status_applications(db, config, &local_status_refs),
        find_media_attachments_by_status_ids(db, &local_status_ids),
        load_account_filter_matcher(db, viewer.id()),
    )?;

    let mut responses = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match candidate {
            SearchStatusCandidate::Local {
                status,
                owner,
                in_reply_to_account_id,
            } => {
                let media = media_by_status_id.remove(&status.id).unwrap_or_default();
                responses.push(
                    build_local_status_response_with_quote_count_preloads(
                        db,
                        config,
                        Some(viewer),
                        &status,
                        &owner,
                        in_reply_to_account_id,
                        media,
                        Some(&filter_matcher),
                        Some(&counts_preload),
                        Some(&quote_counts_preload),
                        Some(&poll_preload),
                        Some(&viewer_state_preload),
                        Some(&application_preload),
                    )
                    .await?,
                );
            }
            SearchStatusCandidate::Remote { status, actor } => {
                responses.push(
                    build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
                );
            }
        }
    }
    Ok(responses)
}

#[cfg(test)]
mod tests {
    use super::search_status_query_limit;

    #[test]
    fn search_status_query_limit_oversamples_text_queries_only() {
        assert_eq!(search_status_query_limit(20, 0, true), 80);
        assert_eq!(search_status_query_limit(40, 20, true), 200);
        assert_eq!(search_status_query_limit(250, 0, true), 250);
        assert_eq!(search_status_query_limit(20, 0, false), 20);
        assert_eq!(search_status_query_limit(40, 20, false), 60);
    }
}
