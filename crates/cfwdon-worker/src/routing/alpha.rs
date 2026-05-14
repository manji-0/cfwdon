use crate::{
    alpha_account_collections_response, alpha_account_in_collections_response,
    alpha_collection_response, async_refresh_response, create_alpha_collection_item_response,
    create_alpha_collection_response, delete_alpha_collection_item_response,
    delete_alpha_collection_response, revoke_alpha_collection_item_response,
    update_alpha_collection_response,
};
use worker::Router;

pub(crate) fn add_alpha_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1_alpha/async_refreshes/:id", |req, ctx| async move {
            async_refresh_response(req, ctx).await
        })
        .get_async(
            "/api/v1_alpha/accounts/:account_id/collections",
            |req, ctx| async move { alpha_account_collections_response(req, ctx).await },
        )
        .get_async(
            "/api/v1_alpha/accounts/:account_id/in_collections",
            |req, ctx| async move { alpha_account_in_collections_response(req, ctx).await },
        )
        .get_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            alpha_collection_response(req, ctx).await
        })
        .post_async("/api/v1_alpha/collections", |req, ctx| async move {
            create_alpha_collection_response(req, ctx).await
        })
        .put_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            update_alpha_collection_response(req, ctx).await
        })
        .patch_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            update_alpha_collection_response(req, ctx).await
        })
        .delete_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            delete_alpha_collection_response(req, ctx).await
        })
        .post_async(
            "/api/v1_alpha/collections/:collection_id/items",
            |req, ctx| async move { create_alpha_collection_item_response(req, ctx).await },
        )
        .delete_async(
            "/api/v1_alpha/collections/:collection_id/items/:id",
            |req, ctx| async move { delete_alpha_collection_item_response(req, ctx).await },
        )
        .post_async(
            "/api/v1_alpha/collections/:collection_id/items/:id/revoke",
            |req, ctx| async move { revoke_alpha_collection_item_response(req, ctx).await },
        )
}
