use super::{
    AppConfig, LocalAccount, RemoteActorRow, RemoteStatusRow, Request, Response, Result,
    RouteContext, StatusRow, build_local_status_response, build_remote_status_response,
    find_account_by_id, find_authenticated_local_account, find_local_status_by_object_uri,
    find_remote_actor_by_actor_uri, find_remote_status_by_id,
    find_remote_status_by_url_or_object_uri, find_status_by_id,
    find_visible_local_status_response_subject, is_public_activitypub_visibility, load_config,
    load_visible_local_status_response_subject, resolve_remote_status_by_url,
    status_id_from_context,
};
use serde::Deserialize;
use worker::{D1Database, Error};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StatusActionQuery {
    pub(crate) uri: Option<String>,
}

pub(crate) enum ResolvedActionStatus {
    Local(StatusRow),
    Remote(RemoteStatusRow, RemoteActorRow),
}

pub(crate) enum ResolvedVisibleActionStatus {
    Local(super::LoadedLocalStatusResponseSubject),
    Remote(RemoteStatusRow, RemoteActorRow),
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
    let db = ctx.d1(&config.database_binding)?;
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

pub(crate) async fn resolve_action_status(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    status_id: &str,
    action_uri: Option<&str>,
) -> Result<Option<ResolvedActionStatus>> {
    if action_uri.is_some() && normalized_action_uri(action_uri).is_none() {
        return Err(Error::RustError(
            "uri query parameter must not be empty".to_owned(),
        ));
    }

    if let Some(action_uri) = normalized_action_uri(action_uri) {
        return resolve_action_uri_reference(db, config, &action_uri).await;
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
    resolve_action_uri_reference(db, config, &decoded_status_id).await
}

pub(crate) async fn resolve_visible_action_status(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &LocalAccount,
    status_id: &str,
    action_uri: Option<&str>,
) -> Result<Option<ResolvedVisibleActionStatus>> {
    match resolve_action_status(db, config, status_id, action_uri).await? {
        Some(ResolvedActionStatus::Local(status)) => Ok(
            load_visible_local_status_response_subject(db, Some(viewer), status)
                .await?
                .map(ResolvedVisibleActionStatus::Local),
        ),
        Some(ResolvedActionStatus::Remote(status, actor)) => {
            if !is_public_activitypub_visibility(status.visibility.as_str()) {
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

    if let Some((status, actor)) = resolve_remote_status_by_url(db, config, value).await? {
        return Ok(Some(ResolvedActionStatus::Remote(status, actor)));
    }

    Ok(None)
}

pub(crate) async fn build_local_action_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    subject: super::LoadedLocalStatusResponseSubject,
) -> Result<crate::MastodonStatusResponse> {
    let super::LoadedLocalStatusResponseSubject {
        status,
        account,
        preload,
    } = subject;
    build_local_status_response(
        db,
        config,
        Some(viewer),
        &status,
        &account,
        preload.in_reply_to_account_id,
        preload.media,
    )
    .await
}

pub(crate) async fn build_saved_status_collection_response<
    T,
    FCreatedAt,
    FStatusId,
    FRemoteStatusId,
>(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    entries: &[T],
    limit: u32,
    created_at: FCreatedAt,
    status_id: FStatusId,
    remote_status_id: FRemoteStatusId,
) -> Result<Response>
where
    FCreatedAt: Fn(&T) -> &str,
    FStatusId: Fn(&T) -> Option<&str>,
    FRemoteStatusId: Fn(&T) -> Option<&str>,
{
    let mut response_entries = Vec::new();
    for entry in entries {
        if let Some(local_status_id) = status_id(entry)
            && let Some(subject) =
                find_visible_local_status_response_subject(db, Some(viewer), local_status_id)
                    .await?
        {
            let response = build_local_action_status_response(db, config, viewer, subject).await?;
            response_entries.push((
                created_at(entry).to_owned(),
                serde_json::to_value(response).unwrap_or_default(),
            ));
            continue;
        }

        if let Some(remote_status_id) = remote_status_id(entry)
            && let Some(status) = find_remote_status_by_id(db, remote_status_id).await?
            && let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await?
        {
            let response =
                build_remote_status_response(db, config, Some(viewer), &status, &actor).await?;
            response_entries.push((
                created_at(entry).to_owned(),
                serde_json::to_value(response).unwrap_or_default(),
            ));
        }
    }

    response_entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &response_entries
            .into_iter()
            .map(|(_, value)| value)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}
