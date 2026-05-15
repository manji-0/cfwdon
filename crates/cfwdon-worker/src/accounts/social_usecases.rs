use super::{
    set_relationship_email_subscription, set_relationship_endorsement, set_relationship_note,
};
use crate::{
    AppConfig, D1Database, LocalAccount, build_relationship_for_target, find_follow_by_target,
};
use worker::Error;

pub(crate) struct ResolvedRelationshipTarget<'a> {
    pub(crate) viewer: &'a LocalAccount,
    pub(crate) target_account_id: Option<&'a str>,
    pub(crate) target_id: &'a str,
    pub(crate) target_actor_uri: &'a str,
}

pub(crate) enum SocialActionError {
    FollowRequired,
    Worker(Error),
}

impl From<Error> for SocialActionError {
    fn from(error: Error) -> Self {
        Self::Worker(error)
    }
}

pub(crate) async fn endorse_relationship_target(
    db: &D1Database,
    config: &AppConfig,
    target: ResolvedRelationshipTarget<'_>,
    endorsed: bool,
) -> std::result::Result<crate::RelationshipResponse, SocialActionError> {
    let Some(follow) =
        find_follow_by_target(db, &target.viewer.id, target.target_actor_uri).await?
    else {
        return Err(SocialActionError::FollowRequired);
    };
    if follow.state != "accepted" {
        return Err(SocialActionError::FollowRequired);
    }

    set_relationship_endorsement(
        db,
        &target.viewer.id,
        target.target_account_id,
        target.target_actor_uri,
        endorsed,
    )
    .await?;
    build_relationship_for_target(
        db,
        config,
        target.viewer,
        target.target_id,
        target.target_actor_uri,
    )
    .await
    .map_err(SocialActionError::from)
}

pub(crate) async fn note_relationship_target(
    db: &D1Database,
    config: &AppConfig,
    target: ResolvedRelationshipTarget<'_>,
    note: &str,
) -> std::result::Result<crate::RelationshipResponse, SocialActionError> {
    set_relationship_note(
        db,
        &target.viewer.id,
        target.target_account_id,
        target.target_actor_uri,
        note,
    )
    .await?;
    build_relationship_for_target(
        db,
        config,
        target.viewer,
        target.target_id,
        target.target_actor_uri,
    )
    .await
    .map_err(SocialActionError::from)
}

pub(crate) async fn set_relationship_email_subscription_usecase(
    db: &D1Database,
    target: ResolvedRelationshipTarget<'_>,
    enabled: bool,
) -> std::result::Result<serde_json::Value, SocialActionError> {
    set_relationship_email_subscription(
        db,
        &target.viewer.id,
        target.target_account_id,
        target.target_actor_uri,
        enabled,
    )
    .await?;
    Ok(serde_json::json!({
        "id": target.target_id,
        "email_notifications": enabled,
    }))
}
