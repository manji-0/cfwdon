//! Status API response builders.
//!
//! Local and remote status response entry points in this module are intentional
//! graph bridges: timelines, quotes, and detail routes converge here so shared
//! preload/viewer/quote embedding stays consistent. Prefer extracting cohesive
//! helpers (mentions, quote documents) into sibling modules rather than adding
//! more route-specific forks.

use super::reblog_response::{
    local_reblog_wrapper_response_from_embedded, remote_reblog_wrapper_response_from_embedded,
};
use super::{
    AccountFilterMatcher, AppConfig, BoostTarget, BoostTargetPreload, FederatedEmojiMap,
    LocalAccount, LocalStatusResponseDetails, LocalStatusResponseViewerState,
    LocalStatusViewerStatePreload, MastodonPollResponsePreload, MastodonStatusResponse,
    MediaAttachmentRow, MentionAccountsPreload, RemoteActorRow, RemoteMastodonPollResponsePreload,
    RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload,
    RemoteStatusFederatedEmojisPreload, RemoteStatusResponseDetails,
    RemoteStatusResponseViewerState, RemoteStatusRow, RemoteStatusViewerStatePreload,
    StatusApplicationPreload, StatusCountsPreload, StatusQuoteCountsPreload, StatusRow,
    build_local_quote_approval, build_remote_quote_approval, build_remote_status_card_value,
    build_status_application, build_status_card_value, build_status_mentions,
    build_status_mentions_with_preload, effective_remote_status_quote_state,
    effective_status_quote_state, extract_federated_emojis_from_activitypub_object,
    find_local_status_by_object_uri, find_remote_actor_by_actor_uri,
    find_remote_status_attachments_by_status_id, find_remote_status_by_url_or_object_uri,
    find_remote_status_raw_object_by_id, has_remote_status_edit_snapshots,
    is_local_status_bookmarked_by, is_local_status_favourited_by, is_local_status_pinned_by,
    is_local_status_reblogged_by, is_local_status_thread_muted_by, is_muted_actor,
    is_remote_status_bookmarked_by, is_remote_status_favourited_by, is_remote_status_reblogged_by,
    load_local_status_counts, load_local_status_response_preload, load_mastodon_poll_response,
    load_remote_mastodon_poll_response, load_remote_status_counts, load_remote_status_updated_at,
    load_status_filtered, load_stored_remote_status_mentions, load_stored_status_mentions,
    local_quoted_status_document_state, local_status_edited_at, local_status_poll_response,
    pending_quote_document, preloaded_local_status_response_viewer_state,
    preloaded_remote_status_response_viewer_state, quote_document_for_local_state,
    quote_document_from_response, remote_quote_visibility_is_embeddable,
    remote_quoted_status_document_state, resolve_boost_target,
    resolve_local_status_response_subject, status_quotes_count, status_response_config,
    unauthorized_quote_document,
};
use std::collections::HashMap;
use worker::Result;

use crate::D1Database;

fn remote_media_attachment_values(
    attachments: &[crate::RemoteStatusAttachmentRow],
) -> Vec<serde_json::Value> {
    attachments
        .iter()
        .map(|media| {
            serde_json::to_value(crate::MastodonMediaAttachmentResponse::from_remote_row(
                media,
            ))
            .unwrap_or(serde_json::Value::Null)
        })
        .collect()
}

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

