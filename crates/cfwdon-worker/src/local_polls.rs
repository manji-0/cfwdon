mod store;
mod votes;

pub(crate) use store::*;
pub(crate) use votes::*;

use super::CreateStatusPollRequest;
use super::time_html::is_iso_timestamp_in_past;
use super::timestamp_to_mastodon_iso8601;
<<<<<<< HEAD
=======
use crate::{d1_in_value_chunk_size, sql_placeholders};
use cfwdon_core::AppConfig;
>>>>>>> d03d281 (fix(worker): chunk D1 IN queries under the 100-bind limit)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) voted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) own_votes: Option<Vec<u32>>,
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
    preloaded_status_ids: HashSet<String>,
}

impl MastodonPollResponsePreload {
    pub(crate) fn poll_response(&self, status_id: &str) -> Option<Option<serde_json::Value>> {
        self.preloaded_status_ids
            .contains(status_id)
            .then(|| self.by_status_id.get(status_id).cloned())
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

    Ok(Some(
        PollDraft::try_new(
            options,
            expires_in_seconds,
            poll.multiple.unwrap_or(false),
            poll.hide_totals.unwrap_or(false),
        )
        .map_err(|error| error.to_string())?,
    ))
}

pub(crate) async fn preload_mastodon_poll_responses(
    db: &D1Database,
    status_ids: &[String],
    viewer: Option<&LocalAccount>,
) -> Result<MastodonPollResponsePreload> {
    let ids = unique_poll_preload_status_ids(status_ids);
    if ids.is_empty() {
        return Ok(MastodonPollResponsePreload::default());
    }
    let preloaded_status_ids = ids.iter().map(|id| (*id).clone()).collect::<HashSet<_>>();

    let polls = load_status_polls_for_status_ids(db, &ids).await?;
    if polls.is_empty() {
        return Ok(MastodonPollResponsePreload {
            by_status_id: HashMap::new(),
            preloaded_status_ids,
        });
    }

    let poll_ids = polls.iter().map(|poll| poll.id.clone()).collect::<Vec<_>>();
    let poll_bindings = poll_id_bindings(&poll_ids);
    let mut options_by_poll_id =
        preload_poll_options_by_poll_id(db, &poll_ids, &poll_bindings).await?;
    let mut own_votes_by_poll_id = preload_own_votes_by_poll_id(db, &poll_ids, viewer).await?;
    let voters_count_by_poll_id =
        preload_voters_count_by_poll_id(db, &poll_ids, &poll_bindings).await?;
    let mut by_status_id = HashMap::new();

    for poll in polls {
        let own_votes = if viewer.is_some() {
            Some(own_votes_by_poll_id.remove(&poll.id).unwrap_or_default())
        } else {
            None
        };
        let Some(response) = mastodon_poll_response_from_rows(
            &poll,
            options_by_poll_id.remove(&poll.id).unwrap_or_default(),
            own_votes,
            voters_count_by_poll_id.get(&poll.id).copied(),
        ) else {
            continue;
        };
        by_status_id.insert(
            poll.status_id,
            serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
        );
    }

    Ok(MastodonPollResponsePreload {
        by_status_id,
        preloaded_status_ids,
    })
}

fn unique_poll_preload_status_ids(status_ids: &[String]) -> Vec<&String> {
    let mut seen = HashSet::new();
    status_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .collect()
}

async fn load_status_polls_for_status_ids(
    db: &D1Database,
    ids: &[&String],
) -> Result<Vec<StatusPollRow>> {
    let mut polls = Vec::new();
    for chunk in ids.chunks(d1_in_value_chunk_size(0)) {
        let status_placeholders = sql_placeholders(1, chunk.len());
        let poll_sql = format!(
            "SELECT id, status_id, multiple, hide_totals, expires_at
             FROM status_polls
             WHERE status_id IN ({status_placeholders})"
        );
        let status_bindings = chunk
            .iter()
            .map(|id| D1Type::Text(id.as_str()))
            .collect::<Vec<_>>();
        let poll_result = db
            .prepare(&poll_sql)
            .bind_refs(status_bindings.iter())?
            .all()
            .await?;
        polls.extend(poll_result.results::<StatusPollRow>()?);
    }
    Ok(polls)
}

fn poll_id_bindings(poll_ids: &[String]) -> Vec<D1Type<'_>> {
    poll_ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect()
}

