use super::{
    PreparedTimelineCandidate, PublicTimelineCandidate, PublicTimelineCandidateEntry,
    TimelineEntry, collect_boost_target_preload_ids,
    preload_public_timeline_candidate_counts, preload_public_timeline_local_polls,
    preload_public_timeline_local_viewer_state, preload_public_timeline_quote_counts,
    preload_public_timeline_remote_attachments, preload_public_timeline_remote_edits,
    preload_public_timeline_remote_federated_emojis, preload_public_timeline_remote_polls,
    preload_public_timeline_remote_viewer_state, preload_remote_in_reply_to_status_ids,
    preload_timeline_candidate_reply_account_ids,
};
use std::collections::HashSet;
use crate::{
    AccountFilterMatcher, AppConfig, D1Database, LocalAccount, Result,
    build_local_status_response_with_timeline_preloads,
    build_remote_status_response_with_timeline_preloads, config_with_resolved_custom_emojis,
    enrich_card_with_remote_preview, find_remote_actors_by_actor_uris,
    find_remote_status_attachments_by_status_ids, preload_boost_targets,
    preload_mention_accounts_from_texts, preload_remote_mastodon_poll_responses,
    preload_remote_status_edit_updated_at, preload_remote_status_federated_emojis,
    preload_status_applications, preload_status_counts, preload_status_quote_counts,
};
use std::collections::HashMap;
use worker::Error;

/// Boost and quote target URIs referenced by a page, for [`crate::preload_boost_targets`].
fn embedded_status_uris(candidates: &[PublicTimelineCandidateEntry]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut uris = Vec::new();
    for entry in candidates {
        let (boost_of_uri, quote_of_uri) = match &entry.candidate {
            PublicTimelineCandidate::Local { status, .. } => {
                (status.boost_of_uri.as_ref(), status.quote_of_uri.as_ref())
            }
            PublicTimelineCandidate::Remote { status, .. } => {
                (status.boost_of_uri.as_ref(), status.quote_of_uri.as_ref())
            }
        };
        for uri in [boost_of_uri, quote_of_uri].into_iter().flatten() {
            if seen.insert(uri.as_str()) {
                uris.push(uri.clone());
            }
        }
    }
    uris
}

struct TimelineCandidateRenderContext {
    counts_preload: crate::StatusCountsPreload,
    quote_counts_preload: crate::StatusQuoteCountsPreload,
    local_poll_preload: crate::MastodonPollResponsePreload,
    local_viewer_state_preload: crate::LocalStatusViewerStatePreload,
    remote_viewer_state_preload: crate::RemoteStatusViewerStatePreload,
    remote_poll_preload: crate::RemoteMastodonPollResponsePreload,
    remote_edit_updated_at_preload: crate::RemoteStatusEditUpdatedAtPreload,
    remote_federated_emojis_preload: crate::RemoteStatusFederatedEmojisPreload,
    in_reply_to_account_ids: HashMap<String, String>,
    application_preload: crate::StatusApplicationPreload,
    remote_attachments_by_status_id: HashMap<String, Vec<crate::RemoteStatusAttachmentRow>>,
    mention_preload: crate::MentionAccountsPreload,
    emoji_resolved_config: AppConfig,
    boost_target_preload: crate::BoostTargetPreload,
    remote_in_reply_to_preload: HashMap<String, Option<String>>,
    remote_actors_preload: HashMap<String, crate::RemoteActorRow>,
}

fn collect_timeline_candidate_mention_texts(
    candidates: &[PublicTimelineCandidateEntry],
) -> Vec<String> {
    let mut mention_texts = Vec::with_capacity(candidates.len());
    let mut remote_text_owned = Vec::new();
    for candidate in candidates {
        match &candidate.candidate {
            PublicTimelineCandidate::Local { status, .. } => {
                mention_texts.push(status.text.clone());
            }
            PublicTimelineCandidate::Remote { status, .. } => {
                remote_text_owned.push(status.plain_text());
            }
        }
    }
    mention_texts.extend(remote_text_owned);
    mention_texts
}

async fn preload_public_timeline_status_applications(
    db: &D1Database,
    config: &AppConfig,
    candidates: &[PublicTimelineCandidateEntry],
) -> Result<crate::StatusApplicationPreload> {
    let statuses = candidates
        .iter()
        .filter_map(|entry| match &entry.candidate {
            PublicTimelineCandidate::Local { status, .. } => Some(status),
            PublicTimelineCandidate::Remote { .. } => None,
        })
        .collect::<Vec<_>>();

    preload_status_applications(db, config, &statuses).await
}

