use super::id_utils::generate_entity_id;
use super::local_polls::{MastodonPollOptionResponse, MastodonPollResponse};
use super::queue_remote_actor_activity_required;
use super::time_html::is_iso_timestamp_in_past;
use super::{
    RemoteActorRow, RemotePollDraft, RemoteStatusPollOptionRow, RemoteStatusPollRow,
    RemoteStatusRow, build_poll_vote_activity, extract_remote_note_object,
    extract_remote_poll_draft, fetch_remote_activitypub_document, fetch_remote_actor_profile,
    find_remote_status_poll_by_status_id, find_remote_status_raw_object_by_id,
    has_remote_poll_votes_created_after, is_local_account_following_remote_actor,
    list_remote_poll_votes_for_account, list_remote_status_poll_options, note_targets_account,
    note_targets_followers, remap_remote_poll_vote_positions, upsert_remote_actor,
    upsert_remote_status, validate_poll_vote_submission,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use std::collections::{HashMap, HashSet};
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

pub(crate) async fn load_remote_mastodon_poll_response(
    db: &D1Database,
    status: &RemoteStatusRow,
    viewer: Option<&LocalAccount>,
) -> Result<Option<serde_json::Value>> {
    let Some(poll) = find_remote_status_poll_by_status_id(db, &status.id).await? else {
        return Ok(None);
    };

    Ok(build_remote_mastodon_poll_response(db, &poll, viewer)
        .await?
        .map(|value| serde_json::to_value(value).unwrap_or(serde_json::Value::Null)))
}

pub(crate) async fn build_remote_mastodon_poll_response(
    db: &D1Database,
    poll: &RemoteStatusPollRow,
    viewer: Option<&LocalAccount>,
) -> Result<Option<MastodonPollResponse>> {
    let options = list_remote_status_poll_options(db, &poll.id).await?;
    if options.is_empty() {
        return Ok(None);
    }
    let own_votes = match viewer {
        Some(viewer) => remap_remote_poll_vote_positions(
            &options,
            &list_remote_poll_votes_for_account(db, &poll.id, &viewer.id).await?,
        ),
        None => Vec::new(),
    };

    Ok(Some(MastodonPollResponse {
        id: poll.id.clone(),
        expires_at: poll.expires_at.clone().unwrap_or_default(),
        expired: poll.expired != 0
            || poll
                .expires_at
                .as_deref()
                .map(|value| is_iso_timestamp_in_past(value).unwrap_or(false))
                .unwrap_or(false),
        multiple: poll.multiple != 0,
        votes_count: poll.votes_count.max(0) as u64,
        voters_count: if poll.multiple != 0 {
            poll.voters_count.map(|value| value.max(0) as u64)
        } else {
            None
        },
        voted: !own_votes.is_empty(),
        own_votes,
        options: options
            .into_iter()
            .map(|option| MastodonPollOptionResponse {
                title: option.title,
                votes_count: Some(option.votes_count.max(0) as u64),
            })
            .collect(),
        emojis: Vec::new(),
    }))
}

pub(crate) fn remote_poll_should_refresh(
    poll: &RemoteStatusPollRow,
    viewer: Option<&LocalAccount>,
) -> bool {
    if viewer.is_none() || poll.expired != 0 {
        return false;
    }

    poll.expires_at
        .as_deref()
        .map(|value| !is_iso_timestamp_in_past(value).unwrap_or(false))
        .unwrap_or(true)
}

#[cfg(test)]
pub(crate) fn remote_status_targets_local_viewer(
    raw_status: &serde_json::Value,
    viewer: &LocalAccount,
    config: &AppConfig,
) -> bool {
    extract_remote_note_object(raw_status)
        .map(|object| {
            note_targets_account(object, viewer, config)
                || note_targets_followers(object, viewer, config)
        })
        .unwrap_or(false)
}

pub(crate) fn remote_status_targets_local_viewer_account(
    raw_status: &serde_json::Value,
    viewer: &LocalAccount,
    config: &AppConfig,
) -> bool {
    extract_remote_note_object(raw_status)
        .map(|object| note_targets_account(object, viewer, config))
        .unwrap_or(false)
}

pub(crate) fn remote_status_targets_local_viewer_followers(
    raw_status: &serde_json::Value,
    viewer: &LocalAccount,
    config: &AppConfig,
) -> bool {
    extract_remote_note_object(raw_status)
        .map(|object| note_targets_followers(object, viewer, config))
        .unwrap_or(false)
}

pub(crate) async fn remote_poll_is_visible_to_viewer(
    db: &D1Database,
    config: &AppConfig,
    poll: &RemoteStatusPollRow,
    status: &RemoteStatusRow,
    viewer: Option<&LocalAccount>,
) -> Result<bool> {
    if matches!(status.visibility.as_str(), "public" | "unlisted") {
        return Ok(true);
    }

    let Some(viewer) = viewer else {
        return Ok(false);
    };
    let has_own_vote = !list_remote_poll_votes_for_account(db, &poll.id, &viewer.id)
        .await?
        .is_empty();

    if let Some(raw_status) = find_remote_status_raw_object_by_id(db, &status.id).await? {
        if remote_status_targets_local_viewer_account(&raw_status, viewer, config) {
            return Ok(true);
        }
        if remote_status_targets_local_viewer_followers(&raw_status, viewer, config)
            && is_local_account_following_remote_actor(db, &viewer.id, &status.actor_uri).await?
        {
            return Ok(true);
        }
        return Ok(has_own_vote);
    }

    if is_local_account_following_remote_actor(db, &viewer.id, &status.actor_uri).await? {
        return Ok(true);
    }

    Ok(has_own_vote)
}

pub(crate) async fn refresh_remote_poll_if_needed(
    db: &D1Database,
    config: &AppConfig,
    status: &RemoteStatusRow,
    poll: &RemoteStatusPollRow,
    viewer: Option<&LocalAccount>,
) -> Result<()> {
    if !remote_poll_should_refresh(poll, viewer) {
        return Ok(());
    }

    let document = match fetch_remote_activitypub_document(&status.object_uri).await {
        Ok(document) => document,
        Err(_) => return Ok(()),
    };
    let Some(object) = extract_remote_note_object(&document) else {
        return Ok(());
    };
    let Some(fetched_poll) = extract_remote_poll_draft(object) else {
        return Ok(());
    };
    if has_remote_poll_votes_created_after(db, &poll.id, &poll.updated_at).await? {
        let options = list_remote_status_poll_options(db, &poll.id).await?;
        if !remote_poll_draft_acknowledges_local_snapshot(poll, &options, &fetched_poll) {
            return Ok(());
        }
    }
    let actor_uri = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| document.get("actor").and_then(serde_json::Value::as_str))
        .unwrap_or(&status.actor_uri);
    let actor = match fetch_remote_actor_profile(actor_uri).await {
        Ok(actor) => actor,
        Err(_) => return Ok(()),
    };
    upsert_remote_actor(db, &actor).await?;
    upsert_remote_status(db, config, &actor, object).await
}

