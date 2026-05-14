use crate::{
    D1Database, RemotePollDraft, RemotePollOptionDraft, Result, list_remote_status_poll_options,
    prune_remote_poll_vote_rows,
};
use worker::d1::D1Type;

pub(crate) async fn upsert_remote_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &RemotePollDraft,
) -> Result<()> {
    let poll_id = format!("remote-{status_id}");
    let bindings = remote_status_poll_upsert_bindings(&poll_id, status_id, poll);
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

    let delete_bindings = remote_status_poll_options_delete_bindings(&poll_id);
    db.prepare(
        "DELETE FROM remote_status_poll_options
         WHERE poll_id = ?1",
    )
    .bind_refs(delete_bindings.iter())?
    .run()
    .await?;

    for (position, option) in poll.options.iter().enumerate() {
        let bindings = remote_status_poll_option_insert_bindings(&poll_id, position, option);
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

fn remote_status_poll_upsert_bindings<'a>(
    poll_id: &'a str,
    status_id: &'a str,
    poll: &'a RemotePollDraft,
) -> [D1Type<'a>; 7] {
    [
        D1Type::Text(poll_id),
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
    ]
}

fn remote_status_poll_options_delete_bindings(poll_id: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(poll_id)]
}

fn remote_status_poll_option_insert_bindings<'a>(
    poll_id: &'a str,
    position: usize,
    option: &'a RemotePollOptionDraft,
) -> [D1Type<'a>; 4] {
    [
        D1Type::Text(poll_id),
        D1Type::Integer(position as i32),
        D1Type::Text(option.title.as_str()),
        D1Type::Integer(option.votes_count.min(i32::MAX as u64) as i32),
    ]
}

pub(crate) async fn delete_remote_status_poll_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<()> {
    let bindings = remote_status_poll_status_delete_bindings(status_id);
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

fn remote_status_poll_status_delete_bindings(status_id: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(status_id)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote_poll_draft_for_test() -> RemotePollDraft {
        RemotePollDraft {
            multiple: true,
            expires_at: Some("2026-05-14T00:00:00Z".to_owned()),
            voters_count: Some(3),
            votes_count: 5,
            expired: false,
            options: vec![
                RemotePollOptionDraft {
                    title: "first".to_owned(),
                    votes_count: 2,
                },
                RemotePollOptionDraft {
                    title: "second".to_owned(),
                    votes_count: 3,
                },
            ],
        }
    }

    #[test]
    fn remote_status_poll_upsert_bindings_keep_sql_slot_order_stable() {
        let poll = remote_poll_draft_for_test();
        let bindings = remote_status_poll_upsert_bindings("poll-1", "status-1", &poll);

        assert!(matches!(bindings[0], D1Type::Text("poll-1")));
        assert!(matches!(bindings[1], D1Type::Text("status-1")));
        assert!(matches!(bindings[2], D1Type::Integer(1)));
        assert!(matches!(bindings[3], D1Type::Text("2026-05-14T00:00:00Z")));
        assert!(matches!(bindings[4], D1Type::Integer(3)));
        assert!(matches!(bindings[5], D1Type::Integer(5)));
        assert!(matches!(bindings[6], D1Type::Integer(0)));
    }

    #[test]
    fn remote_status_poll_upsert_bindings_use_nulls_for_optional_counts() {
        let mut poll = remote_poll_draft_for_test();
        poll.expires_at = None;
        poll.voters_count = None;
        let bindings = remote_status_poll_upsert_bindings("poll-1", "status-1", &poll);

        assert!(matches!(bindings[3], D1Type::Null));
        assert!(matches!(bindings[4], D1Type::Null));
    }

    #[test]
    fn remote_status_poll_options_delete_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_poll_options_delete_bindings("poll-1");

        assert!(matches!(bindings[0], D1Type::Text("poll-1")));
    }

    #[test]
    fn remote_status_poll_option_insert_bindings_keep_sql_slot_order_stable() {
        let option = RemotePollOptionDraft {
            title: "first".to_owned(),
            votes_count: 2,
        };
        let bindings = remote_status_poll_option_insert_bindings("poll-1", 1, &option);

        assert!(matches!(bindings[0], D1Type::Text("poll-1")));
        assert!(matches!(bindings[1], D1Type::Integer(1)));
        assert!(matches!(bindings[2], D1Type::Text("first")));
        assert!(matches!(bindings[3], D1Type::Integer(2)));
    }

    #[test]
    fn remote_status_poll_status_delete_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_poll_status_delete_bindings("status-1");

        assert!(matches!(bindings[0], D1Type::Text("status-1")));
    }
}
