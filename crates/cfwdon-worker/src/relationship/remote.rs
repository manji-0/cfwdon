use crate::{
    D1Database, FollowAccountRequest, FollowRow, LocalAccount, RemoteActorProfile, RemoteActorRow,
};
use cfwdon_domain::{RemoteFollowState, initial_remote_follow_state};
use worker::d1::D1Type;
use worker::{Error, Result};

#[derive(Debug)]
struct RemoteFollowUpsertDraft {
    follower_account_id: String,
    target_actor_uri: String,
    target_inbox_uri: Option<String>,
    target_shared_inbox_uri: Option<String>,
    follow_activity_id: String,
    state: RemoteFollowState,
    show_reblogs: bool,
    notify: bool,
    languages_json: Option<String>,
}

impl RemoteFollowUpsertDraft {
    fn new(
        follower: &LocalAccount,
        actor: &RemoteActorRow,
        request: &FollowAccountRequest,
        follow_activity_id: &str,
        inbox_uris: (Option<String>, Option<String>),
    ) -> Result<Self> {
        Ok(Self {
            follower_account_id: follower.id().to_owned(),
            target_actor_uri: actor.actor_uri.clone(),
            target_inbox_uri: inbox_uris.0,
            target_shared_inbox_uri: inbox_uris.1,
            follow_activity_id: follow_activity_id.to_owned(),
            state: initial_remote_follow_state(actor.locked),
            show_reblogs: request.reblogs.unwrap_or(true),
            notify: request.notify.unwrap_or(false),
            languages_json: serialize_follow_languages(request)?,
        })
    }
}

const REMOTE_FOLLOW_UPSERT_SQL: &str = "INSERT INTO follows (
            id,
            follower_account_id,
            target_account_id,
            target_actor_uri,
            target_inbox_uri,
            target_shared_inbox_uri,
            follow_activity_id,
            state,
            show_reblogs,
            notify,
            languages_json,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            NULL,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8,
            ?9,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(follower_account_id, target_actor_uri) DO UPDATE SET
            target_inbox_uri = excluded.target_inbox_uri,
            target_shared_inbox_uri = excluded.target_shared_inbox_uri,
            follow_activity_id = excluded.follow_activity_id,
            state = excluded.state,
            show_reblogs = excluded.show_reblogs,
            notify = excluded.notify,
            languages_json = excluded.languages_json,
            updated_at = CURRENT_TIMESTAMP";

pub(crate) async fn find_follow_by_activity_id(
    db: &D1Database,
    follow_activity_id: &str,
) -> Result<Option<FollowRow>> {
    let follow_activity_id = D1Type::Text(follow_activity_id);
    db.prepare(
        "SELECT follower_account_id, target_account_id, target_actor_uri, follow_activity_id, state
         FROM follows
         WHERE follow_activity_id = ?1
         LIMIT 1",
    )
    .bind_refs(&follow_activity_id)?
    .first::<FollowRow>(None)
    .await
}

