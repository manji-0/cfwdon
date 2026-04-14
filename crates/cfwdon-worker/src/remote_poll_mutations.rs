use crate::{
    D1Database, RemotePollDraft, Result, list_remote_status_poll_options,
    prune_remote_poll_vote_rows,
};
use worker::d1::D1Type;

pub(crate) async fn upsert_remote_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &RemotePollDraft,
) -> Result<()> {
    let poll_id = format!("remote-{status_id}");
    let bindings = [
        D1Type::Text(poll_id.as_str()),
        D1Type::Text(status_id),
        D1Type::Integer(if poll.multiple { 1 } else { 0 }),
        match poll.expires_at.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match poll.voters_count {
            Some(value) => D1Type::Integer(value as i32),
            None => D1Type::Null,
        },
        D1Type::Integer(poll.votes_count.min(i32::MAX as u64) as i32),
        D1Type::Integer(if poll.expired { 1 } else { 0 }),
    ];
    db.prepare(
        "INSERT INTO remote_status_polls (
            id,
            status_id,
            multiple,
            expires_at,
            voters_count,
            votes_count,
            expired,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(status_id) DO UPDATE SET
            id = excluded.id,
            multiple = excluded.multiple,
            expires_at = excluded.expires_at,
            voters_count = excluded.voters_count,
            votes_count = excluded.votes_count,
            expired = excluded.expired,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let delete_bindings = [D1Type::Text(poll_id.as_str())];
    db.prepare(
        "DELETE FROM remote_status_poll_options
         WHERE poll_id = ?1",
    )
    .bind_refs(delete_bindings.iter())?
    .run()
    .await?;

    for (position, option) in poll.options.iter().enumerate() {
        let bindings = [
            D1Type::Text(poll_id.as_str()),
            D1Type::Integer(position as i32),
            D1Type::Text(option.title.as_str()),
            D1Type::Integer(option.votes_count.min(i32::MAX as u64) as i32),
        ];
        db.prepare(
            "INSERT INTO remote_status_poll_options (
                poll_id,
                position,
                title,
                votes_count
            ) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    let current_options = list_remote_status_poll_options(db, &poll_id).await?;
    prune_remote_poll_vote_rows(db, &poll_id, &current_options).await?;

    Ok(())
}

pub(crate) async fn delete_remote_status_poll_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(status_id)];
    db.prepare(
        "DELETE FROM remote_status_poll_votes
         WHERE poll_id IN (
            SELECT id
            FROM remote_status_polls
            WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "DELETE FROM remote_status_poll_options
         WHERE poll_id IN (
            SELECT id
            FROM remote_status_polls
            WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "DELETE FROM remote_status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}
