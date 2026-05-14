use crate::{
    direct_timeline_response, home_timeline_response, link_timeline_response,
    list_timeline_response, public_timeline_response, tag_timeline_response,
};
use worker::Router;

pub(crate) fn add_timeline_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/timelines/link", |req, ctx| async move {
            link_timeline_response(req, ctx).await
        })
        .get_async("/api/v1/timelines/home", |req, ctx| async move {
            home_timeline_response(req, ctx).await
        })
        .get_async("/api/v1/timelines/direct", |req, ctx| async move {
            direct_timeline_response(req, ctx).await
        })
        .get_async("/api/v1/timelines/public", |req, ctx| async move {
            public_timeline_response(req, ctx).await
        })
        .get_async("/api/v1/timelines/tag/:hashtag", |req, ctx| async move {
            tag_timeline_response(req, ctx).await
        })
        .get_async("/api/v1/timelines/list/:id", |req, ctx| async move {
            list_timeline_response(req, ctx).await
        })
}
