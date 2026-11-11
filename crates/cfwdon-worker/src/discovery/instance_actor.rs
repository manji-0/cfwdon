//! Mastodon's instance actor (`/actor`). Its key signs the ActivityPub GETs
//! this server makes on its own behalf; remotes fetch this document to verify
//! those signatures.

use crate::activitypub::build_instance_actor_document;
use crate::db_session::bind_request_d1;
use crate::identity::instance_actor_url;
use crate::response_utils::{CACHE_TTL_FEDERATION, cache_public_json_response};
use crate::runtime_config::load_config;
use crate::store::instance_actor::instance_actor_keys;
use worker::{Response, Result, RouteContext};

pub(crate) async fn instance_actor_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let keys = instance_actor_keys(&db, &config).await?;
    cache_public_json_response(
        &build_instance_actor_document(&config, &keys.public_key_pem),
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[],
    )
}

/// The instance actor publishes nothing, so its outbox is always empty.
pub(crate) fn instance_actor_outbox_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    cache_public_json_response(
        &serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "id": format!("{}/outbox", instance_actor_url(&config)),
            "type": "OrderedCollection",
            "totalItems": 0,
            "orderedItems": [],
        }),
        "application/activity+json",
        CACHE_TTL_FEDERATION,
        &[],
    )
}