async fn preload_timeline_candidate_render_context(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    local_accounts_by_id: &HashMap<String, LocalAccount>,
    candidates: &[PublicTimelineCandidateEntry],
    known_viewer_has_thread_mutes: Option<bool>,
) -> Result<TimelineCandidateRenderContext> {
    let mention_texts = collect_timeline_candidate_mention_texts(candidates);
    let mention_text_refs = mention_texts.iter().map(String::as_str).collect::<Vec<_>>();
    let boost_of_uris = embedded_status_uris(candidates);

    let (
        mut counts_preload,
        mut quote_counts_preload,
        local_poll_preload,
        local_viewer_state_preload,
        remote_viewer_state_preload,
        mut remote_poll_preload,
        mut remote_edit_updated_at_preload,
        mut remote_federated_emojis_preload,
        in_reply_to_account_ids,
        application_preload,
        mut remote_attachments_by_status_id,
        mention_preload,
        emoji_resolved_config,
        boost_target_preload,
    ) = futures_util::try_join!(
        preload_public_timeline_candidate_counts(db, candidates),
        preload_public_timeline_quote_counts(db, config, candidates, local_accounts_by_id),
        preload_public_timeline_local_polls(db, candidates, viewer),
        preload_public_timeline_local_viewer_state(
            db,
            candidates,
            viewer,
            known_viewer_has_thread_mutes,
        ),
        preload_public_timeline_remote_viewer_state(db, candidates, viewer),
        preload_public_timeline_remote_polls(db, candidates, viewer),
        preload_public_timeline_remote_edits(db, candidates),
        preload_public_timeline_remote_federated_emojis(db, candidates),
        preload_timeline_candidate_reply_account_ids(db, candidates),
        preload_public_timeline_status_applications(db, config, candidates),
        preload_public_timeline_remote_attachments(db, candidates),
        preload_mention_accounts_from_texts(db, config, &mention_text_refs),
        config_with_resolved_custom_emojis(db, config),
        preload_boost_targets(db, config, &boost_of_uris),
    )?;

    let boost_ids = collect_boost_target_preload_ids(&boost_target_preload);
    let boost_remote_status_refs = boost_ids.remote_statuses.iter().collect::<Vec<_>>();
    let (
        boost_counts,
        boost_quote_counts,
        boost_remote_polls,
        boost_remote_edits,
        boost_remote_emojis,
        boost_remote_attachments,
        boost_remote_actors,
        remote_in_reply_to_preload,
    ) = futures_util::try_join!(
        preload_status_counts(db, &boost_ids.local_ids, &boost_ids.remote_ids),
        preload_status_quote_counts(db, &boost_ids.remote_quote_uris),
        preload_remote_mastodon_poll_responses(db, &boost_ids.remote_ids, viewer),
        preload_remote_status_edit_updated_at(db, &boost_ids.remote_ids),
        preload_remote_status_federated_emojis(db, &boost_ids.remote_ids),
        find_remote_status_attachments_by_status_ids(db, &boost_ids.remote_ids),
        find_remote_actors_by_actor_uris(db, &boost_ids.remote_actor_uris),
        preload_remote_in_reply_to_status_ids(db, config, candidates, &boost_remote_status_refs),
    )?;
    counts_preload.extend(boost_counts);
    quote_counts_preload.extend(boost_quote_counts);
    remote_poll_preload.extend(boost_remote_polls);
    remote_edit_updated_at_preload.extend(boost_remote_edits);
    remote_federated_emojis_preload.extend(boost_remote_emojis);
    for (status_id, attachments) in boost_remote_attachments {
        remote_attachments_by_status_id
            .entry(status_id)
            .or_insert(attachments);
    }

    Ok(TimelineCandidateRenderContext {
        counts_preload,
        quote_counts_preload,
        local_poll_preload,
        local_viewer_state_preload,
        remote_viewer_state_preload,
        remote_poll_preload,
        remote_edit_updated_at_preload,
        remote_federated_emojis_preload,
        in_reply_to_account_ids,
        application_preload,
        remote_attachments_by_status_id,
        mention_preload,
        emoji_resolved_config,
        boost_target_preload,
        remote_in_reply_to_preload,
        remote_actors_preload: boost_remote_actors,
    })
}