async fn build_local_status_response_inner(
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
        return build_local_reblog_wrapper_response(
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
        build_quoted_status_value(
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

struct LocalViewerInteractionFields {
    favourited: Option<bool>,
    reblogged: Option<bool>,
    muted: Option<bool>,
    bookmarked: Option<bool>,
    pinned: Option<bool>,
}

fn local_viewer_interaction_fields(
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

async fn local_status_response_viewer_state(
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

pub(crate) async fn build_remote_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_with_filter_matcher(db, config, viewer, status, actor, None, None)
        .await
}

pub(crate) async fn build_remote_status_response_with_filter_matcher(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_with_preloads(
        db,
        config,
        viewer,
        status,
        actor,
        filter_matcher,
        None,
        federated_emojis_preload,
    )
    .await
}

pub(crate) async fn build_remote_status_response_with_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_inner(
        db,
        config,
        viewer,
        status,
        actor,
        filter_matcher,
        counts_preload,
        None,
        None,
        None,
        None,
        federated_emojis_preload,
        None,
        None,
        None,
        None,
        None,
        None,
        true,
    )
    .await
}

pub(crate) async fn build_remote_status_response_with_timeline_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    remote_attachments: Vec<RemoteStatusAttachmentRow>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
) -> Result<MastodonStatusResponse> {
    build_remote_status_response_inner(
        db,
        config,
        viewer,
        status,
        actor,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        viewer_state_preload,
        poll_preload,
        edit_updated_at_preload,
        federated_emojis_preload,
        Some(remote_attachments),
        mention_preload,
        boost_target_preload,
        remote_in_reply_to_preload,
        remote_actors_preload,
        remote_attachments_preload,
        true,
    )
    .await
}

async fn federated_emojis_for_remote_status(
    db: &D1Database,
    status_id: &str,
    preload: Option<&RemoteStatusFederatedEmojisPreload>,
) -> Result<Option<FederatedEmojiMap>> {
    if let Some(preload) = preload {
        return Ok(preload.get(status_id).cloned());
    }
    let object = find_remote_status_raw_object_by_id(db, status_id).await?;
    Ok(object.map(|value| extract_federated_emojis_from_activitypub_object(&value)))
}

fn federated_emojis_from_json(federated_emojis_json: &str) -> Option<FederatedEmojiMap> {
    if federated_emojis_json.is_empty()
        || federated_emojis_json == "[]"
        || federated_emojis_json == "{}"
    {
        return None;
    }
    serde_json::from_str(federated_emojis_json).ok()
}

async fn build_remote_status_response_inner(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    remote_attachments: Option<Vec<RemoteStatusAttachmentRow>>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    if let Some(boost_of_uri) = status.boost_of_uri.as_deref() {
        return build_remote_reblog_wrapper_response(
            db,
            config,
            viewer,
            status,
            actor,
            boost_of_uri,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            viewer_state_preload,
            poll_preload,
            edit_updated_at_preload,
            federated_emojis_preload,
            mention_preload,
            boost_target_preload,
            remote_in_reply_to_preload,
            remote_actors_preload,
            remote_attachments_preload,
            include_quote,
        )
        .await;
    }

    let federated_emojis =
        if let Some(emojis) = federated_emojis_from_json(&status.federated_emojis_json) {
            Some(emojis)
        } else {
            federated_emojis_for_remote_status(db, &status.id, federated_emojis_preload).await?
        };
    let mut response =
        MastodonStatusResponse::from_remote_row(status, actor, config, federated_emojis.as_ref());
    let details = load_remote_status_response_details(
        db,
        config,
        viewer,
        status,
        actor,
        &response.uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        viewer_state_preload,
        poll_preload,
        edit_updated_at_preload,
        remote_attachments,
        mention_preload,
        remote_in_reply_to_preload,
        boost_target_preload,
        include_quote,
    )
    .await?;
    response.apply_remote_details(details);
    if !response.media_attachments.is_empty() {
        response.card = None;
    }
    Ok(response)
}

async fn load_remote_status_response_details(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    status_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    remote_attachments: Option<Vec<RemoteStatusAttachmentRow>>,
    mention_preload: Option<&MentionAccountsPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    boost_target_preload: Option<&BoostTargetPreload>,
    include_quote: bool,
) -> Result<RemoteStatusResponseDetails> {
    let text_content = status.plain_text();
    let remote_attachments = match remote_attachments {
        Some(attachments) => attachments,
        None => find_remote_status_attachments_by_status_id(db, &status.id).await?,
    };
    let card = if let Some(json) = status.card_json.as_deref().filter(|s| !s.is_empty()) {
        serde_json::from_str(json).ok()
    } else {
        build_remote_status_card_value(&text_content, &remote_attachments)
    };
    let media_attachments = remote_media_attachment_values(&remote_attachments);
    let mentions = if let Some(stored) = load_stored_remote_status_mentions(db, &status.id).await? {
        stored
    } else {
        build_status_mentions_with_preload(db, config, &text_content, mention_preload).await?
    };
    let (favourites_count, reblogs_count) =
        remote_status_counts(db, counts_preload, &status.id).await?;
    let quotes_count = status_quotes_count(db, quote_counts_preload, status_uri).await?;
    let viewer_state =
        remote_status_response_viewer_state(db, viewer, status, actor, viewer_state_preload)
            .await?;
    let poll = match poll_preload {
        Some(preload) => preload.poll_response(&status.id),
        None => load_remote_mastodon_poll_response(db, status, viewer).await?,
    };
    let edited_at = if let Some(ref ts) = status.edited_at {
        Some(ts.clone())
    } else {
        match edit_updated_at_preload {
            Some(preload) => preload.updated_at(&status.id).map(ToOwned::to_owned),
            None => {
                if has_remote_status_edit_snapshots(db, &status.id).await? {
                    load_remote_status_updated_at(db, &status.id).await?
                } else {
                    None
                }
            }
        }
    };
    let filtered = if viewer.is_some() {
        Some(
            remote_status_filtered_for_viewer(db, viewer, status, &text_content, filter_matcher)
                .await?,
        )
    } else {
        None
    };
    let quote_approval = Some(build_remote_quote_approval(status));
    let quote = if include_quote {
        build_quoted_status_value(
            db,
            config,
            viewer,
            status.quote_of_uri.as_deref(),
            Some(effective_remote_status_quote_state(status)),
            false,
            filter_matcher,
            counts_preload,
            boost_target_preload,
        )
        .await?
    } else {
        None
    };
    let (favourited, reblogged, muted, bookmarked) = if viewer.is_some() {
        (
            Some(viewer_state.favourited),
            Some(viewer_state.reblogged),
            Some(viewer_state.muted),
            Some(viewer_state.bookmarked),
        )
    } else {
        (None, None, None, None)
    };
    let in_reply_to_id = if let Some(ref id) = status.in_reply_to_id {
        Some(id.clone())
    } else {
        match remote_in_reply_to_preload {
            Some(preload) => preload.get(&status.id).cloned().unwrap_or(None),
            None => {
                resolve_remote_in_reply_to_status_id(db, config, status.in_reply_to_uri.as_deref())
                    .await?
            }
        }
    };

    Ok(RemoteStatusResponseDetails {
        media_attachments,
        card,
        poll,
        mentions,
        favourites_count,
        reblogs_count,
        quotes_count,
        favourited,
        reblogged,
        muted,
        bookmarked,
        edited_at,
        filtered,
        quote_approval,
        quote,
        in_reply_to_id,
    })
}

async fn resolve_remote_in_reply_to_status_id(
    db: &D1Database,
    config: &AppConfig,
    in_reply_to_uri: Option<&str>,
) -> Result<Option<String>> {
    let Some(in_reply_to_uri) = in_reply_to_uri.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, in_reply_to_uri).await? {
        return Ok(Some(status.id));
    }
    if let Some(status) = find_local_status_by_object_uri(db, config, in_reply_to_uri).await? {
        return Ok(Some(status.id));
    }
    Ok(None)
}

async fn remote_status_response_viewer_state(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
    preload: Option<&RemoteStatusViewerStatePreload>,
) -> Result<RemoteStatusResponseViewerState> {
    if let Some(state) =
        preloaded_remote_status_response_viewer_state(viewer, &status.id, &actor.actor_uri, preload)
    {
        return Ok(state);
    }

    let Some(viewer) = viewer else {
        return Ok(RemoteStatusResponseViewerState::default());
    };

    Ok(RemoteStatusResponseViewerState {
        favourited: is_remote_status_favourited_by(db, viewer.id(), &status.id).await?,
        reblogged: is_remote_status_reblogged_by(db, viewer.id(), &status.id).await?,
        bookmarked: is_remote_status_bookmarked_by(db, viewer.id(), &status.id).await?,
        muted: is_muted_actor(db, viewer.id(), &actor.actor_uri).await?,
    })
}

async fn build_quoted_status_value(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: Option<&str>,
    local_quote_state: Option<&str>,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(None);
    };
    if let Some(document) = quote_document_for_local_state(local_quote_state) {
        return Ok(Some(document));
    }

    if let Some(resolved) = boost_target_preload.and_then(|targets| targets.target(quote_of_uri)) {
        match resolved {
            Some(BoostTarget::Local(local_status)) => {
                return build_local_quoted_status_from_row(
                    db,
                    config,
                    viewer,
                    local_status.clone(),
                    filter_matcher,
                    counts_preload,
                )
                .await;
            }
            Some(BoostTarget::Remote(remote_status)) => {
                return build_remote_quoted_status_from_row(
                    db,
                    config,
                    viewer,
                    remote_status.clone(),
                    pending_remote_quote,
                    filter_matcher,
                    counts_preload,
                )
                .await;
            }
            None => return Ok(None),
        }
    }

    if let Some(document) = build_local_quoted_status_document(
        db,
        config,
        viewer,
        quote_of_uri,
        filter_matcher,
        counts_preload,
    )
    .await?
    {
        return Ok(Some(document));
    }

    build_remote_quoted_status_document(
        db,
        config,
        viewer,
        quote_of_uri,
        pending_remote_quote,
        filter_matcher,
        counts_preload,
    )
    .await
}

