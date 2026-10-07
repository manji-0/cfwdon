use super::request_context::{
    ResolvedStatus, resolve_status_detail_base_context, resolve_status_reference,
};
use crate::accounts::{find_accounts_by_ids, load_account_stats_map};
use crate::activitypub::is_public_activitypub_visibility;
use crate::db_session::with_d1_bookmark;
use crate::identity::remote_account_rest_id;
use crate::responses::MastodonAccountResponse;
use crate::statuses::{
    list_local_favourite_account_ids_for_remote_status,
    list_local_favourite_account_ids_for_status, list_local_reblog_account_ids_for_remote_status,
    list_local_reblog_account_ids_for_status, list_remote_favourite_actor_uris_for_status,
    list_remote_reblog_actor_uris_for_status,
};
use crate::store::remote::{
    RemoteActorStatusSummary, find_remote_actors_by_actor_uris, load_remote_actor_status_summaries,
};
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use url::Url;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Default, Deserialize)]
pub(super) struct StatusInteractionAccountsQuery {
    limit: Option<u32>,
}

#[derive(Clone, Copy)]
pub(super) enum StatusInteractionKind {
    Reblogged,
    Favourited,
}

pub(super) async fn build_local_interaction_account_responses(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account_ids: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let (accounts_by_id, stats_by_id) = futures_util::try_join!(
        find_accounts_by_ids(db, account_ids),
        load_account_stats_map(db, account_ids),
    )?;
    let default_stats = Default::default();
    Ok(account_ids
        .iter()
        .filter_map(|account_id| {
            let account = accounts_by_id.get(account_id)?;
            Some(MastodonAccountResponse::from_account_with_stats(
                account,
                config,
                stats_by_id.get(account_id).unwrap_or(&default_stats),
            ))
        })
        .collect())
}

/// An account for an actor never stored, built from its URI.
fn placeholder_remote_interaction_account_response(
    actor_uri: &str,
    status_summary: RemoteActorStatusSummary,
) -> Option<MastodonAccountResponse> {
    let parsed = Url::parse(actor_uri).ok()?;
    let domain = parsed.host_str().map(str::to_owned)?;
    let username = parsed
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| segment.trim_start_matches('@').to_owned())
        .filter(|segment| !segment.is_empty())?;

    Some(MastodonAccountResponse {
        id: remote_account_rest_id(actor_uri),
        username: username.clone(),
        acct: format!("{username}@{domain}"),
        uri: actor_uri.to_owned(),
        display_name: username.clone(),
        locked: false,
        bot: false,
        group: false,
        discoverable: true,
        indexable: true,
        noindex: None,
        hide_collections: None,
        show_media: None,
        show_media_replies: None,
        show_featured: None,
        last_status_at: status_summary.last_status_at,
        created_at: String::new(),
        note: String::new(),
        url: actor_uri.to_owned(),
        avatar: String::new(),
        avatar_static: String::new(),
        avatar_description: String::new(),
        header: String::new(),
        header_static: String::new(),
        header_description: String::new(),
        emojis: Vec::new(),
        fields: Vec::new(),
        roles: None,
        feature_approval: serde_json::json!({
            "automatic": [],
            "manual": [],
            "current_user": "missing",
        }),
        followers_count: 0,
        following_count: 0,
        statuses_count: status_summary.statuses_count,
        source: None,
        role: None,
    })
}

pub(super) async fn build_remote_interaction_account_responses(
    db: &D1Database,
    actor_uris: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let (actors_by_uri, mut summaries_by_uri) = futures_util::try_join!(
        find_remote_actors_by_actor_uris(db, actor_uris),
        load_remote_actor_status_summaries(db, actor_uris),
    )?;
    Ok(actor_uris
        .iter()
        .filter_map(|actor_uri| {
            let summary = summaries_by_uri.remove(actor_uri).unwrap_or_default();
            match actors_by_uri.get(actor_uri) {
                Some(actor) => {
                    let mut response = MastodonAccountResponse::from_remote_actor(actor);
                    response.statuses_count = summary.statuses_count;
                    response.last_status_at = summary.last_status_at;
                    Some(response)
                }
                None => placeholder_remote_interaction_account_response(actor_uri, summary),
            }
        })
        .collect())
}

pub(super) async fn status_interaction_accounts_response(
    req: Request,
    ctx: RouteContext<()>,
    kind: StatusInteractionKind,
) -> Result<Response> {
    let Some(detail) = resolve_status_detail_base_context(&req, &ctx)? else {
        return Response::error("missing status id route parameter", 400);
    };
    let Some(status) =
        resolve_status_reference(&detail.db, &detail.config, &detail.status_id).await?
    else {
        return Response::error("status not found", 404);
    };
    let query: StatusInteractionAccountsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).min(80);

    let mut responses = match status {
        ResolvedStatus::Local(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let local_accounts = match kind {
                StatusInteractionKind::Reblogged => {
                    list_local_reblog_account_ids_for_status(&detail.db, &status.id, limit).await?
                }
                StatusInteractionKind::Favourited => {
                    list_local_favourite_account_ids_for_status(&detail.db, &status.id, limit)
                        .await?
                }
            };
            let mut responses = build_local_interaction_account_responses(
                &detail.db,
                &detail.config,
                &local_accounts,
            )
            .await?;
            if responses.len() < limit as usize {
                let remaining = limit.saturating_sub(responses.len() as u32);
                let remote_actor_uris = match kind {
                    StatusInteractionKind::Reblogged => {
                        list_remote_reblog_actor_uris_for_status(&detail.db, &status.id, remaining)
                            .await?
                    }
                    StatusInteractionKind::Favourited => {
                        list_remote_favourite_actor_uris_for_status(
                            &detail.db, &status.id, remaining,
                        )
                        .await?
                    }
                };
                responses.extend(
                    build_remote_interaction_account_responses(&detail.db, &remote_actor_uris)
                        .await?,
                );
            }
            responses
        }
        ResolvedStatus::Remote(status) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
                return Response::error("status not found", 404);
            }
            let local_accounts = match kind {
                StatusInteractionKind::Reblogged => {
                    list_local_reblog_account_ids_for_remote_status(&detail.db, &status.id, limit)
                        .await?
                }
                StatusInteractionKind::Favourited => {
                    list_local_favourite_account_ids_for_remote_status(
                        &detail.db, &status.id, limit,
                    )
                    .await?
                }
            };
            build_local_interaction_account_responses(&detail.db, &detail.config, &local_accounts)
                .await?
        }
    };
    responses.truncate(limit as usize);
    with_d1_bookmark(Response::from_json(&responses)?, &detail.session)
}

pub(crate) async fn status_reblogged_by_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    status_interaction_accounts_response(req, ctx, StatusInteractionKind::Reblogged).await
}

pub(crate) async fn status_favourited_by_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    status_interaction_accounts_response(req, ctx, StatusInteractionKind::Favourited).await
}