fn prepare_owned_timeline_candidates<'a>(
    local_accounts_by_id: &'a HashMap<String, LocalAccount>,
    candidates: Vec<PublicTimelineCandidateEntry>,
    remote_attachments_by_status_id: &mut HashMap<String, Vec<crate::RemoteStatusAttachmentRow>>,
) -> Vec<(String, String, PreparedTimelineCandidate<'a>)> {
    let mut prepared = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match candidate.candidate {
            PublicTimelineCandidate::Local { status, media } => {
                let Some(account) = local_accounts_by_id.get(&status.account_id) else {
                    continue;
                };
                prepared.push((
                    candidate.timestamp,
                    candidate.id,
                    PreparedTimelineCandidate::Local {
                        status,
                        media,
                        account,
                    },
                ));
            }
            PublicTimelineCandidate::Remote { status, actor } => {
                let attachments = remote_attachments_by_status_id
                    .remove(&status.id)
                    .unwrap_or_default();
                prepared.push((
                    candidate.timestamp,
                    candidate.id,
                    PreparedTimelineCandidate::Remote {
                        status,
                        actor,
                        attachments,
                    },
                ));
            }
        }
    }
    prepared
}

async fn render_prepared_timeline_candidates(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    filter_matcher: Option<&AccountFilterMatcher>,
    context: &TimelineCandidateRenderContext,
    prepared: Vec<(String, String, PreparedTimelineCandidate<'_>)>,
    enrich_cards: bool,
) -> Result<Vec<TimelineEntry>> {
    futures_util::future::try_join_all(prepared.into_iter().map(
        |(timestamp, id, prepared)| async move {
            let mut value = match prepared {
                PreparedTimelineCandidate::Local {
                    status,
                    media,
                    account,
                } => serde_json::to_value(
                    build_local_status_response_with_timeline_preloads(
                        db,
                        config,
                        Some(&context.emoji_resolved_config),
                        viewer,
                        &status,
                        account,
                        context.in_reply_to_account_ids.get(&status.id).cloned(),
                        media,
                        filter_matcher,
                        Some(&context.counts_preload),
                        Some(&context.quote_counts_preload),
                        Some(&context.local_poll_preload),
                        Some(&context.local_viewer_state_preload),
                        Some(&context.application_preload),
                        Some(&context.mention_preload),
                        Some(&context.boost_target_preload),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null),
                PreparedTimelineCandidate::Remote {
                    status,
                    actor,
                    attachments,
                } => serde_json::to_value(
                    build_remote_status_response_with_timeline_preloads(
                        db,
                        config,
                        viewer,
                        &status,
                        &actor,
                        filter_matcher,
                        Some(&context.counts_preload),
                        Some(&context.quote_counts_preload),
                        Some(&context.remote_viewer_state_preload),
                        Some(&context.remote_poll_preload),
                        Some(&context.remote_edit_updated_at_preload),
                        Some(&context.remote_federated_emojis_preload),
                        attachments,
                        Some(&context.mention_preload),
                        Some(&context.boost_target_preload),
                        Some(&context.remote_in_reply_to_preload),
                        Some(&context.remote_actors_preload),
                        Some(&context.remote_attachments_by_status_id),
                    )
                    .await?,
                )
                .unwrap_or(serde_json::Value::Null),
            };
            if enrich_cards && let Some(card) = value.get_mut("card") {
                let _ = enrich_card_with_remote_preview(card).await;
            }
            Ok::<TimelineEntry, Error>((timestamp, id, value))
        },
    ))
    .await
}

pub(super) async fn timeline_entries_from_candidates(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    filter_matcher: Option<&AccountFilterMatcher>,
    local_accounts_by_id: &HashMap<String, LocalAccount>,
    candidates: Vec<PublicTimelineCandidateEntry>,
    enrich_cards: bool,
    known_viewer_has_thread_mutes: Option<bool>,
) -> Result<Vec<TimelineEntry>> {
    let mut context = preload_timeline_candidate_render_context(
        db,
        config,
        viewer,
        local_accounts_by_id,
        &candidates,
        known_viewer_has_thread_mutes,
    )
    .await?;
    let prepared = prepare_owned_timeline_candidates(
        local_accounts_by_id,
        candidates,
        &mut context.remote_attachments_by_status_id,
    );
    render_prepared_timeline_candidates(
        db,
        config,
        viewer,
        filter_matcher,
        &context,
        prepared,
        enrich_cards,
    )
    .await
}
