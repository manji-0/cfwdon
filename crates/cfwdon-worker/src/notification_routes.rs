use super::{
    Error, NotificationEntry, Request, Response, Result, RouteContext, build_timeline_link_header,
    clear_notifications_for_account, collect_visible_notifications,
    dismiss_notification_for_account, load_config, notification_sort_key,
    require_authenticated_local_account,
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

fn normalized_notification_cursor(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn notification_cursor_key(entry: &NotificationEntry) -> (String, String) {
    (notification_sort_key(&entry.created_at), entry.id.clone())
}

fn resolve_notification_cursor_key(
    entries: &[NotificationEntry],
    cursor_id: Option<&str>,
) -> Option<(String, String)> {
    let cursor_id = normalized_notification_cursor(cursor_id)?;
    entries
        .iter()
        .find(|entry| entry.id == cursor_id)
        .map(notification_cursor_key)
}

pub(crate) fn filter_notification_entries_by_query(
    entries: Vec<NotificationEntry>,
    query: &NotificationsQuery,
) -> Vec<NotificationEntry> {
    let max_cursor = resolve_notification_cursor_key(&entries, query.max_id.as_deref());
    let min_cursor = resolve_notification_cursor_key(
        &entries,
        query.min_id.as_deref().or(query.since_id.as_deref()),
    );

    entries
        .into_iter()
        .filter(|entry| {
            let cursor_key = notification_cursor_key(entry);
            max_cursor.as_ref().is_none_or(|value| cursor_key < *value)
                && min_cursor.as_ref().is_none_or(|value| cursor_key > *value)
        })
        .collect()
}

fn notifications_fetch_limit(query: &NotificationsQuery, limit: u32) -> u32 {
    if query.max_id.is_some() || query.since_id.is_some() || query.min_id.is_some() {
        1000
    } else {
        limit.saturating_mul(4)
    }
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
    let entries = collect_visible_notifications(
        &db,
        &config,
        &viewer,
        &query,
        notifications_fetch_limit(&query, limit),
    )
    .await?;
    let filtered_entries = filter_notification_entries_by_query(entries, &query);
    let limited_entries = filtered_entries
        .into_iter()
        .take(limit as usize)
        .collect::<Vec<_>>();
    let first_id = limited_entries.first().map(|entry| entry.id.clone());
    let last_id = limited_entries.last().map(|entry| entry.id.clone());

    let mut builder = Response::from_json(
        &limited_entries
            .into_iter()
            .map(|entry| entry.value)
            .collect::<Vec<_>>(),
    )?;
    if let Some(link_header) =
        build_timeline_link_header(&req, limit, first_id.as_deref(), last_id.as_deref())?
    {
        builder.headers_mut().set("Link", &link_header)?;
    }
    Ok(builder)
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
        let collection = entry.value.get("collection").cloned();

        let mut group = serde_json::json!({
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
        });
        if let Some(collection) = collection {
            group["collection"] = collection;
        }
        groups.push(group);
    }

    serde_json::json!({
        "accounts": accounts,
        "statuses": statuses,
        "notification_groups": groups,
    })
}

fn notification_group_key(entry: &NotificationEntry) -> &str {
    entry
        .value
        .get("group_key")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(entry.id.as_str())
}

fn notification_group_entries<'a>(
    entries: &'a [NotificationEntry],
    group_key: &str,
) -> Vec<&'a NotificationEntry> {
    entries
        .iter()
        .filter(|entry| notification_group_key(entry) == group_key)
        .collect()
}

fn build_notification_group_document(entries: &[&NotificationEntry]) -> serde_json::Value {
    let group_entries = entries
        .iter()
        .map(|entry| NotificationEntry {
            id: entry.id.clone(),
            created_at: entry.created_at.clone(),
            value: entry.value.clone(),
        })
        .collect::<Vec<_>>();
    let document = build_notifications_v2_document(&group_entries);
    serde_json::json!({
        "accounts": document.get("accounts").cloned().unwrap_or_default(),
        "statuses": document.get("statuses").cloned().unwrap_or_default(),
        "notification_group": document
            .get("notification_groups")
            .and_then(serde_json::Value::as_array)
            .and_then(|groups| groups.first().cloned())
            .unwrap_or_default(),
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
    let entries = collect_visible_notifications(
        &db,
        &config,
        &viewer,
        &query,
        notifications_fetch_limit(&query, limit),
    )
    .await?;
    let limited_entries = filter_notification_entries_by_query(entries, &query)
        .into_iter()
        .take(limit as usize)
        .collect::<Vec<_>>();

    Response::from_json(&build_notifications_v2_document(&limited_entries))
}

pub(crate) async fn notification_group_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let per_type_limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let group_key = ctx
        .param("group_key")
        .ok_or_else(|| Error::RustError("missing notification group key".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, per_type_limit).await?;
    let group_entries = notification_group_entries(&entries, &group_key);
    if group_entries.is_empty() {
        return Response::error("notification group not found", 404);
    }

    Response::from_json(&build_notification_group_document(&group_entries))
}

pub(crate) async fn notification_group_accounts_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let per_type_limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let group_key = ctx
        .param("group_key")
        .ok_or_else(|| Error::RustError("missing notification group key".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, per_type_limit).await?;
    let group_entries = notification_group_entries(&entries, &group_key);
    if group_entries.is_empty() {
        return Response::error("notification group not found", 404);
    }

    let document = build_notification_group_document(&group_entries);
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
    let config = load_config(&ctx);
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let per_type_limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let group_key = ctx
        .param("group_key")
        .ok_or_else(|| Error::RustError("missing notification group key".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, per_type_limit).await?;
    let group_entries = notification_group_entries(&entries, &group_key);
    if group_entries.is_empty() {
        return Response::error("notification group not found", 404);
    }

    for entry in group_entries {
        dismiss_notification_for_account(&db, &viewer.id, &entry.id).await?;
    }
    Response::from_json(&serde_json::json!({}))
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