async fn build_local_quoted_status_document(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(local_status) = find_local_status_by_object_uri(db, config, quote_of_uri).await?
    else {
        return Ok(None);
    };
    build_local_quoted_status_from_row(
        db,
        config,
        viewer,
        local_status,
        filter_matcher,
        counts_preload,
    )
    .await
}

async fn build_local_quoted_status_from_row(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    local_status: StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(subject) = resolve_local_status_response_subject(db, viewer, local_status).await?
    else {
        return Ok(None);
    };
    let super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
        return Ok(Some(unauthorized_quote_document()));
    };
    let super::LoadedLocalStatusResponseSubject {
        status: local_status,
        account: local_account,
        preload:
            super::LocalStatusResponsePreload {
                media,
                in_reply_to_account_id,
            },
    } = subject;
    let mut response = MastodonStatusResponse::from_row(
        &local_status,
        &local_account,
        config,
        in_reply_to_account_id,
        media,
    );
    response.card = build_status_card_value(&local_status.text);
    response.poll = load_mastodon_poll_response(db, &local_status.id, viewer).await?;
    response.filtered = if viewer.is_some() {
        Some(local_status_filtered_for_viewer(db, viewer, &local_status, filter_matcher).await?)
    } else {
        None
    };
    response.mentions = build_status_mentions(db, config, &local_status.text).await?;
    let (favourites_count, reblogs_count) =
        local_status_counts(db, counts_preload, &local_status.id).await?;
    response.favourites_count = favourites_count;
    response.reblogs_count = reblogs_count;
    let viewer_state = local_status_response_viewer_state(db, viewer, &local_status, None).await?;
    let viewer_fields = local_viewer_interaction_fields(viewer, &local_status, viewer_state);
    response.favourited = viewer_fields.favourited;
    response.reblogged = viewer_fields.reblogged;
    response.bookmarked = viewer_fields.bookmarked;
    response.pinned = viewer_fields.pinned;
    response.muted = viewer_fields.muted;
    response.quote = None;
    let state = local_quoted_status_document_state(db, config, viewer, &local_account).await?;
    Ok(Some(quote_document_from_response(state, response)))
}

