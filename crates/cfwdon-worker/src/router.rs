use super::{
    account_directory, account_lookup, account_relationships, account_response, account_search,
    account_statuses_response, actor_response, add_list_accounts_response, announcements_response,
    block_account, bookmark_status, bookmarks_response, conversations_response,
    create_list_response, create_media_attachment, create_report, create_status,
    custom_emojis_response, delete_conversation_response, delete_list_response, delete_status,
    familiar_followers_response, favourite_status, favourites_response, feature_tag_response,
    featured_collection_response, featured_tag_suggestions_response,
    featured_tags_collection_response, featured_tags_response, follow_account,
    followers_collection_response, following_collection_response, home_timeline_response,
    inbox_response, instance_activity_response, instance_extended_description_response,
    instance_peers_response, instance_privacy_policy_response, instance_rules_response,
    instance_summary_response, instance_terms_of_service_response,
    instance_translation_languages_response, instance_v2_response, list_accounts_response,
    list_response, list_timeline_response, lists_response, markers_response,
    media_content_response, media_metadata_response, mute_account, mute_status_response,
    mutes_response, nodeinfo_links_response, nodeinfo_response, notification_dismiss_response,
    notification_response, notifications_clear_response, notifications_policy_response,
    notifications_response, notifications_unread_count_response, notifications_v2_response,
    outbox_response, pin_status_response, poll_response, preferences_response,
    process_expired_polls, process_outbox_deliveries, prune_orphan_media, public_timeline_response,
    read_conversation_response, reblog_status, root_document, save_markers_response, search_v2,
    shared_inbox_response, status_api_response, status_card_response, status_context_response,
    status_favourited_by_response, status_history_response, status_object_response,
    status_reblogged_by_response, status_source_response, tag_response, tag_timeline_response,
    trending_links_response, trending_statuses_response, trending_tags_response, unblock_account,
    unbookmark_status, unfavourite_status, unfeature_tag_response, unfollow_account,
    unmute_account, unmute_status_response, unpin_status_response, unreblog_status,
    update_credentials, update_list_response, update_media_attachment,
    update_notifications_policy_response, update_status, verify_credentials, vote_in_poll,
    webfinger_response,
};
use worker::{Env, Request, Response, Result, Router};

pub(crate) async fn handle_fetch(req: Request, env: Env) -> Result<Response> {
    Router::new()
        .get("/", |_req, _ctx| Response::from_json(&root_document()))
        .get("/healthz", |_req, _ctx| Response::ok("ok"))
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
        .get_async("/api/v2/instance", |_req, ctx| async move {
            instance_v2_response(ctx).await
        })
        .get_async("/api/v1/announcements", |_req, ctx| async move {
            announcements_response(ctx).await
        })
        .get_async("/api/v1/trends", |_req, ctx| async move {
            trending_tags_response(ctx).await
        })
        .get_async("/api/v1/trends/statuses", |_req, ctx| async move {
            trending_statuses_response(ctx).await
        })
        .get_async("/api/v1/trends/tags", |_req, ctx| async move {
            trending_tags_response(ctx).await
        })
        .get_async("/api/v1/trends/links", |_req, ctx| async move {
            trending_links_response(ctx).await
        })
        .get_async("/api/v1/custom_emojis", |_req, ctx| async move {
            custom_emojis_response(ctx).await
        })
        .get_async("/api/v1/timelines/home", |req, ctx| async move {
            home_timeline_response(req, ctx).await
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
        .get_async("/api/v1/tags/:name", |_req, ctx| async move {
            tag_response(ctx).await
        })
        .get_async("/.well-known/webfinger", |req, ctx| async move {
            webfinger_response(req, ctx).await
        })
        .get_async("/.well-known/nodeinfo", |_req, ctx| async move {
            nodeinfo_links_response(ctx).await
        })
        .get_async("/nodeinfo/2.0", |_req, ctx| async move {
            nodeinfo_response(ctx).await
        })
        .get_async("/users/:username", |_req, ctx| async move {
            actor_response(ctx).await
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
        .get_async("/users/:username/outbox", |_req, ctx| async move {
            outbox_response(ctx).await
        })
        .get_async("/users/:username/statuses/:id", |_req, ctx| async move {
            status_object_response(ctx).await
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
        .post_async("/api/v2/media", |req, ctx| async move {
            create_media_attachment(req, ctx).await
        })
        .get_async("/api/v1/media/:id", |_req, ctx| async move {
            media_metadata_response(ctx).await
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
        .get_async("/api/v1/bookmarks", |req, ctx| async move {
            bookmarks_response(req, ctx).await
        })
        .get_async("/api/v1/mutes", |req, ctx| async move {
            mutes_response(req, ctx).await
        })
        .get_async("/api/v1/notifications", |req, ctx| async move {
            notifications_response(req, ctx).await
        })
        .get_async("/api/v2/notifications", |req, ctx| async move {
            notifications_v2_response(req, ctx).await
        })
        .get_async("/api/v2/notifications/policy", |req, ctx| async move {
            notifications_policy_response(req, ctx).await
        })
        .patch_async("/api/v2/notifications/policy", |mut req, ctx| async move {
            update_notifications_policy_response(&mut req, ctx).await
        })
        .get_async(
            "/api/v1/notifications/unread_count",
            |req, ctx| async move { notifications_unread_count_response(req, ctx).await },
        )
        .get_async("/api/v1/notifications/:id", |req, ctx| async move {
            notification_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/clear", |req, ctx| async move {
            notifications_clear_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/:id/dismiss", |req, ctx| async move {
            notification_dismiss_response(req, ctx).await
        })
        .get_async("/api/v2/search", |req, ctx| async move {
            search_v2(req, ctx).await
        })
        .get_async("/api/v1/polls/:id", |req, ctx| async move {
            poll_response(req, ctx).await
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
        .get_async(
            "/api/v1/accounts/verify_credentials",
            |req, ctx| async move { verify_credentials(req, ctx).await },
        )
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
        .delete_async("/api/v1/lists/:id", |req, ctx| async move {
            delete_list_response(req, ctx).await
        })
        .get_async("/api/v1/lists/:id/accounts", |req, ctx| async move {
            list_accounts_response(req, ctx).await
        })
        .post_async("/api/v1/lists/:id/accounts", |mut req, ctx| async move {
            add_list_accounts_response(&mut req, ctx).await
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
        .get_async("/api/v1/accounts/:id/statuses", |req, ctx| async move {
            account_statuses_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id", |_req, ctx| async move {
            account_response(ctx).await
        })
        .run(req, env)
        .await
}
