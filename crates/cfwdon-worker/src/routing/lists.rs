use crate::lists::{
    add_list_accounts_response, create_list_response, delete_list_accounts_response,
    delete_list_response, list_accounts_response, list_response, lists_response,
    update_list_response,
};
use worker::Router;

pub(crate) fn add_list_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/lists", |req, ctx| async move {
            lists_response(req, ctx).await
        })
        .post_async("/api/v1/lists", |mut req, ctx| async move {
            create_list_response(&mut req, ctx).await
        })
        .get_async("/api/v1/lists/:id", |req, ctx| async move {
            list_response(req, ctx).await
        })
        .put_async("/api/v1/lists/:id", |mut req, ctx| async move {
            update_list_response(&mut req, ctx).await
        })
        .patch_async("/api/v1/lists/:id", |mut req, ctx| async move {
            update_list_response(&mut req, ctx).await
        })
        .delete_async("/api/v1/lists/:id", |req, ctx| async move {
            delete_list_response(req, ctx).await
        })
        .get_async("/api/v1/lists/:id/accounts", |req, ctx| async move {
            list_accounts_response(req, ctx).await
        })
        .post_async("/api/v1/lists/:id/accounts", |mut req, ctx| async move {
            add_list_accounts_response(&mut req, ctx).await
        })
        .delete_async("/api/v1/lists/:id/accounts", |mut req, ctx| async move {
            delete_list_accounts_response(&mut req, ctx).await
        })
}