async fn build_remote_quoted_status_document(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    quote_of_uri: &str,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    let Some(remote_status) = find_remote_status_by_url_or_object_uri(db, quote_of_uri).await?
    else {
        return Ok(None);
    };
    build_remote_quoted_status_from_row(
        db,
        config,
        viewer,
        remote_status,
        pending_remote_quote,
        filter_matcher,
        counts_preload,
    )
    .await
}

async fn build_remote_quoted_status_from_row(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    remote_status: RemoteStatusRow,
    pending_remote_quote: bool,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
) -> Result<Option<serde_json::Value>> {
    if pending_remote_quote {
        return Ok(Some(pending_quote_document()));
    }
    if !remote_quote_visibility_is_embeddable(remote_status.visibility.as_str()) {
        return Ok(Some(unauthorized_quote_document()));
    }
    let Some(actor) = find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await? else {
        return Ok(None);
    };
    let federated_emojis = federated_emojis_for_remote_status(db, &remote_status.id, None).await?;
    let mut response = MastodonStatusResponse::from_remote_row(
        &remote_status,
        &actor,
        config,
        federated_emojis.as_ref(),
    );
    let text_content = remote_status.plain_text();
    let remote_attachments =
        find_remote_status_attachments_by_status_id(db, &remote_status.id).await?;
    response.card = build_remote_status_card_value(&text_content, &remote_attachments);
    response.media_attachments = remote_media_attachment_values(&remote_attachments);
    response.filtered = if viewer.is_some() {
        Some(
            remote_status_filtered_for_viewer(
                db,
                viewer,
                &remote_status,
                &text_content,
                filter_matcher,
            )
            .await?,
        )
    } else {
        None
    };
    response.mentions = build_status_mentions(db, config, &text_content).await?;
    let (favourites_count, reblogs_count) =
        remote_status_counts(db, counts_preload, &remote_status.id).await?;
    response.favourites_count = favourites_count;
    response.reblogs_count = reblogs_count;
    let viewer_state =
        remote_status_response_viewer_state(db, viewer, &remote_status, &actor, None).await?;
    if viewer.is_some() {
        response.favourited = Some(viewer_state.favourited);
        response.reblogged = Some(viewer_state.reblogged);
        response.bookmarked = Some(viewer_state.bookmarked);
        response.muted = Some(viewer_state.muted);
    }
    response.in_reply_to_id =
        resolve_remote_in_reply_to_status_id(db, config, remote_status.in_reply_to_uri.as_deref())
            .await?;
    response.poll = load_remote_mastodon_poll_response(db, &remote_status, viewer).await?;
    response.quote = None;
    let state = remote_quoted_status_document_state(db, viewer, &actor).await?;
    Ok(Some(quote_document_from_response(state, response)))
}

