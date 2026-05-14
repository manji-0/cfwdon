use crate::{
    account_directory, account_email_subscriptions_response, account_endorsements_response,
    account_featured_tags_response, account_followers_response, account_following_response,
    account_lists_response, account_lookup, account_relationships, account_response,
    account_search, account_statuses_response, accounts_index_response,
    authorize_follow_request_response, block_account, blocks_response, bookmarks_response,
    create_account_placeholder_response, delete_profile_avatar_response,
    delete_profile_header_response, endorse_account_response, endorsements_response,
    familiar_followers_response, favourites_response, follow_account, follow_request_response,
    follow_requests_response, followed_tags_response, identity_proofs_response, mute_account,
    mutes_response, note_account_response, pin_account_response, preferences_response,
    profile_response, reject_follow_request_response, remove_from_followers_response,
    unblock_account, unendorse_account_response, unfollow_account, unmute_account,
    unpin_account_response, update_credentials, update_profile_response, verify_credentials,
};
use worker::Router;

pub(crate) fn add_account_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/accounts", |req, ctx| async move {
            accounts_index_response(req, ctx).await
        })
        .post_async("/api/v1/accounts", |req, ctx| async move {
            create_account_placeholder_response(req, ctx).await
        })
        .get_async(
            "/api/v1/accounts/verify_credentials",
            |req, ctx| async move { verify_credentials(req, ctx).await },
        )
        .patch_async(
            "/api/v1/accounts/update_credentials",
            |mut req, ctx| async move { update_credentials(&mut req, ctx).await },
        )
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
        .get_async("/api/v1/profile", |req, ctx| async move {
            profile_response(req, ctx).await
        })
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
        .get_async("/api/v1/preferences", |req, ctx| async move {
            preferences_response(req, ctx).await
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
        .get_async("/api/v1/blocks", |req, ctx| async move {
            blocks_response(req, ctx).await
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
        .get_async("/api/v1/accounts/:id", |_req, ctx| async move {
            account_response(ctx).await
        })
}
