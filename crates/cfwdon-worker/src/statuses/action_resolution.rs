use super::{
    LocalAccount, find_local_status_by_object_uri, find_remote_statuses_with_actors_by_ids,
    find_status_by_id, find_statuses_by_ids, load_visible_local_status_response_subject,
};
use crate::auth::{find_account_by_id, find_authenticated_local_account};
use crate::db_session::bind_request_d1;
use crate::remote::{
    find_remote_status_by_id, find_remote_status_by_url_or_object_uri, resolve_remote_status_by_url,
};
use crate::request_utils::status_id_from_context;
use crate::runtime_config::load_config;
use crate::store::remote::{RemoteActorRow, find_remote_actor_by_actor_uri};
use crate::timelines::{StatusRenderItem, render_status_items};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalStatus, RemoteStatus};
use serde::Deserialize;
use std::collections::HashMap;
use worker::{Error, Request, Response, Result, RouteContext};
#[derive(Debug, Default, Deserialize)]
pub(crate) struct StatusActionQuery {
    pub(crate) uri: Option<String>,
}

pub(crate) enum ResolvedActionStatus {
    Local(LocalStatus),
    Remote(RemoteStatus, RemoteActorRow),
}

pub(crate) enum ResolvedVisibleActionStatus {
    Local(super::LoadedLocalStatusResponseSubject),
    Remote(RemoteStatus, RemoteActorRow),
}

pub(crate) struct AuthenticatedStatusViewerContext {
    pub(crate) db: D1Database,
    pub(crate) config: AppConfig,
    pub(crate) viewer: LocalAccount,
}

pub(crate) struct AuthenticatedStatusActionContext {
    pub(crate) auth: AuthenticatedStatusViewerContext,
    pub(crate) status_id: String,
    pub(crate) action_uri: Option<String>,
}

pub(crate) enum AuthenticatedStatusActionContextResolution {
    MissingStatusId,
    Unauthenticated,
    Ready(AuthenticatedStatusActionContext),
}

pub(crate) fn normalized_action_uri(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }

    Some(
        urlencoding::decode(value)
            .map(|decoded| decoded.into_owned())
            .unwrap_or_else(|_| value.to_owned()),
    )
}

pub(crate) async fn resolve_authenticated_status_viewer_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<AuthenticatedStatusViewerContext>> {
    let config = load_config(ctx);
    let db = bind_request_d1(ctx, &config)?;
    let viewer = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Ok(None),
    };
    Ok(Some(AuthenticatedStatusViewerContext {
        db,
        config,
        viewer,
    }))
}

pub(crate) async fn resolve_authenticated_status_action_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<AuthenticatedStatusActionContextResolution> {
    let status_id = match status_id_from_context(ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Ok(AuthenticatedStatusActionContextResolution::MissingStatusId),
    };
    let action_query: StatusActionQuery = req.query().unwrap_or_default();
    let action_uri = match normalized_action_uri(action_query.uri.as_deref()) {
        Some(uri) => Some(uri),
        None if action_query.uri.is_some() => {
            return Err(Error::RustError(
                "uri query parameter must not be empty".to_owned(),
            ));
        }
        None => None,
    };
    let Some(auth) = resolve_authenticated_status_viewer_context(req, ctx).await? else {
        return Ok(AuthenticatedStatusActionContextResolution::Unauthenticated);
    };
    Ok(AuthenticatedStatusActionContextResolution::Ready(
        AuthenticatedStatusActionContext {
            auth,
            status_id,
            action_uri,
        },
    ))
}

pub(crate) async fn resolve_action_status_with_viewer(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    status_id: &str,
    action_uri: Option<&str>,
) -> Result<Option<ResolvedActionStatus>> {
    if action_uri.is_some() && normalized_action_uri(action_uri).is_none() {
        return Err(Error::RustError(
            "uri query parameter must not be empty".to_owned(),
        ));
    }

    if let Some(action_uri) = normalized_action_uri(action_uri) {
        return resolve_action_uri_reference(db, config, viewer, &action_uri).await;
    }

    if let Some(status) = find_status_by_id(db, status_id).await?
        && find_account_by_id(db, &status.account_id).await?.is_some()
    {
        return Ok(Some(ResolvedActionStatus::Local(status)));
    }

    if let Some(status) = find_remote_status_by_id(db, status_id).await?
        && let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await?
    {
        return Ok(Some(ResolvedActionStatus::Remote(status, actor)));
    }

    let decoded_status_id = urlencoding::decode(status_id)
        .map(|decoded| decoded.into_owned())
        .unwrap_or_else(|_| status_id.to_owned());
    resolve_action_uri_reference(db, config, viewer, &decoded_status_id).await
}

