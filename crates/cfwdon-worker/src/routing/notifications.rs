use crate::{
    accept_notification_request_response, accept_notification_requests_response,
    dismiss_notification_request_response, dismiss_notification_requests_response,
    notification_dismiss_response, notification_group_accounts_response,
    notification_group_dismiss_response, notification_group_response,
    notification_request_response, notification_requests_merged_response,
    notification_requests_response, notification_response, notifications_clear_response,
    notifications_policy_response, notifications_response, notifications_unread_count_response,
    notifications_v2_response, update_notifications_policy_response,
};
use worker::Router;

pub(crate) fn add_notification_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/notifications", |req, ctx| async move {
            notifications_response(req, ctx).await
        })
        .get_async("/api/v1/notifications/requests", |req, ctx| async move {
            notification_requests_response(req, ctx).await
        })
        .get_async(
            "/api/v1/notifications/requests/:id",
            |req, ctx| async move { notification_request_response(req, ctx).await },
        )
        .post_async(
            "/api/v1/notifications/requests/accept",
            |mut req, ctx| async move { accept_notification_requests_response(&mut req, ctx).await },
        )
        .post_async(
            "/api/v1/notifications/requests/dismiss",
            |mut req, ctx| async move {
                dismiss_notification_requests_response(&mut req, ctx).await
            },
        )
        .get_async(
            "/api/v1/notifications/requests/merged",
            |req, ctx| async move { notification_requests_merged_response(req, ctx).await },
        )
        .post_async(
            "/api/v1/notifications/requests/:id/accept",
            |req, ctx| async move { accept_notification_request_response(req, ctx).await },
        )
        .post_async(
            "/api/v1/notifications/requests/:id/dismiss",
            |req, ctx| async move { dismiss_notification_request_response(req, ctx).await },
        )
        .get_async("/api/v2/notifications", |req, ctx| async move {
            notifications_v2_response(req, ctx).await
        })
        .get_async("/api/v2/notifications/:group_key", |req, ctx| async move {
            notification_group_response(req, ctx).await
        })
        .get_async("/api/v2/notifications/policy", |req, ctx| async move {
            notifications_policy_response(req, ctx).await
        })
        .get_async("/api/v1/notifications/policy", |req, ctx| async move {
            notifications_policy_response(req, ctx).await
        })
        .put_async("/api/v1/notifications/policy", |mut req, ctx| async move {
            update_notifications_policy_response(&mut req, ctx).await
        })
        .patch_async("/api/v1/notifications/policy", |mut req, ctx| async move {
            update_notifications_policy_response(&mut req, ctx).await
        })
        .put_async("/api/v2/notifications/policy", |mut req, ctx| async move {
            update_notifications_policy_response(&mut req, ctx).await
        })
        .patch_async("/api/v2/notifications/policy", |mut req, ctx| async move {
            update_notifications_policy_response(&mut req, ctx).await
        })
        .get_async(
            "/api/v1/notifications/unread_count",
            |req, ctx| async move { notifications_unread_count_response(req, ctx).await },
        )
        .get_async(
            "/api/v2/notifications/unread_count",
            |req, ctx| async move { notifications_unread_count_response(req, ctx).await },
        )
        .get_async("/api/v1/notifications/:id", |req, ctx| async move {
            notification_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/clear", |req, ctx| async move {
            notifications_clear_response(req, ctx).await
        })
        .post_async("/api/v2/notifications/clear", |req, ctx| async move {
            notifications_clear_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/:id/dismiss", |req, ctx| async move {
            notification_dismiss_response(req, ctx).await
        })
        .post_async(
            "/api/v2/notifications/:group_key/dismiss",
            |req, ctx| async move { notification_group_dismiss_response(req, ctx).await },
        )
        .get_async(
            "/api/v2/notifications/:group_key/accounts",
            |req, ctx| async move { notification_group_accounts_response(req, ctx).await },
        )
}
