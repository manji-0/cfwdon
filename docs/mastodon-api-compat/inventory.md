# Inventory

このファイルは `scripts/generate_mastodon_api_compat.py` で生成する。

`cfwdon` のローカル route は `crates/cfwdon-worker/src/router.rs` の handler 名にマッピングする。

## Discovery / OAuth / Meta

| Method | Mastodon route | cfwdon handler | Status | Note |
| --- | --- | --- | --- | --- |
| GET | `/.well-known/oauth-authorization-server` | `oauth_authorization_server_response` | `implemented` |  |
| GET | `/.well-known/nodeinfo` | `nodeinfo_links_response` | `implemented` |  |
| GET | `/.well-known/webfinger` | `webfinger_response` | `implemented` |  |
| GET | `/oauth/userinfo` | `oauth_userinfo_response` | `implemented` |  |
| POST | `/oauth/userinfo` | `oauth_userinfo_response` | `implemented` |  |
| GET | `/api/oembed` | `oembed_response` | `implemented` |  |

## Experimental / Alpha

| Method | Mastodon route | cfwdon handler | Status | Note |
| --- | --- | --- | --- | --- |
| GET | `/api/v1_alpha/accounts/:account_id/collections` | `alpha_account_collections_response` | `implemented` |  |
| GET | `/api/v1_alpha/accounts/:account_id/in_collections` | `alpha_account_in_collections_response` | `implemented` |  |
| GET | `/api/v1_alpha/async_refreshes/:id` | `async_refresh_response` | `implemented` |  |
| GET | `/api/v1_alpha/collections/:id` | `alpha_collection_response` | `implemented` |  |
| POST | `/api/v1_alpha/collections` | `create_alpha_collection_response` | `implemented` |  |
| PUT | `/api/v1_alpha/collections/:id` | `update_alpha_collection_response` | `implemented` |  |
| PATCH | `/api/v1_alpha/collections/:id` | `update_alpha_collection_response` | `implemented` |  |
| DELETE | `/api/v1_alpha/collections/:id` | `delete_alpha_collection_response` | `implemented` |  |
| POST | `/api/v1_alpha/collections/:collection_id/items` | `create_alpha_collection_item_response` | `implemented` |  |
| DELETE | `/api/v1_alpha/collections/:collection_id/items/:id` | `delete_alpha_collection_item_response` | `implemented` |  |
| POST | `/api/v1_alpha/collections/:collection_id/items/:id/revoke` | `revoke_alpha_collection_item_response` | `implemented` |  |

## Instance / Apps / Trends

