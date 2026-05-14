use crate::{
    feature_tag_response, feature_tag_v1_response, featured_tag_suggestions_response,
    featured_tags_response, follow_tag_response, tag_response, unfeature_tag_response,
    unfeature_tag_v1_response, unfollow_tag_response,
};
use worker::Router;

pub(crate) fn add_tag_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/tags/:name", |_req, ctx| async move {
            tag_response(ctx).await
        })
        .post_async("/api/v1/tags/:id/follow", |req, ctx| async move {
            follow_tag_response(req, ctx).await
        })
        .post_async("/api/v1/tags/:id/unfollow", |req, ctx| async move {
            unfollow_tag_response(req, ctx).await
        })
        .post_async("/api/v1/tags/:id/feature", |req, ctx| async move {
            feature_tag_v1_response(req, ctx).await
        })
        .post_async("/api/v1/tags/:id/unfeature", |req, ctx| async move {
            unfeature_tag_v1_response(req, ctx).await
        })
        .get_async("/api/v1/featured_tags", |req, ctx| async move {
            featured_tags_response(req, ctx).await
        })
        .post_async("/api/v1/featured_tags", |mut req, ctx| async move {
            feature_tag_response(&mut req, ctx).await
        })
        .delete_async("/api/v1/featured_tags/:id", |req, ctx| async move {
            unfeature_tag_response(req, ctx).await
        })
        .get_async("/api/v1/featured_tags/suggestions", |req, ctx| async move {
            featured_tag_suggestions_response(req, ctx).await
        })
}
