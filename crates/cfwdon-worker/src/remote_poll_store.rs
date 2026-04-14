use std::collections::BTreeSet;

use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusPollRow {
    pub(crate) id: String,
    pub(crate) status_id: String,
    pub(crate) multiple: i32,
    pub(crate) expires_at: Option<String>,
    pub(crate) voters_count: Option<i64>,
    pub(crate) votes_count: i64,
    pub(crate) expired: i32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusPollOptionRow {
    pub(crate) title: String,
    pub(crate) votes_count: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusPollVoteRow {
    pub(crate) option_position: i64,
    pub(crate) option_title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteStatusPollVoteWithIdRow {
    pub(crate) id: String,
    pub(crate) option_position: i64,
    pub(crate) option_title: Option<String>,
}

pub(crate) async fn find_remote_status_poll_by_id(
    db: &D1Database,
    poll_id: &str,
) -> Result<Option<RemoteStatusPollRow>> {
    let poll_id = D1Type::Text(poll_id);
    db.prepare(
        "SELECT id, status_id, multiple, expires_at, voters_count, votes_count, expired
         FROM remote_status_polls
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&poll_id)?
    .first::<RemoteStatusPollRow>(None)
    .await
}

pub(crate) async fn find_remote_status_poll_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<RemoteStatusPollRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, status_id, multiple, expires_at, voters_count, votes_count, expired
         FROM remote_status_polls
         WHERE status_id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<RemoteStatusPollRow>(None)
    .await
}

pub(crate) async fn list_remote_status_poll_options(
    db: &D1Database,
    poll_id: &str,
) -> Result<Vec<RemoteStatusPollOptionRow>> {
    let bindings = [D1Type::Text(poll_id)];
    let result = db
        .prepare(
            "SELECT title, votes_count
             FROM remote_status_poll_options
             WHERE poll_id = ?1
             ORDER BY position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusPollOptionRow>()
}

pub(crate) fn resolve_remote_poll_vote_position(
    options: &[RemoteStatusPollOptionRow],
    option_position: i64,
    option_title: Option<&str>,
) -> Option<u32> {
    let stored_position = u32::try_from(option_position).ok();
    if let Some(title) = option_title {
        if let Some(position) = stored_position
            .filter(|position| (*position as usize) < options.len())
            .filter(|position| options[*position as usize].title == title)
        {
            return Some(position);
        }

        if let Some(position) = options
            .iter()
            .position(|option| option.title == title)
            .and_then(|position| u32::try_from(position).ok())
        {
            return Some(position);
        }
    }

    stored_position.filter(|position| (*position as usize) < options.len())
}

pub(crate) fn remap_remote_poll_vote_positions(
    options: &[RemoteStatusPollOptionRow],
    votes: &[RemoteStatusPollVoteRow],
) -> Vec<u32> {
    votes
        .iter()
        .filter_map(|vote| {
            resolve_remote_poll_vote_position(
                options,
                vote.option_position,
                vote.option_title.as_deref(),
            )
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) async fn list_remote_poll_votes_for_account(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
) -> Result<Vec<RemoteStatusPollVoteRow>> {
    let bindings = [D1Type::Text(poll_id), D1Type::Text(account_id)];
    let result = db
        .prepare(
            "SELECT option_position, option_title
             FROM remote_status_poll_votes
             WHERE poll_id = ?1
               AND account_id = ?2
             ORDER BY option_position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusPollVoteRow>()
}

pub(crate) async fn list_remote_poll_votes_by_poll(
    db: &D1Database,
    poll_id: &str,
) -> Result<Vec<RemoteStatusPollVoteWithIdRow>> {
    let bindings = [D1Type::Text(poll_id)];
    let result = db
        .prepare(
            "SELECT id, option_position, option_title
             FROM remote_status_poll_votes
             WHERE poll_id = ?1",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusPollVoteWithIdRow>()
}

pub(crate) async fn prune_remote_poll_vote_rows(
    db: &D1Database,
    poll_id: &str,
    options: &[RemoteStatusPollOptionRow],
) -> Result<()> {
    for vote in list_remote_poll_votes_by_poll(db, poll_id).await? {
        if resolve_remote_poll_vote_position(
            options,
            vote.option_position,
            vote.option_title.as_deref(),
        )
        .is_some()
        {
            continue;
        }

        let bindings = [D1Type::Text(vote.id.as_str())];
        db.prepare(
            "DELETE FROM remote_status_poll_votes
             WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}
