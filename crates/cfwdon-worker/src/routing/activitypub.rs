use crate::{
    account_statuses_by_username_response, actor_response, featured_collection_response,
    featured_tags_collection_response, followers_collection_response,
    following_collection_response, inbox_response, nodeinfo_links_response, nodeinfo_response,
    outbox_response, process_outbox_deliveries, remote_follow_response, shared_inbox_response,
    webfinger_response,
};
use worker::{Response, Result, Router};

pub(super) const ACTIVITYPUB_CONTENT_TYPE: &str = "application/activity+json";
const JSON_CONTENT_TYPE: &str = "application/json";
const JRD_CONTENT_TYPE: &str = "application/jrd+json";

pub(crate) fn add_activitypub_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/.well-known/webfinger", |req, ctx| async move {
            webfinger_response(req, ctx).await
        })
        .head_async("/.well-known/webfinger", |_req, _ctx| async move {
            static_head_response(JRD_CONTENT_TYPE)
        })
        .get_async("/.well-known/nodeinfo", |_req, ctx| async move {
            nodeinfo_links_response(ctx).await
        })
        .head_async("/.well-known/nodeinfo", |_req, _ctx| async move {
            static_head_response(JSON_CONTENT_TYPE)
        })
        .get_async("/nodeinfo/2.0", |_req, ctx| async move {
            nodeinfo_response(ctx).await
        })
        .head_async("/nodeinfo/2.0", |_req, _ctx| async move {
            static_head_response(JSON_CONTENT_TYPE)
        })
        .get_async("/users/:username", |req, ctx| async move {
            actor_response(req, ctx).await
        })
        .head_async("/users/:username", |_req, _ctx| async move {
            static_head_response(ACTIVITYPUB_CONTENT_TYPE)
        })
        .get_async("/users/:username/statuses", |req, ctx| async move {
            account_statuses_by_username_response(req, ctx).await
        })
        .head_async("/users/:username/statuses", |_req, _ctx| async move {
            static_head_response(ACTIVITYPUB_CONTENT_TYPE)
        })
        .get_async("/users/:username/remote-follow", |req, ctx| async move {
            remote_follow_response(req, ctx).await
        })
        .get_async("/users/:username/followers", |req, ctx| async move {
            followers_collection_response(req, ctx).await
        })
        .head_async("/users/:username/followers", |_req, _ctx| async move {
            static_head_response(ACTIVITYPUB_CONTENT_TYPE)
        })
        .get_async("/users/:username/following", |req, ctx| async move {
            following_collection_response(req, ctx).await
        })
        .head_async("/users/:username/following", |_req, _ctx| async move {
            static_head_response(ACTIVITYPUB_CONTENT_TYPE)
        })
        .get_async(
            "/users/:username/collections/featured",
            |_req, ctx| async move { featured_collection_response(ctx).await },
        )
        .head_async(
            "/users/:username/collections/featured",
            |_req, _ctx| async move { static_head_response(ACTIVITYPUB_CONTENT_TYPE) },
        )
        .get_async(
            "/users/:username/collections/tags",
            |_req, ctx| async move { featured_tags_collection_response(ctx).await },
        )
        .head_async(
            "/users/:username/collections/tags",
            |_req, _ctx| async move { static_head_response(ACTIVITYPUB_CONTENT_TYPE) },
        )
        .post_async("/inbox", |req, ctx| async move {
            shared_inbox_response(req, ctx).await
        })
        .post_async("/users/:username/inbox", |req, ctx| async move {
            inbox_response(req, ctx).await
        })
        .get_async("/users/:username/outbox", |req, ctx| async move {
            outbox_response(req, ctx).await
        })
        .head_async("/users/:username/outbox", |_req, _ctx| async move {
            static_head_response(ACTIVITYPUB_CONTENT_TYPE)
        })
        .post_async("/internal/outbox/process", |req, ctx| async move {
            process_outbox_deliveries(req, ctx).await
        })
}

pub(super) fn static_head_response(content_type: &str) -> Result<Response> {
    let mut response = Response::empty()?;
    response.headers_mut().set("Content-Type", content_type)?;
    Ok(response)
}
