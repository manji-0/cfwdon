use crate::{
    create_media_attachment, delete_media_attachment, media_content_response,
    media_metadata_response, prune_orphan_media, update_media_attachment,
};
use worker::Router;

pub(crate) fn add_media_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/media/:id", |_req, ctx| async move {
            media_content_response(ctx).await
        })
        .post_async("/internal/media/prune-orphans", |req, ctx| async move {
            prune_orphan_media(req, ctx).await
        })
        .post_async("/api/v1/media", |req, ctx| async move {
            create_media_attachment(req, ctx).await
        })
        .post_async("/api/v2/media", |req, ctx| async move {
            create_media_attachment(req, ctx).await
        })
        .get_async("/api/v1/media/:id", |_req, ctx| async move {
            media_metadata_response(ctx).await
        })
        .delete_async("/api/v1/media/:id", |req, ctx| async move {
            delete_media_attachment(req, ctx).await
        })
        .put_async("/api/v1/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
        .patch_async("/api/v1/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
        .put_async("/api/v2/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
        .patch_async("/api/v2/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
}
