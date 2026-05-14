use crate::{
    delete_scheduled_status_response, poll_response, process_expired_polls,
    scheduled_status_response, scheduled_statuses_response, update_scheduled_status_response,
    vote_in_poll,
};
use worker::Router;

pub(crate) fn add_poll_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .post_async("/internal/polls/process-expired", |req, ctx| async move {
            process_expired_polls(req, ctx).await
        })
        .get_async("/api/v1/polls/:id", |req, ctx| async move {
            poll_response(req, ctx).await
        })
        .post_async("/api/v1/polls/:id/votes", |mut req, ctx| async move {
            vote_in_poll(&mut req, ctx).await
        })
        .get_async("/api/v1/scheduled_statuses", |req, ctx| async move {
            scheduled_statuses_response(req, ctx).await
        })
        .get_async("/api/v1/scheduled_statuses/:id", |req, ctx| async move {
            scheduled_status_response(req, ctx).await
        })
        .put_async("/api/v1/scheduled_statuses/:id", |req, ctx| async move {
            update_scheduled_status_response(req, ctx).await
        })
        .patch_async("/api/v1/scheduled_statuses/:id", |req, ctx| async move {
            update_scheduled_status_response(req, ctx).await
        })
        .delete_async("/api/v1/scheduled_statuses/:id", |req, ctx| async move {
            delete_scheduled_status_response(req, ctx).await
        })
}
