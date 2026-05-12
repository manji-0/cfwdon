use super::{
    accept_notification_request_response, accept_notification_requests_response, account_directory,
    account_email_subscriptions_response, account_endorsements_response,
    account_featured_tags_response, account_followers_response, account_following_response,
    account_lists_response, account_lookup, account_relationships, account_response,
    account_search, account_statuses_by_username_response, account_statuses_response,
    accounts_index_response, actor_response, add_list_accounts_response,
    alpha_account_collections_response, alpha_account_in_collections_response,
    alpha_collection_response, announcement_reaction_mutation_response, announcements_response,
    annual_report_action_response, annual_report_response, annual_report_state_response,
    annual_reports_response, app_verify_credentials_response, async_refresh_response,
    authorize_follow_request_response, authorize_interaction_response,
    authorize_interaction_submit_response, block_account, blocks_response, bookmark_status,
    bookmarks_response, check_email_confirmation_response, conversations_response,
    create_account_placeholder_response, create_alpha_collection_item_response,
    create_alpha_collection_response, create_app_response, create_domain_block_response,
    create_email_confirmation_response, create_filter_keyword_response,
    create_filter_status_response, create_filter_v1_response, create_filter_v2_response,
    create_list_response, create_media_attachment, create_push_subscription_response,
    create_report, create_status, custom_emojis_response, custom_emojis_response_direct,
    delete_alpha_collection_item_response, delete_alpha_collection_response,
    delete_conversation_response, delete_domain_block_response, delete_filter_keyword_response,
    delete_filter_status_response, delete_filter_v1_response, delete_filter_v2_response,
    delete_list_accounts_response, delete_list_response, delete_media_attachment,
    delete_profile_avatar_response, delete_profile_header_response,
    delete_push_subscription_response, delete_scheduled_status_response, delete_status,
    delete_suggestion_response, direct_timeline_response, dismiss_announcement_mutation_response,
    dismiss_notification_request_response, dismiss_notification_requests_response,
    domain_blocks_preview_response, domain_blocks_response, donation_campaigns_response,
    email_confirmation_page_response, endorse_account_response, endorsements_response,
    familiar_followers_response, favourite_status, favourites_response, feature_tag_response,
    feature_tag_v1_response, featured_collection_response, featured_tag_suggestions_response,
    featured_tags_collection_response, featured_tags_response, filter_keyword_response,
    filter_keywords_response, filter_status_response, filter_statuses_response, filter_v1_response,
    filter_v2_response, filters_v1_response, filters_v2_response, follow_account,
    follow_request_response, follow_requests_response, follow_tag_response, followed_tags_response,
    followers_collection_response, following_collection_response, home_timeline_response,
    identity_proofs_response, inbox_response, instance_activity_response,
    instance_domain_blocks_response, instance_domain_blocks_response_direct,
    instance_extended_description_response, instance_languages_response,
    instance_languages_response_from_env, instance_peers_response, instance_peers_search_response,
    instance_privacy_policy_response, instance_rules_response, instance_rules_response_direct,
    instance_summary_response, instance_summary_response_from_env,
    instance_terms_of_service_response, instance_terms_of_service_version_response,
    instance_translation_languages_response, instance_v2_response, instance_v2_response_from_env,
    link_timeline_response, list_accounts_response, list_response, list_timeline_response,
    lists_response, markers_response, media_content_response, media_metadata_response,
    mute_account, mute_status_response, mutes_response, nodeinfo_links_response,
    nodeinfo_links_response_from_env, nodeinfo_response, nodeinfo_response_from_env,
    note_account_response, notification_dismiss_response, notification_group_accounts_response,
    notification_group_dismiss_response, notification_group_response,
    notification_request_response, notification_requests_merged_response,
    notification_requests_response, notification_response, notifications_clear_response,
    notifications_policy_response, notifications_response, notifications_unread_count_response,
    notifications_v2_response, oauth_authorization_server_response,
    oauth_authorization_server_response_from_env, oauth_authorize_response, oauth_token_response,
    oauth_userinfo_response, oembed_response, outbox_response, pin_account_response,
    pin_status_response, poll_response, preferences_response, process_expired_polls,
    process_outbox_deliveries, profile_response, prune_orphan_media, public_timeline_response,
    push_subscription_response, read_conversation_response, reblog_status,
    reject_follow_request_response, remote_follow_response, remove_from_followers_response,
    revoke_alpha_collection_item_response, revoke_quote_response, root_document,
    save_markers_response, scheduled_status_response, scheduled_statuses_response, search_v1,
    search_v2, shared_inbox_response, status_api_response, status_card_response,
    status_context_response, status_favourited_by_response, status_history_response,
    status_interaction_policy_response, status_object_response, status_quotes_response,
    status_reblogged_by_response, status_source_response, statuses_index_placeholder_response,
    streaming_placeholder_response, suggestions_v1_response, suggestions_v2_response, tag_response,
    tag_timeline_response, translate_status_response, trending_links_response,
    trending_statuses_response, trending_tags_response, unblock_account, unbookmark_status,
    unendorse_account_response, unfavourite_status, unfeature_tag_response,
    unfeature_tag_v1_response, unfollow_account, unfollow_tag_response, unmute_account,
    unmute_status_response, unpin_account_response, unpin_status_response,
    unread_conversation_response, unreblog_status, update_alpha_collection_response,
    update_credentials, update_filter_keyword_response, update_filter_v1_response,
    update_filter_v2_response, update_list_response, update_media_attachment,
    update_notifications_policy_response, update_profile_response,
    update_push_subscription_response, update_scheduled_status_response, update_status,
    verify_credentials, vote_in_poll, webfinger_response,
};
use crate::{
    add_log_message, log_json_event, observability_duration_ms, observability_started_at_ms,
};
use worker::{Env, Request, Response, Result, Router};

