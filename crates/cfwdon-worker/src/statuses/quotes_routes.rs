use super::{
    Request, Response, Result, RouteContext, build_accept_quote_request_activity,
    build_create_quote_authorization_activity, build_delete_quote_authorization_activity,
    build_local_status_response, build_local_status_response_with_quote_count_preloads,
    build_quote_request_object, build_reject_quote_request_activity, build_remote_status_response,
    build_remote_status_response_with_timeline_preloads, build_timeline_link_header,
    can_view_local_status, clear_local_status_quote, clear_remote_status_quote,
    enqueue_status_update_activity, enqueue_targeted_outbox_activity, find_account_by_id,
    find_accounts_by_ids, find_authenticated_local_account, find_media_attachments_by_status_id,
    find_media_attachments_by_status_ids, find_remote_actor_by_actor_uri,
    find_remote_actors_by_actor_uris, find_remote_status_attachments_by_status_ids,
    find_status_by_id, insert_status_edit_snapshot, is_public_activitypub_visibility,
    list_follower_delivery_targets, load_config, load_in_reply_to_account_id,
    load_in_reply_to_account_ids, local_quote_revoke_allowed, local_status_target_uri,
    normalize_status_history_entry, now_iso_string, preload_local_status_viewer_state,
    preload_mastodon_poll_responses, preload_remote_mastodon_poll_responses,
    preload_remote_status_edit_updated_at, preload_remote_status_viewer_state,
    preload_status_applications, preload_status_counts, preload_status_quote_counts,
    queue_remote_actor_activity, quote_authorization_uri, quote_request_uri,
    resolve_status_reference, resolve_timeline_cursor, timeline_fetch_limit, timeline_limit,
    update_local_status_quote_state, update_remote_status_quote_state,
};
use crate::timelines::TimelinePaginationQuery;
use crate::{
    append_resolved_timeline_cursor_bindings, seekable_resolved_timeline_cursor_predicates,
};
use cfwdon_domain::{OwnerQuoteAction, QuoteState};
use serde::Deserialize;
use std::collections::HashSet;
use worker::d1::D1Type;

#[derive(Debug, Default, Deserialize)]
struct QuotesQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

impl QuotesQuery {
    fn pagination(&self) -> TimelinePaginationQuery {
        TimelinePaginationQuery {
            limit: self.limit,
            max_id: self.max_id.clone(),
            since_id: self.since_id.clone(),
            min_id: self.min_id.clone(),
        }
    }
}

struct StatusQuotesPreloads {
    local_accounts_by_id: std::collections::HashMap<String, crate::LocalAccount>,
    local_media_by_status_id: std::collections::HashMap<String, Vec<crate::MediaAttachmentRow>>,
    local_in_reply_to_account_ids: std::collections::HashMap<String, String>,
    counts_preload: crate::StatusCountsPreload,
    quote_counts_preload: crate::StatusQuoteCountsPreload,
    local_poll_preload: crate::MastodonPollResponsePreload,
    local_viewer_state_preload: crate::LocalStatusViewerStatePreload,
    application_preload: crate::StatusApplicationPreload,
    remote_actors_by_uri: std::collections::HashMap<String, crate::RemoteActorRow>,
    remote_attachments_by_status_id:
        std::collections::HashMap<String, Vec<crate::RemoteStatusAttachmentRow>>,
    remote_poll_preload: crate::RemoteMastodonPollResponsePreload,
    remote_edit_updated_at_preload: crate::RemoteStatusEditUpdatedAtPreload,
    remote_viewer_state_preload: crate::RemoteStatusViewerStatePreload,
}

pub(crate) async fn status_quotes_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: QuotesQuery = req.query().unwrap_or_default();
    let pagination = query.pagination();
    let limit = timeline_limit(&pagination);
    let query_limit = timeline_fetch_limit(limit);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let cursor = resolve_timeline_cursor(&db, &pagination).await?;

    let Some(status_id) = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing status id route parameter", 400);
    };

    let Some(target_uri) =
        resolve_visible_status_quotes_target(&db, &config, &status_id, viewer.as_ref()).await?
    else {
        return Response::error("status not found", 404);
    };

    let (local_quotes, remote_quotes) =
        load_accepted_status_quotes(&db, &target_uri, &cursor, query_limit).await?;
    let mut preloads =
        preload_status_quotes(&db, &config, viewer.as_ref(), &local_quotes, &remote_quotes).await?;
    let quotes = build_status_quote_values(
        &db,
        &config,
        viewer.as_ref(),
        local_quotes,
        remote_quotes,
        &mut preloads,
    )
    .await?;
    paginated_status_quotes_response(&req, limit, quotes)
}

