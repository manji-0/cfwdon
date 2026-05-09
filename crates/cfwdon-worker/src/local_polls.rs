use super::CreateStatusPollRequest;
use super::time_html::is_iso_timestamp_in_past;
use super::{
    StatusPollOptionRow, StatusPollRow, count_poll_voters, find_status_poll_by_status_id,
    list_poll_vote_positions_for_account, list_status_poll_options,
};
use cfwdon_domain::{LocalAccount, PollDraft};
use serde::Serialize;
use worker::{D1Database, Result};

#[derive(Debug, Serialize)]
pub(crate) struct MastodonPollResponse {
    pub(crate) id: String,
    pub(crate) expires_at: String,
    pub(crate) expired: bool,
    pub(crate) multiple: bool,
    pub(crate) votes_count: u64,
    pub(crate) voters_count: Option<u64>,
    pub(crate) voted: bool,
    pub(crate) own_votes: Vec<u32>,
    pub(crate) options: Vec<MastodonPollOptionResponse>,
    pub(crate) emojis: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonPollOptionResponse {
    pub(crate) title: String,
    pub(crate) votes_count: Option<u64>,
}

pub(crate) fn normalize_status_poll(
    poll: Option<CreateStatusPollRequest>,
) -> std::result::Result<Option<PollDraft>, String> {
    let Some(poll) = poll else {
        return Ok(None);
    };
    let options = poll
        .options
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if options.is_empty()
        && poll.expires_in.is_none()
        && poll.multiple.is_none()
        && poll.hide_totals.is_none()
    {
        return Ok(None);
    }
    if options.len() < 2 || options.len() > 4 {
        return Err("poll must include between 2 and 4 non-empty options".to_owned());
    }
    let expires_in_seconds = poll
        .expires_in
        .filter(|value| *value >= 300)
        .ok_or_else(|| "poll[expires_in] must be at least 300 seconds".to_owned())?;

    Ok(Some(PollDraft {
        options,
        expires_in_seconds,
        multiple: poll.multiple.unwrap_or(false),
        hide_totals: poll.hide_totals.unwrap_or(false),
    }))
}

pub(crate) async fn load_mastodon_poll_response(
    db: &D1Database,
    status_id: &str,
    viewer: Option<&LocalAccount>,
) -> Result<Option<serde_json::Value>> {
    let Some(poll) = find_status_poll_by_status_id(db, status_id).await? else {
        return Ok(None);
    };
    build_mastodon_poll_response(db, &poll, viewer)
        .await
        .map(|value| {
            value.map(|poll| serde_json::to_value(poll).unwrap_or(serde_json::Value::Null))
        })
}

pub(crate) async fn build_mastodon_poll_response(
    db: &D1Database,
    poll: &StatusPollRow,
    viewer: Option<&LocalAccount>,
) -> Result<Option<MastodonPollResponse>> {
    let options = list_status_poll_options(db, &poll.id).await?;
    if options.is_empty() {
        return Ok(None);
    }
    let votes_count = options
        .iter()
        .map(|option| option.votes_count.max(0) as u64)
        .sum();
    let expired = is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false);
    let reveal_totals = expired || poll.hide_totals == 0;
    let own_votes = match viewer {
        Some(viewer) => list_poll_vote_positions_for_account(db, &poll.id, &viewer.id).await?,
        None => Vec::new(),
    };
    let voters_count = if poll.multiple != 0 {
        if reveal_totals {
            Some(count_poll_voters(db, &poll.id).await?)
        } else {
            None
        }
    } else if reveal_totals {
        Some(votes_count)
    } else {
        None
    };

    Ok(Some(MastodonPollResponse {
        id: poll.id.clone(),
        expires_at: poll.expires_at.clone(),
        expired,
        multiple: poll.multiple != 0,
        votes_count: if reveal_totals { votes_count } else { 0 },
        voters_count,
        voted: !own_votes.is_empty(),
        own_votes,
        options: options
            .into_iter()
            .map(|option| MastodonPollOptionResponse {
                title: option.title,
                votes_count: reveal_totals.then_some(option.votes_count.max(0) as u64),
            })
            .collect(),
        emojis: Vec::new(),
    }))
}

pub(crate) fn apply_activitypub_poll_fields(
    object: &mut serde_json::Value,
    poll: &StatusPollRow,
    options: &[StatusPollOptionRow],
    voters_count: u64,
    expired: bool,
) {
    if options.is_empty() {
        return;
    }

    object["type"] = serde_json::json!("Question");
    object["endTime"] = serde_json::json!(poll.expires_at.clone());
    object["votersCount"] = serde_json::json!(voters_count);
    if expired {
        object["closed"] = serde_json::json!(poll.expires_at.clone());
    }

    let rendered_options = options
        .iter()
        .map(|option| {
            serde_json::json!({
                "type": "Note",
                "name": option.title,
                "replies": {
                    "type": "Collection",
                    "totalItems": option.votes_count.max(0) as u64,
                }
            })
        })
        .collect::<Vec<_>>();

    if poll.multiple != 0 {
        object["anyOf"] = serde_json::Value::Array(rendered_options);
    } else {
        object["oneOf"] = serde_json::Value::Array(rendered_options);
    }
}