pub(crate) async fn resolve_visible_action_status(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &LocalAccount,
    status_id: &str,
    action_uri: Option<&str>,
) -> Result<Option<ResolvedVisibleActionStatus>> {
    match resolve_action_status_with_viewer(db, config, Some(viewer), status_id, action_uri).await?
    {
        Some(ResolvedActionStatus::Local(status)) => Ok(
            load_visible_local_status_response_subject(db, Some(viewer), status)
                .await?
                .map(ResolvedVisibleActionStatus::Local),
        ),
        Some(ResolvedActionStatus::Remote(status, actor)) => {
            if !super::can_view_remote_status(db, &status, Some(viewer)).await? {
                return Ok(None);
            }
            Ok(Some(ResolvedVisibleActionStatus::Remote(status, actor)))
        }
        None => Ok(None),
    }
}

async fn resolve_action_uri_reference(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    value: &str,
) -> Result<Option<ResolvedActionStatus>> {
    if let Some(status) = find_local_status_by_object_uri(db, config, value).await?
        && find_account_by_id(db, &status.account_id).await?.is_some()
    {
        return Ok(Some(ResolvedActionStatus::Local(status)));
    }

    if let Some(status) = find_remote_status_by_url_or_object_uri(db, value).await?
        && let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await?
    {
        return Ok(Some(ResolvedActionStatus::Remote(status, actor)));
    }

    if let Ok(Some((status, actor))) = resolve_remote_status_by_url(db, config, value, viewer).await
    {
        return Ok(Some(ResolvedActionStatus::Remote(status, actor)));
    }

    Ok(None)
}

/// Render the acting viewer's status response through the batched preloads.
pub(crate) async fn build_local_action_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    subject: super::LoadedLocalStatusResponseSubject,
) -> Result<serde_json::Value> {
    Ok(render_status_items(
        db,
        config,
        Some(viewer),
        vec![StatusRenderItem::Local(subject.status)],
    )
    .await?
    .pop()
    .unwrap_or(serde_json::Value::Null))
}

pub(crate) async fn build_saved_status_collection_response<
    T,
    FCursorId,
    FStatusId,
    FRemoteStatusId,
>(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    req: &Request,
    page: super::SavedStatusesPage,
    entries: &[T],
    cursor_id: FCursorId,
    status_id: FStatusId,
    remote_status_id: FRemoteStatusId,
) -> Result<Response>
where
    FCursorId: Fn(&T) -> i64,
    FStatusId: Fn(&T) -> Option<&str>,
    FRemoteStatusId: Fn(&T) -> Option<&str>,
{
    let local_status_ids = entries
        .iter()
        .filter_map(|entry| status_id(entry).map(str::to_owned))
        .collect::<Vec<_>>();
    let remote_status_ids = entries
        .iter()
        .filter_map(|entry| remote_status_id(entry).map(str::to_owned))
        .collect::<Vec<_>>();
    let (local_statuses, remote_statuses) = futures_util::try_join!(
        find_statuses_by_ids(db, &local_status_ids),
        find_remote_statuses_with_actors_by_ids(db, &remote_status_ids),
    )?;
    // Visibility checks run concurrently; rendering is one batched pass below.
    let visible_local = futures_util::future::try_join_all(
        local_statuses
            .into_iter()
            .map(|status| load_visible_local_status_response_subject(db, Some(viewer), status)),
    )
    .await?
    .into_iter()
    .flatten()
    .map(|subject| (subject.status.id.clone(), subject.status))
    .collect::<HashMap<_, _>>();
    let mut remote_by_id = remote_statuses
        .into_iter()
        .map(|(status, actor)| (status.id.clone(), (status, actor)))
        .collect::<HashMap<_, _>>();
    let mut visible_local = visible_local;

    let mut ordered = Vec::new();
    for entry in entries {
        let item = if let Some(status) = status_id(entry).and_then(|id| visible_local.remove(id)) {
            StatusRenderItem::Local(status)
        } else if let Some((status, actor)) =
            remote_status_id(entry).and_then(|id| remote_by_id.remove(id))
        {
            StatusRenderItem::Remote { status, actor }
        } else {
            continue;
        };
        ordered.push((cursor_id(entry), item));
    }
    // Entries arrive newest first. A forward (`min_id`) page keeps the entries
    // closest to the cursor, i.e. the oldest ones.
    let limit = page.limit as usize;
    if page.walks_forward() && ordered.len() > limit {
        ordered.drain(..ordered.len() - limit);
    }
    ordered.truncate(limit);
    let link = super::saved_statuses_link_header(
        &req.url()?,
        page.limit,
        ordered.first().map(|(cursor, _)| *cursor),
        ordered.last().map(|(cursor, _)| *cursor),
    )?;
    let items = ordered.into_iter().map(|(_, item)| item).collect();

    let mut response =
        Response::from_json(&render_status_items(db, config, Some(viewer), items).await?)?;
    if let Some(link) = link {
        response.headers_mut().set("Link", &link)?;
    }
    Ok(response)
}
