use super::super::{
    AccountFilterMatcher, AppConfig, BoostTargetPreload, LocalAccount, LocalStatusResponseDetails,
    LocalStatusResponseViewerState, LocalStatusViewerStatePreload, MastodonPollResponsePreload,
    MastodonStatusResponse, MediaAttachmentRow, MentionAccountsPreload, StatusApplicationPreload,
    StatusCountsPreload, StatusQuoteCountsPreload, StatusRow, build_local_quote_approval,
    build_status_application, build_status_card_value, build_status_mentions_with_preload,
    effective_status_quote_state, is_local_status_bookmarked_by, is_local_status_favourited_by,
    is_local_status_pinned_by, is_local_status_reblogged_by, is_local_status_thread_muted_by,
    load_local_status_counts, load_local_status_response_preload, load_stored_status_mentions,
    local_status_edited_at, local_status_poll_response,
    preloaded_local_status_response_viewer_state, status_quotes_count, status_response_config,
};
use worker::Result;

use crate::D1Database;

pub(crate) async fn build_local_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_filter_matcher(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        None,
    )
    .await
}

pub(crate) async fn build_loaded_local_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
) -> Result<MastodonStatusResponse> {
    let preload = load_local_status_response_preload(db, status).await?;
    build_local_status_response(
        db,
        config,
        viewer,
        status,
        account,
        preload.in_reply_to_account_id,
        preload.media,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_filter_matcher(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_preloads(
        db,
        config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        None,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_timeline_preloads(
        db,
        config,
        None,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        counts_preload,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_quote_count_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_with_timeline_preloads(
        db,
        config,
        None,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        None,
        None,
    )
    .await
}

pub(crate) async fn build_local_status_response_with_timeline_preloads(
    db: &D1Database,
    config: &AppConfig,
    resolved_config: Option<&AppConfig>,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
) -> Result<MastodonStatusResponse> {
    build_local_status_response_inner(
        db,
        config,
        resolved_config,
        viewer,
        status,
        account,
        in_reply_to_account_id,
        media_attachments,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        mention_preload,
        boost_target_preload,
        true,
    )
    .await
}

pub(super) async fn build_local_status_response_inner(
    db: &D1Database,
    config: &AppConfig,
    resolved_config: Option<&AppConfig>,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let config = status_response_config(db, config, resolved_config).await?;
    if let Some(boost_of_uri) = status.boost_of_uri.as_deref() {
        return super::build_local_reblog_wrapper_response(
            db,
            &config,
            viewer,
            status,
            account,
            in_reply_to_account_id,
            boost_of_uri,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            poll_preload,
            viewer_state_preload,
            application_preload,
            boost_target_preload,
            include_quote,
        )
        .await;
    }

    let mut response = MastodonStatusResponse::from_row(
        status,
        account,
        &config,
        in_reply_to_account_id,
        media_attachments,
    );
    let details = load_local_status_response_details(
        db,
        &config,
        viewer,
        status,
        account,
        &response.uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        mention_preload,
        boost_target_preload,
        include_quote,
    )
    .await?;
    response.apply_local_details(details);
    if !response.media_attachments.is_empty() {
        response.card = None;
    }
    Ok(response)
}

async fn load_local_status_response_details(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    status_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    include_quote: bool,
) -> Result<LocalStatusResponseDetails> {
    let application =
        match application_preload.and_then(|preload| preload.application(status.application_id)) {
            Some(application) => application,
            None => build_status_application(db, status.application_id).await?,
        };
    let card = if let Some(json) = status.card_json.as_deref().filter(|s| !s.is_empty()) {
        serde_json::from_str(json).ok()
    } else {
        build_status_card_value(&status.text)
    };
    let poll = local_status_poll_response(db, poll_preload, &status.id, viewer).await?;
    let mentions = if let Some(stored) = load_stored_status_mentions(db, &status.id).await? {
        stored
    } else {
        build_status_mentions_with_preload(db, config, &status.text, mention_preload).await?
    };
    let (favourites_count, reblogs_count) =
        local_status_counts(db, counts_preload, &status.id).await?;
    let quotes_count = status_quotes_count(db, quote_counts_preload, status_uri).await?;
    let viewer_state =
        local_status_response_viewer_state(db, viewer, status, viewer_state_preload).await?;
    let edited_at = local_status_edited_at(db, status).await?;
    let filtered = if viewer.is_some() {
        Some(local_status_filtered_for_viewer(db, viewer, status, filter_matcher).await?)
    } else {
        None
    };
    let quote_approval = Some(build_local_quote_approval(db, status, viewer, account).await?);
    let quote = if include_quote {
        super::build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_status_quote_state(status)),
            true,
            filter_matcher,
            counts_preload,
            boost_target_preload,
        )
        .await?
    } else {
        None
    };
    let viewer_fields = local_viewer_interaction_fields(viewer, status, viewer_state);

    Ok(LocalStatusResponseDetails {
        application,
        card,
        poll,
        mentions,
        favourites_count,
        reblogs_count,
        quotes_count,
        favourited: viewer_fields.favourited,
        reblogged: viewer_fields.reblogged,
        muted: viewer_fields.muted,
        bookmarked: viewer_fields.bookmarked,
        pinned: viewer_fields.pinned,
        edited_at,
        filtered,
        quote_approval,
        quote,
    })
}

fn local_status_is_pinnable(viewer: &LocalAccount, status: &StatusRow) -> bool {
    viewer.id() == status.account_id
        && status.boost_of_uri.is_none()
        && matches!(
            status.visibility,
            cfwdon_domain::Visibility::Public
                | cfwdon_domain::Visibility::Unlisted
                | cfwdon_domain::Visibility::FollowersOnly
        )
}

pub(super) struct LocalViewerInteractionFields {
    pub(super) favourited: Option<bool>,
    pub(super) reblogged: Option<bool>,
    pub(super) muted: Option<bool>,
    pub(super) bookmarked: Option<bool>,
    pub(super) pinned: Option<bool>,
}

pub(super) fn local_viewer_interaction_fields(
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    viewer_state: LocalStatusResponseViewerState,
) -> LocalViewerInteractionFields {
    let Some(viewer) = viewer else {
        return LocalViewerInteractionFields {
            favourited: None,
            reblogged: None,
            muted: None,
            bookmarked: None,
            pinned: None,
        };
    };
    LocalViewerInteractionFields {
        favourited: Some(viewer_state.favourited),
        reblogged: Some(viewer_state.reblogged),
        muted: Some(viewer_state.muted),
        bookmarked: Some(viewer_state.bookmarked),
        pinned: local_status_is_pinnable(viewer, status).then_some(viewer_state.pinned),
    }
}

pub(super) async fn local_status_response_viewer_state(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    preload: Option<&LocalStatusViewerStatePreload>,
) -> Result<LocalStatusResponseViewerState> {
    if let Some(state) = preloaded_local_status_response_viewer_state(viewer, status, preload) {
        if let Some(muted) = state.muted {
            return Ok(LocalStatusResponseViewerState {
                favourited: state.favourited,
                reblogged: state.reblogged,
                bookmarked: state.bookmarked,
                pinned: state.pinned,
                muted,
            });
        }

        let Some(viewer) = viewer else {
            return Ok(LocalStatusResponseViewerState::default());
        };
        return Ok(LocalStatusResponseViewerState {
            favourited: state.favourited,
            reblogged: state.reblogged,
            bookmarked: state.bookmarked,
            pinned: state.pinned,
            muted: is_local_status_thread_muted_by(db, viewer.id(), status).await?,
        });
    }

    let Some(viewer) = viewer else {
        return Ok(LocalStatusResponseViewerState::default());
    };

    Ok(LocalStatusResponseViewerState {
        favourited: is_local_status_favourited_by(db, viewer.id(), status).await?,
        reblogged: is_local_status_reblogged_by(db, viewer.id(), status).await?,
        bookmarked: is_local_status_bookmarked_by(db, viewer.id(), status).await?,
        pinned: is_local_status_pinned_by(db, viewer.id(), &status.id).await?,
        muted: is_local_status_thread_muted_by(db, viewer.id(), status).await?,
    })
}

pub(super) async fn local_status_filtered_for_viewer(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<Vec<serde_json::Value>> {
    let Some(viewer) = viewer else {
        return Ok(Vec::new());
    };
    super::filtered_status_for_viewer(
        db,
        filter_matcher,
        viewer.id(),
        &status.id,
        &status.text,
        &status.spoiler_text,
    )
    .await
}

pub(super) async fn local_status_counts(
    db: &D1Database,
    counts_preload: Option<&StatusCountsPreload>,
    status_id: &str,
) -> Result<(u64, u64)> {
    if let Some(counts) = counts_preload.and_then(|counts| counts.local_counts(status_id)) {
        return Ok(counts);
    }

    load_local_status_counts(db, status_id).await
}
