use crate::{
    account_statuses_by_username_response, actor_response, featured_collection_response,
    featured_tags_collection_response, followers_collection_response,
    following_collection_response, inbox_response, nodeinfo_links_response, nodeinfo_response,
    outbox_response, process_outbox_deliveries, remote_follow_response, shared_inbox_response,
    webfinger_response,
};
use worker::Router;

pub(crate) fn add_activitypub_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/.well-known/webfinger", |req, ctx| async move {
            webfinger_response(req, ctx).await
        })
        .get_async("/.well-known/nodeinfo", |_req, ctx| async move {
            nodeinfo_links_response(ctx).await
        })
        .get_async("/nodeinfo/2.0", |_req, ctx| async move {
            nodeinfo_response(ctx).await
        })
        .get_async("/users/:username", |req, ctx| async move {
            actor_response(req, ctx).await
        })
        .get_async("/users/:username/statuses", |req, ctx| async move {
            account_statuses_by_username_response(req, ctx).await
        })
        .get_async("/users/:username/remote-follow", |req, ctx| async move {
            remote_follow_response(req, ctx).await
        })
        .get_async("/users/:username/followers", |req, ctx| async move {
            followers_collection_response(req, ctx).await
        })
        .get_async("/users/:username/following", |req, ctx| async move {
            following_collection_response(req, ctx).await
        })
        .get_async(
            "/users/:username/collections/featured",
            |_req, ctx| async move { featured_collection_response(ctx).await },
        )
        .get_async(
            "/users/:username/collections/tags",
            |_req, ctx| async move { featured_tags_collection_response(ctx).await },
        )
        .post_async("/inbox", |req, ctx| async move {
            shared_inbox_response(req, ctx).await
        })
        .post_async("/users/:username/inbox", |req, ctx| async move {
            inbox_response(req, ctx).await
        })
        .get_async("/users/:username/outbox", |_req, ctx| async move {
            outbox_response(ctx).await
        })
        .post_async("/internal/outbox/process", |req, ctx| async move {
            process_outbox_deliveries(req, ctx).await
        })
}