async fn local_status_filtered_for_viewer(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<Vec<serde_json::Value>> {
    let Some(viewer) = viewer else {
        return Ok(Vec::new());
    };
    filtered_status_for_viewer(
        db,
        filter_matcher,
        viewer.id(),
        &status.id,
        &status.text,
        &status.spoiler_text,
    )
    .await
}

async fn remote_status_filtered_for_viewer(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    text_content: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
) -> Result<Vec<serde_json::Value>> {
    let Some(viewer) = viewer else {
        return Ok(Vec::new());
    };
    filtered_status_for_viewer(
        db,
        filter_matcher,
        viewer.id(),
        &status.id,
        text_content,
        &status.spoiler_text,
    )
    .await
}

async fn filtered_status_for_viewer(
    db: &D1Database,
    filter_matcher: Option<&AccountFilterMatcher>,
    account_id: &str,
    status_id: &str,
    text: &str,
    spoiler_text: &str,
) -> Result<Vec<serde_json::Value>> {
    if let Some(filter_matcher) = filter_matcher {
        return Ok(filter_matcher.filtered_status(status_id, text, spoiler_text));
    }

    load_status_filtered(db, account_id, status_id, text, spoiler_text).await
}

async fn local_status_counts(
    db: &D1Database,
    counts_preload: Option<&StatusCountsPreload>,
    status_id: &str,
) -> Result<(u64, u64)> {
    if let Some(counts) = counts_preload.and_then(|counts| counts.local_counts(status_id)) {
        return Ok(counts);
    }

    load_local_status_counts(db, status_id).await
}

async fn remote_status_counts(
    db: &D1Database,
    counts_preload: Option<&StatusCountsPreload>,
    status_id: &str,
) -> Result<(u64, u64)> {
    if let Some(counts) = counts_preload.and_then(|counts| counts.remote_counts(status_id)) {
        return Ok(counts);
    }

    load_remote_status_counts(db, status_id).await
}

async fn build_remote_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &RemoteStatusRow,
    wrapper_actor: &RemoteActorRow,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    let embedded = build_reblog_embedded_response(
        db,
        config,
        None,
        viewer,
        boost_of_uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        None,
        None,
        None,
        boost_target_preload,
        viewer_state_preload,
        poll_preload,
        edit_updated_at_preload,
        federated_emojis_preload,
        mention_preload,
        remote_in_reply_to_preload,
        remote_actors_preload,
        remote_attachments_preload,
        include_quote,
    )
    .await?;

    Ok(remote_reblog_wrapper_response_from_embedded(
        embedded,
        wrapper_status,
        wrapper_actor,
        config,
    ))
}

