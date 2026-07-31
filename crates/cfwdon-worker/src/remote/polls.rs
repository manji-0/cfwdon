use super::generate_entity_id;
use super::is_iso_timestamp_in_past;
use super::queue_remote_actor_activity_required;
use super::timestamp_to_mastodon_iso8601;
use super::{MastodonPollOptionResponse, MastodonPollResponse};
use super::{
    RemoteActorRow, RemotePollDraft, RemoteStatusPollOptionRow, RemoteStatusPollRow,
    RemoteStatusPollVoteRow, RemoteStatusRow, build_poll_vote_activity, extract_remote_note_object,
    extract_remote_poll_draft, fetch_remote_activitypub_document, fetch_remote_actor_profile,
    find_remote_status_poll_by_status_id, find_remote_status_raw_object_by_id,
    has_remote_poll_votes_created_after, is_local_account_following_remote_actor,
    json_string_array, list_remote_poll_votes_for_account, list_remote_status_poll_options,
    note_targets_account, note_targets_followers, remap_remote_poll_vote_positions,
    sql_in_json_each, upsert_remote_actor, upsert_remote_status, validate_poll_vote_submission,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, StoredRemotePollVoteIntent};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

#[derive(Debug, Deserialize)]
struct RemoteStatusPollOptionPreloadRow {
    poll_id: String,
    title: String,
    votes_count: i64,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusPollVotePreloadRow {
    poll_id: String,
    option_position: i64,
    option_title: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct RemoteMastodonPollResponsePreload {
    polls_by_status_id: HashMap<String, serde_json::Value>,
}

impl RemoteMastodonPollResponsePreload {
    pub(crate) fn poll_response(&self, status_id: &str) -> Option<serde_json::Value> {
        self.polls_by_status_id.get(status_id).cloned()
    }
}

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
        Some(viewer) => Some(remap_remote_poll_vote_positions(
            &options,
            &list_remote_poll_votes_for_account(db, &poll.id, viewer.id()).await?,
        )),
        None => None,
    };

    Ok(remote_mastodon_poll_response_from_parts(
        poll, options, own_votes,
    ))
}

pub(crate) async fn preload_remote_mastodon_poll_responses(
    db: &D1Database,
    status_ids: &[String],
    viewer: Option<&LocalAccount>,
) -> Result<RemoteMastodonPollResponsePreload> {
    let ids = unique_remote_poll_preload_status_ids(status_ids);
    if ids.is_empty() {
        return Ok(RemoteMastodonPollResponsePreload::default());
    }

    let polls = load_remote_status_polls_for_status_ids(db, &ids).await?;
    if polls.is_empty() {
        return Ok(RemoteMastodonPollResponsePreload::default());
    }

    let poll_ids = polls.iter().map(|poll| poll.id.clone()).collect::<Vec<_>>();
    let poll_bindings = remote_poll_id_bindings(&poll_ids);
    let mut options_by_poll_id =
        preload_remote_poll_options_by_poll_id(db, &poll_ids, &poll_bindings).await?;
    let votes_by_poll_id = preload_remote_poll_votes_by_poll_id(db, &poll_ids, viewer).await?;
    let mut polls_by_status_id = HashMap::new();

    for poll in polls {
        let options = options_by_poll_id.remove(&poll.id).unwrap_or_default();
        let own_votes = if viewer.is_some() {
            Some(
                votes_by_poll_id
                    .get(&poll.id)
                    .map(|votes| remap_remote_poll_vote_positions(&options, votes))
                    .unwrap_or_default(),
            )
        } else {
            None
        };
        if let Some(response) = remote_mastodon_poll_response_from_parts(&poll, options, own_votes)
        {
            polls_by_status_id.insert(
                poll.status_id.clone(),
                serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
            );
        }
    }

    Ok(RemoteMastodonPollResponsePreload { polls_by_status_id })
}

fn unique_remote_poll_preload_status_ids(status_ids: &[String]) -> Vec<&String> {
    let mut seen = HashSet::new();
    status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect()
}

