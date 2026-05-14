use crate::{
    delete_suggestion_response, search_v1, search_v2, suggestions_v1_response,
    suggestions_v2_response,
};
use worker::Router;

pub(crate) fn add_search_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/search", |req, ctx| async move {
            search_v1(req, ctx).await
        })
        .get_async("/api/v2/search", |req, ctx| async move {
            search_v2(req, ctx).await
        })
        .get_async("/api/v1/suggestions", |req, ctx| async move {
            suggestions_v1_response(req, ctx).await
        })
        .get_async("/api/v2/suggestions", |req, ctx| async move {
            suggestions_v2_response(req, ctx).await
        })
        .delete_async("/api/v1/suggestions/:id", |req, ctx| async move {
            delete_suggestion_response(req, ctx).await
        })
}