pub(crate) fn remote_poll_draft_acknowledges_vote(
    poll: &RemoteStatusPollRow,
    options: &[RemoteStatusPollOptionRow],
    fetched_poll: &RemotePollDraft,
    had_existing_votes: bool,
    new_choices: &[u32],
) -> bool {
    if fetched_poll.multiple != (poll.multiple != 0) {
        return false;
    }

    let expected_votes_count = poll
        .votes_count
        .saturating_add(i64::try_from(new_choices.len()).unwrap_or(i64::MAX))
        .max(0) as u64;
    if fetched_poll.votes_count < expected_votes_count {
        return false;
    }

    if poll.multiple != 0
        && !remote_poll_has_expected_voters_count(
            fetched_poll,
            expected_voters_count_after_vote(poll, had_existing_votes, new_choices),
        )
    {
        return false;
    }

    let remote_votes_by_title = remote_poll_votes_by_title(fetched_poll);
    for choice in new_choices {
        let Some(local_option) = options.get(*choice as usize) else {
            return false;
        };
        let expected_option_votes = local_option.votes_count.saturating_add(1).max(0) as u64;
        let Some(remote_option_votes) = remote_votes_by_title.get(local_option.title.as_str())
        else {
            return false;
        };
        if *remote_option_votes < expected_option_votes {
            return false;
        }
    }

    true
}

