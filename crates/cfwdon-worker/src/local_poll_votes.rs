use crate::{
    PollVoteIdRow, PollVoteTargetRow, StatusPollRow,
    find_status_poll_vote_for_remote_actor_by_activity_uri, find_status_poll_vote_id_by_position,
    generate_entity_id, list_poll_vote_positions_for_account, list_status_poll_options,
    validate_poll_vote_submission,
};
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

#[derive(Debug, PartialEq, Eq)]
struct LocalPollVoteInsertDraft {
    vote_id: String,
    poll_id: String,
    account_id: String,
    option_position: u32,
    activity_uri: Option<String>,
}

impl LocalPollVoteInsertDraft {
    fn from_parts(
        vote_id: String,
        poll_id: &str,
        account_id: &str,
        option_position: u32,
        activity_uri: Option<&str>,
    ) -> Self {
        Self {
            vote_id,
            poll_id: poll_id.to_owned(),
            account_id: account_id.to_owned(),
            option_position,
            activity_uri: activity_uri.map(str::to_owned),
        }
    }
}

fn validate_local_poll_vote_choices(
    option_count: usize,
    allows_multiple: bool,
    choices: &[u32],
) -> Result<()> {
    if choices
        .iter()
        .any(|choice| (*choice as usize) >= option_count)
    {
        return Err(Error::RustError(
            "poll choice index is out of range".to_owned(),
        ));
    }
    if !allows_multiple && choices.len() != 1 {
        return Err(Error::RustError(
            "poll does not allow multiple choices".to_owned(),
        ));
    }

    Ok(())
}

pub(crate) async fn apply_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    choices: &[u32],
) -> Result<()> {
    let options = list_status_poll_options(db, &poll.id).await?;
    let allows_multiple = poll.multiple != 0;
    validate_local_poll_vote_choices(options.len(), allows_multiple, choices)?;

    let existing = list_poll_vote_positions_for_account(db, &poll.id, account_id).await?;
    validate_poll_vote_submission(existing.len(), allows_multiple, choices.len())
        .map_err(Error::RustError)?;

    replace_account_poll_votes(db, &poll.id, account_id, &existing, choices).await
}

async fn replace_account_poll_votes(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
    existing_choices: &[u32],
    choices: &[u32],
) -> Result<()> {
    remove_existing_account_poll_votes(db, poll_id, account_id, existing_choices).await?;
    insert_account_poll_votes(db, poll_id, account_id, choices).await
}

async fn remove_existing_account_poll_votes(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
    existing_choices: &[u32],
) -> Result<()> {
    for choice in existing_choices {
        decrement_poll_option_vote_count(db, poll_id, i64::from(*choice)).await?;
    }
    delete_poll_votes_for_account(db, poll_id, account_id).await
}

async fn decrement_poll_option_vote_count(
    db: &D1Database,
    poll_id: &str,
    choice: i64,
) -> Result<()> {
    let bindings = poll_option_vote_count_bindings(poll_id, choice);
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
    Ok(())
}

fn poll_option_vote_count_bindings(poll_id: &str, option_position: i64) -> [D1Type<'_>; 2] {
    [
        D1Type::Text(poll_id),
        D1Type::Integer(option_position as i32),
    ]
}