async fn resolve_visible_status_quotes_target(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    status_id: &str,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<Option<String>> {
    let Some(status) = resolve_status_reference(db, config, status_id).await? else {
        return Ok(None);
    };

    match status {
        crate::ResolvedStatus::Local(status) => {
            let Some(account) = find_account_by_id(db, &status.account_id).await? else {
                return Ok(None);
            };
            if !can_view_local_status(db, &status, viewer, &account).await? {
                return Ok(None);
            }
            Ok(Some(local_status_target_uri(&status)))
        }
        crate::ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Ok(None);
            }
            Ok(Some(status.object_uri))
        }
    }
}

async fn load_accepted_status_quotes(
    db: &crate::D1Database,
    target_uri: &str,
    cursor: &crate::ResolvedTimelineCursor,
    query_limit: u32,
) -> Result<(Vec<crate::StatusRow>, Vec<crate::RemoteStatusRow>)> {
    let local_quotes = list_local_status_quotes_by_uri(db, target_uri, cursor, query_limit).await?;
    let remote_quotes =
        list_remote_status_quotes_by_uri(db, target_uri, cursor, query_limit).await?;
    Ok((local_quotes, remote_quotes))
}

async fn preload_status_quotes(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    local_quotes: &[crate::StatusRow],
    remote_quotes: &[crate::RemoteStatusRow],
) -> Result<StatusQuotesPreloads> {
    let local_status_ids = local_quotes
        .iter()
        .map(|quote| quote.id.clone())
        .collect::<Vec<_>>();
    let local_account_ids = local_quotes
        .iter()
        .map(|quote| quote.account_id.clone())
        .collect::<Vec<_>>();
    let remote_status_ids = remote_quotes
        .iter()
        .map(|quote| quote.id.clone())
        .collect::<Vec<_>>();
    let remote_actor_uris = remote_quotes
        .iter()
        .map(|quote| quote.actor_uri.clone())
        .collect::<Vec<_>>();
    let quote_uris = local_quotes
        .iter()
        .map(local_status_target_uri)
        .chain(remote_quotes.iter().map(|quote| quote.object_uri.clone()))
        .collect::<Vec<_>>();
    let local_quote_refs = local_quotes.iter().collect::<Vec<_>>();

    let (
        local_accounts_by_id,
        local_media_by_status_id,
        local_in_reply_to_account_ids,
        counts_preload,
        quote_counts_preload,
        local_poll_preload,
        local_viewer_state_preload,
        application_preload,
        remote_actors_by_uri,
        remote_attachments_by_status_id,
        remote_poll_preload,
        remote_edit_updated_at_preload,
    ) = futures_util::try_join!(
        find_accounts_by_ids(db, &local_account_ids),
        find_media_attachments_by_status_ids(db, &local_status_ids),
        load_in_reply_to_account_ids(db, local_quotes),
        preload_status_counts(db, &local_status_ids, &remote_status_ids),
        preload_status_quote_counts(db, &quote_uris),
        preload_mastodon_poll_responses(db, &local_status_ids, viewer),
        async {
            match viewer {
                Some(viewer) => {
                    preload_local_status_viewer_state(db, viewer.id(), &local_quote_refs, None)
                        .await
                }
                None => Ok(Default::default()),
            }
        },
        preload_status_applications(db, config, &local_quote_refs),
        find_remote_actors_by_actor_uris(db, &remote_actor_uris),
        find_remote_status_attachments_by_status_ids(db, &remote_status_ids),
        preload_remote_mastodon_poll_responses(db, &remote_status_ids, viewer),
        preload_remote_status_edit_updated_at(db, &remote_status_ids),
    )?;

    let remote_quote_refs = remote_quotes
        .iter()
        .filter_map(|quote| {
            remote_actors_by_uri
                .get(&quote.actor_uri)
                .map(|actor| (quote, actor))
        })
        .collect::<Vec<_>>();
    let remote_viewer_state_preload = match viewer {
        Some(viewer) => {
            preload_remote_status_viewer_state(db, viewer.id(), &remote_quote_refs).await?
        }
        None => Default::default(),
    };

    Ok(StatusQuotesPreloads {
        local_accounts_by_id,
        local_media_by_status_id,
        local_in_reply_to_account_ids,
        counts_preload,
        quote_counts_preload,
        local_poll_preload,
        local_viewer_state_preload,
        application_preload,
        remote_actors_by_uri,
        remote_attachments_by_status_id,
        remote_poll_preload,
        remote_edit_updated_at_preload,
        remote_viewer_state_preload,
    })
}