pub(crate) fn remote_poll_draft_acknowledges_local_snapshot(
    poll: &RemoteStatusPollRow,
    options: &[RemoteStatusPollOptionRow],
    fetched_poll: &RemotePollDraft,
) -> bool {
    if fetched_poll.multiple != (poll.multiple != 0) {
        return false;
    }
    if fetched_poll.votes_count < poll.votes_count.max(0) as u64 {
        return false;
    }

    if poll.multiple != 0
        && !remote_poll_has_expected_voters_count(
            fetched_poll,
            poll.voters_count.map(|value| value.max(0) as u64),
        )
    {
        return false;
    }

    let remote_votes_by_title = remote_poll_votes_by_title(fetched_poll);
    for local_option in options {
        let expected_option_votes = local_option.votes_count.max(0) as u64;
        let Some(remote_option_votes) = remote_votes_by_title.get(local_option.title.as_str())
        else {
            return false;
        };
        if *remote_option_votes < expected_option_votes {
            return false;
        }
    }

    true
}

fn remote_poll_votes_by_title(fetched_poll: &RemotePollDraft) -> HashMap<&str, u64> {
    fetched_poll
        .options
        .iter()
        .map(|option| (option.title.as_str(), option.votes_count))
        .collect()
}

fn expected_voters_count_after_vote(
    poll: &RemoteStatusPollRow,
    had_existing_votes: bool,
    new_choices: &[u32],
) -> Option<u64> {
    if had_existing_votes || new_choices.is_empty() {
        poll.voters_count.map(|value| value.max(0) as u64)
    } else {
        Some(poll.voters_count.unwrap_or(0).saturating_add(1).max(0) as u64)
    }
}

fn remote_poll_has_expected_voters_count(
    fetched_poll: &RemotePollDraft,
    expected_voters_count: Option<u64>,
) -> bool {
    expected_voters_count
        .map(|expected| fetched_poll.voters_count.unwrap_or(0) >= expected)
        .unwrap_or(true)
}

async fn refresh_remote_poll_after_vote_if_acknowledged(
    db: &D1Database,
    config: &AppConfig,
    actor: &RemoteActorRow,
    status: &RemoteStatusRow,
    poll: &RemoteStatusPollRow,
    options: &[RemoteStatusPollOptionRow],
    had_existing_votes: bool,
    new_choices: &[u32],
) -> Result<bool> {
    let document = match fetch_remote_activitypub_document(&status.object_uri).await {
        Ok(document) => document,
        Err(_) => return Ok(false),
    };
    let Some(object) = extract_remote_note_object(&document) else {
        return Ok(false);
    };
    let Some(fetched_poll) = extract_remote_poll_draft(object) else {
        return Ok(false);
    };
    if !remote_poll_draft_acknowledges_vote(
        poll,
        options,
        &fetched_poll,
        had_existing_votes,
        new_choices,
    ) {
        return Ok(false);
    }

    let actor_profile = match fetch_remote_actor_profile(&actor.actor_uri).await {
        Ok(actor_profile) => actor_profile,
        Err(_) => return Ok(false),
    };
    upsert_remote_actor(db, &actor_profile).await?;
    upsert_remote_status(db, config, &actor_profile, object).await?;
    Ok(true)
}

fn collect_new_remote_poll_choices(
    option_count: usize,
    existing: &[u32],
    choices: &[u32],
) -> std::result::Result<Vec<u32>, String> {
    let existing = existing.iter().copied().collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut new_choices = Vec::new();

    for choice in choices {
        let position = *choice as usize;
        if position >= option_count {
            return Err("choices contains an out-of-range option".to_owned());
        }
        if existing.contains(choice) || !seen.insert(*choice) {
            continue;
        }
        new_choices.push(*choice);
    }

    Ok(new_choices)
}