async fn build_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    resolved_config: Option<&AppConfig>,
    viewer: Option<&LocalAccount>,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    remote_poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    remote_edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    let target = match boost_target_preload.and_then(|targets| targets.target(boost_of_uri)) {
        Some(target) => target.cloned(),
        None => resolve_boost_target(db, config, boost_of_uri).await?,
    };

    match target {
        Some(BoostTarget::Local(local_status)) => {
            build_local_reblog_embedded_response(
                db,
                config,
                resolved_config,
                viewer,
                local_status,
                filter_matcher,
                counts_preload,
                quote_counts_preload,
                poll_preload,
                viewer_state_preload,
                application_preload,
                boost_target_preload,
                include_quote,
            )
            .await
        }
        Some(BoostTarget::Remote(remote_status)) => {
            build_remote_reblog_embedded_response(
                db,
                config,
                viewer,
                remote_status,
                filter_matcher,
                counts_preload,
                quote_counts_preload,
                remote_viewer_state_preload,
                remote_poll_preload,
                remote_edit_updated_at_preload,
                federated_emojis_preload,
                mention_preload,
                boost_target_preload,
                remote_in_reply_to_preload,
                remote_actors_preload,
                remote_attachments_preload,
                include_quote,
            )
            .await
        }
        None => Ok(None),
    }
}

async fn build_local_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    resolved_config: Option<&AppConfig>,
    viewer: Option<&LocalAccount>,
    local_status: StatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    let Some(subject) = resolve_local_status_response_subject(db, viewer, local_status).await?
    else {
        return Ok(None);
    };
    let super::ResolvedLocalStatusResponseSubject::Loaded(subject) = subject else {
        return Ok(None);
    };
    let super::LoadedLocalStatusResponseSubject {
        status: local_status,
        account: local_account,
        preload:
            super::LocalStatusResponsePreload {
                media,
                in_reply_to_account_id,
            },
    } = subject;
    Ok(Some(
        Box::pin(build_local_status_response_inner(
            db,
            config,
            resolved_config,
            viewer,
            &local_status,
            &local_account,
            in_reply_to_account_id,
            media,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            poll_preload,
            viewer_state_preload,
            application_preload,
            None,
            boost_target_preload,
            include_quote,
        ))
        .await?,
    ))
}

async fn build_remote_reblog_embedded_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    remote_status: RemoteStatusRow,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    viewer_state_preload: Option<&RemoteStatusViewerStatePreload>,
    poll_preload: Option<&RemoteMastodonPollResponsePreload>,
    edit_updated_at_preload: Option<&RemoteStatusEditUpdatedAtPreload>,
    federated_emojis_preload: Option<&RemoteStatusFederatedEmojisPreload>,
    mention_preload: Option<&MentionAccountsPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    remote_in_reply_to_preload: Option<&HashMap<String, Option<String>>>,
    remote_actors_preload: Option<&HashMap<String, RemoteActorRow>>,
    remote_attachments_preload: Option<&HashMap<String, Vec<RemoteStatusAttachmentRow>>>,
    include_quote: bool,
) -> Result<Option<MastodonStatusResponse>> {
    if !matches!(remote_status.visibility.as_str(), "public" | "unlisted") {
        return Ok(None);
    }

    let preloaded_actor =
        remote_actors_preload.and_then(|actors| actors.get(&remote_status.actor_uri));
    let looked_up_actor;
    let actor = if let Some(actor) = preloaded_actor {
        actor
    } else {
        looked_up_actor = match find_remote_actor_by_actor_uri(db, &remote_status.actor_uri).await?
        {
            Some(actor) => actor,
            None => return Ok(None),
        };
        &looked_up_actor
    };

    let remote_attachments = remote_attachments_preload
        .and_then(|attachments| attachments.get(&remote_status.id).cloned());

    Ok(Some(
        Box::pin(build_remote_status_response_inner(
            db,
            config,
            viewer,
            &remote_status,
            actor,
            filter_matcher,
            counts_preload,
            quote_counts_preload,
            viewer_state_preload,
            poll_preload,
            edit_updated_at_preload,
            federated_emojis_preload,
            remote_attachments,
            mention_preload,
            boost_target_preload,
            remote_in_reply_to_preload,
            remote_actors_preload,
            remote_attachments_preload,
            include_quote,
        ))
        .await?,
    ))
}