async fn build_status_quote_values(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    local_quotes: Vec<crate::StatusRow>,
    remote_quotes: Vec<crate::RemoteStatusRow>,
    preloads: &mut StatusQuotesPreloads,
) -> Result<Vec<(String, String, serde_json::Value)>> {
    let mut quotes: Vec<(String, String, serde_json::Value)> = Vec::new();

    for quote in local_quotes {
        let Some(account) = preloads.local_accounts_by_id.get(&quote.account_id) else {
            continue;
        };
        if !can_view_local_status(db, &quote, viewer, account).await? {
            continue;
        }
        let media = preloads
            .local_media_by_status_id
            .remove(&quote.id)
            .unwrap_or_default();
        quotes.push((
            quote.created_at.clone(),
            quote.id.clone(),
            serde_json::to_value(
                build_local_status_response_with_quote_count_preloads(
                    db,
                    config,
                    viewer,
                    &quote,
                    account,
                    preloads
                        .local_in_reply_to_account_ids
                        .get(&quote.id)
                        .cloned(),
                    media,
                    None,
                    Some(&preloads.counts_preload),
                    Some(&preloads.quote_counts_preload),
                    Some(&preloads.local_poll_preload),
                    Some(&preloads.local_viewer_state_preload),
                    Some(&preloads.application_preload),
                )
                .await?,
            )?,
        ));
    }

    for quote in remote_quotes {
        if !is_public_activitypub_visibility(quote.visibility.as_str()) {
            continue;
        }
        let Some(actor) = preloads.remote_actors_by_uri.get(&quote.actor_uri) else {
            continue;
        };
        quotes.push((
            quote.published_at.clone(),
            quote.id.clone(),
            serde_json::to_value(
                build_remote_status_response_with_timeline_preloads(
                    db,
                    config,
                    viewer,
                    &quote,
                    actor,
                    None,
                    Some(&preloads.counts_preload),
                    Some(&preloads.quote_counts_preload),
                    Some(&preloads.remote_viewer_state_preload),
                    Some(&preloads.remote_poll_preload),
                    Some(&preloads.remote_edit_updated_at_preload),
                    None,
                    preloads
                        .remote_attachments_by_status_id
                        .remove(&quote.id)
                        .unwrap_or_default(),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .await?,
            )?,
        ));
    }

    sort_status_quote_entries(&mut quotes);
    Ok(quotes)
}

fn paginated_status_quotes_response(
    req: &Request,
    limit: u32,
    quotes: Vec<(String, String, serde_json::Value)>,
) -> Result<Response> {
    let first_id = quotes
        .first()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let last_id = quotes
        .last()
        .and_then(|(_, id, _)| (!id.is_empty()).then_some(id.clone()));
    let values = quotes
        .into_iter()
        .take(limit as usize)
        .map(|(_, _, value)| value)
        .collect::<Vec<_>>();
    let mut builder = Response::from_json(&values)?;
    if let Some(link) =
        build_timeline_link_header(req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link)?;
    }
    Ok(builder)
}

