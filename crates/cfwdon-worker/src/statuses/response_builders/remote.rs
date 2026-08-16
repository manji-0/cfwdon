use super::super::{
    AccountFilterMatcher, AppConfig, BoostTargetPreload, FederatedEmojiMap, LocalAccount,
    MastodonStatusResponse, MentionAccountsPreload, RemoteActorRow,
    RemoteMastodonPollResponsePreload, RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload,
    RemoteStatusFederatedEmojisPreload, RemoteStatusResponseDetails,
    RemoteStatusResponseViewerState, RemoteStatusRow, RemoteStatusViewerStatePreload,
    StatusCountsPreload, StatusQuoteCountsPreload, build_remote_quote_approval,
    build_remote_status_card_value, build_status_mentions_with_preload,
    effective_remote_status_quote_state, extract_federated_emojis_from_activitypub_object,
    find_local_status_by_object_uri, find_remote_status_attachments_by_status_id,
    find_remote_status_by_url_or_object_uri, find_remote_status_raw_object_by_id,
    has_remote_status_edit_snapshots, is_muted_actor, is_remote_status_bookmarked_by,
    is_remote_status_favourited_by, is_remote_status_reblogged_by,
    load_remote_mastodon_poll_response, load_remote_status_counts, load_remote_status_updated_at,
    load_stored_remote_status_mentions, preloaded_remote_status_response_viewer_state,
    status_quotes_count,
};
use std::collections::HashMap;
use worker::Result;

use crate::D1Database;

pub(super) fn remote_media_attachment_values(
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

pub(super) async fn federated_emojis_for_remote_status(
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

pub(super) fn federated_emojis_from_json(federated_emojis_json: &str) -> Option<FederatedEmojiMap> {
    if federated_emojis_json.is_empty()
        || federated_emojis_json == "[]"
        || federated_emojis_json == "{}"
    {
        return None;
    }
    serde_json::from_str(federated_emojis_json).ok()
}

pub(super) async fn build_remote_status_response_inner(
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
        return super::build_remote_reblog_wrapper_response(
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
        super::build_quoted_status_value(
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

pub(super) async fn resolve_remote_in_reply_to_status_id(
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

pub(super) async fn remote_status_response_viewer_state(
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

pub(super) async fn remote_status_filtered_for_viewer(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    text_content: &str,
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
        text_content,
        &status.spoiler_text,
    )
    .await
}

pub(super) async fn remote_status_counts(
    db: &D1Database,
    counts_preload: Option<&StatusCountsPreload>,
    status_id: &str,
) -> Result<(u64, u64)> {
    if let Some(counts) = counts_preload.and_then(|counts| counts.remote_counts(status_id)) {
        return Ok(counts);
    }

    load_remote_status_counts(db, status_id).await
}
