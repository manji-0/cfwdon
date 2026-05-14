use crate::{
    conversations_response, delete_conversation_response, read_conversation_response,
    unread_conversation_response,
};
use worker::Router;

pub(crate) fn add_conversation_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/conversations", |req, ctx| async move {
            conversations_response(req, ctx).await
        })
        .delete_async("/api/v1/conversations/:id", |req, ctx| async move {
            delete_conversation_response(req, ctx).await
        })
        .post_async("/api/v1/conversations/:id/read", |req, ctx| async move {
            read_conversation_response(req, ctx).await
        })
        .post_async("/api/v1/conversations/:id/unread", |req, ctx| async move {
            unread_conversation_response(req, ctx).await
        })
}
