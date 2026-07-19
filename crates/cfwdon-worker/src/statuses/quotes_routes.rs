use super::{
    Request, Response, Result, RouteContext, build_local_status_response_with_quote_count_preloads,
    build_remote_status_response_with_timeline_preloads, build_timeline_link_header,
    can_view_local_status, find_account_by_id, find_accounts_by_ids,
    find_authenticated_local_account, find_media_attachments_by_status_ids,
    find_remote_actors_by_actor_uris, find_remote_status_attachments_by_status_ids,
    is_public_activitypub_visibility, load_config, load_in_reply_to_account_ids,
    local_status_target_uri, preload_local_status_viewer_state, preload_mastodon_poll_responses,
    preload_remote_mastodon_poll_responses, preload_remote_status_edit_updated_at,
    preload_remote_status_viewer_state, preload_status_applications, preload_status_counts,
    preload_status_quote_counts, resolve_status_reference, resolve_timeline_cursor,
    timeline_fetch_limit, timeline_limit,
};
use crate::timelines::TimelinePaginationQuery;
use serde::Deserialize;
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
    let db = ctx.d1(&config.database_binding)?;
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
    db: &worker::D1Database,
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
    db: &worker::D1Database,
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
    db: &worker::D1Database,
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
    db: &worker::D1Database,
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
                    preloads
                        .remote_attachments_by_status_id
                        .remove(&quote.id)
                        .unwrap_or_default(),
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
    db: &worker::D1Database,
    status_uri: &str,
    cursor: &crate::ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<crate::StatusRow>> {
    let bindings = quote_cursor_bindings(status_uri, cursor, limit);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, boost_of_uri, quote_of_uri, content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, created_at
             FROM statuses
             WHERE quote_of_uri = ?1
               AND quote_state = 'accepted'
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
    result
        .results::<crate::StatusRecord>()
        .and_then(crate::statuses_from_records)
}

async fn list_remote_status_quotes_by_uri(
    db: &worker::D1Database,
    status_uri: &str,
    cursor: &crate::ResolvedTimelineCursor,
    limit: u32,
) -> Result<Vec<crate::RemoteStatusRow>> {
    let bindings = quote_cursor_bindings(status_uri, cursor, limit);
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri, content_html, spoiler_text, visibility, sensitive, language, quote_state, published_at
             FROM remote_statuses
             WHERE quote_of_uri = ?1
               AND quote_state = 'accepted'
               AND (
                    ?2 IS NULL
                    OR published_at < ?2
                    OR (published_at = ?2 AND id < ?3)
               )
               AND (
                    ?4 IS NULL
                    OR published_at > ?4
                    OR (published_at = ?4 AND id > ?5)
               )
             ORDER BY published_at DESC, id DESC
             LIMIT ?6",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    result
        .results::<crate::RemoteStatusRecord>()
        .and_then(crate::remote_statuses_from_records)
}

fn quote_cursor_bindings<'a>(
    status_uri: &'a str,
    cursor: &'a crate::ResolvedTimelineCursor,
    limit: u32,
) -> [D1Type<'a>; 6] {
    [
        D1Type::Text(status_uri),
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
    ]
}

#[cfg(test)]
mod tests {
    use super::sort_status_quote_entries;

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
