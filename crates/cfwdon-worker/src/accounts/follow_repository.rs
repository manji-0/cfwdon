use super::{FollowAccountRequest, upsert_local_follow};
use crate::{
    AppConfig, D1Database, LocalAccount, Result, actor_url, build_relationship_for_target,
    delete_follow_by_target, follow_remote_account, unfollow_remote_account,
};

pub(crate) async fn follow_local_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    target: &LocalAccount,
    request: &FollowAccountRequest,
) -> Result<crate::RelationshipResponse> {
    upsert_local_follow(db, config, follower, target, request).await?;
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
) -> Result<crate::RelationshipResponse> {
    let target_actor_uri = actor_url(config, target.username());
    delete_follow_by_target(db, follower.id(), &target_actor_uri).await?;
    build_relationship_for_target(db, config, follower, target.id(), &target_actor_uri).await
}

pub(crate) async fn follow_remote_account_relationship(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &crate::RemoteActorRow,
    request: &FollowAccountRequest,
) -> Result<crate::RelationshipResponse> {
    follow_remote_account(db, config, follower, actor, request).await
}

pub(crate) async fn unfollow_remote_account_relationship(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &crate::RemoteActorRow,
) -> Result<crate::RelationshipResponse> {
    unfollow_remote_account(db, config, follower, actor).await
}