| Method | Mastodon route | cfwdon handler | Status | Note |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/custom_emojis` | `custom_emojis_response` | `implemented` |  |
| GET | `/api/v1/suggestions` | `suggestions_v1_response` | `implemented` |  |
| DELETE | `/api/v1/suggestions/:id` | `delete_suggestion_response` | `implemented` |  |
| GET | `/api/v1/preferences` | `preferences_response` | `implemented` |  |
| GET | `/api/v1/donation_campaigns` | `donation_campaigns_response` | `implemented` |  |
| GET | `/api/v1/annual_reports` | `annual_reports_response` | `implemented` |  |
| GET | `/api/v1/annual_reports/:id` | `annual_report_response` | `implemented` |  |
| POST | `/api/v1/annual_reports/:id/read` | `annual_report_action_response` | `implemented` |  |
| POST | `/api/v1/annual_reports/:id/generate` | `annual_report_action_response` | `implemented` |  |
| GET | `/api/v1/annual_reports/:id/state` | `annual_report_state_response` | `implemented` |  |
| GET | `/api/v1/announcements` | `announcements_response` | `implemented` |  |
| PUT | `/api/v1/announcements/:announcement_id/reactions/:id` | `announcement_reaction_mutation_response` | `implemented` |  |
| PATCH | `/api/v1/announcements/:announcement_id/reactions/:id` | `announcement_reaction_mutation_response` | `implemented` |  |
| DELETE | `/api/v1/announcements/:announcement_id/reactions/:id` | `announcement_reaction_mutation_response` | `implemented` |  |
| POST | `/api/v1/announcements/:id/dismiss` | `dismiss_announcement_mutation_response` | `implemented` |  |
| GET | `/api/v1/trends` | `trending_tags_response` | `implemented` |  |
| GET | `/api/v1/apps/verify_credentials` | `app_verify_credentials_response` | `implemented` |  |
| POST | `/api/v1/apps` | `create_app_response` | `implemented` |  |
| GET | `/api/v1/trends/tags` | `trending_tags_response` | `implemented` |  |
| GET | `/api/v1/trends/links` | `trending_links_response` | `implemented` |  |
| GET | `/api/v1/trends/statuses` | `trending_statuses_response` | `implemented` |  |
| POST | `/api/v1/emails/confirmations` | `create_email_confirmation_response` | `implemented` |  |
| GET | `/api/v1/emails/check_confirmation` | `check_email_confirmation_response` | `implemented` |  |
| GET | `/api/v1/instance` | `instance_summary_response` | `implemented` |  |
| GET | `/api/v1/instance/peers` | `instance_peers_response` | `implemented` |  |
| GET | `/api/v1/instance/rules` | `instance_rules_response` | `implemented` |  |
| GET | `/api/v1/instance/domain_blocks` | `instance_domain_blocks_response` | `implemented` |  |
| GET | `/api/v1/instance/terms_of_service` | `instance_terms_of_service_response` | `implemented` |  |
| GET | `/api/v1/instance/terms_of_service/:date` | `instance_terms_of_service_version_response` | `implemented` |  |
| GET | `/api/v1/instance/privacy_policy` | `instance_privacy_policy_response` | `implemented` |  |
| GET | `/api/v1/instance/extended_description` | `instance_extended_description_response` | `implemented` |  |
| GET | `/api/v1/instance/translation_languages` | `instance_translation_languages_response` | `implemented` |  |
| GET | `/api/v1/instance/languages` | `instance_languages_response` | `implemented` |  |
| GET | `/api/v1/instance/activity` | `instance_activity_response` | `implemented` |  |
| GET | `/api/v2/suggestions` | `suggestions_v2_response` | `implemented` |  |
| GET | `/api/v2/instance` | `instance_v2_response` | `implemented` |  |

## Timelines / Search / Streaming

| Method | Mastodon route | cfwdon handler | Status | Note |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/timelines/home` | `home_timeline_response` | `implemented` |  |
| GET | `/api/v1/timelines/public` | `public_timeline_response` | `implemented` |  |
| GET | `/api/v1/timelines/link` | `link_timeline_response` | `implemented` |  |
| GET | `/api/v1/timelines/tag/:id` | `tag_timeline_response` | `implemented` |  |
| GET | `/api/v1/timelines/list/:id` | `list_timeline_response` | `implemented` |  |
| GET | `/api/v1/streaming` | `streaming_placeholder_response` | `implemented` |  |
| GET | `/api/v1/streaming/(*any)` | `streaming_placeholder_response` | `implemented` |  |
| GET | `/api/v2/search` | `search_v2` | `implemented` |  |

## Statuses / Polls / Media

