use super::CreateStatusPollRequest;
use super::time_html::is_iso_timestamp_in_past;
use super::{
    StatusPollOptionRow, StatusPollRow, count_poll_voters, find_status_poll_by_status_id,
    list_poll_vote_positions_for_account, list_status_poll_options,
};
use cfwdon_domain::{LocalAccount, PollDraft};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use worker::{D1Database, Result, d1::D1Type};

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

#[derive(Debug, Deserialize)]
struct PreloadedStatusPollOptionRow {
    poll_id: String,
    title: String,
    votes_count: i64,
}

#[derive(Debug, Deserialize)]
struct PreloadedPollVotePositionRow {
    poll_id: String,
    option_position: i64,
}

#[derive(Debug, Deserialize)]
struct PreloadedPollVotersCountRow {
    poll_id: String,
    count: u64,
}

#[derive(Debug, Default)]
pub(crate) struct MastodonPollResponsePreload {
    by_status_id: HashMap<String, serde_json::Value>,
}

impl MastodonPollResponsePreload {
    pub(crate) fn poll_response(&self, status_id: &str) -> Option<serde_json::Value> {
        self.by_status_id.get(status_id).cloned()
    }
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

pub(crate) async fn preload_mastodon_poll_responses(
    db: &D1Database,
    status_ids: &[String],
    viewer: Option<&LocalAccount>,
) -> Result<MastodonPollResponsePreload> {
    let mut seen = HashSet::new();
    let ids = status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(MastodonPollResponsePreload::default());
    }

    let status_placeholders = (1..=ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let poll_sql = format!(
        "SELECT id, status_id, multiple, hide_totals, expires_at
         FROM status_polls
         WHERE status_id IN ({status_placeholders})"
    );
    let status_bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let poll_result = db
        .prepare(&poll_sql)
        .bind_refs(status_bindings.iter())?
        .all()
        .await?;
    let polls = poll_result.results::<StatusPollRow>()?;
    if polls.is_empty() {
        return Ok(MastodonPollResponsePreload::default());
    }

    let poll_ids = polls.iter().map(|poll| poll.id.clone()).collect::<Vec<_>>();
    let poll_placeholders = (1..=poll_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let poll_bindings = poll_ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();

    let options_sql = format!(
        "SELECT poll_id, title, votes_count
         FROM status_poll_options
         WHERE poll_id IN ({poll_placeholders})
         ORDER BY poll_id ASC, position ASC"
    );
    let options_result = db
        .prepare(&options_sql)
        .bind_refs(poll_bindings.iter())?
        .all()
        .await?;
    let mut options_by_poll_id: HashMap<String, Vec<StatusPollOptionRow>> = HashMap::new();
    for row in options_result.results::<PreloadedStatusPollOptionRow>()? {
        options_by_poll_id
            .entry(row.poll_id)
            .or_default()
            .push(StatusPollOptionRow {
                title: row.title,
                votes_count: row.votes_count,
            });
    }

    let mut own_votes_by_poll_id: HashMap<String, Vec<u32>> = HashMap::new();
    if let Some(viewer) = viewer {
        let vote_placeholders = (2..=(poll_ids.len() + 1))
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let vote_sql = format!(
            "SELECT poll_id, option_position
             FROM status_poll_votes
             WHERE account_id = ?1
               AND poll_id IN ({vote_placeholders})
             ORDER BY poll_id ASC, option_position ASC"
        );
        let mut vote_bindings = Vec::with_capacity(poll_ids.len() + 1);
        vote_bindings.push(D1Type::Text(viewer.id.as_str()));
        vote_bindings.extend(poll_ids.iter().map(|id| D1Type::Text(id.as_str())));
        let vote_result = db
            .prepare(&vote_sql)
            .bind_refs(vote_bindings.iter())?
            .all()
            .await?;
        for row in vote_result.results::<PreloadedPollVotePositionRow>()? {
            if let Ok(position) = u32::try_from(row.option_position) {
                own_votes_by_poll_id
                    .entry(row.poll_id)
                    .or_default()
                    .push(position);
            }
        }
    }

    let voters_sql = format!(
        "SELECT poll_id, COUNT(DISTINCT account_id) AS count
         FROM status_poll_votes
         WHERE poll_id IN ({poll_placeholders})
         GROUP BY poll_id"
    );
    let voters_result = db
        .prepare(&voters_sql)
        .bind_refs(poll_bindings.iter())?
        .all()
        .await?;
    let voters_count_by_poll_id = voters_result
        .results::<PreloadedPollVotersCountRow>()?
        .into_iter()
        .map(|row| (row.poll_id, row.count))
        .collect::<HashMap<_, _>>();

    let mut by_status_id = HashMap::new();
    for poll in polls {
        let Some(options) = options_by_poll_id.remove(&poll.id) else {
            continue;
        };
        if options.is_empty() {
            continue;
        }
        let votes_count = options
            .iter()
            .map(|option| option.votes_count.max(0) as u64)
            .sum();
        let expired = is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false);
        let reveal_totals = expired || poll.hide_totals == 0;
        let own_votes = own_votes_by_poll_id.remove(&poll.id).unwrap_or_default();
        let voters_count = if poll.multiple != 0 {
            reveal_totals.then_some(*voters_count_by_poll_id.get(&poll.id).unwrap_or(&0))
        } else {
            reveal_totals.then_some(votes_count)
        };
        let response = MastodonPollResponse {
            id: poll.id,
            expires_at: poll.expires_at,
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
        };
        by_status_id.insert(
            poll.status_id,
            serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
        );
    }

    Ok(MastodonPollResponsePreload { by_status_id })
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