async fn build_local_reblog_wrapper_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    wrapper_status: &StatusRow,
    wrapper_account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    boost_of_uri: &str,
    filter_matcher: Option<&AccountFilterMatcher>,
    counts_preload: Option<&StatusCountsPreload>,
    quote_counts_preload: Option<&StatusQuoteCountsPreload>,
    poll_preload: Option<&MastodonPollResponsePreload>,
    viewer_state_preload: Option<&LocalStatusViewerStatePreload>,
    application_preload: Option<&StatusApplicationPreload>,
    boost_target_preload: Option<&BoostTargetPreload>,
    include_quote: bool,
) -> Result<MastodonStatusResponse> {
    // The only caller resolves the emoji registry before delegating here, so the
    // embedded status can reuse it instead of re-reading `custom_emojis`.
    let embedded = build_reblog_embedded_response(
        db,
        config,
        Some(config),
        viewer,
        boost_of_uri,
        filter_matcher,
        counts_preload,
        quote_counts_preload,
        poll_preload,
        viewer_state_preload,
        application_preload,
        boost_target_preload,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        include_quote,
    )
    .await?;

    Ok(local_reblog_wrapper_response_from_embedded(
        embedded,
        wrapper_status,
        wrapper_account,
        in_reply_to_account_id,
        config,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::LocalAccountRecord;

    #[test]
    fn remote_media_attachment_values_allows_empty_attachments() {
        assert!(remote_media_attachment_values(&[]).is_empty());
    }

    #[test]
    fn remote_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_actor = remote_actor_row_fixture();
        let wrapper_status =
            remote_status_row_fixture("wrapper-status", "https://remote.example/announce/1");
        let embedded_status =
            remote_status_row_fixture("embedded-status", "https://remote.example/statuses/1");
        let mut embedded = MastodonStatusResponse::from_remote_row(
            &embedded_status,
            &wrapper_actor,
            &config,
            None,
        );
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = remote_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_actor,
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(response.uri, "https://remote.example/announce/1");
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    #[test]
    fn local_reblog_wrapper_response_overlays_wrapper_fields_and_clears_embedded_body() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let wrapper_account = local_account_fixture();
        let wrapper_status = status_row_fixture(
            "wrapper-status",
            Some("https://social.example/users/alice/statuses/wrapper"),
        );
        let embedded_status = status_row_fixture(
            "embedded-status",
            Some("https://social.example/users/alice/statuses/embedded"),
        );
        let mut embedded = MastodonStatusResponse::from_row(
            &embedded_status,
            &wrapper_account,
            &config,
            None,
            Vec::new(),
        );
        embedded.media_attachments = vec![serde_json::json!({"id": "media-1"})];
        embedded.quote = Some(serde_json::json!({"state": "accepted"}));

        let response = local_reblog_wrapper_response_from_embedded(
            Some(embedded),
            &wrapper_status,
            &wrapper_account,
            Some("reply-account".to_owned()),
            &config,
        );

        assert_eq!(response.id, "wrapper-status");
        assert_eq!(
            response.uri,
            "https://social.example/users/alice/statuses/wrapper"
        );
        assert_eq!(
            response.in_reply_to_account_id.as_deref(),
            Some("reply-account")
        );
        assert!(response.reblog.is_some());
        assert!(response.content.is_empty());
        assert!(response.media_attachments.is_empty());
        assert!(response.quote.is_none());
    }

    fn remote_status_row_fixture(id: &str, object_uri: &str) -> RemoteStatusRow {
        RemoteStatusRow {
            id: id.to_owned(),
            actor_uri: "https://remote.example/users/alice".to_owned(),
            object_uri: object_uri.to_owned(),
            url: None,
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text_content: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: cfwdon_domain::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_state: cfwdon_domain::QuoteState::Accepted,
            published_at: "2026-05-10T01:02:03Z".to_owned(),
            edited_at: None,
            card_json: None,
            federated_emojis_json: "[]".to_owned(),
            in_reply_to_id: None,
        }
    }

    fn remote_actor_row_fixture() -> RemoteActorRow {
        RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-05-01T00:00:00Z".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Alice".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: None,
            header_url: None,
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            social_counts_updated_at: None,
        }
    }

    fn status_row_fixture(id: &str, ap_id: Option<&str>) -> StatusRow {
        StatusRow {
            id: id.to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id: ap_id.map(str::to_owned),
            in_reply_to_id: None,
            in_reply_to_account_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: cfwdon_domain::Visibility::Public,
            sensitive: false,
            language: Some("en".to_owned()),
            quote_approval_policy: None,
            quote_state: cfwdon_domain::QuoteState::Accepted,
            application_id: None,
            card_json: None,
            created_at: "2026-05-10T01:02:03Z".to_owned(),
            updated_at: None,
        }
    }

    fn local_account_fixture() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice"))
    }
}