pub(crate) async fn update_follow_state_from_response(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    state: &str,
) -> Result<()> {
    let Some(follow_activity_id) = activity
        .get("object")
        .and_then(|object| object.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    let Some(follow) = find_follow_by_activity_id(db, follow_activity_id).await? else {
        return Ok(());
    };
    if follow.target_actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    let bindings = [D1Type::Text(state), D1Type::Text(follow_activity_id)];
    db.prepare(
        "UPDATE follows
         SET state = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE follow_activity_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn upsert_remote_follow(
    db: &D1Database,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
    request: &FollowAccountRequest,
    follow_activity_id: &str,
) -> Result<()> {
    let inbox_uris = load_remote_actor_inbox_uris(db, &actor.actor_uri).await?;
    let draft =
        RemoteFollowUpsertDraft::new(follower, actor, request, follow_activity_id, inbox_uris)?;
    upsert_remote_follow_row(db, &draft).await
}

async fn upsert_remote_follow_row(db: &D1Database, draft: &RemoteFollowUpsertDraft) -> Result<()> {
    let bindings = remote_follow_upsert_bindings(draft);
    db.prepare(REMOTE_FOLLOW_UPSERT_SQL)
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    Ok(())
}

fn serialize_follow_languages(request: &FollowAccountRequest) -> Result<Option<String>> {
    request
        .languages
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| Error::RustError(format!("failed to serialize follow languages: {error}")))
}

fn bool_d1(value: bool) -> D1Type<'static> {
    D1Type::Integer(if value { 1 } else { 0 })
}

fn remote_follow_upsert_bindings(draft: &RemoteFollowUpsertDraft) -> [D1Type<'_>; 9] {
    [
        D1Type::Text(draft.follower_account_id.as_str()),
        D1Type::Text(draft.target_actor_uri.as_str()),
        draft
            .target_inbox_uri
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        draft
            .target_shared_inbox_uri
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(draft.follow_activity_id.as_str()),
        D1Type::Text(draft.state.as_str()),
        bool_d1(draft.show_reblogs),
        bool_d1(draft.notify),
        draft
            .languages_json
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    ]
}

pub(crate) async fn load_remote_actor_inbox_uris(
    db: &D1Database,
    actor_uri: &str,
) -> Result<(Option<String>, Option<String>)> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT inbox_uri, shared_inbox_uri
             FROM remote_actors
             WHERE actor_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&actor_uri)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok((
        row.as_ref()
            .and_then(|value| value.get("inbox_uri"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        row.as_ref()
            .and_then(|value| value.get("shared_inbox_uri"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    ))
}

pub(crate) async fn load_remote_actor_delivery_inbox(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<String>> {
    let (inbox_uri, shared_inbox_uri) = load_remote_actor_inbox_uris(db, actor_uri).await?;
    Ok(shared_inbox_uri.or(inbox_uri))
}

pub(crate) async fn load_follow_activity_id(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<Option<String>> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT follow_activity_id
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("follow_activity_id"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::LocalAccountRecord;

    fn local_account(id: &str) -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture(id, "alice"))
    }

    fn remote_actor(actor_uri: &str, locked: bool) -> RemoteActorRow {
        RemoteActorRow {
            actor_uri: actor_uri.to_owned(),
            username: "bob".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            locked,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Bob".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@bob".to_owned()),
            avatar_url: None,
            header_url: None,
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            social_counts_updated_at: None,
        }
    }

    #[test]
    fn remote_follow_upsert_draft_uses_defaults_for_unlocked_actor() {
        let follower = local_account("viewer");
        let actor = remote_actor("https://remote.example/users/bob", false);
        let draft = RemoteFollowUpsertDraft::new(
            &follower,
            &actor,
            &FollowAccountRequest::default(),
            "activity-1",
            (None, None),
        )
        .expect("draft");

        assert_eq!(draft.follower_account_id, "viewer");
        assert_eq!(draft.target_actor_uri, "https://remote.example/users/bob");
        assert_eq!(draft.follow_activity_id, "activity-1");
        assert_eq!(draft.state, RemoteFollowState::Accepted);
        assert!(draft.show_reblogs);
        assert!(!draft.notify);
        assert_eq!(draft.languages_json, None);
    }

    #[test]
    fn remote_follow_upsert_draft_maps_request_and_locked_state() {
        let follower = local_account("viewer");
        let actor = remote_actor("https://remote.example/users/bob", true);
        let request = FollowAccountRequest {
            reblogs: Some(false),
            notify: Some(true),
            languages: Some(vec!["en".to_owned(), "ja".to_owned()]),
        };
        let draft = RemoteFollowUpsertDraft::new(
            &follower,
            &actor,
            &request,
            "activity-2",
            (
                Some("https://remote.example/inbox".to_owned()),
                Some("https://remote.example/shared-inbox".to_owned()),
            ),
        )
        .expect("draft");

        assert_eq!(draft.state, RemoteFollowState::Pending);
        assert!(!draft.show_reblogs);
        assert!(draft.notify);
        assert_eq!(draft.languages_json, Some("[\"en\",\"ja\"]".to_owned()));
        assert_eq!(
            draft.target_inbox_uri,
            Some("https://remote.example/inbox".to_owned())
        );
        assert_eq!(
            draft.target_shared_inbox_uri,
            Some("https://remote.example/shared-inbox".to_owned())
        );
    }

    #[test]
    fn remote_follow_upsert_bindings_keep_sql_slot_order_stable() {
        let draft = RemoteFollowUpsertDraft {
            follower_account_id: "viewer".to_owned(),
            target_actor_uri: "https://remote.example/users/bob".to_owned(),
            target_inbox_uri: None,
            target_shared_inbox_uri: Some("https://remote.example/shared".to_owned()),
            follow_activity_id: "activity-3".to_owned(),
            state: RemoteFollowState::Pending,
            show_reblogs: false,
            notify: true,
            languages_json: Some("[\"ja\"]".to_owned()),
        };
        let bindings = remote_follow_upsert_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("https://remote.example/users/bob")
        ));
        assert!(matches!(bindings[2], D1Type::Null));
        assert!(matches!(
            bindings[3],
            D1Type::Text("https://remote.example/shared")
        ));
        assert!(matches!(bindings[4], D1Type::Text("activity-3")));
        assert!(matches!(bindings[5], D1Type::Text("pending")));
        assert!(matches!(bindings[6], D1Type::Integer(0)));
        assert!(matches!(bindings[7], D1Type::Integer(1)));
        assert!(matches!(bindings[8], D1Type::Text("[\"ja\"]")));
    }
}
