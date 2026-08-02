use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
#[derive(Debug, Deserialize)]
pub(crate) struct FollowerTargetRow {
    pub(crate) target_inbox: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsernameRow {
    pub(crate) username: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FollowRow {
    pub(crate) follower_account_id: String,
    #[serde(rename = "target_account_id")]
    pub(crate) _target_account_id: Option<String>,
    pub(crate) target_actor_uri: String,
    #[serde(rename = "follow_activity_id")]
    pub(crate) _follow_activity_id: Option<String>,
    pub(crate) state: String,
}

pub(crate) async fn delete_follow_by_target(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    db.prepare(
        "DELETE FROM follows
         WHERE follower_account_id = ?1
           AND target_actor_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn find_follow_by_target(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<Option<FollowRow>> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    db.prepare(
        "SELECT follower_account_id, target_account_id, target_actor_uri, follow_activity_id, state
         FROM follows
         WHERE follower_account_id = ?1
           AND target_actor_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<FollowRow>(None)
    .await
}