async fn preload_poll_options_by_poll_id(
    db: &D1Database,
    poll_ids: &[String],
    _poll_bindings: &[D1Type<'_>],
) -> Result<HashMap<String, Vec<StatusPollOptionRow>>> {
    let mut options_by_poll_id: HashMap<String, Vec<StatusPollOptionRow>> = HashMap::new();
    for chunk in poll_ids.chunks(d1_in_value_chunk_size(0)) {
        let poll_placeholders = sql_placeholders(1, chunk.len());
        let options_sql = format!(
            "SELECT poll_id, title, votes_count
             FROM status_poll_options
             WHERE poll_id IN ({poll_placeholders})
             ORDER BY poll_id ASC, position ASC"
        );
        let poll_bindings = chunk
            .iter()
            .map(|id| D1Type::Text(id.as_str()))
            .collect::<Vec<_>>();
        let options_result = db
            .prepare(&options_sql)
            .bind_refs(poll_bindings.iter())?
            .all()
            .await?;
        for row in options_result.results::<PreloadedStatusPollOptionRow>()? {
            options_by_poll_id
                .entry(row.poll_id)
                .or_default()
                .push(StatusPollOptionRow {
                    title: row.title,
                    votes_count: row.votes_count,
                });
        }
    }
    Ok(options_by_poll_id)
}

async fn preload_own_votes_by_poll_id(
    db: &D1Database,
    poll_ids: &[String],
    viewer: Option<&LocalAccount>,
) -> Result<HashMap<String, Vec<u32>>> {
    let mut own_votes_by_poll_id: HashMap<String, Vec<u32>> = HashMap::new();
    if let Some(viewer) = viewer {
        for chunk in poll_ids.chunks(d1_in_value_chunk_size(1)) {
            let vote_placeholders = sql_placeholders(2, chunk.len());
            let vote_sql = format!(
                "SELECT poll_id, option_position
                 FROM status_poll_votes
                 WHERE account_id = ?1
                   AND poll_id IN ({vote_placeholders})
                 ORDER BY poll_id ASC, option_position ASC"
            );
            let mut vote_bindings = Vec::with_capacity(chunk.len() + 1);
            vote_bindings.push(D1Type::Text(viewer.id()));
            vote_bindings.extend(chunk.iter().map(|id| D1Type::Text(id.as_str())));
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
    }
    Ok(own_votes_by_poll_id)
}

async fn preload_voters_count_by_poll_id(
    db: &D1Database,
    poll_ids: &[String],
    _poll_bindings: &[D1Type<'_>],
) -> Result<HashMap<String, u64>> {
    let mut voters_by_poll_id = HashMap::new();
    for chunk in poll_ids.chunks(d1_in_value_chunk_size(0)) {
        let poll_placeholders = sql_placeholders(1, chunk.len());
        let voters_sql = format!(
            "SELECT poll_id, COUNT(DISTINCT account_id) AS count
             FROM status_poll_votes
             WHERE poll_id IN ({poll_placeholders})
             GROUP BY poll_id"
        );
        let poll_bindings = chunk
            .iter()
            .map(|id| D1Type::Text(id.as_str()))
            .collect::<Vec<_>>();
        let voters_result = db
            .prepare(&voters_sql)
            .bind_refs(poll_bindings.iter())?
            .all()
            .await?;
        voters_by_poll_id.extend(
            voters_result
                .results::<PreloadedPollVotersCountRow>()?
                .into_iter()
                .map(|row| (row.poll_id, row.count)),
        );
    }
    Ok(voters_by_poll_id)
}

fn mastodon_poll_response_from_rows(
    poll: &StatusPollRow,
    options: Vec<StatusPollOptionRow>,
    own_votes: Option<Vec<u32>>,
    multiple_voters_count: Option<u64>,
) -> Option<MastodonPollResponse> {
    if options.is_empty() {
        return None;
    }
    let votes_count = options
        .iter()
        .map(|option| option.votes_count.max(0) as u64)
        .sum();
    let expired = is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false);
    let reveal_totals = expired || poll.hide_totals == 0;
    let voters_count = if poll.multiple != 0 {
        reveal_totals.then_some(multiple_voters_count.unwrap_or(0))
    } else {
        None
    };
    let (voted, own_votes) = match own_votes {
        Some(own_votes) => (Some(!own_votes.is_empty()), Some(own_votes)),
        None => (None, None),
    };
    Some(MastodonPollResponse {
        id: poll.id.clone(),
        expires_at: timestamp_to_mastodon_iso8601(&poll.expires_at),
        expired,
        multiple: poll.multiple != 0,
        votes_count: if reveal_totals { votes_count } else { 0 },
        voters_count,
        voted,
        own_votes,
        options: options
            .into_iter()
            .map(|option| MastodonPollOptionResponse {
                title: option.title,
                votes_count: reveal_totals.then_some(option.votes_count.max(0) as u64),
            })
            .collect(),
        emojis: Vec::new(),
    })
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
    let expired = is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false);
    let reveal_totals = expired || poll.hide_totals == 0;
    let own_votes = match viewer {
        Some(viewer) => {
            Some(list_poll_vote_positions_for_account(db, &poll.id, viewer.id()).await?)
        }
        None => None,
    };
    let multiple_voters_count = if poll.multiple != 0 {
        if reveal_totals {
            Some(count_poll_voters(db, &poll.id).await?)
        } else {
            None
        }
    } else {
        None
    };

    Ok(mastodon_poll_response_from_rows(
        poll,
        options,
        own_votes,
        multiple_voters_count,
    ))
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
    object["endTime"] = serde_json::json!(timestamp_to_mastodon_iso8601(&poll.expires_at));
    object["votersCount"] = serde_json::json!(voters_count);
    if expired {
        object["closed"] = serde_json::json!(timestamp_to_mastodon_iso8601(&poll.expires_at));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn poll_row(multiple: i32, hide_totals: i32, expires_at: &str) -> StatusPollRow {
        StatusPollRow {
            id: "poll-1".to_owned(),
            status_id: "status-1".to_owned(),
            multiple,
            hide_totals,
            expires_at: expires_at.to_owned(),
        }
    }

    fn poll_options() -> Vec<StatusPollOptionRow> {
        vec![
            StatusPollOptionRow {
                title: "red".to_owned(),
                votes_count: 2,
            },
            StatusPollOptionRow {
                title: "blue".to_owned(),
                votes_count: 3,
            },
        ]
    }

    #[test]
    fn preloaded_poll_response_distinguishes_known_absent_from_unknown() {
        let preload = MastodonPollResponsePreload {
            by_status_id: HashMap::new(),
            preloaded_status_ids: HashSet::from(["known".to_owned()]),
        };

        assert_eq!(preload.poll_response("known"), Some(None));
        assert_eq!(preload.poll_response("unknown"), None);
    }

    #[test]
    fn mastodon_poll_response_hides_unexpired_hidden_totals() {
        let response = mastodon_poll_response_from_rows(
            &poll_row(1, 1, "2099-01-01T00:00:00Z"),
            poll_options(),
            Some(vec![1]),
            Some(2),
        )
        .expect("poll response");

        assert_eq!(response.votes_count, 0);
        assert_eq!(response.voters_count, None);
        assert_eq!(response.voted, Some(true));
        assert_eq!(response.own_votes, Some(vec![1]));
        assert!(
            response
                .options
                .iter()
                .all(|option| option.votes_count.is_none())
        );
    }

    #[test]
    fn mastodon_poll_response_reveals_single_choice_totals_from_options() {
        let response = mastodon_poll_response_from_rows(
            &poll_row(0, 0, "2099-01-01T00:00:00Z"),
            poll_options(),
            Some(Vec::new()),
            None,
        )
        .expect("poll response");

        assert_eq!(response.votes_count, 5);
        assert_eq!(response.voters_count, None);
        assert_eq!(response.voted, Some(false));
        assert_eq!(
            response
                .options
                .iter()
                .map(|option| option.votes_count)
                .collect::<Vec<_>>(),
            vec![Some(2), Some(3)]
        );
    }

    #[test]
    fn mastodon_poll_response_omits_vote_fields_without_viewer() {
        let response = mastodon_poll_response_from_rows(
            &poll_row(0, 0, "2099-01-01T00:00:00Z"),
            poll_options(),
            None,
            None,
        )
        .expect("poll response");

        assert_eq!(response.voted, None);
        assert_eq!(response.own_votes, None);
    }
}
