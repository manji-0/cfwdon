use super::{
    Error, Request, Response, Result, RouteContext, actor_url, build_activitypub_actor_document,
    build_outbox_activities, build_tag_response, ensure_account_keys, find_account_by_username,
    instance_host, json_response, list_follower_actor_uris, list_following_actor_uris,
    list_local_follower_usernames, list_public_outbox_statuses, load_config, normalize_hashtag,
    parse_webfinger_resource,
};
use std::collections::HashSet;

#[derive(Debug, serde::Deserialize)]
struct WebFingerQuery {
    resource: String,
}

#[derive(Debug, serde::Serialize)]
struct WebFingerResponse {
    subject: String,
    links: Vec<WebFingerLink>,
}

#[derive(Debug, serde::Serialize)]
struct WebFingerLink {
    rel: &'static str,
    #[serde(rename = "type")]
    link_type: &'static str,
    href: String,
}

#[derive(Debug, Default, serde::Deserialize)]
struct CollectionPagingQuery {
    page: Option<bool>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(crate) async fn webfinger_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: WebFingerQuery = req.query()?;
    let handle = parse_webfinger_resource(&query.resource)?;

    if !handle.is_local_to(&config.instance_domain) {
        return Response::error("resource not found", 404);
    }

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &handle.username).await? else {
        return Response::error("resource not found", 404);
    };

    let instance_host = instance_host(&config);
    let response = WebFingerResponse {
        subject: format!("acct:{}@{}", account.username, instance_host),
        links: vec![WebFingerLink {
            rel: "self",
            link_type: "application/activity+json",
            href: actor_url(&config, &account.username),
        }],
    };

    json_response(
        &response,
        "application/jrd+json",
        &[("Access-Control-Allow-Origin", "*")],
    )
}

pub(crate) async fn actor_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let account = ensure_account_keys(&db, account).await?;

    let response = build_activitypub_actor_document(&config, &account);

    json_response(&response, "application/activity+json", &[])
}

pub(crate) async fn tag_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("name")
        .or_else(|| ctx.param("hashtag"))
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing tag route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    Response::from_json(&build_tag_response(&db, &config, &tag).await?)
}

pub(crate) async fn followers_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionPagingQuery = req.query().unwrap_or_default();
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let mut ordered_items = list_follower_actor_uris(&db, &account.id).await?;
    let mut seen = ordered_items.iter().cloned().collect::<HashSet<_>>();
    for username in list_local_follower_usernames(&db, &account.id).await? {
        let actor_uri = actor_url(&config, &username);
        if seen.insert(actor_uri.clone()) {
            ordered_items.push(actor_uri);
        }
    }
    let collection_id = format!("{}/followers", actor_url(&config, &account.username));
    json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        &[],
    )
}

pub(crate) async fn following_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionPagingQuery = req.query().unwrap_or_default();
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let ordered_items = list_following_actor_uris(&db, &account.id).await?;
    let collection_id = format!("{}/following", actor_url(&config, &account.username));

    json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        &[],
    )
}

pub(crate) async fn outbox_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };

    let statuses = list_public_outbox_statuses(&db, &account.id, 20).await?;
    let actor = actor_url(&config, &account.username);
    let outbox = format!("{actor}/outbox");
    let ordered_items = build_outbox_activities(&db, &config, &account, &statuses).await?;

    json_response(
        &serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollection",
            "id": outbox,
            "totalItems": ordered_items.len(),
            "orderedItems": ordered_items,
        }),
        "application/activity+json",
        &[],
    )
}

fn build_ordered_collection_document(
    collection_id: &str,
    ordered_items: &[String],
    query: &CollectionPagingQuery,
) -> serde_json::Value {
    let total_items = ordered_items.len();
    let limit = query.limit.unwrap_or(50).clamp(1, 80) as usize;
    let offset = query.offset.unwrap_or(0) as usize;

    if query.page.unwrap_or(false) || query.offset.unwrap_or(0) > 0 {
        let page_items = ordered_items
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset.saturating_add(page_items.len());
        let next = if next_offset < total_items {
            Some(format!(
                "{collection_id}?page=true&offset={next_offset}&limit={limit}"
            ))
        } else {
            None
        };

        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollectionPage",
            "id": format!("{collection_id}?page=true&offset={offset}&limit={limit}"),
            "partOf": collection_id,
            "next": next,
            "orderedItems": page_items,
        })
    } else {
        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollection",
            "id": collection_id,
            "totalItems": total_items,
            "first": format!("{collection_id}?page=true&offset=0&limit={limit}"),
        })
    }
}