pub(crate) async fn handle_fetch(req: Request, env: Env) -> Result<Response> {
    let request_started_at_ms = observability_started_at_ms();
    let request_url = req.url()?;
    let request_path = request_url.path().to_owned();
    let request_method = req.method().to_string();
    let request_origin = req.headers().get("Origin")?;
    let request_user_agent = req.headers().get("User-Agent")?.unwrap_or_default();
    let log_api_requests = api_request_logging_enabled(&env);
    if request_method == "OPTIONS" && is_cors_enabled_path(&request_path) {
        let response = cors_preflight_response(request_origin.as_deref())?;
        log_api_request(
            log_api_requests,
            &request_method,
            &request_path,
            response.status_code(),
            &request_user_agent,
            observability_duration_ms(request_started_at_ms),
        );
        return Ok(response);
    }

    if request_method == "GET" && request_path == "/" {
        let response = Response::from_json(&root_document())?;
        log_api_request(
            log_api_requests,
            &request_method,
            &request_path,
            response.status_code(),
            &request_user_agent,
            observability_duration_ms(request_started_at_ms),
        );
        return Ok(response);
    }

    if request_method == "GET" && request_path == "/healthz" {
        let response = Response::ok("ok")?;
        log_api_request(
            log_api_requests,
            &request_method,
            &request_path,
            response.status_code(),
            &request_user_agent,
            observability_duration_ms(request_started_at_ms),
        );
        return Ok(response);
    }

    if let Some(response) =
        dispatch_exact_without_router(&request_method, &request_path, &env).await?
    {
        return finish_response(
            response,
            &request_method,
            &request_path,
            request_origin.as_deref(),
            &request_user_agent,
            log_api_requests,
            request_started_at_ms,
        );
    }

    if let Some(kind) = fast_router_kind(&request_method, &request_path) {
        let response = run_fast_router(kind, req, env).await?;
        return finish_response(
            response,
            &request_method,
            &request_path,
            request_origin.as_deref(),
            &request_user_agent,
            log_api_requests,
            request_started_at_ms,
        );
    }

    let response = Router::new()
        .get("/", |_req, _ctx| Response::from_json(&root_document()))
        .get("/healthz", |_req, _ctx| Response::ok("ok"))
        .get_async("/.well-known/oauth-authorization-server", |_req, ctx| async move {
            oauth_authorization_server_response(ctx).await
        })
        .get_async("/api/v1/instance", |_req, ctx| async move {
            instance_summary_response(ctx).await
        })
        .get_async("/api/v1_alpha/async_refreshes/:id", |req, ctx| async move {
            async_refresh_response(req, ctx).await
        })
        .get_async(
            "/api/v1_alpha/accounts/:account_id/collections",
            |req, ctx| async move { alpha_account_collections_response(req, ctx).await },
        )
        .get_async(
            "/api/v1_alpha/accounts/:account_id/in_collections",
            |req, ctx| async move { alpha_account_in_collections_response(req, ctx).await },
        )
        .get_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            alpha_collection_response(req, ctx).await
        })
        .post_async("/api/v1_alpha/collections", |req, ctx| async move {
            create_alpha_collection_response(req, ctx).await
        })
        .put_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            update_alpha_collection_response(req, ctx).await
        })
        .patch_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            update_alpha_collection_response(req, ctx).await
        })
        .delete_async("/api/v1_alpha/collections/:id", |req, ctx| async move {
            delete_alpha_collection_response(req, ctx).await
        })
        .post_async(
            "/api/v1_alpha/collections/:collection_id/items",
            |req, ctx| async move { create_alpha_collection_item_response(req, ctx).await },
        )
        .delete_async(
            "/api/v1_alpha/collections/:collection_id/items/:id",
            |req, ctx| async move { delete_alpha_collection_item_response(req, ctx).await },
        )
        .post_async(
            "/api/v1_alpha/collections/:collection_id/items/:id/revoke",
            |req, ctx| async move { revoke_alpha_collection_item_response(req, ctx).await },
        )
        .get_async("/api/v1/instance/peers", |_req, ctx| async move {
            instance_peers_response(ctx).await
        })
        .get_async("/api/v1/peers/search", |req, ctx| async move {
            instance_peers_search_response(req, ctx).await
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
        .get_async("/api/v1/domain_blocks/preview", |req, ctx| async move {
            domain_blocks_preview_response(req, ctx).await
        })
        .get_async("/api/v1/domain_blocks", |req, ctx| async move {
            domain_blocks_response(req, ctx).await
        })
        .post_async("/api/v1/domain_blocks", |req, ctx| async move {
            create_domain_block_response(req, ctx).await
        })
        .delete_async("/api/v1/domain_blocks", |req, ctx| async move {
            delete_domain_block_response(req, ctx).await
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
        .post_async("/api/v1/annual_reports/:id/generate", |req, ctx| async move {
            annual_report_action_response(req, ctx).await
        })
        .get_async("/api/v1/annual_reports/:id/state", |req, ctx| async move {
            annual_report_state_response(req, ctx).await
        })
        .get_async("/api/v1/apps/verify_credentials", |req, ctx| async move {
            app_verify_credentials_response(req, ctx).await
        })
        .post_async("/api/v1/apps", |req, ctx| async move {
            create_app_response(req, ctx).await
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
        .get_async("/api/v1/suggestions", |req, ctx| async move {
            suggestions_v1_response(req, ctx).await
        })
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
        .post_async("/api/v1/statuses/:id/quotes/:quote_id/revoke", |req, ctx| async move {
            revoke_quote_response(req, ctx).await
        })
        .put_async("/api/v1/statuses/:id/interaction_policy", |req, ctx| async move {
            status_interaction_policy_response(req, ctx).await
        })
        .patch_async("/api/v1/statuses/:id/interaction_policy", |req, ctx| async move {
            status_interaction_policy_response(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/translate", |req, ctx| async move {
            translate_status_response(req, ctx).await
        })
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
        .get_async("/.well-known/webfinger", |req, ctx| async move {
            webfinger_response(req, ctx).await
        })
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
        .post_async("/oauth/token", |req, ctx| async move {
            oauth_token_response(req, ctx).await
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
        .get_async("/.well-known/nodeinfo", |_req, ctx| async move {
            nodeinfo_links_response(ctx).await
        })
        .get_async("/nodeinfo/2.0", |_req, ctx| async move {
            nodeinfo_response(ctx).await
        })
        .get_async("/users/:username", |req, ctx| async move {
            actor_response(req, ctx).await
        })
        .get_async("/users/:username/statuses", |req, ctx| async move {
            account_statuses_by_username_response(req, ctx).await
        })
        .get_async("/users/:username/remote-follow", |req, ctx| async move {
            remote_follow_response(req, ctx).await
        })
        .get_async("/users/:username/followers", |req, ctx| async move {
            followers_collection_response(req, ctx).await
        })
        .get_async("/users/:username/following", |req, ctx| async move {
            following_collection_response(req, ctx).await
        })
        .get_async(
            "/users/:username/collections/featured",
            |_req, ctx| async move { featured_collection_response(ctx).await },
        )
        .get_async(
            "/users/:username/collections/tags",
            |_req, ctx| async move { featured_tags_collection_response(ctx).await },
        )
        .post_async("/inbox", |req, ctx| async move {
            shared_inbox_response(req, ctx).await
        })
        .post_async("/users/:username/inbox", |req, ctx| async move {
            inbox_response(req, ctx).await
        })
        .get_async("/api/v1/streaming", |req, ctx| async move {
            streaming_placeholder_response(req, ctx).await
        })
        .get_async("/api/v1/streaming/*any", |req, ctx| async move {
            streaming_placeholder_response(req, ctx).await
        })
        .get_async("/users/:username/outbox", |_req, ctx| async move {
            outbox_response(ctx).await
        })
        .get_async("/users/:username/statuses/:id", |req, ctx| async move {
            status_object_response(req, ctx).await
        })
        .get_async("/media/:id", |_req, ctx| async move {
            media_content_response(ctx).await
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
        .post_async("/internal/outbox/process", |req, ctx| async move {
            process_outbox_deliveries(req, ctx).await
        })
        .post_async("/internal/media/prune-orphans", |req, ctx| async move {
            prune_orphan_media(req, ctx).await
        })
        .post_async("/internal/polls/process-expired", |req, ctx| async move {
            process_expired_polls(req, ctx).await
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
        .get_async("/api/v1/accounts", |req, ctx| async move {
            accounts_index_response(req, ctx).await
        })
        .post_async("/api/v1/accounts", |req, ctx| async move {
            create_account_placeholder_response(req, ctx).await
        })
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
        .get_async("/api/v1/accounts/relationships", |req, ctx| async move {
            account_relationships(req, ctx).await
        })
        .get_async(
            "/api/v1/accounts/familiar_followers",
            |req, ctx| async move { familiar_followers_response(req, ctx).await },
        )
        .get_async("/api/v1/accounts/:id/followers", |req, ctx| async move {
            account_followers_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id/following", |req, ctx| async move {
            account_following_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id/featured_tags", |_req, ctx| async move {
            account_featured_tags_response(ctx).await
        })
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
        .get_async("/api/v1/blocks", |req, ctx| async move {
            blocks_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/lookup", |req, ctx| async move {
            account_lookup(req, ctx).await
        })
        .get_async("/api/v1/accounts/search", |req, ctx| async move {
            account_search(req, ctx).await
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
        .get_async("/api/v1/notifications", |req, ctx| async move {
            notifications_response(req, ctx).await
        })
        .get_async("/api/v1/notifications/requests", |req, ctx| async move {
            notification_requests_response(req, ctx).await
        })
        .get_async("/api/v1/notifications/requests/:id", |req, ctx| async move {
            notification_request_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/requests/accept", |mut req, ctx| async move {
            accept_notification_requests_response(&mut req, ctx).await
        })
        .post_async("/api/v1/notifications/requests/dismiss", |mut req, ctx| async move {
            dismiss_notification_requests_response(&mut req, ctx).await
        })
        .get_async("/api/v1/notifications/requests/merged", |req, ctx| async move {
            notification_requests_merged_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/requests/:id/accept", |req, ctx| async move {
            accept_notification_request_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/requests/:id/dismiss", |req, ctx| async move {
            dismiss_notification_request_response(req, ctx).await
        })
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
        .post_async("/api/v1/accounts/:id/email_subscriptions", |req, ctx| async move {
            account_email_subscriptions_response(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/remove_from_followers", |req, ctx| async move {
            remove_from_followers_response(req, ctx).await
        })
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
        .post_async("/api/v2/notifications/:group_key/dismiss", |req, ctx| async move {
            notification_group_dismiss_response(req, ctx).await
        })
        .get_async("/api/v2/notifications/:group_key/accounts", |req, ctx| async move {
            notification_group_accounts_response(req, ctx).await
        })
        .get_async("/api/v1/search", |req, ctx| async move {
            search_v1(req, ctx).await
        })
        .get_async("/api/v2/search", |req, ctx| async move {
            search_v2(req, ctx).await
        })
        .get_async("/api/v2/suggestions", |req, ctx| async move {
            suggestions_v2_response(req, ctx).await
        })
        .get_async("/api/v1/polls/:id", |req, ctx| async move {
            poll_response(req, ctx).await
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
        .post_async("/api/v1/polls/:id/votes", |mut req, ctx| async move {
            vote_in_poll(&mut req, ctx).await
        })
        .post_async("/api/v1/reports", |mut req, ctx| async move {
            create_report(&mut req, ctx).await
        })
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
        .get_async(
            "/api/v1/accounts/verify_credentials",
            |req, ctx| async move { verify_credentials(req, ctx).await },
        )
        .get_async("/api/v1/profile", |req, ctx| async move {
            profile_response(req, ctx).await
        })
        .get_async("/api/v1/preferences", |req, ctx| async move {
            preferences_response(req, ctx).await
        })
        .get_async("/api/v1/lists", |req, ctx| async move {
            lists_response(req, ctx).await
        })
        .post_async("/api/v1/lists", |mut req, ctx| async move {
            create_list_response(&mut req, ctx).await
        })
        .get_async("/api/v1/lists/:id", |req, ctx| async move {
            list_response(req, ctx).await
        })
        .put_async("/api/v1/lists/:id", |mut req, ctx| async move {
            update_list_response(&mut req, ctx).await
        })
        .patch_async("/api/v1/lists/:id", |mut req, ctx| async move {
            update_list_response(&mut req, ctx).await
        })
        .delete_async("/api/v1/lists/:id", |req, ctx| async move {
            delete_list_response(req, ctx).await
        })
        .get_async("/api/v1/lists/:id/accounts", |req, ctx| async move {
            list_accounts_response(req, ctx).await
        })
        .post_async("/api/v1/lists/:id/accounts", |mut req, ctx| async move {
            add_list_accounts_response(&mut req, ctx).await
        })
        .delete_async("/api/v1/lists/:id/accounts", |mut req, ctx| async move {
            delete_list_accounts_response(&mut req, ctx).await
        })
        .post_async("/api/v1/push/subscription", |req, ctx| async move {
            create_push_subscription_response(req, ctx).await
        })
        .get_async("/api/v1/push/subscription", |req, ctx| async move {
            push_subscription_response(req, ctx).await
        })
        .put_async("/api/v1/push/subscription", |req, ctx| async move {
            update_push_subscription_response(req, ctx).await
        })
        .patch_async("/api/v1/push/subscription", |req, ctx| async move {
            update_push_subscription_response(req, ctx).await
        })
        .delete_async("/api/v1/push/subscription", |req, ctx| async move {
            delete_push_subscription_response(req, ctx).await
        })
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
        .get_async("/api/v1/markers", |req, ctx| async move {
            markers_response(req, ctx).await
        })
        .post_async("/api/v1/markers", |req, ctx| async move {
            save_markers_response(req, ctx).await
        })
        .patch_async(
            "/api/v1/accounts/update_credentials",
            |mut req, ctx| async move { update_credentials(&mut req, ctx).await },
        )
        .put_async("/api/v1/profile", |mut req, ctx| async move {
            update_profile_response(&mut req, ctx).await
        })
        .patch_async("/api/v1/profile", |mut req, ctx| async move {
            update_profile_response(&mut req, ctx).await
        })
        .delete_async("/api/v1/profile/avatar", |req, ctx| async move {
            delete_profile_avatar_response(req, ctx).await
        })
        .delete_async("/api/v1/profile/header", |req, ctx| async move {
            delete_profile_header_response(req, ctx).await
        })
        .delete_async("/api/v1/suggestions/:id", |req, ctx| async move {
            delete_suggestion_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id/statuses", |req, ctx| async move {
            account_statuses_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id", |_req, ctx| async move {
            account_response(ctx).await
        })
        .run(req, env)
        .await?;

    finish_response(
        response,
        &request_method,
        &request_path,
        request_origin.as_deref(),
        &request_user_agent,
        log_api_requests,
        request_started_at_ms,
    )
}

fn finish_response(
    mut response: Response,
    method: &str,
    path: &str,
    origin: Option<&str>,
    user_agent: &str,
    log_api_requests: bool,
    request_started_at_ms: f64,
) -> Result<Response> {
    if is_cors_enabled_path(path) {
        apply_cors_headers(&mut response, origin)?;
    }
    log_api_request(
        log_api_requests,
        method,
        path,
        response.status_code(),
        user_agent,
        observability_duration_ms(request_started_at_ms),
    );
    Ok(response)
}

async fn dispatch_exact_without_router(
    method: &str,
    path: &str,
    env: &Env,
) -> Result<Option<Response>> {
    let Some(kind) = exact_without_router_kind(method, path) else {
        return Ok(None);
    };

    match kind {
        ExactWithoutRouterKind::InstanceV1 => {
            instance_summary_response_from_env(env).await.map(Some)
        }
        ExactWithoutRouterKind::InstanceV2 => instance_v2_response_from_env(env).await.map(Some),
        ExactWithoutRouterKind::OauthAuthorizationServer => {
            oauth_authorization_server_response_from_env(env).map(Some)
        }
        ExactWithoutRouterKind::NodeinfoLinks => nodeinfo_links_response_from_env(env).map(Some),
        ExactWithoutRouterKind::Nodeinfo => nodeinfo_response_from_env(env).await.map(Some),
        ExactWithoutRouterKind::InstanceRules => instance_rules_response_direct().map(Some),
        ExactWithoutRouterKind::InstanceDomainBlocks => {
            instance_domain_blocks_response_direct().map(Some)
        }
        ExactWithoutRouterKind::InstanceLanguages => {
            instance_languages_response_from_env(env).map(Some)
        }
        ExactWithoutRouterKind::CustomEmojis => custom_emojis_response_direct().map(Some),
    }
}

#[derive(Clone, Copy)]
enum ExactWithoutRouterKind {
    CustomEmojis,
    InstanceDomainBlocks,
    InstanceLanguages,
    InstanceRules,
    InstanceV1,
    InstanceV2,
    Nodeinfo,
    NodeinfoLinks,
    OauthAuthorizationServer,
}

fn exact_without_router_kind(method: &str, path: &str) -> Option<ExactWithoutRouterKind> {
    if method != "GET" {
        return None;
    }

    match path {
        "/api/v1/instance" => Some(ExactWithoutRouterKind::InstanceV1),
        "/api/v2/instance" => Some(ExactWithoutRouterKind::InstanceV2),
        "/.well-known/oauth-authorization-server" => {
            Some(ExactWithoutRouterKind::OauthAuthorizationServer)
        }
        "/.well-known/nodeinfo" => Some(ExactWithoutRouterKind::NodeinfoLinks),
        "/nodeinfo/2.0" => Some(ExactWithoutRouterKind::Nodeinfo),
        "/api/v1/instance/rules" => Some(ExactWithoutRouterKind::InstanceRules),
        "/api/v1/instance/domain_blocks" => Some(ExactWithoutRouterKind::InstanceDomainBlocks),
        "/api/v1/instance/languages" => Some(ExactWithoutRouterKind::InstanceLanguages),
        "/api/v1/custom_emojis" => Some(ExactWithoutRouterKind::CustomEmojis),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum FastRouterKind {
    Account,
    Discovery,
    Instance,
    Media,
    OAuth,
    Status,
    Timeline,
}

fn fast_router_kind(method: &str, path: &str) -> Option<FastRouterKind> {
    if matches!(
        path,
        "/.well-known/oauth-authorization-server"
            | "/.well-known/webfinger"
            | "/.well-known/nodeinfo"
            | "/nodeinfo/2.0"
            | "/api/oembed"
            | "/authorize_interaction"
    ) {
        return Some(FastRouterKind::Discovery);
    }
    if path.starts_with("/oauth/") {
        return Some(FastRouterKind::OAuth);
    }
    if method == "GET"
        && (path.starts_with("/api/v1/instance")
            || matches!(
                path,
                "/api/v2/instance"
                    | "/api/v1/custom_emojis"
                    | "/api/v1/trends"
                    | "/api/v1/trends/statuses"
                    | "/api/v1/trends/tags"
                    | "/api/v1/trends/links"
                    | "/api/v1/announcements"
                    | "/api/v1/donation_campaigns"
            ))
    {
        return Some(FastRouterKind::Instance);
    }
    if path.starts_with("/api/v1/timelines/") {
        return Some(FastRouterKind::Timeline);
    }
    if path == "/api/v1/accounts"
        || path.starts_with("/api/v1/accounts/")
        || matches!(
            path,
            "/api/v1/blocks"
                | "/api/v1/directory"
                | "/api/v1/favourites"
                | "/api/v1/endorsements"
                | "/api/v1/bookmarks"
                | "/api/v1/followed_tags"
                | "/api/v1/mutes"
                | "/api/v1/follow_requests"
        )
        || path.starts_with("/api/v1/follow_requests/")
    {
        return Some(FastRouterKind::Account);
    }
    if path == "/api/v1/statuses" || path.starts_with("/api/v1/statuses/") {
        return Some(FastRouterKind::Status);
    }
    if path == "/api/v1/media"
        || path == "/api/v2/media"
        || path.starts_with("/api/v1/media/")
        || path.starts_with("/api/v2/media/")
        || path.starts_with("/media/")
    {
        return Some(FastRouterKind::Media);
    }
    None
}

async fn run_fast_router(kind: FastRouterKind, req: Request, env: Env) -> Result<Response> {
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
        .get_async("/api/v1/accounts/:id", |_req, ctx| async move {
            account_response(ctx).await
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
    Router::new()
        .get_async(
            "/.well-known/oauth-authorization-server",
            |_req, ctx| async move { oauth_authorization_server_response(ctx).await },
        )
        .get_async("/.well-known/webfinger", |req, ctx| async move {
            webfinger_response(req, ctx).await
        })
        .get_async("/.well-known/nodeinfo", |_req, ctx| async move {
            nodeinfo_links_response(ctx).await
        })
        .get_async("/nodeinfo/2.0", |_req, ctx| async move {
            nodeinfo_response(ctx).await
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
        .post_async("/oauth/token", |req, ctx| async move {
            oauth_token_response(req, ctx).await
        })
}

fn api_request_logging_enabled(env: &Env) -> bool {
    env.var("CFWDON_API_REQUEST_LOG")
        .ok()
        .map(|value| {
            let value = value.to_string();
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

fn log_api_request(
    enabled: bool,
    method: &str,
    path: &str,
    status: u16,
    user_agent: &str,
    duration_ms: u64,
) {
    if !enabled || !is_logged_api_path(path) {
        return;
    }
    let payload = add_log_message(
        serde_json::json!({
            "event": "api_request",
            "method": method,
            "path": path,
            "status": status,
            "duration_ms": duration_ms,
            "user_agent": sanitize_log_value(user_agent),
        }),
        format!("API request {method} {path} completed with HTTP {status} in {duration_ms}ms"),
    );

    log_json_event(payload);
}

fn is_logged_api_path(path: &str) -> bool {
    path.starts_with("/api/") || path.starts_with("/oauth/") || path.starts_with("/.well-known/")
}

fn sanitize_log_value(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\n' | '\r' | '\t' => ' ',
            _ => character,
        })
        .take(200)
        .collect()
}

pub(crate) fn is_cors_enabled_path(path: &str) -> bool {
    path.starts_with("/api/")
        || path.starts_with("/oauth/")
        || path.starts_with("/media/")
        || path.starts_with("/profiles/")
        || path == "/.well-known/oauth-authorization-server"
}

fn cors_preflight_response(origin: Option<&str>) -> Result<Response> {
    let mut response = Response::empty()?.with_status(204);
    apply_cors_headers(&mut response, origin)?;
    response
        .headers_mut()
        .set("Access-Control-Max-Age", "86400")?;
    Ok(response)
}

fn apply_cors_headers(response: &mut Response, origin: Option<&str>) -> Result<()> {
    response
        .headers_mut()
        .set("Access-Control-Allow-Origin", origin.unwrap_or("*"))?;
    response.headers_mut().set(
        "Access-Control-Allow-Methods",
        "GET,HEAD,POST,PUT,PATCH,DELETE,OPTIONS",
    )?;
    response.headers_mut().set(
        "Access-Control-Allow-Headers",
        "Authorization,Content-Type,Accept,Idempotency-Key",
    )?;
    response
        .headers_mut()
        .set("Access-Control-Expose-Headers", "Link,Authorization")?;
    response.headers_mut().set("Vary", "Origin")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ExactWithoutRouterKind, FastRouterKind, fast_router_kind};

    #[test]
    fn fast_router_kind_covers_hot_exact_and_prefix_routes() {
        assert!(matches!(
            fast_router_kind("GET", "/api/v1/instance"),
            Some(FastRouterKind::Instance)
        ));
        assert!(matches!(
            fast_router_kind("GET", "/api/v1/timelines/public"),
            Some(FastRouterKind::Timeline)
        ));
        assert!(matches!(
            fast_router_kind("POST", "/api/v1/statuses/1/favourite"),
            Some(FastRouterKind::Status)
        ));
        assert!(matches!(
            fast_router_kind("GET", "/api/v1/accounts/relationships"),
            Some(FastRouterKind::Account)
        ));
        assert!(matches!(
            fast_router_kind("POST", "/oauth/token"),
            Some(FastRouterKind::OAuth)
        ));
    }

    #[test]
    fn fast_router_kind_leaves_unclassified_routes_for_fallback_router() {
        assert!(fast_router_kind("GET", "/api/v1/lists").is_none());
        assert!(fast_router_kind("GET", "/users/alice").is_none());
    }

    #[test]
    fn exact_without_router_only_handles_safe_get_routes() {
        assert!(matches!(
            super::exact_without_router_kind("GET", "/api/v1/instance"),
            Some(ExactWithoutRouterKind::InstanceV1)
        ));
        assert!(matches!(
            super::exact_without_router_kind("GET", "/.well-known/oauth-authorization-server"),
            Some(ExactWithoutRouterKind::OauthAuthorizationServer)
        ));
        assert!(super::exact_without_router_kind("POST", "/api/v1/instance").is_none());
        assert!(super::exact_without_router_kind("GET", "/api/v1/statuses/1").is_none());
    }
}