async fn list_local_status_quotes_by_uri(
    db: &crate::D1Database,
    status_uri: &str,
    cursor: &crate::ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<crate::StatusRow>> {
    let (sql, bindings) = status_quotes_list_sql(
        "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE quote_of_uri = ?1
               AND quote_state = 'accepted'",
        "created_at",
        "id",
        status_uri,
        cursor,
        limit,
    );
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    crate::d1_results::<crate::StatusRecord>(&result).and_then(crate::statuses_from_records)
}

async fn list_remote_status_quotes_by_uri(
    db: &crate::D1Database,
    status_uri: &str,
    cursor: &crate::ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<crate::RemoteStatusRow>> {
    let (sql, bindings) = status_quotes_list_sql(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE quote_of_uri = ?1
               AND quote_state = 'accepted'",
        "published_at",
        "id",
        status_uri,
        cursor,
        limit,
    );
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
    crate::d1_results::<crate::RemoteStatusRecord>(&result)
        .and_then(crate::remote_statuses_from_records)
}

fn status_quotes_list_sql<'a>(
    select_from_where: &str,
    timestamp_column: &str,
    id_column: &str,
    status_uri: &'a str,
    cursor: &'a crate::ResolvedTimelineCursor,
    limit: u32,
) -> (String, Vec<D1Type<'a>>) {
    let mut bindings = vec![D1Type::Text(status_uri)];
    let slots = append_resolved_timeline_cursor_bindings(&mut bindings, cursor);
    bindings.push(D1Type::Integer(limit as i32));
    let limit_slot = bindings.len();
    let cursor_predicates =
        seekable_resolved_timeline_cursor_predicates(timestamp_column, id_column, &slots);
    let sql = format!(
        "{select_from_where}{cursor_predicates}
             ORDER BY {timestamp_column} DESC, {id_column} DESC
             LIMIT ?{limit_slot}"
    );
    (sql, bindings)
}

#[cfg(test)]
mod tests {
    use super::{sort_status_quote_entries, status_quotes_list_sql};
    use crate::ResolvedTimelineCursor;
    use worker::d1::D1Type;

    #[test]
    fn status_quotes_list_sql_emits_seekable_cursor_bounds() {
        let cursor = ResolvedTimelineCursor {
            max_timestamp: Some("2026-01-02T00:00:00Z".to_owned()),
            max_id: Some("quote-max".to_owned()),
            min_timestamp: None,
            min_id: None,
        };
        let (sql, bindings) = status_quotes_list_sql(
            "SELECT id FROM statuses WHERE quote_of_uri = ?1",
            "created_at",
            "id",
            "https://example.com/statuses/1",
            &cursor,
            20,
        );

        assert!(sql.contains("created_at <= ?2"));
        assert!(sql.contains("(created_at < ?2 OR id < ?3)"));
        assert!(sql.contains("LIMIT ?4"));
        assert!(!sql.contains("?2 IS NULL"));
        assert!(matches!(
            bindings[0],
            D1Type::Text("https://example.com/statuses/1")
        ));
        assert!(matches!(bindings[3], D1Type::Integer(20)));
    }

    #[test]
    fn status_quote_entries_sort_newest_first_then_id() {
        let mut quotes = vec![
            ("2025-01-01T00:00:00Z".to_owned(), "b".to_owned(), ()),
            ("2025-01-02T00:00:00Z".to_owned(), "a".to_owned(), ()),
            ("2025-01-02T00:00:00Z".to_owned(), "c".to_owned(), ()),
        ];
        sort_status_quote_entries(&mut quotes);
        assert_eq!(
            quotes
                .iter()
                .map(|(created_at, id, _)| (created_at.as_str(), id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("2025-01-02T00:00:00Z", "c"),
                ("2025-01-02T00:00:00Z", "a"),
                ("2025-01-01T00:00:00Z", "b"),
            ]
        );
    }
}

fn sort_status_quote_entries<T>(quotes: &mut [(String, String, T)]) {
    quotes.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
}

async fn enqueue_quote_revocation_federation(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    target_status_id: &str,
    target_uri: &str,
    interacting_object_uri: &str,
    authorization_key: &str,
    follower_inboxes: &[String],
    remote_quote_author_actor_uri: Option<&str>,
) -> Result<()> {
    let payload = build_delete_quote_authorization_activity(
        config,
        requester,
        interacting_object_uri,
        target_uri,
        authorization_key,
    )?;

    let mut unique_follower_inboxes = Vec::new();
    let mut seen = HashSet::new();
    for inbox in follower_inboxes {
        let inbox = inbox.trim();
        if !inbox.is_empty() && seen.insert(inbox.to_owned()) {
            unique_follower_inboxes.push(inbox.to_owned());
        }
    }
    if !unique_follower_inboxes.is_empty() {
        enqueue_targeted_outbox_activity(
            db,
            requester.id(),
            Some(target_status_id),
            &payload,
            &unique_follower_inboxes,
        )
        .await?;
    }
    if let Some(actor_uri) = remote_quote_author_actor_uri {
        let _ = queue_remote_actor_activity(db, requester.id(), actor_uri, &payload).await?;
    }

    Ok(())
}

async fn enqueue_quote_approval_federation(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    target_status_id: &str,
    target_uri: &str,
    interacting_object_uri: &str,
    authorization_key: &str,
    follower_inboxes: &[String],
    remote_quote_author_actor_uri: Option<&str>,
) -> Result<()> {
    let authorization_uri = quote_authorization_uri(target_uri, authorization_key);
    let create_payload = build_create_quote_authorization_activity(
        config,
        requester,
        interacting_object_uri,
        target_uri,
        authorization_key,
    )?;

    let mut unique_follower_inboxes = Vec::new();
    let mut seen = HashSet::new();
    for inbox in follower_inboxes {
        let inbox = inbox.trim();
        if !inbox.is_empty() && seen.insert(inbox.to_owned()) {
            unique_follower_inboxes.push(inbox.to_owned());
        }
    }
    if !unique_follower_inboxes.is_empty() {
        enqueue_targeted_outbox_activity(
            db,
            requester.id(),
            Some(target_status_id),
            &create_payload,
            &unique_follower_inboxes,
        )
        .await?;
    }

    if let Some(remote_actor_uri) = remote_quote_author_actor_uri {
        let quote_request = build_quote_request_object(
            &quote_request_uri(interacting_object_uri, authorization_key),
            remote_actor_uri,
            target_uri,
            interacting_object_uri,
        );
        let accept_payload = build_accept_quote_request_activity(
            config,
            requester,
            &quote_request,
            &authorization_uri,
            remote_actor_uri,
        )?;
        let _ = queue_remote_actor_activity(db, requester.id(), remote_actor_uri, &accept_payload)
            .await?;
    }

    Ok(())
}

async fn enqueue_quote_rejection_federation(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    target_uri: &str,
    interacting_object_uri: &str,
    authorization_key: &str,
    remote_quote_author_actor_uri: &str,
) -> Result<()> {
    let quote_request = build_quote_request_object(
        &quote_request_uri(interacting_object_uri, authorization_key),
        remote_quote_author_actor_uri,
        target_uri,
        interacting_object_uri,
    );
    let reject_payload = build_reject_quote_request_activity(
        config,
        requester,
        &quote_request,
        remote_quote_author_actor_uri,
    )?;
    let _ = queue_remote_actor_activity(
        db,
        requester.id(),
        remote_quote_author_actor_uri,
        &reject_payload,
    )
    .await?;

    Ok(())
}

async fn enqueue_quote_owner_decision_federation(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    action: OwnerQuoteAction,
    target_status_id: &str,
    target_uri: &str,
    interacting_object_uri: &str,
    authorization_key: &str,
    local_quote_author: Option<&cfwdon_domain::LocalAccount>,
    remote_quote_author_actor_uri: Option<&str>,
) -> Result<()> {
    match action {
        OwnerQuoteAction::Approve => {
            let follower_inboxes = list_follower_delivery_targets(db, requester.id()).await?;
            enqueue_quote_approval_federation(
                db,
                config,
                requester,
                target_status_id,
                target_uri,
                interacting_object_uri,
                authorization_key,
                &follower_inboxes,
                remote_quote_author_actor_uri,
            )
            .await?;
            if let Some(quote_author) = local_quote_author {
                let Some(updated_quote) = find_status_by_id(db, authorization_key).await? else {
                    return Ok(());
                };
                enqueue_status_update_activity(db, config, quote_author, &updated_quote).await?;
            }
        }
        OwnerQuoteAction::Reject => {
            if let Some(remote_actor_uri) = remote_quote_author_actor_uri {
                enqueue_quote_rejection_federation(
                    db,
                    config,
                    requester,
                    target_uri,
                    interacting_object_uri,
                    authorization_key,
                    remote_actor_uri,
                )
                .await?;
            } else if let Some(quote_author) = local_quote_author {
                let Some(updated_quote) = find_status_by_id(db, authorization_key).await? else {
                    return Ok(());
                };
                enqueue_status_update_activity(db, config, quote_author, &updated_quote).await?;
            }
        }
        OwnerQuoteAction::Revoke => {}
    }

    Ok(())
}

pub(crate) async fn approve_quote_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    quote_owner_action_response(req, ctx, OwnerQuoteAction::Approve).await
}

pub(crate) async fn reject_quote_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    quote_owner_action_response(req, ctx, OwnerQuoteAction::Reject).await
}

