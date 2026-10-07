use crate::auth::find_account_by_username;
use crate::db_session::bind_request_d1;
use crate::delivery::list_follower_actor_uris;
use crate::identity::actor_url;
use crate::relationship::{list_following_actor_uris, list_local_follower_usernames};
use crate::response_cache::{cache_federation_document, cached_federation_document};
use crate::response_utils::{
    CACHE_TTL_FEDERATION, cache_public_json_response, cache_public_response,
};
use crate::runtime_config::load_config;
use crate::statuses::{
    build_outbox_activities, count_public_outbox_statuses, list_public_outbox_statuses_page,
};
use std::collections::HashSet;
use worker::{Error, Request, Response, Result, RouteContext};

#[derive(Debug, Default, serde::Deserialize)]
struct CollectionPagingQuery {
    page: Option<bool>,
    limit: Option<u32>,
    offset: Option<u32>,
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
    let req_url = req.url()?.to_string();
    if let Some(response) = cached_federation_document(&req_url).await? {
        return cache_public_response(response, CACHE_TTL_FEDERATION);
    }

    let db = bind_request_d1(&ctx, &config)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let mut ordered_items = list_follower_actor_uris(&db, account.id()).await?;
    let mut seen = ordered_items.iter().cloned().collect::<HashSet<_>>();
    for username in list_local_follower_usernames(&db, account.id()).await? {
        let actor_uri = actor_url(&config, &username);
        if seen.insert(actor_uri.clone()) {
            ordered_items.push(actor_uri);
        }
    }
    let collection_id = format!("{}/followers", actor_url(&config, account.username()));
    let cache_tag = format!("account-{username}");
    let document = build_ordered_collection_document(&collection_id, &ordered_items, &query);
    cache_federation_document(&req_url, &document, &cache_tag).await?;
    cache_public_json_response(
        &document,
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Cache-Tag", &cache_tag)],
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
    let req_url = req.url()?.to_string();
    if let Some(response) = cached_federation_document(&req_url).await? {
        return cache_public_response(response, CACHE_TTL_FEDERATION);
    }

    let db = bind_request_d1(&ctx, &config)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let ordered_items = list_following_actor_uris(&db, account.id()).await?;
    let collection_id = format!("{}/following", actor_url(&config, account.username()));

    let cache_tag = format!("account-{username}");
    let document = build_ordered_collection_document(&collection_id, &ordered_items, &query);
    cache_federation_document(&req_url, &document, &cache_tag).await?;
    cache_public_json_response(
        &document,
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Cache-Tag", &cache_tag)],
    )
}

pub(crate) async fn outbox_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionPagingQuery = req.query().unwrap_or_default();
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;
    let req_url = req.url()?.to_string();
    if let Some(response) = cached_federation_document(&req_url).await? {
        return cache_public_response(response, CACHE_TTL_FEDERATION);
    }

    let db = bind_request_d1(&ctx, &config)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };

    let actor = actor_url(&config, account.username());
    let outbox = format!("{actor}/outbox");
    let total_items = count_public_outbox_statuses(&db, account.id()).await?;
    let limit = query.limit.unwrap_or(20).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let cache_tag = format!("account-{username}");

    if query.page.unwrap_or(false) || query.offset.unwrap_or(0) > 0 {
        let statuses = list_public_outbox_statuses_page(&db, account.id(), limit, offset).await?;
        let ordered_items = build_outbox_activities(&db, &config, &account, &statuses).await?;
        let next_offset = offset.saturating_add(ordered_items.len() as u32);
        let next = if (next_offset as u64) < total_items {
            Some(format!(
                "{outbox}?page=true&offset={next_offset}&limit={limit}"
            ))
        } else {
            None
        };

        let document = serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollectionPage",
            "id": format!("{outbox}?page=true&offset={offset}&limit={limit}"),
            "partOf": outbox,
            "next": next,
            "orderedItems": ordered_items,
        });
        cache_federation_document(&req_url, &document, &cache_tag).await?;
        return cache_public_json_response(
            &document,
            "application/activity+json",
            CACHE_TTL_FEDERATION,
            &[("Cache-Tag", &cache_tag)],
        );
    }

    let document = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "type": "OrderedCollection",
        "id": outbox,
        "totalItems": total_items,
        "first": format!("{outbox}?page=true&offset=0&limit={limit}"),
    });
    cache_federation_document(&req_url, &document, &cache_tag).await?;
    cache_public_json_response(
        &document,
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[("Cache-Tag", &cache_tag)],
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
