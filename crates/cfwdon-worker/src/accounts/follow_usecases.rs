use super::{
    follow_local_account, follow_remote_account_relationship, unfollow_local_account,
    unfollow_remote_account_relationship,
};
use crate::{
    AccountReference, AppConfig, D1Database, FollowAccountRequest, LocalAccount,
    resolve_account_reference,
};
use worker::Error;

pub(crate) enum FollowActionError {
    NotFound,
    CannotFollowSelf,
    Worker(Error),
}

impl From<Error> for FollowActionError {
    fn from(error: Error) -> Self {
        Self::Worker(error)
    }
}

pub(crate) async fn follow_account_usecase(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    target_account_id: &str,
    request: &FollowAccountRequest,
) -> std::result::Result<crate::RelationshipResponse, FollowActionError> {
    match resolve_account_reference(db, target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if follower.id() == target.id() {
                return Err(FollowActionError::CannotFollowSelf);
            }
            follow_local_account(db, config, follower, &target, request)
                .await
                .map_err(FollowActionError::from)
        }
        Some(AccountReference::Remote(actor)) => {
            follow_remote_account_relationship(db, config, follower, &actor, request)
                .await
                .map_err(FollowActionError::from)
        }
        None => Err(FollowActionError::NotFound),
    }
}

pub(crate) async fn unfollow_account_usecase(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    target_account_id: &str,
) -> std::result::Result<crate::RelationshipResponse, FollowActionError> {
    match resolve_account_reference(db, target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            unfollow_local_account(db, config, follower, &target)
                .await
                .map_err(FollowActionError::from)
        }
        Some(AccountReference::Remote(actor)) => {
            unfollow_remote_account_relationship(db, config, follower, &actor)
                .await
                .map_err(FollowActionError::from)
        }
        None => Err(FollowActionError::NotFound),
    }
}
