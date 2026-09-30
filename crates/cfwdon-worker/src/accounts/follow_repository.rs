use super::{FollowAccountRequest, upsert_local_follow};
use crate::identity::actor_url;
use crate::relationships::{
    RelationshipResponse, build_relationship_for_target, follow_remote_account,
    unfollow_remote_account,
};
use crate::store::relationship::delete_follow_by_target;
use crate::store::remote::RemoteActorRow;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::{Env, Result};

pub(crate) async fn follow_local_account(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    follower: &LocalAccount,
    target: &LocalAccount,
    request: &FollowAccountRequest,
) -> Result<RelationshipResponse> {
    upsert_local_follow(db, config, env, follower, target, request).await?;
    build_relationship_for_target(
        db,
        config,
        follower,
        target.id(),
        &actor_url(config, target.username()),
    )
    .await
}

pub(crate) async fn unfollow_local_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    target: &LocalAccount,
) -> Result<RelationshipResponse> {
    let target_actor_uri = actor_url(config, target.username());
    delete_follow_by_target(db, follower.id(), &target_actor_uri).await?;
    build_relationship_for_target(db, config, follower, target.id(), &target_actor_uri).await
}

pub(crate) async fn follow_remote_account_relationship(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
    request: &FollowAccountRequest,
) -> Result<RelationshipResponse> {
    follow_remote_account(db, config, follower, actor, request).await
}

pub(crate) async fn unfollow_remote_account_relationship(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
) -> Result<RelationshipResponse> {
    unfollow_remote_account(db, config, follower, actor).await
}
