use super::selection::FastRouterKind;
use crate::{
    account_directory, account_email_subscriptions_response, account_endorsements_response,
    account_featured_tags_response, account_followers_response, account_following_response,
    account_lists_response, account_lookup, account_relationships, account_response,
    account_search, account_statuses_response, accounts_index_response, announcements_response,
    approve_quote_response, auth0_callback_response, authorize_follow_request_response,
    authorize_interaction_response, authorize_interaction_submit_response, block_account,
    blocks_response, bookmark_status, bookmarks_response, create_account_placeholder_response,
    create_media_attachment, create_status, custom_emojis_response, delete_media_attachment,
    delete_status, direct_timeline_response, donation_campaigns_response, endorse_account_response,
    endorsements_response, familiar_followers_response, favourite_status, favourites_response,
    follow_account, follow_request_response, follow_requests_response, followed_tags_response,
    home_timeline_response, host_meta_response, identity_proofs_response,
    instance_activity_response, instance_domain_blocks_response,
    instance_extended_description_response, instance_languages_response, instance_peers_response,
    instance_privacy_policy_response, instance_rules_response, instance_summary_response,
    instance_terms_of_service_response, instance_terms_of_service_version_response,
    instance_translation_languages_response, instance_v2_response, link_timeline_response,
    list_timeline_response, media_content_response, media_metadata_response, mute_account,
    mute_status_response, mutes_response, nodeinfo_links_response, nodeinfo_response,
    note_account_response, oauth_authorization_server_response, oauth_authorize_response,
    oauth_token_response, oauth_userinfo_response, oembed_response, pin_account_response,
    pin_status_response, public_timeline_response, reblog_status, reject_follow_request_response,
    reject_quote_response, remove_from_followers_response, revoke_quote_response,
    status_api_response, status_card_response, status_context_response,
    status_favourited_by_response, status_history_response, status_interaction_policy_response,
    status_quotes_response, status_reblogged_by_response, status_source_response,
    statuses_index_placeholder_response, tag_timeline_response, translate_status_response,
    trending_links_response, trending_statuses_response, trending_tags_response, unblock_account,
    unbookmark_status, unendorse_account_response, unfavourite_status, unfollow_account,
    unmute_account, unmute_status_response, unpin_account_response, unpin_status_response,
    unreblog_status, update_credentials, update_media_attachment, update_status,
    verify_credentials, webfinger_response,
};
use worker::{Env, Request, Response, Result, Router};

pub(crate) async fn run_fast_router(
    kind: FastRouterKind,
    req: Request,
    env: Env,
) -> Result<Response> {
    match kind {
        FastRouterKind::Account => account_router().run(req, env).await,
        FastRouterKind::Discovery => discovery_router().run(req, env).await,
        FastRouterKind::Instance => instance_router().run(req, env).await,
        FastRouterKind::Media => media_router().run(req, env).await,
        FastRouterKind::OAuth => oauth_router().run(req, env).await,
        FastRouterKind::Status => status_router().run(req, env).await,
        FastRouterKind::Timeline => timeline_router().run(req, env).await,
    }
}