| Method | Mastodon route | cfwdon handler | Status | Note |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/statuses` | `statuses_index_placeholder_response` | `implemented` |  |
| POST | `/api/v1/statuses` | `create_status` | `implemented` |  |
| GET | `/api/v1/statuses/:id` | `status_api_response` | `implemented` |  |
| PUT | `/api/v1/statuses/:id` | `update_status` | `implemented` |  |
| PATCH | `/api/v1/statuses/:id` | `update_status` | `implemented` |  |
| DELETE | `/api/v1/statuses/:id` | `delete_status` | `implemented` |  |
| GET | `/api/v1/statuses/:status_id/reblogged_by` | `status_reblogged_by_response` | `implemented` |  |
| GET | `/api/v1/statuses/:status_id/favourited_by` | `status_favourited_by_response` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/reblog` | `reblog_status` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/unreblog` | `unreblog_status` | `implemented` |  |
| GET | `/api/v1/statuses/:status_id/quotes` | `status_quotes_response` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/quotes/:id/revoke` | `revoke_quote_response` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/favourite` | `favourite_status` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/unfavourite` | `unfavourite_status` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/bookmark` | `bookmark_status` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/unbookmark` | `unbookmark_status` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/mute` | `mute_status_response` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/unmute` | `unmute_status_response` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/pin` | `pin_status_response` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/unpin` | `unpin_status_response` | `implemented` |  |
| GET | `/api/v1/statuses/:status_id/history` | `status_history_response` | `implemented` |  |
| GET | `/api/v1/statuses/:status_id/source` | `status_source_response` | `implemented` |  |
| PUT | `/api/v1/statuses/:status_id/interaction_policy` | `status_interaction_policy_response` | `implemented` |  |
| PATCH | `/api/v1/statuses/:status_id/interaction_policy` | `status_interaction_policy_response` | `implemented` |  |
| POST | `/api/v1/statuses/:status_id/translate` | `translate_status_response` | `implemented` |  |
| GET | `/api/v1/statuses/:id/context` | `status_context_response` | `implemented` |  |
| GET | `/api/v1/scheduled_statuses` | `scheduled_statuses_response` | `implemented` |  |
| GET | `/api/v1/scheduled_statuses/:id` | `scheduled_status_response` | `implemented` |  |
| PUT | `/api/v1/scheduled_statuses/:id` | `update_scheduled_status_response` | `implemented` |  |
| PATCH | `/api/v1/scheduled_statuses/:id` | `update_scheduled_status_response` | `implemented` |  |
| DELETE | `/api/v1/scheduled_statuses/:id` | `delete_scheduled_status_response` | `implemented` |  |
| POST | `/api/v1/media` | `create_media_attachment` | `implemented` |  |
| PUT | `/api/v1/media/:id` | `update_media_attachment` | `implemented` |  |
| PATCH | `/api/v1/media/:id` | `update_media_attachment` | `implemented` |  |
| GET | `/api/v1/media/:id` | `media_metadata_response` | `implemented` |  |
| DELETE | `/api/v1/media/:id` | `delete_media_attachment` | `implemented` |  |
| GET | `/api/v1/polls/:id` | `poll_response` | `implemented` |  |
| POST | `/api/v1/polls/:poll_id/votes` | `vote_in_poll` | `implemented` |  |
| POST | `/api/v2/media` | `create_media_attachment` | `implemented` |  |

## Accounts / Relationships / Tags

