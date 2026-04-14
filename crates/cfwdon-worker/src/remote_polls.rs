use super::id_utils::generate_entity_id;
use super::local_polls::{MastodonPollOptionResponse, MastodonPollResponse};
use super::queue_remote_actor_activity_required;
use super::time_html::is_iso_timestamp_in_past;
use super::{
    RemoteActorRow, RemoteStatusPollRow, RemoteStatusRow, build_poll_vote_activity,
    find_remote_status_poll_by_status_id, list_remote_poll_votes_for_account,
    list_remote_status_poll_options, remap_remote_poll_vote_positions,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
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
    if poll.multiple == 0 && !existing.is_empty() {
        return Err(Error::RustError(
            "you have already voted in this poll".to_owned(),
        ));
    }
    if poll.multiple == 0 && choices.len() > 1 {
        return Err(Error::RustError(
            "single-choice polls accept exactly one choice".to_owned(),
        ));
    }

    let mut new_choices = Vec::new();
    for choice in choices {
        let position = *choice as usize;
        if position >= options.len() {
            return Err(Error::RustError(
                "choices contains an out-of-range option".to_owned(),
            ));
        }
        if existing.iter().any(|value| value == choice)
            || new_choices.iter().any(|value| value == choice)
        {
            continue;
        }
        new_choices.push(*choice);
    }
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

    let mut own_votes = existing;
    own_votes.extend(new_choices);
    own_votes.sort_unstable();
    own_votes.dedup();
    Ok(own_votes)
}