async fn resolve_owned_local_quote_target(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    target_status_id: &str,
) -> Result<Option<(String, String)>> {
    let Some(target_status) = resolve_status_reference(db, config, target_status_id).await? else {
        return Ok(None);
    };
    match target_status {
        crate::ResolvedStatus::Local(status) => {
            let Some(account) = find_account_by_id(db, &status.account_id).await? else {
                return Ok(None);
            };
            if status.account_id != requester.id()
                || !can_view_local_status(db, &status, Some(requester), &account).await?
            {
                return Ok(None);
            }
            Ok(Some((local_status_target_uri(&status), status.id)))
        }
        crate::ResolvedStatus::Remote(_) => Ok(None),
    }
}

async fn apply_owner_action_to_local_quote(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    action: OwnerQuoteAction,
    target_status_id: &str,
    target_uri: &str,
    quote_status: crate::StatusRow,
) -> Result<Response> {
    if quote_status.quote_of_uri.as_deref() != Some(target_uri)
        || quote_status.effective_quote_state() != QuoteState::Pending
    {
        return Response::error("status not found", 404);
    }
    let Some(quote_author) = find_account_by_id(db, &quote_status.account_id).await? else {
        return Response::error("status not found", 404);
    };
    let updated_at = now_iso_string()?;
    let updated_status = match action {
        OwnerQuoteAction::Revoke => {
            clear_local_status_quote(db, &quote_status, &updated_at).await?
        }
        OwnerQuoteAction::Approve | OwnerQuoteAction::Reject => {
            update_local_status_quote_state(
                db,
                &quote_status,
                quote_status
                    .quote_state
                    .quote_state_after_owner_action(action),
                &updated_at,
            )
            .await?
        }
    };
    enqueue_quote_owner_decision_federation(
        db,
        config,
        requester,
        action,
        target_status_id,
        target_uri,
        &local_status_target_uri(&updated_status),
        &updated_status.id,
        Some(&quote_author),
        None,
    )
    .await?;
    let media = find_media_attachments_by_status_id(db, &updated_status.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(db, &updated_status).await?;
    let response = build_local_status_response(
        db,
        config,
        Some(requester),
        &updated_status,
        &quote_author,
        in_reply_to_account_id,
        media,
    )
    .await?;
    Response::from_json(&response)
}

async fn apply_owner_action_to_remote_quote(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &cfwdon_domain::LocalAccount,
    action: OwnerQuoteAction,
    target_status_id: &str,
    target_uri: &str,
    quote_status: crate::RemoteStatusRow,
) -> Result<Response> {
    if quote_status.quote_of_uri.as_deref() != Some(target_uri)
        || quote_status.effective_quote_state() != QuoteState::Pending
    {
        return Response::error("status not found", 404);
    }
    let Some(quote_author) = find_remote_actor_by_actor_uri(db, &quote_status.actor_uri).await?
    else {
        return Response::error("status not found", 404);
    };
    let updated_status = match action {
        OwnerQuoteAction::Revoke => clear_remote_status_quote(db, &quote_status).await?,
        OwnerQuoteAction::Reject | OwnerQuoteAction::Approve => {
            update_remote_status_quote_state(
                db,
                &quote_status.id,
                quote_status
                    .quote_state
                    .quote_state_after_owner_action(action),
            )
            .await?
        }
    };
    enqueue_quote_owner_decision_federation(
        db,
        config,
        requester,
        action,
        target_status_id,
        target_uri,
        &quote_status.object_uri,
        &quote_status.id,
        None,
        Some(&quote_author.actor_uri),
    )
    .await?;
    let response =
        build_remote_status_response(db, config, Some(requester), &updated_status, &quote_author)
            .await?;
    Response::from_json(&response)
}

async fn quote_owner_action_response(
    req: Request,
    ctx: RouteContext<()>,
    action: OwnerQuoteAction,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let Some(target_status_id) = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(quote_status_id) = ctx
        .param("quote_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing quote status id route parameter", 400);
    };

    let Some((target_uri, target_status_id)) =
        resolve_owned_local_quote_target(&db, &config, &requester, &target_status_id).await?
    else {
        return Response::error("status not found", 404);
    };

    match resolve_status_reference(&db, &config, &quote_status_id).await? {
        Some(crate::ResolvedStatus::Local(quote_status)) => {
            apply_owner_action_to_local_quote(
                &db,
                &config,
                &requester,
                action,
                &target_status_id,
                &target_uri,
                quote_status,
            )
            .await
        }
        Some(crate::ResolvedStatus::Remote(quote_status)) => {
            apply_owner_action_to_remote_quote(
                &db,
                &config,
                &requester,
                action,
                &target_status_id,
                &target_uri,
                quote_status,
            )
            .await
        }
        None => Response::error("status not found", 404),
    }
}