async fn delete_poll_votes_for_account(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
) -> Result<()> {
    let bindings = account_poll_votes_delete_bindings(poll_id, account_id);
    db.prepare(
        "DELETE FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

fn account_poll_votes_delete_bindings<'a>(
    poll_id: &'a str,
    account_id: &'a str,
) -> [D1Type<'a>; 2] {
    [D1Type::Text(poll_id), D1Type::Text(account_id)]
}

async fn insert_account_poll_votes(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
    choices: &[u32],
) -> Result<()> {
    for choice in choices {
        insert_account_poll_vote(db, poll_id, account_id, *choice).await?;
        increment_poll_option_vote_count(db, poll_id, *choice).await?;
    }

    Ok(())
}

async fn insert_account_poll_vote(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
    choice: u32,
) -> Result<()> {
    let vote_id = generate_entity_id(16)?;
    let draft = LocalPollVoteInsertDraft::from_parts(vote_id, poll_id, account_id, choice, None);
    insert_account_poll_vote_row(db, &draft).await
}

async fn insert_account_poll_vote_row(
    db: &D1Database,
    draft: &LocalPollVoteInsertDraft,
) -> Result<()> {
    let bindings = local_poll_vote_insert_bindings(draft);
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
    Ok(())
}

fn local_poll_vote_insert_bindings(draft: &LocalPollVoteInsertDraft) -> [D1Type<'_>; 4] {
    [
        D1Type::Text(draft.vote_id.as_str()),
        D1Type::Text(draft.poll_id.as_str()),
        D1Type::Text(draft.account_id.as_str()),
        D1Type::Integer(draft.option_position as i32),
    ]
}

async fn increment_poll_option_vote_count(
    db: &D1Database,
    poll_id: &str,
    choice: u32,
) -> Result<()> {
    let bindings = poll_option_vote_count_bindings(poll_id, i64::from(choice));
    db.prepare(
        "UPDATE status_poll_options
             SET votes_count = votes_count + 1
             WHERE poll_id = ?1
               AND position = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
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
    let draft =
        LocalPollVoteInsertDraft::from_parts(vote_id, &poll.id, account_id, choice, activity_uri);
    insert_incoming_poll_vote_row(db, &draft).await
}

async fn insert_incoming_poll_vote_row(
    db: &D1Database,
    draft: &LocalPollVoteInsertDraft,
) -> Result<()> {
    let bindings = incoming_poll_vote_insert_bindings(draft);
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

fn incoming_poll_vote_insert_bindings(draft: &LocalPollVoteInsertDraft) -> [D1Type<'_>; 5] {
    [
        D1Type::Text(draft.vote_id.as_str()),
        D1Type::Text(draft.poll_id.as_str()),
        D1Type::Text(draft.account_id.as_str()),
        D1Type::Integer(draft.option_position as i32),
        match draft.activity_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ]
}

pub(crate) async fn delete_incoming_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    activity_uri: Option<&str>,
    choice_name: Option<&str>,
) -> Result<bool> {
    let Some(target) =
        resolve_incoming_poll_vote_deletion_target(db, poll, account_id, activity_uri, choice_name)
            .await?
    else {
        return Ok(false);
    };
    delete_incoming_poll_vote_target(db, account_id, &target).await?;
    decrement_poll_option_vote_count(db, &target.poll_id, target.option_position).await?;

    Ok(true)
}

struct IncomingPollVoteDeletionTarget {
    poll_id: String,
    option_position: i64,
    filter: IncomingPollVoteDeletionFilter,
}

enum IncomingPollVoteDeletionFilter {
    ActivityUri(String),
    VoteId(String),
}

async fn resolve_incoming_poll_vote_deletion_target(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    activity_uri: Option<&str>,
    choice_name: Option<&str>,
) -> Result<Option<IncomingPollVoteDeletionTarget>> {
    if let Some(activity_uri) = activity_uri {
        return find_status_poll_vote_for_remote_actor_by_activity_uri(
            db,
            account_id,
            activity_uri,
        )
        .await
        .map(|target| {
            target
                .map(|target| incoming_poll_vote_deletion_target_for_activity(activity_uri, target))
        });
    }

    let Some(choice_name) = choice_name else {
        return Ok(None);
    };
    incoming_poll_vote_deletion_target_for_choice_name(db, poll, account_id, choice_name).await
}

fn incoming_poll_vote_deletion_target_for_activity(
    activity_uri: &str,
    target: PollVoteTargetRow,
) -> IncomingPollVoteDeletionTarget {
    IncomingPollVoteDeletionTarget {
        poll_id: target.poll_id,
        option_position: target.option_position,
        filter: IncomingPollVoteDeletionFilter::ActivityUri(activity_uri.to_owned()),
    }
}

async fn incoming_poll_vote_deletion_target_for_choice_name(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    choice_name: &str,
) -> Result<Option<IncomingPollVoteDeletionTarget>> {
    let Some(position) = incoming_poll_vote_choice_position(db, poll, choice_name).await? else {
        return Ok(None);
    };
    let Some(PollVoteIdRow { id }) =
        find_status_poll_vote_id_by_position(db, &poll.id, account_id, position).await?
    else {
        return Ok(None);
    };

    Ok(Some(IncomingPollVoteDeletionTarget {
        poll_id: poll.id.clone(),
        option_position: i64::from(position),
        filter: IncomingPollVoteDeletionFilter::VoteId(id),
    }))
}

async fn incoming_poll_vote_choice_position(
    db: &D1Database,
    poll: &StatusPollRow,
    choice_name: &str,
) -> Result<Option<u32>> {
    let options = list_status_poll_options(db, &poll.id).await?;
    Ok(options
        .iter()
        .position(|option| option.title == choice_name)
        .and_then(|position| u32::try_from(position).ok()))
}

async fn delete_incoming_poll_vote_target(
    db: &D1Database,
    account_id: &str,
    target: &IncomingPollVoteDeletionTarget,
) -> Result<()> {
    match &target.filter {
        IncomingPollVoteDeletionFilter::ActivityUri(activity_uri) => {
            delete_incoming_poll_vote_by_activity_uri(db, &target.poll_id, account_id, activity_uri)
                .await
        }
        IncomingPollVoteDeletionFilter::VoteId(vote_id) => {
            delete_incoming_poll_vote_by_id(db, vote_id).await
        }
    }
}

async fn delete_incoming_poll_vote_by_activity_uri(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
    activity_uri: &str,
) -> Result<()> {
    let bindings = incoming_poll_vote_activity_delete_bindings(poll_id, account_id, activity_uri);
    db.prepare(
        "DELETE FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2
           AND activity_uri = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

fn incoming_poll_vote_activity_delete_bindings<'a>(
    poll_id: &'a str,
    account_id: &'a str,
    activity_uri: &'a str,
) -> [D1Type<'a>; 3] {
    [
        D1Type::Text(poll_id),
        D1Type::Text(account_id),
        D1Type::Text(activity_uri),
    ]
}

async fn delete_incoming_poll_vote_by_id(db: &D1Database, vote_id: &str) -> Result<()> {
    let bindings = incoming_poll_vote_id_delete_bindings(vote_id);
    db.prepare(
        "DELETE FROM status_poll_votes
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

fn incoming_poll_vote_id_delete_bindings(vote_id: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(vote_id)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_local_poll_vote_choices_accepts_valid_single_choice() {
        validate_local_poll_vote_choices(2, false, &[1]).unwrap();
    }

    #[test]
    fn validate_local_poll_vote_choices_accepts_valid_multiple_choices() {
        validate_local_poll_vote_choices(3, true, &[0, 2]).unwrap();
    }

    #[test]
    fn validate_local_poll_vote_choices_rejects_out_of_range_choice() {
        let error = validate_local_poll_vote_choices(2, true, &[2]).unwrap_err();

        assert_eq!(error.to_string(), "poll choice index is out of range");
    }

    #[test]
    fn validate_local_poll_vote_choices_rejects_multiple_choices_for_single_choice_poll() {
        let error = validate_local_poll_vote_choices(2, false, &[0, 1]).unwrap_err();

        assert_eq!(error.to_string(), "poll does not allow multiple choices");
    }

    #[test]
    fn local_poll_vote_insert_draft_maps_storage_fields() {
        let draft = LocalPollVoteInsertDraft::from_parts(
            "vote-1".to_owned(),
            "poll-1",
            "acct-1",
            2,
            Some("https://remote.example/votes/1"),
        );

        assert_eq!(draft.vote_id, "vote-1");
        assert_eq!(draft.poll_id, "poll-1");
        assert_eq!(draft.account_id, "acct-1");
        assert_eq!(draft.option_position, 2);
        assert_eq!(
            draft.activity_uri.as_deref(),
            Some("https://remote.example/votes/1")
        );
    }

    #[test]
    fn local_poll_vote_insert_bindings_keep_sql_slot_order_stable() {
        let draft =
            LocalPollVoteInsertDraft::from_parts("vote-1".to_owned(), "poll-1", "acct-1", 2, None);
        let bindings = local_poll_vote_insert_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("vote-1")));
        assert!(matches!(bindings[1], D1Type::Text("poll-1")));
        assert!(matches!(bindings[2], D1Type::Text("acct-1")));
        assert!(matches!(bindings[3], D1Type::Integer(2)));
    }

    #[test]
    fn poll_option_vote_count_bindings_keep_sql_slot_order_stable() {
        let bindings = poll_option_vote_count_bindings("poll-1", 2);

        assert!(matches!(bindings[0], D1Type::Text("poll-1")));
        assert!(matches!(bindings[1], D1Type::Integer(2)));
    }

    #[test]
    fn account_poll_votes_delete_bindings_keep_sql_slot_order_stable() {
        let bindings = account_poll_votes_delete_bindings("poll-1", "acct-1");

        assert!(matches!(bindings[0], D1Type::Text("poll-1")));
        assert!(matches!(bindings[1], D1Type::Text("acct-1")));
    }

    #[test]
    fn incoming_poll_vote_insert_bindings_keep_sql_slot_order_stable() {
        let draft = LocalPollVoteInsertDraft::from_parts(
            "vote-1".to_owned(),
            "poll-1",
            "acct-1",
            2,
            Some("https://remote.example/votes/1"),
        );
        let bindings = incoming_poll_vote_insert_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("vote-1")));
        assert!(matches!(bindings[1], D1Type::Text("poll-1")));
        assert!(matches!(bindings[2], D1Type::Text("acct-1")));
        assert!(matches!(bindings[3], D1Type::Integer(2)));
        assert!(matches!(
            bindings[4],
            D1Type::Text("https://remote.example/votes/1")
        ));
    }

    #[test]
    fn incoming_poll_vote_activity_delete_bindings_keep_sql_slot_order_stable() {
        let bindings = incoming_poll_vote_activity_delete_bindings(
            "poll-1",
            "acct-1",
            "https://remote.example/votes/1",
        );

        assert!(matches!(bindings[0], D1Type::Text("poll-1")));
        assert!(matches!(bindings[1], D1Type::Text("acct-1")));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://remote.example/votes/1")
        ));
    }

    #[test]
    fn incoming_poll_vote_id_delete_bindings_keep_sql_slot_order_stable() {
        let bindings = incoming_poll_vote_id_delete_bindings("vote-1");

        assert!(matches!(bindings[0], D1Type::Text("vote-1")));
    }

    #[test]
    fn incoming_poll_vote_deletion_target_for_activity_preserves_storage_fields() {
        let target = incoming_poll_vote_deletion_target_for_activity(
            "https://remote.example/votes/1",
            PollVoteTargetRow {
                poll_id: "poll-1".to_owned(),
                status_id: "status-1".to_owned(),
                status_account_id: "account-1".to_owned(),
                option_position: 2,
            },
        );

        assert_eq!(target.poll_id, "poll-1");
        assert_eq!(target.option_position, 2);
        match target.filter {
            IncomingPollVoteDeletionFilter::ActivityUri(activity_uri) => {
                assert_eq!(activity_uri, "https://remote.example/votes/1");
            }
            IncomingPollVoteDeletionFilter::VoteId(_) => panic!("expected activity uri filter"),
        }
    }
}
