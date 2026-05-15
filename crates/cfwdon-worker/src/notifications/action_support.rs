use super::{
    Error, Request, Result, RouteContext, load_config, require_authenticated_local_account,
};

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct NotificationsQuery {
    pub(crate) limit: Option<u32>,
    pub(crate) account_id: Option<String>,
    #[serde(rename = "types[]")]
    pub(crate) types: Option<Vec<String>>,
    #[serde(rename = "exclude_types[]")]
    pub(crate) exclude_types: Option<Vec<String>>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
    #[serde(skip)]
    pub(crate) min_created_at: Option<String>,
}

pub(crate) struct AuthenticatedNotificationContext {
    pub(crate) db: worker::D1Database,
    pub(crate) config: cfwdon_core::AppConfig,
    pub(crate) viewer: cfwdon_domain::LocalAccount,
}

pub(crate) async fn resolve_authenticated_notification_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<AuthenticatedNotificationContext>> {
    let config = load_config(ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Ok(None),
    };
    Ok(Some(AuthenticatedNotificationContext {
        db,
        config,
        viewer,
    }))
}

pub(crate) struct NotificationGroupRouteContext {
    pub(crate) auth: AuthenticatedNotificationContext,
    pub(crate) query: NotificationsQuery,
    pub(crate) per_type_limit: u32,
    pub(crate) group_key: String,
}

pub(crate) struct NotificationListRouteContext {
    pub(crate) auth: AuthenticatedNotificationContext,
    pub(crate) query: NotificationsQuery,
    pub(crate) limit: u32,
}

pub(crate) async fn resolve_notification_list_route_context(
    req: &Request,
    ctx: &RouteContext<()>,
    default_limit: u32,
    max_limit: u32,
) -> Result<Option<NotificationListRouteContext>> {
    let Some(auth) = resolve_authenticated_notification_context(req, ctx).await? else {
        return Ok(None);
    };
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(default_limit).clamp(1, max_limit);
    Ok(Some(NotificationListRouteContext { auth, query, limit }))
}

pub(crate) struct NotificationEntryRouteContext {
    pub(crate) auth: AuthenticatedNotificationContext,
    pub(crate) notification_id: String,
}

pub(crate) async fn resolve_notification_entry_route_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<NotificationEntryRouteContext>> {
    let Some(auth) = resolve_authenticated_notification_context(req, ctx).await? else {
        return Ok(None);
    };
    let notification_id = ctx
        .param("id")
        .map(|value| value.to_owned())
        .ok_or_else(|| Error::RustError("missing notification id route parameter".to_owned()))?;
    Ok(Some(NotificationEntryRouteContext {
        auth,
        notification_id,
    }))
}

pub(crate) async fn resolve_notification_group_route_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<NotificationGroupRouteContext>> {
    let Some(list) = resolve_notification_list_route_context(req, ctx, 100, 1000).await? else {
        return Ok(None);
    };
    let group_key = ctx
        .param("group_key")
        .map(|value| value.to_owned())
        .ok_or_else(|| Error::RustError("missing notification group key".to_owned()))?;
    Ok(Some(NotificationGroupRouteContext {
        auth: list.auth,
        query: list.query,
        per_type_limit: list.limit,
        group_key,
    }))
}
