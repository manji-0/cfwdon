use super::{
    AppConfig, FollowAccountRequest, LocalAccount, RemoteActorRow, actor_url,
    build_follow_activity, build_undo_follow_activity, delete_follow_by_target,
    load_follow_activity_id, queue_remote_actor_activity, queue_remote_actor_activity_required,
    remote_account_rest_id, upsert_remote_follow,
};
use js_sys::Date;
use serde::{Deserialize, Serialize};
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

#[derive(Debug, Serialize)]
pub(crate) struct RelationshipResponse {
    pub(crate) id: String,
    pub(crate) following: bool,
    pub(crate) showing_reblogs: bool,
    pub(crate) notifying: bool,
    pub(crate) languages: Option<Vec<String>>,
    pub(crate) followed_by: bool,
    pub(crate) blocking: bool,
    pub(crate) blocked_by: bool,
    pub(crate) muting: bool,
    pub(crate) muting_notifications: bool,
    pub(crate) muting_expires_at: Option<String>,
    pub(crate) requested: bool,
    pub(crate) requested_by: bool,
    pub(crate) domain_blocking: bool,
    pub(crate) endorsed: bool,
    pub(crate) note: String,
}

#[derive(Debug, Deserialize)]
struct RelationshipStateRow {
    follow_state: Option<String>,
    show_reblogs: Option<i32>,
    notify: Option<i32>,
    languages_json: Option<String>,
    reciprocal_following: i32,
    followed_by_remote: i32,
    blocking: i32,
    blocked_by: i32,
    mute_notifications: Option<i32>,
    mute_expires_at: Option<String>,
    requested_by: i32,
    endorsed: Option<i32>,
    note: Option<String>,
}

pub(crate) async fn build_relationship_for_target(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    target_id: &str,
    target_actor_uri: &str,
) -> Result<RelationshipResponse> {
    let viewer_actor_uri = actor_url(config, &viewer.username);
    let state_row =
        load_relationship_state(db, viewer, target_id, target_actor_uri, &viewer_actor_uri).await?;
    let languages = state_row
        .languages_json
        .as_deref()
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok());
    let state = state_row.follow_state.as_deref().unwrap_or("none");

    Ok(RelationshipResponse {
        id: target_id.to_owned(),
        following: state == "accepted",
        showing_reblogs: state_row
            .show_reblogs
            .map(|value| value != 0)
            .unwrap_or(false),
        notifying: state_row.notify.map(|value| value != 0).unwrap_or(false),
        languages,
        followed_by: state_row.reciprocal_following != 0 || state_row.followed_by_remote != 0,
        blocking: state_row.blocking != 0,
        blocked_by: state_row.blocked_by != 0,
        muting: state_row.mute_notifications.is_some(),
        muting_notifications: state_row
            .mute_notifications
            .map(|value| value != 0)
            .unwrap_or(false),
        muting_expires_at: state_row.mute_expires_at,
        requested: state == "pending",
        requested_by: state_row.requested_by != 0,
        domain_blocking: false,
        endorsed: state_row.endorsed.map(|value| value != 0).unwrap_or(false),
        note: state_row.note.unwrap_or_default(),
    })
}

async fn load_relationship_state(
    db: &D1Database,
    viewer: &LocalAccount,
    target_id: &str,
    target_actor_uri: &str,
    viewer_actor_uri: &str,
) -> Result<RelationshipStateRow> {
    let is_remote_target = i32::from(target_id.starts_with("r_"));
    let bindings = [
        D1Type::Text(viewer.id.as_str()),
        D1Type::Text(viewer_actor_uri),
        D1Type::Text(target_id),
        D1Type::Text(target_actor_uri),
        D1Type::Integer(is_remote_target),
    ];

    db.prepare(
        "SELECT
            f.state AS follow_state,
            f.show_reblogs AS show_reblogs,
            f.notify AS notify,
            f.languages_json AS languages_json,
            EXISTS (
                SELECT 1
                FROM follows reciprocal
                WHERE ?5 = 0
                  AND reciprocal.follower_account_id = ?3
                  AND reciprocal.target_actor_uri = ?2
                  AND reciprocal.state = 'accepted'
                LIMIT 1
            ) AS reciprocal_following,
            EXISTS (
                SELECT 1
                FROM followers remote_follower
                WHERE remote_follower.account_id = ?1
                  AND remote_follower.actor_uri = ?4
                LIMIT 1
            ) AS followed_by_remote,
            EXISTS (
                SELECT 1
                FROM blocks viewer_block
                WHERE viewer_block.blocker_account_id = ?1
                  AND viewer_block.target_actor_uri = ?4
                LIMIT 1
            ) AS blocking,
            EXISTS (
                SELECT 1
                FROM blocks target_block
                WHERE ?5 = 0
                  AND target_block.blocker_account_id = ?3
                  AND target_block.target_actor_uri = ?2
                LIMIT 1
            ) AS blocked_by,
            mute.notifications AS mute_notifications,
            mute.expires_at AS mute_expires_at,
            CASE
                WHEN ?5 != 0 THEN EXISTS (
                    SELECT 1
                    FROM follow_requests remote_request
                    WHERE remote_request.account_id = ?1
                      AND remote_request.requester_actor_uri = ?4
                    LIMIT 1
                )
                ELSE EXISTS (
                    SELECT 1
                    FROM follows local_request
                    WHERE local_request.target_account_id = ?1
                      AND local_request.follower_account_id = ?3
                      AND local_request.state = 'pending'
                    LIMIT 1
                )
            END AS requested_by,
            metadata.endorsed AS endorsed,
            metadata.note AS note
         FROM (SELECT 1) seed
         LEFT JOIN follows f
           ON f.follower_account_id = ?1
          AND f.target_actor_uri = ?4
         LEFT JOIN mutes mute
           ON mute.account_id = ?1
          AND mute.target_actor_uri = ?4
          AND (mute.expires_at IS NULL OR mute.expires_at > CURRENT_TIMESTAMP)
         LEFT JOIN account_social_metadata metadata
           ON metadata.account_id = ?1
          AND metadata.target_actor_uri = ?4
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<RelationshipStateRow>(None)
    .await?
    .ok_or_else(|| Error::RustError("failed to load relationship state".to_owned()))
}

pub(crate) fn expiry_from_duration_seconds(duration: u32) -> Result<String> {
    let now = Date::new_0();
    now.set_time(now.get_time() + (duration as f64 * 1000.0));
    now.to_iso_string()
        .as_string()
        .ok_or_else(|| Error::RustError("failed to compute mute expiry timestamp".to_owned()))
}

pub(crate) async fn follow_remote_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
    request: &FollowAccountRequest,
) -> Result<RelationshipResponse> {
    let (_, payload) = build_follow_activity(config, follower, &actor.actor_uri)?;
    let follow_activity_id =
        queue_remote_actor_activity_required(db, &follower.id, &actor.actor_uri, &payload).await?;
    upsert_remote_follow(db, follower, actor, request, &follow_activity_id).await?;
    build_relationship_for_target(
        db,
        config,
        follower,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}

pub(crate) async fn unfollow_remote_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
) -> Result<RelationshipResponse> {
    if let Some(follow_activity_id) =
        load_follow_activity_id(db, &follower.id, &actor.actor_uri).await?
    {
        let payload =
            build_undo_follow_activity(config, follower, &follow_activity_id, &actor.actor_uri)?;
        let _ = queue_remote_actor_activity(db, &follower.id, &actor.actor_uri, &payload).await?;
    }

    delete_follow_by_target(db, &follower.id, &actor.actor_uri).await?;
    build_relationship_for_target(
        db,
        config,
        follower,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}
