use super::activitypub::{ACTIVITYPUB_CONTENT_TYPE, static_head_response};
use crate::{
    bookmark_status, create_status, delete_status, favourite_status, mute_status_response,
    pin_status_response, reblog_status, revoke_quote_response, status_api_response,
    status_card_response, status_context_response, status_favourited_by_response,
    status_history_response, status_interaction_policy_response, status_object_response,
    status_quotes_response, status_reblogged_by_response, status_source_response,
    statuses_index_placeholder_response, translate_status_response, unbookmark_status,
    unfavourite_status, unmute_status_response, unpin_status_response, unreblog_status,
    update_status,
};
use worker::Router;

pub(crate) fn add_status_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/statuses", |req, ctx| async move {
            statuses_index_placeholder_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id", |req, ctx| async move {
            status_api_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/card", |req, ctx| async move {
            status_card_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/source", |req, ctx| async move {
            status_source_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/history", |req, ctx| async move {
            status_history_response(req, ctx).await
        })
        .get_async(
            "/api/v1/statuses/:id/favourited_by",
            |req, ctx| async move { status_favourited_by_response(req, ctx).await },
        )
        .get_async("/api/v1/statuses/:id/reblogged_by", |req, ctx| async move {
            status_reblogged_by_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/context", |req, ctx| async move {
            status_context_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/quotes", |req, ctx| async move {
            status_quotes_response(req, ctx).await
        })
        .post_async(
            "/api/v1/statuses/:id/quotes/:quote_id/revoke",
            |req, ctx| async move { revoke_quote_response(req, ctx).await },
        )
        .put_async(
            "/api/v1/statuses/:id/interaction_policy",
            |req, ctx| async move { status_interaction_policy_response(req, ctx).await },
        )
        .patch_async(
            "/api/v1/statuses/:id/interaction_policy",
            |req, ctx| async move { status_interaction_policy_response(req, ctx).await },
        )
        .post_async("/api/v1/statuses/:id/translate", |req, ctx| async move {
            translate_status_response(req, ctx).await
        })
        .get_async("/users/:username/statuses/:id", |req, ctx| async move {
            status_object_response(req, ctx).await
        })
        .head_async("/users/:username/statuses/:id", |_req, _ctx| async move {
            static_head_response(ACTIVITYPUB_CONTENT_TYPE)
        })
        .post_async("/api/v1/statuses", |req, ctx| async move {
            create_status(req, ctx).await
        })
        .put_async("/api/v1/statuses/:id", |req, ctx| async move {
            update_status(req, ctx).await
        })
        .patch_async("/api/v1/statuses/:id", |req, ctx| async move {
            update_status(req, ctx).await
        })
        .delete_async("/api/v1/statuses/:id", |req, ctx| async move {
            delete_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/favourite", |req, ctx| async move {
            favourite_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unfavourite", |req, ctx| async move {
            unfavourite_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/reblog", |mut req, ctx| async move {
            reblog_status(&mut req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unreblog", |req, ctx| async move {
            unreblog_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/pin", |req, ctx| async move {
            pin_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unpin", |req, ctx| async move {
            unpin_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/mute", |req, ctx| async move {
            mute_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unmute", |req, ctx| async move {
            unmute_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/bookmark", |req, ctx| async move {
            bookmark_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unbookmark", |req, ctx| async move {
            unbookmark_status(req, ctx).await
        })
}
