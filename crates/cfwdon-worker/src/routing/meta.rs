use crate::{
    admin_create_custom_emoji_response, admin_custom_emojis_response,
    admin_delete_custom_emoji_response, admin_update_custom_emoji_response,
    announcement_reaction_mutation_response, announcements_response, annual_report_action_response,
    annual_report_response, annual_report_state_response, annual_reports_response,
    check_email_confirmation_response, create_email_confirmation_response, create_report,
    custom_emojis_response, dismiss_announcement_mutation_response, donation_campaigns_response,
    email_confirmation_page_response, markers_response, oembed_response, save_markers_response,
    streaming_placeholder_response, trending_links_response, trending_statuses_response,
    trending_tags_response,
};
use worker::Router;

pub(crate) fn add_meta_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/announcements", |req, ctx| async move {
            announcements_response(req, ctx).await
        })
        .put_async(
            "/api/v1/announcements/:announcement_id/reactions/:id",
            |req, ctx| async move { announcement_reaction_mutation_response(req, ctx).await },
        )
        .patch_async(
            "/api/v1/announcements/:announcement_id/reactions/:id",
            |req, ctx| async move { announcement_reaction_mutation_response(req, ctx).await },
        )
        .delete_async(
            "/api/v1/announcements/:announcement_id/reactions/:id",
            |req, ctx| async move { announcement_reaction_mutation_response(req, ctx).await },
        )
        .post_async("/api/v1/announcements/:id/dismiss", |req, ctx| async move {
            dismiss_announcement_mutation_response(req, ctx).await
        })
        .get_async("/api/v1/donation_campaigns", |req, ctx| async move {
            donation_campaigns_response(req, ctx).await
        })
        .get_async("/api/v1/annual_reports", |req, ctx| async move {
            annual_reports_response(req, ctx).await
        })
        .get_async("/api/v1/annual_reports/:id", |req, ctx| async move {
            annual_report_response(req, ctx).await
        })
        .post_async("/api/v1/annual_reports/:id/read", |req, ctx| async move {
            annual_report_action_response(req, ctx).await
        })
        .post_async(
            "/api/v1/annual_reports/:id/generate",
            |req, ctx| async move { annual_report_action_response(req, ctx).await },
        )
        .get_async("/api/v1/annual_reports/:id/state", |req, ctx| async move {
            annual_report_state_response(req, ctx).await
        })
        .post_async("/api/v1/emails/confirmations", |req, ctx| async move {
            create_email_confirmation_response(req, ctx).await
        })
        .get_async("/auth/confirmation", |req, ctx| async move {
            email_confirmation_page_response(req, ctx).await
        })
        .get_async("/api/v1/emails/check_confirmation", |req, ctx| async move {
            check_email_confirmation_response(req, ctx).await
        })
        .get_async("/api/v1/trends", |req, ctx| async move {
            trending_tags_response(req, ctx).await
        })
        .get_async("/api/v1/trends/statuses", |req, ctx| async move {
            trending_statuses_response(req, ctx).await
        })
        .get_async("/api/v1/trends/tags", |req, ctx| async move {
            trending_tags_response(req, ctx).await
        })
        .get_async("/api/v1/trends/links", |req, ctx| async move {
            trending_links_response(req, ctx).await
        })
        .get_async("/api/v1/custom_emojis", |_req, ctx| async move {
            custom_emojis_response(ctx).await
        })
        .get_async("/api/v1/admin/custom_emojis", |req, ctx| async move {
            admin_custom_emojis_response(req, ctx).await
        })
        .post_async("/api/v1/admin/custom_emojis", |req, ctx| async move {
            admin_create_custom_emoji_response(req, ctx).await
        })
        .patch_async("/api/v1/admin/custom_emojis/:id", |req, ctx| async move {
            admin_update_custom_emoji_response(req, ctx).await
        })
        .delete_async("/api/v1/admin/custom_emojis/:id", |req, ctx| async move {
            admin_delete_custom_emoji_response(req, ctx).await
        })
        .get_async("/api/oembed", |req, ctx| async move {
            oembed_response(req, ctx).await
        })
        .get_async("/api/v1/streaming", |req, ctx| async move {
            streaming_placeholder_response(req, ctx).await
        })
        .get_async("/api/v1/streaming/*any", |req, ctx| async move {
            streaming_placeholder_response(req, ctx).await
        })
        .post_async("/api/v1/reports", |mut req, ctx| async move {
            create_report(&mut req, ctx).await
        })
        .get_async("/api/v1/markers", |req, ctx| async move {
            markers_response(req, ctx).await
        })
        .post_async("/api/v1/markers", |req, ctx| async move {
            save_markers_response(req, ctx).await
        })
}
