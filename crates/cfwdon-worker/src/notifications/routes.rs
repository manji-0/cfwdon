use super::{
    Request, Response, Result, RouteContext, build_notification_group_document,
    build_notifications_v2_document, clear_notifications_usecase,
    dismiss_notification_entry_usecase, dismiss_notification_group_usecase,
    list_notification_group_entries_usecase, list_notifications_usecase,
    load_notification_entry_usecase, resolve_notification_entry_route_context,
    resolve_notification_group_route_context, resolve_notification_list_route_context,
    unread_notifications_count_usecase,
};
use crate::timelines::build_timeline_link_header;

pub(crate) async fn notifications_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(list) = resolve_notification_list_route_context(&req, &ctx, 20, 40).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    let limited_entries = list_notifications_usecase(
        &list.auth.db,
        &list.auth.config,
        &list.auth.viewer,
        &list.query,
        list.limit,
    )
    .await?;
    let first_id = limited_entries.first().map(|entry| entry.id.clone());
    let last_id = limited_entries.last().map(|entry| entry.id.clone());

    let mut builder = Response::from_json(
        &limited_entries
            .into_iter()
            .map(|entry| entry.value)
            .collect::<Vec<_>>(),
    )?;
    if let Some(link_header) =
        build_timeline_link_header(&req, list.limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link_header)?;
    }
    Ok(builder)
}

pub(crate) async fn notifications_v2_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(list) = resolve_notification_list_route_context(&req, &ctx, 20, 40).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    let limited_entries = list_notifications_usecase(
        &list.auth.db,
        &list.auth.config,
        &list.auth.viewer,
        &list.query,
        list.limit,
    )
    .await?;

    Response::from_json(&build_notifications_v2_document(&limited_entries))
}

pub(crate) async fn notification_group_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(group) = resolve_notification_group_route_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    let entries = list_notification_group_entries_usecase(
        &group.auth.db,
        &group.auth.config,
        &group.auth.viewer,
        &group.query,
        group.per_type_limit,
        &group.group_key,
    )
    .await?;
    if entries.is_empty() {
        return Response::error("notification group not found", 404);
    }

    let entry_refs = entries.iter().collect::<Vec<_>>();
    Response::from_json(&build_notification_group_document(&entry_refs))
}

pub(crate) async fn notification_group_accounts_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(group) = resolve_notification_group_route_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    let entries = list_notification_group_entries_usecase(
        &group.auth.db,
        &group.auth.config,
        &group.auth.viewer,
        &group.query,
        group.per_type_limit,
        &group.group_key,
    )
    .await?;
    if entries.is_empty() {
        return Response::error("notification group not found", 404);
    }

    let entry_refs = entries.iter().collect::<Vec<_>>();
    let document = build_notification_group_document(&entry_refs);
    Response::from_json(
        &document
            .get("accounts")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Array(Vec::new())),
    )
}

pub(crate) async fn notification_group_dismiss_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(group) = resolve_notification_group_route_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    if !dismiss_notification_group_usecase(
        &group.auth.db,
        &group.auth.config,
        &group.auth.viewer,
        &group.query,
        group.per_type_limit,
        &group.group_key,
    )
    .await?
    {
        return Response::error("notification group not found", 404);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn notification_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some(entry_context) = resolve_notification_entry_route_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    let Some(entry) = load_notification_entry_usecase(
        &entry_context.auth.db,
        &entry_context.auth.config,
        &entry_context.auth.viewer,
        &entry_context.notification_id,
    )
    .await?
    else {
        return Response::error("notification not found", 404);
    };

    Response::from_json(&entry.value)
}

pub(crate) async fn notification_dismiss_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(entry_context) = resolve_notification_entry_route_context(&req, &ctx).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    if !dismiss_notification_entry_usecase(
        &entry_context.auth.db,
        &entry_context.auth.config,
        &entry_context.auth.viewer,
        &entry_context.notification_id,
    )
    .await?
    {
        return Response::error("notification not found", 404);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn notifications_clear_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(list) = resolve_notification_list_route_context(&req, &ctx, 100, 1000).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    clear_notifications_usecase(&list.auth.db, &list.auth.viewer).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn notifications_unread_count_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some(list) = resolve_notification_list_route_context(&req, &ctx, 100, 1000).await? else {
        return Response::error("Auth0 authentication required", 401);
    };

    Response::from_json(&serde_json::json!({
        "count": unread_notifications_count_usecase(&list.auth.db, &list.auth.config, &list.auth.viewer, &list.query, list.limit).await?,
    }))
}
