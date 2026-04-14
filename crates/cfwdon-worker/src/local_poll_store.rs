use crate::db_utils::count_rows;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct StatusPollRow {
    pub(crate) id: String,
    pub(crate) status_id: String,
    pub(crate) multiple: i32,
    pub(crate) hide_totals: i32,
    pub(crate) expires_at: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StatusPollOptionRow {
    pub(crate) title: String,
    pub(crate) votes_count: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PollVoteTargetRow {
    pub(crate) poll_id: String,
    pub(crate) status_id: String,
    pub(crate) status_account_id: String,
    pub(crate) option_position: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PollVoteIdRow {
    pub(crate) id: String,
}

pub(crate) async fn find_status_poll_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<StatusPollRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, status_id, multiple, hide_totals, expires_at
         FROM status_polls
         WHERE status_id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<StatusPollRow>(None)
    .await
}

pub(crate) async fn find_status_poll_by_id(
    db: &D1Database,
    poll_id: &str,
) -> Result<Option<StatusPollRow>> {
    let poll_id = D1Type::Text(poll_id);
    db.prepare(
        "SELECT id, status_id, multiple, hide_totals, expires_at
         FROM status_polls
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&poll_id)?
    .first::<StatusPollRow>(None)
    .await
}

pub(crate) async fn list_status_poll_options(
    db: &D1Database,
    poll_id: &str,
) -> Result<Vec<StatusPollOptionRow>> {
    let bindings = [D1Type::Text(poll_id)];
    let result = db
        .prepare(
            "SELECT title, votes_count
             FROM status_poll_options
             WHERE poll_id = ?1
             ORDER BY position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusPollOptionRow>()
}

pub(crate) async fn list_poll_vote_positions_for_account(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
) -> Result<Vec<u32>> {
    let bindings = [D1Type::Text(poll_id), D1Type::Text(account_id)];
    let result = db
        .prepare(
            "SELECT option_position
             FROM status_poll_votes
             WHERE poll_id = ?1
               AND account_id = ?2
             ORDER BY option_position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<serde_json::Value>()?
        .into_iter()
        .filter_map(|value| {
            value
                .get("option_position")
                .and_then(serde_json::Value::as_u64)
        })
        .filter_map(|value| u32::try_from(value).ok())
        .collect())
}

pub(crate) async fn find_status_poll_vote_by_activity_uri(
    db: &D1Database,
    activity_uri: &str,
) -> Result<Option<PollVoteTargetRow>> {
    let bindings = [D1Type::Text(activity_uri)];
    db.prepare(
        "SELECT v.poll_id,
                p.status_id,
                s.account_id AS status_account_id,
                v.option_position
         FROM status_poll_votes v
         JOIN status_polls p
           ON p.id = v.poll_id
         JOIN statuses s
           ON s.id = p.status_id
         WHERE v.activity_uri = ?1
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<PollVoteTargetRow>(None)
    .await
}

pub(crate) async fn find_status_poll_vote_for_remote_actor_by_activity_uri(
    db: &D1Database,
    account_id: &str,
    activity_uri: &str,
) -> Result<Option<PollVoteTargetRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(activity_uri)];
    db.prepare(
        "SELECT v.poll_id,
                p.status_id,
                s.account_id AS status_account_id,
                v.option_position
         FROM status_poll_votes v
         JOIN status_polls p
           ON p.id = v.poll_id
         JOIN statuses s
           ON s.id = p.status_id
         WHERE v.account_id = ?1
           AND v.activity_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<PollVoteTargetRow>(None)
    .await
}

pub(crate) async fn find_status_poll_vote_id_by_position(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
    option_position: u32,
) -> Result<Option<PollVoteIdRow>> {
    let bindings = [
        D1Type::Text(poll_id),
        D1Type::Text(account_id),
        D1Type::Integer(option_position as i32),
    ];
    db.prepare(
        "SELECT id
         FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2
           AND option_position = ?3
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<PollVoteIdRow>(None)
    .await
}

pub(crate) async fn count_poll_voters(db: &D1Database, poll_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(DISTINCT account_id) AS count
         FROM status_poll_votes
         WHERE poll_id = ?1",
        poll_id,
    )
    .await
}