async fn load_remote_status_polls_for_status_ids(
    db: &D1Database,
    ids: &[&String],
) -> Result<Vec<RemoteStatusPollRow>> {
    let ids_json = json_string_array(ids);
    let poll_sql = format!(
        "SELECT id, status_id, multiple, expires_at, voters_count, votes_count, expired, updated_at
         FROM remote_status_polls
         WHERE status_id {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(ids_json.as_str());
    let result = db.prepare(&poll_sql).bind_refs(&binding)?.all().await?;
    result.results::<RemoteStatusPollRow>()
}

fn remote_poll_id_bindings(poll_ids: &[String]) -> Vec<D1Type<'_>> {
    poll_ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect()
}

async fn preload_remote_poll_options_by_poll_id(
    db: &D1Database,
    poll_ids: &[String],
    _poll_bindings: &[D1Type<'_>],
) -> Result<HashMap<String, Vec<RemoteStatusPollOptionRow>>> {
    let poll_ids_json = json_string_array(poll_ids);
    let options_sql = format!(
        "SELECT poll_id, title, votes_count
         FROM remote_status_poll_options
         WHERE poll_id {}
         ORDER BY poll_id ASC, position ASC",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(poll_ids_json.as_str());
    let option_rows = db
        .prepare(&options_sql)
        .bind_refs(&binding)?
        .all()
        .await?
        .results::<RemoteStatusPollOptionPreloadRow>()?;
    let mut options_by_poll_id: HashMap<String, Vec<RemoteStatusPollOptionRow>> = HashMap::new();
    for row in option_rows {
        options_by_poll_id
            .entry(row.poll_id)
            .or_default()
            .push(RemoteStatusPollOptionRow {
                title: row.title,
                votes_count: row.votes_count,
            });
    }
    Ok(options_by_poll_id)
}

async fn preload_remote_poll_votes_by_poll_id(
    db: &D1Database,
    poll_ids: &[String],
    viewer: Option<&LocalAccount>,
) -> Result<HashMap<String, Vec<RemoteStatusPollVoteRow>>> {
    let Some(viewer) = viewer else {
        return Ok(HashMap::new());
    };
    let poll_ids_json = json_string_array(poll_ids);
    let vote_sql = format!(
        "SELECT poll_id, option_position, option_title
             FROM remote_status_poll_votes
             WHERE account_id = ?1
               AND poll_id {}
             ORDER BY poll_id ASC, option_position ASC",
        sql_in_json_each(2)
    );
    let vote_bindings = [
        D1Type::Text(viewer.id()),
        D1Type::Text(poll_ids_json.as_str()),
    ];
    let vote_rows = db
        .prepare(&vote_sql)
        .bind_refs(vote_bindings.iter())?
        .all()
        .await?
        .results::<RemoteStatusPollVotePreloadRow>()?;
    let mut rows_by_poll_id: HashMap<String, Vec<RemoteStatusPollVoteRow>> = HashMap::new();
    for row in vote_rows {
        rows_by_poll_id
            .entry(row.poll_id)
            .or_default()
            .push(RemoteStatusPollVoteRow {
                option_position: row.option_position,
                option_title: row.option_title,
            });
    }
    Ok(rows_by_poll_id)
}

fn remote_mastodon_poll_response_from_parts(
    poll: &RemoteStatusPollRow,
    options: Vec<RemoteStatusPollOptionRow>,
    own_votes: Option<Vec<u32>>,
) -> Option<MastodonPollResponse> {
    if options.is_empty() {
        return None;
    }
    let (voted, own_votes) = match own_votes {
        Some(own_votes) => (Some(!own_votes.is_empty()), Some(own_votes)),
        None => (None, None),
    };

    Some(MastodonPollResponse {
        id: poll.id.clone(),
        expires_at: poll
            .expires_at
            .as_deref()
            .map(timestamp_to_mastodon_iso8601)
            .unwrap_or_default(),
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
        voted,
        own_votes,
        options: options
            .into_iter()
            .map(|option| MastodonPollOptionResponse {
                title: option.title,
                votes_count: Some(option.votes_count.max(0) as u64),
            })
            .collect(),
        emojis: Vec::new(),
    })
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
    let has_own_vote = !list_remote_poll_votes_for_account(db, &poll.id, viewer.id())
        .await?
        .is_empty();

    if let Some(raw_status) = find_remote_status_raw_object_by_id(db, &status.id).await? {
        if remote_status_targets_local_viewer_account(&raw_status, viewer, config) {
            return Ok(true);
        }
        if status.visibility.as_str() != "direct"
            && remote_status_targets_local_viewer_followers(&raw_status, viewer, config)
            && is_local_account_following_remote_actor(db, viewer.id(), &status.actor_uri).await?
        {
            return Ok(true);
        }
        return Ok(has_own_vote);
    }

    if status.visibility.as_str() == "direct" {
        return Ok(has_own_vote);
    }

    if is_local_account_following_remote_actor(db, viewer.id(), &status.actor_uri).await? {
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
    // No env on purpose: a poll refresh only re-reads counters, and the caller
    // serves the refreshed status in its own response.
    upsert_remote_status(db, config, &actor, object, None).await
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
    // No env on purpose: see refresh_remote_poll_if_needed.
    upsert_remote_status(db, config, &actor_profile, object, None).await?;
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

struct RemotePollVotePlan {
    options: Vec<RemoteStatusPollOptionRow>,
    existing: Vec<u32>,
    had_existing_votes: bool,
    new_choices: Vec<u32>,
}

async fn remote_poll_vote_plan(
    db: &D1Database,
    viewer: &LocalAccount,
    poll: &RemoteStatusPollRow,
    choices: &[u32],
) -> Result<RemotePollVotePlan> {
    let options = list_remote_status_poll_options(db, &poll.id).await?;
    if options.is_empty() {
        return Err(Error::RustError("poll not found".to_owned()));
    }

    let existing_votes = list_remote_poll_votes_for_account(db, &poll.id, viewer.id()).await?;
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

    Ok(RemotePollVotePlan {
        options,
        existing,
        had_existing_votes,
        new_choices,
    })
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
    let plan = remote_poll_vote_plan(db, viewer, poll, choices).await?;
    queue_and_insert_remote_poll_votes(db, config, viewer, actor, status, poll, &plan).await?;

    apply_optimistic_remote_poll_vote_tally(
        db,
        &poll.id,
        poll.multiple != 0,
        plan.had_existing_votes,
        &plan.new_choices,
    )
    .await?;
    let _ = refresh_remote_poll_after_vote_if_acknowledged(
        db,
        config,
        actor,
        status,
        poll,
        &plan.options,
        plan.had_existing_votes,
        &plan.new_choices,
    )
    .await;

    Ok(merged_remote_poll_own_votes(
        plan.existing,
        plan.new_choices,
    ))
}

async fn queue_and_insert_remote_poll_votes(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    actor: &RemoteActorRow,
    status: &RemoteStatusRow,
    poll: &RemoteStatusPollRow,
    plan: &RemotePollVotePlan,
) -> Result<()> {
    for choice in &plan.new_choices {
        let option = &plan.options[*choice as usize];
        let (activity_id, payload_json) = build_poll_vote_activity(
            config,
            viewer,
            &actor.actor_uri,
            &status.object_uri,
            &option.title,
        )?;
        queue_remote_actor_activity_required(db, viewer.id(), &actor.actor_uri, &payload_json)
            .await?;

        let vote_id = generate_entity_id(16)?;
        let intent = StoredRemotePollVoteIntent::new(
            vote_id,
            &poll.id,
            viewer.id(),
            *choice,
            &option.title,
            activity_id,
        );
        insert_remote_poll_vote_row(db, &intent).await?;
    }

    Ok(())
}

fn merged_remote_poll_own_votes(existing: Vec<u32>, new_choices: Vec<u32>) -> Vec<u32> {
    let mut own_votes = existing;
    own_votes.extend(new_choices);
    own_votes.sort_unstable();
    own_votes.dedup();
    own_votes
}

async fn insert_remote_poll_vote_row(
    db: &D1Database,
    intent: &StoredRemotePollVoteIntent,
) -> Result<()> {
    let bindings = remote_poll_vote_insert_bindings(intent);
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
    Ok(())
}

fn remote_poll_vote_insert_bindings(intent: &StoredRemotePollVoteIntent) -> [D1Type<'_>; 6] {
    [
        D1Type::Text(intent.vote_id.as_str()),
        D1Type::Text(intent.poll_id.as_str()),
        D1Type::Text(intent.account_id.as_str()),
        D1Type::Integer(intent.option_position as i32),
        D1Type::Text(intent.option_title.as_str()),
        D1Type::Text(intent.activity_id.as_str()),
    ]
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

    #[test]
    fn merged_remote_poll_own_votes_sorts_and_deduplicates() {
        assert_eq!(
            merged_remote_poll_own_votes(vec![3, 1], vec![2, 1]),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn stored_remote_poll_vote_intent_maps_storage_fields() {
        let intent = StoredRemotePollVoteIntent::new(
            "vote-1",
            "poll-1",
            "acct-1",
            2,
            "Choice B",
            "activity-1",
        );

        assert_eq!(intent.vote_id, "vote-1");
        assert_eq!(intent.poll_id, "poll-1");
        assert_eq!(intent.account_id, "acct-1");
        assert_eq!(intent.option_position, 2);
        assert_eq!(intent.option_title, "Choice B");
        assert_eq!(intent.activity_id, "activity-1");
    }

    #[test]
    fn remote_poll_vote_insert_bindings_keep_sql_slot_order_stable() {
        let intent = StoredRemotePollVoteIntent::new(
            "vote-1",
            "poll-1",
            "acct-1",
            2,
            "Choice B",
            "activity-1",
        );
        let bindings = remote_poll_vote_insert_bindings(&intent);

        assert!(matches!(bindings[0], D1Type::Text("vote-1")));
        assert!(matches!(bindings[1], D1Type::Text("poll-1")));
        assert!(matches!(bindings[2], D1Type::Text("acct-1")));
        assert!(matches!(bindings[3], D1Type::Integer(2)));
        assert!(matches!(bindings[4], D1Type::Text("Choice B")));
        assert!(matches!(bindings[5], D1Type::Text("activity-1")));
    }
}
