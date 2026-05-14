use crate::{
    create_filter_keyword_response, create_filter_status_response, create_filter_v1_response,
    create_filter_v2_response, delete_filter_keyword_response, delete_filter_status_response,
    delete_filter_v1_response, delete_filter_v2_response, filter_keyword_response,
    filter_keywords_response, filter_status_response, filter_statuses_response, filter_v1_response,
    filter_v2_response, filters_v1_response, filters_v2_response, update_filter_keyword_response,
    update_filter_v1_response, update_filter_v2_response,
};
use worker::Router;

pub(crate) fn add_filter_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/filters", |req, ctx| async move {
            filters_v1_response(req, ctx).await
        })
        .post_async("/api/v1/filters", |mut req, ctx| async move {
            create_filter_v1_response(&mut req, ctx).await
        })
        .get_async("/api/v1/filters/:id", |req, ctx| async move {
            filter_v1_response(req, ctx).await
        })
        .put_async("/api/v1/filters/:id", |mut req, ctx| async move {
            update_filter_v1_response(&mut req, ctx).await
        })
        .patch_async("/api/v1/filters/:id", |mut req, ctx| async move {
            update_filter_v1_response(&mut req, ctx).await
        })
        .delete_async("/api/v1/filters/:id", |req, ctx| async move {
            delete_filter_v1_response(req, ctx).await
        })
        .get_async("/api/v2/filters", |req, ctx| async move {
            filters_v2_response(req, ctx).await
        })
        .post_async("/api/v2/filters", |mut req, ctx| async move {
            create_filter_v2_response(&mut req, ctx).await
        })
        .get_async("/api/v2/filters/:id", |req, ctx| async move {
            filter_v2_response(req, ctx).await
        })
        .put_async("/api/v2/filters/:id", |mut req, ctx| async move {
            update_filter_v2_response(&mut req, ctx).await
        })
        .patch_async("/api/v2/filters/:id", |mut req, ctx| async move {
            update_filter_v2_response(&mut req, ctx).await
        })
        .delete_async("/api/v2/filters/:id", |req, ctx| async move {
            delete_filter_v2_response(req, ctx).await
        })
        .get_async("/api/v2/filters/:id/keywords", |req, ctx| async move {
            filter_keywords_response(req, ctx).await
        })
        .post_async("/api/v2/filters/:id/keywords", |mut req, ctx| async move {
            create_filter_keyword_response(&mut req, ctx).await
        })
        .get_async("/api/v2/filters/keywords/:id", |req, ctx| async move {
            filter_keyword_response(req, ctx).await
        })
        .put_async("/api/v2/filters/keywords/:id", |mut req, ctx| async move {
            update_filter_keyword_response(&mut req, ctx).await
        })
        .patch_async("/api/v2/filters/keywords/:id", |mut req, ctx| async move {
            update_filter_keyword_response(&mut req, ctx).await
        })
        .delete_async("/api/v2/filters/keywords/:id", |req, ctx| async move {
            delete_filter_keyword_response(req, ctx).await
        })
        .get_async("/api/v2/filters/:id/statuses", |req, ctx| async move {
            filter_statuses_response(req, ctx).await
        })
        .post_async("/api/v2/filters/:id/statuses", |mut req, ctx| async move {
            create_filter_status_response(&mut req, ctx).await
        })
        .get_async("/api/v2/filters/statuses/:id", |req, ctx| async move {
            filter_status_response(req, ctx).await
        })
        .delete_async("/api/v2/filters/statuses/:id", |req, ctx| async move {
            delete_filter_status_response(req, ctx).await
        })
}
