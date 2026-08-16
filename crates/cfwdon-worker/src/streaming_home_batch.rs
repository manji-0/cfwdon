use crate::timelines::{
    ResolvedTimelineCursor, TimelinePaginationQuery, resolve_timeline_cursor, timeline_fetch_limit,
};
use crate::{
    BoostTargetPreload, D1Database, LocalAccount, LocalStatusViewerStatePreload,
    MastodonPollResponsePreload, MentionAccountsPreload, RemoteMastodonPollResponsePreload,
    RemoteStatusEditUpdatedAtPreload, RemoteStatusFederatedEmojisPreload,
    RemoteStatusViewerStatePreload, Result, StatusApplicationPreload, StatusCountsPreload,
    StatusQuoteCountsPreload, StreamingBatch, StreamingEntry, account_has_thread_mutes, actor_url,
    build_local_status_response_with_timeline_preloads,
    build_remote_status_response_with_timeline_preloads, config_with_resolved_custom_emojis,
    find_accounts_by_ids, find_media_attachments_by_status_ids,
    find_remote_status_attachments_by_status_ids, list_active_muted_actor_uris_for_account,
    list_followed_tag_names, list_local_home_timeline_statuses, list_local_public_statuses_by_tag,
    list_remote_home_timeline_statuses, list_remote_public_statuses_by_tag,
    load_in_reply_to_account_ids, local_status_ids_thread_muted_by, preload_boost_targets,
    preload_local_status_viewer_state, preload_mastodon_poll_responses,
    preload_mention_accounts_from_texts, preload_remote_mastodon_poll_responses,
    preload_remote_status_edit_updated_at, preload_remote_status_federated_emojis,
    preload_remote_status_viewer_state, preload_status_applications, preload_status_counts,
    preload_status_quote_counts, streaming_batch_from_entries,
};
use cfwdon_core::AppConfig;
use std::collections::{HashMap, HashSet};

enum StreamingHomeCandidate {
    Local(crate::StatusRow),
    Remote(crate::RemoteStatusRow, crate::RemoteActorRow),
}

enum PreparedStreamingHomeCandidate<'a> {
    Local {
        status: crate::StatusRow,
        media: Vec<crate::MediaAttachmentRow>,
        account: &'a LocalAccount,
    },
    Remote {
        status: crate::RemoteStatusRow,
        actor: crate::RemoteActorRow,
        attachments: Vec<crate::RemoteStatusAttachmentRow>,
    },
}

struct StreamingHomeCandidateLoad {
    candidate_rows: Vec<StreamingHomeCandidate>,
    muted_actor_uris: HashSet<String>,
    viewer_has_thread_mutes: bool,
}

struct StreamingHomeRenderPlan {
    local_status_ids: Vec<String>,
    remote_status_ids: Vec<String>,
    local_statuses_for_replies: Vec<crate::StatusRow>,
    quote_status_uris: Vec<String>,
    mention_texts: Vec<String>,
    boost_of_uris: Vec<String>,
}

struct StreamingHomePreloads {
    counts_preload: StatusCountsPreload,
    quote_counts_preload: StatusQuoteCountsPreload,
    local_poll_preload: MastodonPollResponsePreload,
    local_viewer_state_preload: LocalStatusViewerStatePreload,
    remote_viewer_state_preload: RemoteStatusViewerStatePreload,
    remote_poll_preload: RemoteMastodonPollResponsePreload,
    remote_edit_updated_at_preload: RemoteStatusEditUpdatedAtPreload,
    remote_federated_emojis_preload: RemoteStatusFederatedEmojisPreload,
    in_reply_to_account_ids: HashMap<String, String>,
    application_preload: StatusApplicationPreload,
    mention_preload: MentionAccountsPreload,
    emoji_resolved_config: AppConfig,
    boost_target_preload: BoostTargetPreload,
}

fn push_unique_streaming_home_candidate(
    seen_status_ids: &mut HashSet<String>,
    candidate_rows: &mut Vec<StreamingHomeCandidate>,
    candidate: StreamingHomeCandidate,
) {
    let status_id = match &candidate {
        StreamingHomeCandidate::Local(status) => status.id.clone(),
        StreamingHomeCandidate::Remote(status, _) => status.id.clone(),
    };
    if seen_status_ids.insert(status_id) {
        candidate_rows.push(candidate);
    }
}