fn account_router() -> Router<'static, ()> {
    Router::new()
        .get_async("/api/v1/accounts", |req, ctx| async move {
            accounts_index_response(req, ctx).await
        })
        .post_async("/api/v1/accounts", |req, ctx| async move {
            create_account_placeholder_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/relationships", |req, ctx| async move {
            account_relationships(req, ctx).await
        })
        .get_async(
            "/api/v1/accounts/familiar_followers",
            |req, ctx| async move { familiar_followers_response(req, ctx).await },
        )
        .get_async("/api/v1/accounts/lookup", |req, ctx| async move {
            account_lookup(req, ctx).await
        })
        .get_async("/api/v1/accounts/search", |req, ctx| async move {
            account_search(req, ctx).await
        })
        .get_async(
            "/api/v1/accounts/verify_credentials",
            |req, ctx| async move { verify_credentials(req, ctx).await },
        )
        .patch_async(
            "/api/v1/accounts/update_credentials",
            |mut req, ctx| async move { update_credentials(&mut req, ctx).await },
        )
        .post_async("/api/v1/accounts/:id/follow", |req, ctx| async move {
            follow_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unfollow", |req, ctx| async move {
            unfollow_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/block", |req, ctx| async move {
            block_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unblock", |req, ctx| async move {
            unblock_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/mute", |mut req, ctx| async move {
            mute_account(&mut req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unmute", |req, ctx| async move {
            unmute_account(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id/followers", |req, ctx| async move {
            account_followers_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id/following", |req, ctx| async move {
            account_following_response(req, ctx).await
        })
        .get_async(
            "/api/v1/accounts/:id/featured_tags",
            |_req, ctx| async move { account_featured_tags_response(ctx).await },
        )
        .get_async("/api/v1/accounts/:id/endorsements", |req, ctx| async move {
            account_endorsements_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id/lists", |req, ctx| async move {
            account_lists_response(req, ctx).await
        })
        .get_async(
            "/api/v1/accounts/:id/identity_proofs",
            |req, ctx| async move { identity_proofs_response(req, ctx).await },
        )
        .post_async("/api/v1/accounts/:id/pin", |req, ctx| async move {
            pin_account_response(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unpin", |req, ctx| async move {
            unpin_account_response(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/endorse", |req, ctx| async move {
            endorse_account_response(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unendorse", |req, ctx| async move {
            unendorse_account_response(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/note", |req, ctx| async move {
            note_account_response(req, ctx).await
        })
        .post_async(
            "/api/v1/accounts/:id/email_subscriptions",
            |req, ctx| async move { account_email_subscriptions_response(req, ctx).await },
        )
        .post_async(
            "/api/v1/accounts/:id/remove_from_followers",
            |req, ctx| async move { remove_from_followers_response(req, ctx).await },
        )
        .get_async("/api/v1/accounts/:id/statuses", |req, ctx| async move {
            account_statuses_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id", |req, ctx| async move {
            account_response(req, ctx).await
        })
        .get_async("/api/v1/blocks", |req, ctx| async move {
            blocks_response(req, ctx).await
        })
        .get_async("/api/v1/directory", |req, ctx| async move {
            account_directory(req, ctx).await
        })
        .get_async("/api/v1/favourites", |req, ctx| async move {
            favourites_response(req, ctx).await
        })
        .get_async("/api/v1/endorsements", |req, ctx| async move {
            endorsements_response(req, ctx).await
        })
        .get_async("/api/v1/bookmarks", |req, ctx| async move {
            bookmarks_response(req, ctx).await
        })
        .get_async("/api/v1/followed_tags", |req, ctx| async move {
            followed_tags_response(req, ctx).await
        })
        .get_async("/api/v1/mutes", |req, ctx| async move {
            mutes_response(req, ctx).await
        })
        .get_async("/api/v1/follow_requests", |req, ctx| async move {
            follow_requests_response(req, ctx).await
        })
        .get_async("/api/v1/follow_requests/:id", |req, ctx| async move {
            follow_request_response(req, ctx).await
        })
        .post_async(
            "/api/v1/follow_requests/:id/authorize",
            |req, ctx| async move { authorize_follow_request_response(req, ctx).await },
        )
        .post_async(
            "/api/v1/follow_requests/:id/reject",
            |req, ctx| async move { reject_follow_request_response(req, ctx).await },
        )
}

fn discovery_router() -> Router<'static, ()> {
    use super::activitypub::{
        JRD_CONTENT_TYPE, JSON_CONTENT_TYPE, XRD_CONTENT_TYPE, static_head_response,
    };

    Router::new()
        .get_async(
            "/.well-known/oauth-authorization-server",
            |_req, ctx| async move { oauth_authorization_server_response(ctx).await },
        )
        .get_async("/.well-known/webfinger", |req, ctx| async move {
            webfinger_response(req, ctx).await
        })
        .head_async("/.well-known/webfinger", |_req, _ctx| async move {
            static_head_response(JRD_CONTENT_TYPE)
        })
        .get_async("/.well-known/host-meta", |_req, ctx| async move {
            host_meta_response(ctx).await
        })
        .head_async("/.well-known/host-meta", |_req, _ctx| async move {
            static_head_response(XRD_CONTENT_TYPE)
        })
        .get_async("/.well-known/host-meta.json", |_req, ctx| async move {
            host_meta_response(ctx).await
        })
        .head_async("/.well-known/host-meta.json", |_req, _ctx| async move {
            static_head_response(XRD_CONTENT_TYPE)
        })
        .get_async("/.well-known/nodeinfo", |_req, ctx| async move {
            nodeinfo_links_response(ctx).await
        })
        .head_async("/.well-known/nodeinfo", |_req, _ctx| async move {
            static_head_response(JSON_CONTENT_TYPE)
        })
        .get_async("/nodeinfo/2.0", |_req, ctx| async move {
            nodeinfo_response(ctx).await
        })
        .head_async("/nodeinfo/2.0", |_req, _ctx| async move {
            static_head_response(JSON_CONTENT_TYPE)
        })
        .get_async("/api/oembed", |req, ctx| async move {
            oembed_response(req, ctx).await
        })
        .get_async("/authorize_interaction", |req, ctx| async move {
            authorize_interaction_response(req, ctx).await
        })
        .post_async("/authorize_interaction", |req, ctx| async move {
            authorize_interaction_submit_response(req, ctx).await
        })
}

fn instance_router() -> Router<'static, ()> {
    Router::new()
        .get_async("/api/v1/instance", |_req, ctx| async move {
            instance_summary_response(ctx).await
        })
        .get_async("/api/v1/instance/peers", |_req, ctx| async move {
            instance_peers_response(ctx).await
        })
        .get_async("/api/v1/instance/activity", |_req, ctx| async move {
            instance_activity_response(ctx).await
        })
        .get_async("/api/v1/instance/rules", |_req, ctx| async move {
            instance_rules_response(ctx).await
        })
        .get_async("/api/v1/instance/domain_blocks", |_req, ctx| async move {
            instance_domain_blocks_response(ctx).await
        })
        .get_async(
            "/api/v1/instance/extended_description",
            |_req, ctx| async move { instance_extended_description_response(ctx).await },
        )
        .get_async("/api/v1/instance/privacy_policy", |_req, ctx| async move {
            instance_privacy_policy_response(ctx).await
        })
        .get_async(
            "/api/v1/instance/translation_languages",
            |_req, ctx| async move { instance_translation_languages_response(ctx).await },
        )
        .get_async(
            "/api/v1/instance/terms_of_service",
            |_req, ctx| async move { instance_terms_of_service_response(ctx).await },
        )
        .get_async(
            "/api/v1/instance/terms_of_service/:date",
            |_req, ctx| async move { instance_terms_of_service_version_response(ctx).await },
        )
        .get_async("/api/v1/instance/languages", |_req, ctx| async move {
            instance_languages_response(ctx).await
        })
        .get_async("/api/v2/instance", |_req, ctx| async move {
            instance_v2_response(ctx).await
        })
        .get_async("/api/v1/announcements", |req, ctx| async move {
            announcements_response(req, ctx).await
        })
        .get_async("/api/v1/donation_campaigns", |req, ctx| async move {
            donation_campaigns_response(req, ctx).await
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
}

fn timeline_router() -> Router<'static, ()> {
    Router::new()
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

fn status_router() -> Router<'static, ()> {
    Router::new()
        .get_async("/api/v1/statuses", |req, ctx| async move {
            statuses_index_placeholder_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id", |req, ctx| async move {
            status_api_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/card", |req, ctx| async move {
            status_card_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/source", |req, ctx| async move {
            status_source_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/history", |req, ctx| async move {
            status_history_response(req, ctx).await
        })
        .get_async(
            "/api/v1/statuses/:id/favourited_by",
            |req, ctx| async move { status_favourited_by_response(req, ctx).await },
        )
        .get_async("/api/v1/statuses/:id/reblogged_by", |req, ctx| async move {
            status_reblogged_by_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/context", |req, ctx| async move {
            status_context_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/quotes", |req, ctx| async move {
            status_quotes_response(req, ctx).await
        })
        .post_async("/api/v1/statuses", |req, ctx| async move {
            create_status(req, ctx).await
        })
        .put_async("/api/v1/statuses/:id", |req, ctx| async move {
            update_status(req, ctx).await
        })
        .patch_async("/api/v1/statuses/:id", |req, ctx| async move {
            update_status(req, ctx).await
        })
        .delete_async("/api/v1/statuses/:id", |req, ctx| async move {
            delete_status(req, ctx).await
        })
        .post_async(
            "/api/v1/statuses/:id/quotes/:quote_id/approve",
            |req, ctx| async move { approve_quote_response(req, ctx).await },
        )
        .post_async(
            "/api/v1/statuses/:id/quotes/:quote_id/reject",
            |req, ctx| async move { reject_quote_response(req, ctx).await },
        )
        .post_async(
            "/api/v1/statuses/:id/quotes/:quote_id/revoke",
            |req, ctx| async move { revoke_quote_response(req, ctx).await },
        )
        .put_async(
            "/api/v1/statuses/:id/interaction_policy",
            |req, ctx| async move { status_interaction_policy_response(req, ctx).await },
        )
        .patch_async(
            "/api/v1/statuses/:id/interaction_policy",
            |req, ctx| async move { status_interaction_policy_response(req, ctx).await },
        )
        .post_async("/api/v1/statuses/:id/translate", |req, ctx| async move {
            translate_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/favourite", |req, ctx| async move {
            favourite_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unfavourite", |req, ctx| async move {
            unfavourite_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/reblog", |mut req, ctx| async move {
            reblog_status(&mut req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unreblog", |req, ctx| async move {
            unreblog_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/pin", |req, ctx| async move {
            pin_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unpin", |req, ctx| async move {
            unpin_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/mute", |req, ctx| async move {
            mute_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unmute", |req, ctx| async move {
            unmute_status_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/bookmark", |req, ctx| async move {
            bookmark_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unbookmark", |req, ctx| async move {
            unbookmark_status(req, ctx).await
        })
}

fn media_router() -> Router<'static, ()> {
    Router::new()
        .get_async("/media/:id", |_req, ctx| async move {
            media_content_response(ctx).await
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

fn oauth_router() -> Router<'static, ()> {
    Router::new()
        .get_async("/oauth/userinfo", |req, ctx| async move {
            oauth_userinfo_response(req, ctx).await
        })
        .post_async("/oauth/userinfo", |req, ctx| async move {
            oauth_userinfo_response(req, ctx).await
        })
        .get_async("/oauth/authorize", |req, ctx| async move {
            oauth_authorize_response(req, ctx).await
        })
        .post_async("/oauth/authorize", |req, ctx| async move {
            oauth_authorize_response(req, ctx).await
        })
        .get_async("/oauth/auth0/callback", |req, ctx| async move {
            auth0_callback_response(req, ctx).await
        })
        .post_async("/oauth/token", |req, ctx| async move {
            oauth_token_response(req, ctx).await
        })
}