pub(crate) async fn revoke_quote_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let Some(target_status_id) = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(quote_status_id) = ctx
        .param("quote_id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Response::error("missing quote status id route parameter", 400);
    };

    let Some(target_status) = resolve_status_reference(&db, &config, &target_status_id).await?
    else {
        return Response::error("status not found", 404);
    };
    let (target_status_id, target_uri) = match &target_status {
        crate::ResolvedStatus::Local(status) => {
            (status.id.clone(), local_status_target_uri(status))
        }
        crate::ResolvedStatus::Remote(status) => (status.id.clone(), status.object_uri.clone()),
    };

    match resolve_status_reference(&db, &config, &quote_status_id).await? {
        Some(crate::ResolvedStatus::Local(quote_status)) => {
            if !local_quote_revoke_allowed(requester.id(), &quote_status, target_uri.as_str()) {
                return Response::error("status not found", 404);
            }
            let Some(quote_author) = find_account_by_id(&db, &quote_status.account_id).await?
            else {
                return Response::error("status not found", 404);
            };
            let quote_revocation_targets =
                list_follower_delivery_targets(&db, quote_author.id()).await?;

            let current_media = find_media_attachments_by_status_id(&db, &quote_status.id).await?;
            let previous_in_reply_to_account_id =
                load_in_reply_to_account_id(&db, &quote_status).await?;
            let previous_response = build_local_status_response(
                &db,
                &config,
                Some(&requester),
                &quote_status,
                &quote_author,
                previous_in_reply_to_account_id,
                current_media.clone(),
            )
            .await?;
            let mut previous_snapshot =
                serde_json::to_value(previous_response).unwrap_or_else(|_| serde_json::json!({}));
            let revision_at = now_iso_string()?;
            previous_snapshot["created_at"] = serde_json::json!(revision_at.clone());
            let previous_snapshot = normalize_status_history_entry(previous_snapshot);
            let previous_snapshot_json =
                serde_json::to_string(&previous_snapshot).map_err(|error| {
                    worker::Error::RustError(format!(
                        "failed to serialize status snapshot: {error}"
                    ))
                })?;
            insert_status_edit_snapshot(
                &db,
                &quote_status.id,
                &previous_snapshot_json,
                &revision_at,
            )
            .await?;

            let updated_status = clear_local_status_quote(&db, &quote_status, &revision_at).await?;
            enqueue_status_update_activity(&db, &config, &quote_author, &updated_status).await?;
            enqueue_quote_revocation_federation(
                &db,
                &config,
                &requester,
                &target_status_id,
                &target_uri,
                &local_status_target_uri(&updated_status),
                &updated_status.id,
                &quote_revocation_targets,
                None,
            )
            .await?;

            let media = find_media_attachments_by_status_id(&db, &updated_status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(&db, &updated_status).await?;
            let response = build_local_status_response(
                &db,
                &config,
                Some(&requester),
                &updated_status,
                &quote_author,
                in_reply_to_account_id,
                media,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(crate::ResolvedStatus::Remote(_)) => Response::error("status not found", 404),
        None => Response::error("status not found", 404),
    }
}