async fn load_streaming_home_candidate_rows(
    db: &D1Database,
    viewer_id: &str,
    cursor: &ResolvedTimelineCursor,
    query_limit: u32,
) -> Result<StreamingHomeCandidateLoad> {
    let (
        local_home_statuses,
        remote_home_statuses,
        followed_tags,
        muted_actor_uris,
        viewer_has_thread_mutes,
    ) = futures_util::try_join!(
        list_local_home_timeline_statuses(db, viewer_id, cursor, query_limit),
        list_remote_home_timeline_statuses(db, viewer_id, cursor, query_limit),
        list_followed_tag_names(db, viewer_id),
        list_active_muted_actor_uris_for_account(db, viewer_id),
        account_has_thread_mutes(db, viewer_id),
    )?;
    let followed_tag_candidates =
        futures_util::future::try_join_all(followed_tags.iter().map(|tag| async {
            let (local_statuses, remote_statuses) = futures_util::try_join!(
                list_local_public_statuses_by_tag(db, tag, cursor, query_limit),
                list_remote_public_statuses_by_tag(db, tag, cursor, query_limit),
            )?;
            Ok::<_, worker::Error>((local_statuses, remote_statuses))
        }))
        .await?;

    let mut seen_status_ids = HashSet::new();
    let mut candidate_rows = Vec::new();
    for status in local_home_statuses {
        push_unique_streaming_home_candidate(
            &mut seen_status_ids,
            &mut candidate_rows,
            StreamingHomeCandidate::Local(status),
        );
    }
    for (status, actor) in remote_home_statuses {
        push_unique_streaming_home_candidate(
            &mut seen_status_ids,
            &mut candidate_rows,
            StreamingHomeCandidate::Remote(status, actor),
        );
    }
    for (local_statuses, remote_statuses) in followed_tag_candidates {
        for status in local_statuses {
            push_unique_streaming_home_candidate(
                &mut seen_status_ids,
                &mut candidate_rows,
                StreamingHomeCandidate::Local(status),
            );
        }
        for (status, actor) in remote_statuses {
            push_unique_streaming_home_candidate(
                &mut seen_status_ids,
                &mut candidate_rows,
                StreamingHomeCandidate::Remote(status, actor),
            );
        }
    }

    Ok(StreamingHomeCandidateLoad {
        candidate_rows,
        muted_actor_uris,
        viewer_has_thread_mutes,
    })
}

fn collect_streaming_home_render_plan(
    config: &AppConfig,
    candidates: &[PreparedStreamingHomeCandidate<'_>],
) -> StreamingHomeRenderPlan {
    let mut local_status_ids = Vec::new();
    let mut remote_status_ids = Vec::new();
    let mut local_statuses_for_replies = Vec::new();
    let mut quote_status_uris = Vec::with_capacity(candidates.len());
    let mut mention_texts = Vec::with_capacity(candidates.len());
    let mut boost_of_uris = Vec::new();
    let mut seen_boost_uris = HashSet::new();

    for candidate in candidates {
        match candidate {
            PreparedStreamingHomeCandidate::Local {
                status, account, ..
            } => {
                local_status_ids.push(status.id.clone());
                local_statuses_for_replies.push(status.clone());
                quote_status_uris.push(status.ap_id.clone().unwrap_or_else(|| {
                    format!(
                        "{}/statuses/{}",
                        actor_url(config, account.username()),
                        status.id
                    )
                }));
                mention_texts.push(status.text.clone());
                if let Some(uri) = status.boost_of_uri.as_ref()
                    && seen_boost_uris.insert(uri.as_str())
                {
                    boost_of_uris.push(uri.clone());
                }
            }
            PreparedStreamingHomeCandidate::Remote { status, .. } => {
                remote_status_ids.push(status.id.clone());
                quote_status_uris.push(status.object_uri.clone());
                mention_texts.push(status.plain_text());
                if let Some(uri) = status.boost_of_uri.as_ref()
                    && seen_boost_uris.insert(uri.as_str())
                {
                    boost_of_uris.push(uri.clone());
                }
            }
        }
    }

    StreamingHomeRenderPlan {
        local_status_ids,
        remote_status_ids,
        local_statuses_for_replies,
        quote_status_uris,
        mention_texts,
        boost_of_uris,
    }
}

fn streaming_home_status_refs<'c>(
    candidates: &'c [PreparedStreamingHomeCandidate<'_>],
) -> (
    Vec<&'c crate::StatusRow>,
    Vec<(&'c crate::RemoteStatusRow, &'c crate::RemoteActorRow)>,
) {
    let local_status_refs = candidates
        .iter()
        .filter_map(|candidate| match candidate {
            PreparedStreamingHomeCandidate::Local { status, .. } => Some(status),
            PreparedStreamingHomeCandidate::Remote { .. } => None,
        })
        .collect::<Vec<_>>();
    let remote_status_refs = candidates
        .iter()
        .filter_map(|candidate| match candidate {
            PreparedStreamingHomeCandidate::Local { .. } => None,
            PreparedStreamingHomeCandidate::Remote { status, actor, .. } => Some((status, actor)),
        })
        .collect::<Vec<_>>();
    (local_status_refs, remote_status_refs)
}

