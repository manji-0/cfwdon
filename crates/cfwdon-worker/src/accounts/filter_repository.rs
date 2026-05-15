use crate::{
    AppConfig, D1Database, LocalAccount, Result, actor_url, build_relationship_for_target,
    delete_block_by_target, delete_mute_by_target, remote_account_rest_id, upsert_block,
    upsert_mute,
};

pub(crate) async fn block_local_account(
    db: &D1Database,
    config: &AppConfig,
    blocker: &LocalAccount,
    target: &LocalAccount,
) -> Result<crate::RelationshipResponse> {
    let target_actor_uri = actor_url(config, &target.username);
    upsert_block(db, &blocker.id, Some(target.id.as_str()), &target_actor_uri).await?;
    build_relationship_for_target(db, config, blocker, &target.id, &target_actor_uri).await
}

pub(crate) async fn block_remote_account(
    db: &D1Database,
    config: &AppConfig,
    blocker: &LocalAccount,
    actor: &crate::RemoteActorRow,
) -> Result<crate::RelationshipResponse> {
    upsert_block(db, &blocker.id, None, &actor.actor_uri).await?;
    build_relationship_for_target(
        db,
        config,
        blocker,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}

pub(crate) async fn unblock_local_account(
    db: &D1Database,
    config: &AppConfig,
    blocker: &LocalAccount,
    target: &LocalAccount,
) -> Result<crate::RelationshipResponse> {
    let target_actor_uri = actor_url(config, &target.username);
    delete_block_by_target(db, &blocker.id, &target_actor_uri).await?;
    build_relationship_for_target(db, config, blocker, &target.id, &target_actor_uri).await
}

pub(crate) async fn unblock_remote_account(
    db: &D1Database,
    config: &AppConfig,
    blocker: &LocalAccount,
    actor: &crate::RemoteActorRow,
) -> Result<crate::RelationshipResponse> {
    delete_block_by_target(db, &blocker.id, &actor.actor_uri).await?;
    build_relationship_for_target(
        db,
        config,
        blocker,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}

pub(crate) async fn mute_local_account(
    db: &D1Database,
    config: &AppConfig,
    muter: &LocalAccount,
    target: &LocalAccount,
    notifications: bool,
    expires_at: Option<&str>,
) -> Result<crate::RelationshipResponse> {
    let target_actor_uri = actor_url(config, &target.username);
    upsert_mute(
        db,
        &muter.id,
        Some(target.id.as_str()),
        &target_actor_uri,
        notifications,
        expires_at,
    )
    .await?;
    build_relationship_for_target(db, config, muter, &target.id, &target_actor_uri).await
}

pub(crate) async fn mute_remote_account(
    db: &D1Database,
    config: &AppConfig,
    muter: &LocalAccount,
    actor: &crate::RemoteActorRow,
    notifications: bool,
    expires_at: Option<&str>,
) -> Result<crate::RelationshipResponse> {
    upsert_mute(
        db,
        &muter.id,
        None,
        &actor.actor_uri,
        notifications,
        expires_at,
    )
    .await?;
    build_relationship_for_target(
        db,
        config,
        muter,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}

pub(crate) async fn unmute_local_account(
    db: &D1Database,
    config: &AppConfig,
    muter: &LocalAccount,
    target: &LocalAccount,
) -> Result<crate::RelationshipResponse> {
    let target_actor_uri = actor_url(config, &target.username);
    delete_mute_by_target(db, &muter.id, &target_actor_uri).await?;
    build_relationship_for_target(db, config, muter, &target.id, &target_actor_uri).await
}

pub(crate) async fn unmute_remote_account(
    db: &D1Database,
    config: &AppConfig,
    muter: &LocalAccount,
    actor: &crate::RemoteActorRow,
) -> Result<crate::RelationshipResponse> {
    delete_mute_by_target(db, &muter.id, &actor.actor_uri).await?;
    build_relationship_for_target(
        db,
        config,
        muter,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}
