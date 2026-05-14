use crate::{
    create_push_subscription_response, delete_push_subscription_response,
    push_subscription_response, update_push_subscription_response,
};
use worker::Router;

pub(crate) fn add_push_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .post_async("/api/v1/push/subscription", |req, ctx| async move {
            create_push_subscription_response(req, ctx).await
        })
        .get_async("/api/v1/push/subscription", |req, ctx| async move {
            push_subscription_response(req, ctx).await
        })
        .put_async("/api/v1/push/subscription", |req, ctx| async move {
            update_push_subscription_response(req, ctx).await
        })
        .patch_async("/api/v1/push/subscription", |req, ctx| async move {
            update_push_subscription_response(req, ctx).await
        })
        .delete_async("/api/v1/push/subscription", |req, ctx| async move {
            delete_push_subscription_response(req, ctx).await
        })
}