async fn preload_streaming_home_context(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    viewer_has_thread_mutes: bool,
    render_plan: &StreamingHomeRenderPlan,
    candidates: &mut [PreparedStreamingHomeCandidate<'_>],
) -> Result<StreamingHomePreloads> {
    let (local_status_refs, remote_status_refs) = streaming_home_status_refs(candidates);
    let mention_text_refs = render_plan
        .mention_texts
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (
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
        mut remote_attachments_by_status_id,
        mention_preload,
        emoji_resolved_config,
        boost_target_preload,
    ) = futures_util::try_join!(
        preload_status_counts(
            db,
            &render_plan.local_status_ids,
            &render_plan.remote_status_ids,
        ),
        preload_status_quote_counts(db, &render_plan.quote_status_uris),
        preload_mastodon_poll_responses(db, &render_plan.local_status_ids, Some(viewer)),
        preload_local_status_viewer_state(
            db,
            viewer.id(),
            &local_status_refs,
            Some(viewer_has_thread_mutes),
        ),
        preload_remote_status_viewer_state(db, viewer.id(), &remote_status_refs),
        preload_remote_mastodon_poll_responses(db, &render_plan.remote_status_ids, Some(viewer)),
        preload_remote_status_edit_updated_at(db, &render_plan.remote_status_ids),
        preload_remote_status_federated_emojis(db, &render_plan.remote_status_ids),
        load_in_reply_to_account_ids(db, &render_plan.local_statuses_for_replies),
        preload_status_applications(db, config, &local_status_refs),
        find_remote_status_attachments_by_status_ids(db, &render_plan.remote_status_ids),
        preload_mention_accounts_from_texts(db, config, &mention_text_refs),
        config_with_resolved_custom_emojis(db, config),
        preload_boost_targets(db, config, &render_plan.boost_of_uris),
    )?;

    for candidate in candidates {
        if let PreparedStreamingHomeCandidate::Remote {
            status,
            attachments,
            ..
        } = candidate
        {
            *attachments = remote_attachments_by_status_id
                .remove(&status.id)
                .unwrap_or_default();
        }
    }

    Ok(StreamingHomePreloads {
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
        mention_preload,
        emoji_resolved_config,
        boost_target_preload,
    })
}

async fn render_streaming_home_entries(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    preloads: &StreamingHomePreloads,
    candidates: Vec<PreparedStreamingHomeCandidate<'_>>,
) -> Result<(Vec<StreamingEntry>, Vec<String>)> {
    let rendered =
        futures_util::future::try_join_all(candidates.into_iter().map(|candidate| async {
            let (created_at, id, data) = match candidate {
                PreparedStreamingHomeCandidate::Local {
                    status,
                    media,
                    account,
                } => {
                    let response = build_local_status_response_with_timeline_preloads(
                        db,
                        config,
                        Some(&preloads.emoji_resolved_config),
                        Some(viewer),
                        &status,
                        account,
                        preloads.in_reply_to_account_ids.get(&status.id).cloned(),
                        media,
                        None,
                        Some(&preloads.counts_preload),
                        Some(&preloads.quote_counts_preload),
                        Some(&preloads.local_poll_preload),
                        Some(&preloads.local_viewer_state_preload),
                        Some(&preloads.application_preload),
                        Some(&preloads.mention_preload),
                        Some(&preloads.boost_target_preload),
                    )
                    .await?;
                    (
                        status.created_at,
                        status.id,
                        serde_json::to_string(&response).map_err(|error| {
                            worker::Error::RustError(format!(
                                "failed to serialize home stream payload: {error}"
                            ))
                        })?,
                    )
                }
                PreparedStreamingHomeCandidate::Remote {
                    status,
                    actor,
                    attachments,
                } => {
                    let response = build_remote_status_response_with_timeline_preloads(
                        db,
                        config,
                        Some(viewer),
                        &status,
                        &actor,
                        None,
                        Some(&preloads.counts_preload),
                        Some(&preloads.quote_counts_preload),
                        Some(&preloads.remote_viewer_state_preload),
                        Some(&preloads.remote_poll_preload),
                        Some(&preloads.remote_edit_updated_at_preload),
                        Some(&preloads.remote_federated_emojis_preload),
                        attachments,
                        Some(&preloads.mention_preload),
                        Some(&preloads.boost_target_preload),
                        None,
                        None,
                        None,
                    )
                    .await?;
                    (
                        status.published_at,
                        status.id,
                        serde_json::to_string(&response).map_err(|error| {
                            worker::Error::RustError(format!(
                                "failed to serialize home stream payload: {error}"
                            ))
                        })?,
                    )
                }
            };
            Ok::<(StreamingEntry, String), worker::Error>((
                StreamingEntry::new(created_at, id.clone(), data),
                id,
            ))
        }))
        .await?;
    Ok(rendered.into_iter().unzip())
}

pub(crate) async fn streaming_home_batch(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);

    let load = load_streaming_home_candidate_rows(db, viewer.id(), &cursor, query_limit).await?;
    let viewer_has_thread_mutes = load.viewer_has_thread_mutes;
    let muted_actor_uris = load.muted_actor_uris;
    let candidate_rows = load.candidate_rows;

    let source_local_status_refs = candidate_rows
        .iter()
        .filter_map(|candidate| match candidate {
            StreamingHomeCandidate::Local(status) => Some(status),
            StreamingHomeCandidate::Remote(..) => None,
        })
        .collect::<Vec<_>>();
    let source_local_account_ids = source_local_status_refs
        .iter()
        .map(|status| status.account_id.clone())
        .collect::<Vec<_>>();
    let source_local_status_ids = source_local_status_refs
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    let ((local_accounts_by_id, mut media_by_status_id), muted_local_status_ids) = futures_util::try_join!(
        async {
            futures_util::try_join!(
                find_accounts_by_ids(db, &source_local_account_ids),
                find_media_attachments_by_status_ids(db, &source_local_status_ids),
            )
        },
        async {
            if viewer_has_thread_mutes {
                local_status_ids_thread_muted_by(db, viewer.id(), &source_local_status_refs).await
            } else {
                Ok::<HashSet<String>, worker::Error>(HashSet::new())
            }
        },
    )?;

    let mut candidates = Vec::with_capacity(candidate_rows.len());
    for candidate in candidate_rows {
        match candidate {
            StreamingHomeCandidate::Local(status) => {
                let Some(account) = local_accounts_by_id.get(&status.account_id) else {
                    continue;
                };
                let actor_uri = actor_url(config, account.username());
                if muted_actor_uris.contains(&actor_uri)
                    || muted_local_status_ids.contains(&status.id)
                {
                    continue;
                }
                candidates.push(PreparedStreamingHomeCandidate::Local {
                    media: media_by_status_id.remove(&status.id).unwrap_or_default(),
                    status,
                    account,
                });
            }
            StreamingHomeCandidate::Remote(status, actor) => {
                if muted_actor_uris.contains(&actor.actor_uri) {
                    continue;
                }
                candidates.push(PreparedStreamingHomeCandidate::Remote {
                    status,
                    actor,
                    attachments: Vec::new(),
                });
            }
        }
    }

    let render_plan = collect_streaming_home_render_plan(config, &candidates);
    let preloads = preload_streaming_home_context(
        db,
        config,
        viewer,
        viewer_has_thread_mutes,
        &render_plan,
        &mut candidates,
    )
    .await?;
    let (entries, tracked_status_ids) =
        render_streaming_home_entries(db, config, viewer, &preloads, candidates).await?;

    Ok(streaming_batch_from_entries(
        entries,
        tracked_status_ids,
        "update",
    ))
}
