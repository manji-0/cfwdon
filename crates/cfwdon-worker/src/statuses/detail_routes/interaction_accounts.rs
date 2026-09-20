use super::request_context::{
    ResolvedStatus, resolve_status_detail_base_context, resolve_status_reference,
};
use crate::statuses::{
    MastodonAccountResponse, Request, Response, Result, RouteContext, find_account_by_id,
    find_remote_actor_by_actor_uri, is_public_activitypub_visibility,
    list_local_favourite_account_ids_for_remote_status,
    list_local_favourite_account_ids_for_status, list_local_reblog_account_ids_for_remote_status,
    list_local_reblog_account_ids_for_status, list_remote_favourite_actor_uris_for_status,
    list_remote_reblog_actor_uris_for_status, load_account_stats, remote_account_rest_id,
};
use serde::Deserialize;
use url::Url;

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
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ids: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let mut responses = Vec::new();

    for account_id in account_ids {
        let Some(account) = find_account_by_id(db, account_id).await? else {
            continue;
        };
        let stats = load_account_stats(db, account.id()).await?;
        responses.push(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }

    Ok(responses)
}

pub(super) async fn build_remote_interaction_account_response(
    db: &crate::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let status_summary = crate::load_remote_actor_status_summary(db, actor_uri).await?;

    if let Some(actor) = find_remote_actor_by_actor_uri(db, actor_uri).await? {
        let mut response = MastodonAccountResponse::from_remote_actor(&actor);
        response.statuses_count = status_summary.statuses_count;
        response.last_status_at = status_summary.last_status_at.clone();
        return Ok(Some(response));
    }

    let parsed = match Url::parse(actor_uri) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(None),
    };
    let Some(domain) = parsed.host_str().map(str::to_owned) else {
        return Ok(None);
    };
    let Some(username) = parsed
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .map(|segment| segment.trim_start_matches('@').to_owned())
        .filter(|segment| !segment.is_empty())
    else {
        return Ok(None);
    };

    Ok(Some(MastodonAccountResponse {
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
    }))
}

pub(super) async fn build_remote_interaction_account_responses(
    db: &crate::D1Database,
    actor_uris: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let mut responses = Vec::new();

    for actor_uri in actor_uris {
        if let Some(response) = build_remote_interaction_account_response(db, actor_uri).await? {
            responses.push(response);
        }
    }

    Ok(responses)
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
    crate::with_d1_bookmark(Response::from_json(&responses)?, &detail.session)
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