pub(crate) async fn apply_remote_poll_vote(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    actor: &RemoteActorRow,
    status: &RemoteStatusRow,
    poll: &RemoteStatusPollRow,
    choices: &[u32],
) -> Result<Vec<u32>> {
    let options = list_remote_status_poll_options(db, &poll.id).await?;
    if options.is_empty() {
        return Err(Error::RustError("poll not found".to_owned()));
    }
    let existing_votes = list_remote_poll_votes_for_account(db, &poll.id, &viewer.id).await?;
    let existing = remap_remote_poll_vote_positions(&options, &existing_votes);
    let had_existing_votes = !existing.is_empty();
    validate_poll_vote_submission(existing.len(), poll.multiple != 0, choices.len())
        .map_err(Error::RustError)?;

    let new_choices = collect_new_remote_poll_choices(options.len(), &existing, choices)
        .map_err(Error::RustError)?;
    if new_choices.is_empty() {
        return Err(Error::RustError(
            "you have already voted in this poll".to_owned(),
        ));
    }

    for choice in &new_choices {
        let option = &options[*choice as usize];
        let (activity_id, payload_json) = build_poll_vote_activity(
            config,
            viewer,
            &actor.actor_uri,
            &status.object_uri,
            &option.title,
        )?;
        queue_remote_actor_activity_required(db, &viewer.id, &actor.actor_uri, &payload_json)
            .await?;

        let vote_id = generate_entity_id(16)?;
        let bindings = [
            D1Type::Text(vote_id.as_str()),
            D1Type::Text(poll.id.as_str()),
            D1Type::Text(viewer.id.as_str()),
            D1Type::Integer(*choice as i32),
            D1Type::Text(option.title.as_str()),
            D1Type::Text(activity_id.as_str()),
        ];
        db.prepare(
            "INSERT OR IGNORE INTO remote_status_poll_votes (
                id,
                poll_id,
                account_id,
                option_position,
                option_title,
                activity_id,
                created_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    apply_optimistic_remote_poll_vote_tally(
        db,
        &poll.id,
        poll.multiple != 0,
        had_existing_votes,
        &new_choices,
    )
    .await?;
    let _ = refresh_remote_poll_after_vote_if_acknowledged(
        db,
        config,
        actor,
        status,
        poll,
        &options,
        had_existing_votes,
        &new_choices,
    )
    .await;

    let mut own_votes = existing;
    own_votes.extend(new_choices);
    own_votes.sort_unstable();
    own_votes.dedup();
    Ok(own_votes)
}

async fn apply_optimistic_remote_poll_vote_tally(
    db: &D1Database,
    poll_id: &str,
    multiple: bool,
    had_existing_votes: bool,
    new_choices: &[u32],
) -> Result<()> {
    if new_choices.is_empty() {
        return Ok(());
    }

    let (votes_count_delta, voters_count_delta) =
        optimistic_remote_poll_vote_deltas(multiple, had_existing_votes, new_choices.len());
    let poll_bindings = [
        D1Type::Integer(votes_count_delta),
        voters_count_delta.map_or(D1Type::Null, D1Type::Integer),
        D1Type::Text(poll_id),
    ];
    db.prepare(
        "UPDATE remote_status_polls
         SET votes_count = votes_count + ?1,
             voters_count = CASE
                 WHEN ?2 IS NULL THEN voters_count
                 WHEN voters_count IS NULL THEN ?2
                 ELSE voters_count + ?2
             END,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(poll_bindings.iter())?
    .run()
    .await?;

    for choice in new_choices {
        let option_bindings = [D1Type::Text(poll_id), D1Type::Integer(*choice as i32)];
        db.prepare(
            "UPDATE remote_status_poll_options
             SET votes_count = votes_count + 1
             WHERE poll_id = ?1
               AND position = ?2",
        )
        .bind_refs(option_bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

pub(crate) fn optimistic_remote_poll_vote_deltas(
    multiple: bool,
    had_existing_votes: bool,
    added_vote_count: usize,
) -> (i32, Option<i32>) {
    let votes_count_delta = i32::try_from(added_vote_count).unwrap_or(i32::MAX);
    let voters_count_delta = if multiple && !had_existing_votes && added_vote_count > 0 {
        Some(1)
    } else {
        None
    };
    (votes_count_delta, voters_count_delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_new_remote_poll_choices_preserves_order_and_skips_duplicates() {
        assert_eq!(
            collect_new_remote_poll_choices(5, &[1], &[2, 1, 2, 3]).unwrap(),
            vec![2, 3]
        );
    }

    #[test]
    fn collect_new_remote_poll_choices_rejects_out_of_range_choice() {
        assert_eq!(
            collect_new_remote_poll_choices(2, &[], &[0, 2]).unwrap_err(),
            "choices contains an out-of-range option"
        );
    }
}