| Method | Mastodon route | cfwdon handler | Status | Note |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/blocks` | `blocks_response` | `implemented` |  |
| GET | `/api/v1/mutes` | `mutes_response` | `implemented` |  |
| GET | `/api/v1/favourites` | `favourites_response` | `implemented` |  |
| GET | `/api/v1/bookmarks` | `bookmarks_response` | `implemented` |  |
| GET | `/api/v1/endorsements` | `endorsements_response` | `implemented` |  |
| GET | `/api/v1/profile` | `profile_response` | `implemented` |  |
| PUT | `/api/v1/profile` | `update_profile_response` | `implemented` |  |
| PATCH | `/api/v1/profile` | `update_profile_response` | `implemented` |  |
| DELETE | `/api/v1/profile/avatar` | `delete_profile_avatar_response` | `implemented` |  |
| DELETE | `/api/v1/profile/header` | `delete_profile_header_response` | `implemented` |  |
| GET | `/api/v1/directory` | `account_directory` | `implemented` |  |
| GET | `/api/v1/follow_requests` | `follow_requests_response` | `implemented` |  |
| POST | `/api/v1/follow_requests/:id/authorize` | `authorize_follow_request_response` | `implemented` |  |
| POST | `/api/v1/follow_requests/:id/reject` | `reject_follow_request_response` | `implemented` |  |
| GET | `/api/v1/accounts/verify_credentials` | `verify_credentials` | `implemented` |  |
| PATCH | `/api/v1/accounts/update_credentials` | `update_credentials` | `implemented` |  |
| GET | `/api/v1/accounts/search` | `account_search` | `implemented` |  |
| GET | `/api/v1/accounts/lookup` | `account_lookup` | `implemented` |  |
| GET | `/api/v1/accounts/relationships` | `account_relationships` | `implemented` |  |
| GET | `/api/v1/accounts/familiar_followers` | `familiar_followers_response` | `implemented` |  |
| GET | `/api/v1/accounts` | `accounts_index_response` | `implemented` |  |
| POST | `/api/v1/accounts` | `create_account_placeholder_response` | `implemented` |  |
| GET | `/api/v1/accounts/:id` | `account_response` | `implemented` |  |
| GET | `/api/v1/accounts/:account_id/statuses` | `account_statuses_response` | `implemented` |  |
| GET | `/api/v1/accounts/:account_id/followers` | `account_followers_response` | `implemented` |  |
| GET | `/api/v1/accounts/:account_id/following` | `account_following_response` | `implemented` |  |
| GET | `/api/v1/accounts/:account_id/lists` | `account_lists_response` | `implemented` |  |
| GET | `/api/v1/accounts/:account_id/identity_proofs` | `identity_proofs_response` | `implemented` |  |
| GET | `/api/v1/accounts/:account_id/featured_tags` | `account_featured_tags_response` | `implemented` |  |
| GET | `/api/v1/accounts/:account_id/endorsements` | `account_endorsements_response` | `implemented` |  |
| POST | `/api/v1/accounts/:account_id/email_subscriptions` | `account_email_subscriptions_response` | `implemented` |  |
| POST | `/api/v1/accounts/:id/follow` | `follow_account` | `implemented` |  |
| POST | `/api/v1/accounts/:id/unfollow` | `unfollow_account` | `implemented` |  |
| POST | `/api/v1/accounts/:id/remove_from_followers` | `remove_from_followers_response` | `implemented` |  |
| POST | `/api/v1/accounts/:id/block` | `block_account` | `implemented` |  |
| POST | `/api/v1/accounts/:id/unblock` | `unblock_account` | `implemented` |  |
| POST | `/api/v1/accounts/:id/mute` | `mute_account` | `implemented` |  |
| POST | `/api/v1/accounts/:id/unmute` | `unmute_account` | `implemented` |  |
| POST | `/api/v1/accounts/:account_id/pin` | `pin_account_response` | `implemented` |  |
| POST | `/api/v1/accounts/:account_id/endorse` | `endorse_account_response` | `implemented` |  |
| POST | `/api/v1/accounts/:account_id/unpin` | `unpin_account_response` | `implemented` |  |
| POST | `/api/v1/accounts/:account_id/unendorse` | `unendorse_account_response` | `implemented` |  |
| POST | `/api/v1/accounts/:account_id/note` | `note_account_response` | `implemented` |  |
| GET | `/api/v1/tags/:id` | `tag_response` | `implemented` |  |
| POST | `/api/v1/tags/:id/follow` | `follow_tag_response` | `implemented` |  |
| POST | `/api/v1/tags/:id/unfollow` | `unfollow_tag_response` | `implemented` |  |
| POST | `/api/v1/tags/:id/feature` | `feature_tag_v1_response` | `implemented` |  |
| POST | `/api/v1/tags/:id/unfeature` | `unfeature_tag_v1_response` | `implemented` |  |
| GET | `/api/v1/followed_tags` | `followed_tags_response` | `implemented` |  |
| GET | `/api/v1/featured_tags/suggestions` | `featured_tag_suggestions_response` | `implemented` |  |
| GET | `/api/v1/featured_tags` | `featured_tags_response` | `implemented` |  |
| POST | `/api/v1/featured_tags` | `feature_tag_response` | `implemented` |  |
| DELETE | `/api/v1/featured_tags/:id` | `unfeature_tag_response` | `implemented` |  |

## Notifications / Conversations / Lists / Filters / Push

| Method | Mastodon route | cfwdon handler | Status | Note |
| --- | --- | --- | --- | --- |
| GET | `/api/v1/conversations` | `conversations_response` | `implemented` |  |
| DELETE | `/api/v1/conversations/:id` | `delete_conversation_response` | `implemented` |  |
| POST | `/api/v1/conversations/:id/read` | `read_conversation_response` | `implemented` |  |
| POST | `/api/v1/conversations/:id/unread` | `unread_conversation_response` | `implemented` |  |
| POST | `/api/v1/reports` | `create_report` | `implemented` |  |
| GET | `/api/v1/filters` | `filters_v1_response` | `implemented` |  |
| POST | `/api/v1/filters` | `create_filter_v1_response` | `implemented` |  |
| GET | `/api/v1/filters/:id` | `filter_v1_response` | `implemented` |  |
| PUT | `/api/v1/filters/:id` | `update_filter_v1_response` | `implemented` |  |
| PATCH | `/api/v1/filters/:id` | `update_filter_v1_response` | `implemented` |  |
| DELETE | `/api/v1/filters/:id` | `delete_filter_v1_response` | `implemented` |  |
| GET | `/api/v1/markers` | `markers_response` | `implemented` |  |
| POST | `/api/v1/markers` | `save_markers_response` | `implemented` |  |
| GET | `/api/v1/peers/search` | `instance_peers_search_response` | `implemented` |  |
| GET | `/api/v1/domain_blocks/preview` | `domain_blocks_preview_response` | `implemented` |  |
| GET | `/api/v1/domain_blocks` | `domain_blocks_response` | `implemented` |  |
| POST | `/api/v1/domain_blocks` | `create_domain_block_response` | `implemented` |  |
| DELETE | `/api/v1/domain_blocks` | `delete_domain_block_response` | `implemented` |  |
| GET | `/api/v1/notifications/requests` | `notification_requests_response` | `implemented` |  |
| GET | `/api/v1/notifications/requests/:id` | `notification_request_response` | `implemented` |  |
| POST | `/api/v1/notifications/requests/accept` | `accept_notification_requests_response` | `implemented` |  |
| POST | `/api/v1/notifications/requests/dismiss` | `dismiss_notification_requests_response` | `implemented` |  |
| GET | `/api/v1/notifications/requests/merged` | `notification_requests_merged_response` | `implemented` |  |
| POST | `/api/v1/notifications/requests/:id/accept` | `accept_notification_request_response` | `implemented` |  |
| POST | `/api/v1/notifications/requests/:id/dismiss` | `dismiss_notification_request_response` | `implemented` |  |
| GET | `/api/v1/notifications/policy` | `notifications_policy_response` | `implemented` |  |
| PUT | `/api/v1/notifications/policy` | `update_notifications_policy_response` | `implemented` |  |
| PATCH | `/api/v1/notifications/policy` | `update_notifications_policy_response` | `implemented` |  |
| GET | `/api/v1/notifications` | `notifications_response` | `implemented` |  |
| GET | `/api/v1/notifications/:id` | `notification_response` | `implemented` |  |
| POST | `/api/v1/notifications/clear` | `notifications_clear_response` | `implemented` |  |
| GET | `/api/v1/notifications/unread_count` | `notifications_unread_count_response` | `implemented` |  |
| POST | `/api/v1/notifications/:id/dismiss` | `notification_dismiss_response` | `implemented` |  |
| GET | `/api/v1/lists` | `lists_response` | `implemented` |  |
| POST | `/api/v1/lists` | `create_list_response` | `implemented` |  |
| GET | `/api/v1/lists/:id` | `list_response` | `implemented` |  |
| PUT | `/api/v1/lists/:id` | `update_list_response` | `implemented` |  |
| PATCH | `/api/v1/lists/:id` | `update_list_response` | `implemented` |  |
| DELETE | `/api/v1/lists/:id` | `delete_list_response` | `implemented` |  |
| GET | `/api/v1/lists/:list_id/accounts` | `list_accounts_response` | `implemented` |  |
| POST | `/api/v1/lists/:list_id/accounts` | `add_list_accounts_response` | `implemented` |  |
| DELETE | `/api/v1/lists/:list_id/accounts` | `delete_list_accounts_response` | `implemented` |  |
| POST | `/api/v1/push/subscription` | `create_push_subscription_response` | `implemented` |  |
| GET | `/api/v1/push/subscription` | `push_subscription_response` | `implemented` |  |
| PUT | `/api/v1/push/subscription` | `update_push_subscription_response` | `implemented` |  |
| PATCH | `/api/v1/push/subscription` | `update_push_subscription_response` | `implemented` |  |
| DELETE | `/api/v1/push/subscription` | `delete_push_subscription_response` | `implemented` |  |
| GET | `/api/v2/filters` | `filters_v2_response` | `implemented` |  |
| POST | `/api/v2/filters` | `create_filter_v2_response` | `implemented` |  |
| GET | `/api/v2/filters/:id` | `filter_v2_response` | `implemented` |  |
| PUT | `/api/v2/filters/:id` | `update_filter_v2_response` | `implemented` |  |
| PATCH | `/api/v2/filters/:id` | `update_filter_v2_response` | `implemented` |  |
| DELETE | `/api/v2/filters/:id` | `delete_filter_v2_response` | `implemented` |  |
| GET | `/api/v2/filters/:filter_id/keywords` | `filter_keywords_response` | `implemented` |  |
| POST | `/api/v2/filters/:filter_id/keywords` | `create_filter_keyword_response` | `implemented` |  |
| GET | `/api/v2/filters/:filter_id/statuses` | `filter_statuses_response` | `implemented` |  |
| POST | `/api/v2/filters/:filter_id/statuses` | `create_filter_status_response` | `implemented` |  |
| GET | `/api/v2/filters/keywords/:id` | `filter_keyword_response` | `implemented` |  |
| PUT | `/api/v2/filters/keywords/:id` | `update_filter_keyword_response` | `implemented` |  |
| PATCH | `/api/v2/filters/keywords/:id` | `update_filter_keyword_response` | `implemented` |  |
| DELETE | `/api/v2/filters/keywords/:id` | `delete_filter_keyword_response` | `implemented` |  |
| GET | `/api/v2/filters/statuses/:id` | `filter_status_response` | `implemented` |  |
| DELETE | `/api/v2/filters/statuses/:id` | `delete_filter_status_response` | `implemented` |  |
| GET | `/api/v2/notifications/policy` | `notifications_policy_response` | `implemented` |  |
| PUT | `/api/v2/notifications/policy` | `update_notifications_policy_response` | `implemented` |  |
| PATCH | `/api/v2/notifications/policy` | `update_notifications_policy_response` | `implemented` |  |
| GET | `/api/v2/notifications` | `notifications_v2_response` | `implemented` |  |
| GET | `/api/v2/notifications/:group_key` | `notification_group_response` | `implemented` |  |
| POST | `/api/v2/notifications/clear` | `notifications_clear_response` | `implemented` |  |
| GET | `/api/v2/notifications/unread_count` | `notifications_unread_count_response` | `implemented` |  |
| POST | `/api/v2/notifications/:group_key/dismiss` | `notification_group_dismiss_response` | `implemented` |  |
| GET | `/api/v2/notifications/:group_key/accounts` | `notification_group_accounts_response` | `implemented` |  |
