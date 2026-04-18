use super::{
    Error, NotificationEntry, Request, Response, Result, RouteContext,
    clear_notifications_for_account, collect_visible_notifications,
    dismiss_notification_for_account, load_config, require_authenticated_local_account,
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
    pub(crate) _max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) _since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) _min_id: Option<String>,
}

pub(crate) async fn notifications_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, limit.saturating_mul(4))
            .await?;

    Response::from_json(
        &entries
            .into_iter()
            .take(limit as usize)
            .map(|entry| entry.value)
            .collect::<Vec<_>>(),
    )
}

pub(crate) fn build_notifications_v2_document(entries: &[NotificationEntry]) -> serde_json::Value {
    let mut accounts = Vec::new();
    let mut account_ids = std::collections::HashSet::new();
    let mut statuses = Vec::new();
    let mut status_ids = std::collections::HashSet::new();
    let mut groups = Vec::new();

    for entry in entries {
        let account = entry.value.get("account").cloned();
        let account_id = account
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if let (Some(account), Some(account_id)) = (account.clone(), account_id.clone())
            && account_ids.insert(account_id.clone())
        {
            accounts.push(account);
        }

        let status = entry.value.get("status").cloned();
        let status_id = status
            .as_ref()
            .and_then(|value| value.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        if let (Some(status), Some(status_id)) = (status.clone(), status_id.clone())
            && status_ids.insert(status_id.clone())
        {
            statuses.push(status);
        }

        groups.push(serde_json::json!({
            "group_key": entry
                .value
                .get("group_key")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(entry.id.as_str()),
            "type": entry
                .value
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            "latest_page_notification_at": entry.created_at,
            "most_recent_notification_id": entry.id,
            "page_min_id": entry.id,
            "page_max_id": entry.id,
            "notifications_count": 1,
            "sample_account_ids": account_id.into_iter().collect::<Vec<_>>(),
            "status_id": status_id,
        }));
    }

    serde_json::json!({
        "accounts": accounts,
        "statuses": statuses,
        "notification_groups": groups,
    })
}

pub(crate) async fn notifications_v2_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, limit.saturating_mul(4))
            .await?;
    let limited_entries = entries.into_iter().take(limit as usize).collect::<Vec<_>>();

    Response::from_json(&build_notifications_v2_document(&limited_entries))
}

pub(crate) async fn notification_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let notification_id = ctx
        .param("id")
        .ok_or_else(|| Error::RustError("missing notification id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query = NotificationsQuery {
        limit: Some(200),
        ..NotificationsQuery::default()
    };

    let Some(entry) = collect_visible_notifications(&db, &config, &viewer, &query, 200)
        .await?
        .into_iter()
        .find(|entry| entry.id == notification_id.as_str())
    else {
        return Response::error("notification not found", 404);
    };

    Response::from_json(&entry.value)
}

pub(crate) async fn notification_dismiss_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let notification_id = ctx
        .param("id")
        .ok_or_else(|| Error::RustError("missing notification id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query = NotificationsQuery {
        limit: Some(200),
        ..NotificationsQuery::default()
    };

    let exists = collect_visible_notifications(&db, &config, &viewer, &query, 200)
        .await?
        .into_iter()
        .any(|entry| entry.id == notification_id.as_str());
    if !exists {
        return Response::error("notification not found", 404);
    }

    dismiss_notification_for_account(&db, &viewer.id, &notification_id).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn notifications_clear_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    clear_notifications_for_account(&db, &viewer.id).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn notifications_unread_count_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let per_type_limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, per_type_limit).await?;

    Response::from_json(&serde_json::json!({
        "count": entries.len(),
    }))
}
