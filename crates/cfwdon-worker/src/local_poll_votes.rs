use crate::{
    PollVoteIdRow, StatusPollRow, find_status_poll_vote_for_remote_actor_by_activity_uri,
    find_status_poll_vote_id_by_position, generate_entity_id, list_poll_vote_positions_for_account,
    list_status_poll_options, validate_poll_vote_submission,
};
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

pub(crate) async fn apply_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    choices: &[u32],
) -> Result<()> {
    let options = list_status_poll_options(db, &poll.id).await?;
    let max_index = options.len();
    if choices.iter().any(|choice| (*choice as usize) >= max_index) {
        return Err(Error::RustError(
            "poll choice index is out of range".to_owned(),
        ));
    }
    if poll.multiple == 0 && choices.len() != 1 {
        return Err(Error::RustError(
            "poll does not allow multiple choices".to_owned(),
        ));
    }

    let existing = list_poll_vote_positions_for_account(db, &poll.id, account_id).await?;
    validate_poll_vote_submission(existing.len(), poll.multiple != 0, choices.len())
        .map_err(Error::RustError)?;

    for choice in existing {
        let bindings = [
            D1Type::Text(poll.id.as_str()),
            D1Type::Integer(choice as i32),
        ];
        db.prepare(
            "UPDATE status_poll_options
             SET votes_count = CASE
                 WHEN votes_count > 0 THEN votes_count - 1
                 ELSE 0
             END
             WHERE poll_id = ?1
               AND position = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    let bindings = [D1Type::Text(poll.id.as_str()), D1Type::Text(account_id)];
    db.prepare(
        "DELETE FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    for choice in choices {
        let vote_id = generate_entity_id(16)?;
        let bindings = [
            D1Type::Text(vote_id.as_str()),
            D1Type::Text(poll.id.as_str()),
            D1Type::Text(account_id),
            D1Type::Integer(*choice as i32),
        ];
        db.prepare(
            "INSERT INTO status_poll_votes (
                id,
                poll_id,
                account_id,
                option_position,
                created_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

        let bindings = [
            D1Type::Text(poll.id.as_str()),
            D1Type::Integer(*choice as i32),
        ];
        db.prepare(
            "UPDATE status_poll_options
             SET votes_count = votes_count + 1
             WHERE poll_id = ?1
               AND position = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

pub(crate) async fn apply_incoming_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    choice: u32,
    activity_uri: Option<&str>,
) -> Result<()> {
    let options = list_status_poll_options(db, &poll.id).await?;
    if choice as usize >= options.len() {
        return Ok(());
    }
    let existing = list_poll_vote_positions_for_account(db, &poll.id, account_id).await?;
    if poll.multiple == 0 && !existing.is_empty() {
        return Ok(());
    }
    if existing.iter().any(|position| *position == choice) {
        return Ok(());
    }

    let vote_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(vote_id.as_str()),
        D1Type::Text(poll.id.as_str()),
        D1Type::Text(account_id),
        D1Type::Integer(choice as i32),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT OR IGNORE INTO status_poll_votes (
            id,
            poll_id,
            account_id,
            option_position,
            activity_uri,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn delete_incoming_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    activity_uri: Option<&str>,
    choice_name: Option<&str>,
) -> Result<bool> {
    let target = if let Some(activity_uri) = activity_uri {
        find_status_poll_vote_for_remote_actor_by_activity_uri(db, account_id, activity_uri).await?
    } else if let Some(choice_name) = choice_name {
        let options = list_status_poll_options(db, &poll.id).await?;
        let Some(position) = options
            .iter()
            .position(|option| option.title == choice_name)
            .and_then(|position| u32::try_from(position).ok())
        else {
            return Ok(false);
        };
        let Some(PollVoteIdRow { id }) =
            find_status_poll_vote_id_by_position(db, &poll.id, account_id, position).await?
        else {
            return Ok(false);
        };
        let bindings = [D1Type::Text(id.as_str())];
        db.prepare(
            "DELETE FROM status_poll_votes
             WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
        let bindings = [
            D1Type::Text(poll.id.as_str()),
            D1Type::Integer(position as i32),
        ];
        db.prepare(
            "UPDATE status_poll_options
             SET votes_count = CASE
                 WHEN votes_count > 0 THEN votes_count - 1
                 ELSE 0
             END
             WHERE poll_id = ?1
               AND position = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
        return Ok(true);
    } else {
        None
    };

    let Some(target) = target else {
        return Ok(false);
    };
    let bindings = [
        D1Type::Text(target.poll_id.as_str()),
        D1Type::Text(account_id),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "DELETE FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2
           AND activity_uri = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    let bindings = [
        D1Type::Text(target.poll_id.as_str()),
        D1Type::Integer(target.option_position as i32),
    ];
    db.prepare(
        "UPDATE status_poll_options
         SET votes_count = CASE
             WHEN votes_count > 0 THEN votes_count - 1
             ELSE 0
         END
         WHERE poll_id = ?1
           AND position = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(true)
}
