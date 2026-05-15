use super::{
    block_local_account, block_remote_account, mute_local_account, mute_remote_account,
    unblock_local_account, unblock_remote_account, unmute_local_account, unmute_remote_account,
};
use crate::{AccountReference, AppConfig, D1Database, LocalAccount, resolve_account_reference};
use worker::Error;

pub(crate) enum FilterActionError {
    NotFound,
    CannotTargetSelf,
    Worker(Error),
}

impl From<Error> for FilterActionError {
    fn from(error: Error) -> Self {
        Self::Worker(error)
    }
}

pub(crate) async fn block_account_usecase(
    db: &D1Database,
    config: &AppConfig,
    blocker: &LocalAccount,
    target_account_id: &str,
) -> std::result::Result<crate::RelationshipResponse, FilterActionError> {
    match resolve_account_reference(db, target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if blocker.id == target.id {
                return Err(FilterActionError::CannotTargetSelf);
            }
            block_local_account(db, config, blocker, &target)
                .await
                .map_err(FilterActionError::from)
        }
        Some(AccountReference::Remote(actor)) => block_remote_account(db, config, blocker, &actor)
            .await
            .map_err(FilterActionError::from),
        None => Err(FilterActionError::NotFound),
    }
}

pub(crate) async fn unblock_account_usecase(
    db: &D1Database,
    config: &AppConfig,
    blocker: &LocalAccount,
    target_account_id: &str,
) -> std::result::Result<crate::RelationshipResponse, FilterActionError> {
    match resolve_account_reference(db, target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            unblock_local_account(db, config, blocker, &target)
                .await
                .map_err(FilterActionError::from)
        }
        Some(AccountReference::Remote(actor)) => {
            unblock_remote_account(db, config, blocker, &actor)
                .await
                .map_err(FilterActionError::from)
        }
        None => Err(FilterActionError::NotFound),
    }
}

pub(crate) async fn mute_account_usecase(
    db: &D1Database,
    config: &AppConfig,
    muter: &LocalAccount,
    target_account_id: &str,
    notifications: bool,
    expires_at: Option<&str>,
) -> std::result::Result<crate::RelationshipResponse, FilterActionError> {
    match resolve_account_reference(db, target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if muter.id == target.id {
                return Err(FilterActionError::CannotTargetSelf);
            }
            mute_local_account(db, config, muter, &target, notifications, expires_at)
                .await
                .map_err(FilterActionError::from)
        }
        Some(AccountReference::Remote(actor)) => {
            mute_remote_account(db, config, muter, &actor, notifications, expires_at)
                .await
                .map_err(FilterActionError::from)
        }
        None => Err(FilterActionError::NotFound),
    }
}

pub(crate) async fn unmute_account_usecase(
    db: &D1Database,
    config: &AppConfig,
    muter: &LocalAccount,
    target_account_id: &str,
) -> std::result::Result<crate::RelationshipResponse, FilterActionError> {
    match resolve_account_reference(db, target_account_id).await? {
        Some(AccountReference::Local(target)) => unmute_local_account(db, config, muter, &target)
            .await
            .map_err(FilterActionError::from),
        Some(AccountReference::Remote(actor)) => unmute_remote_account(db, config, muter, &actor)
            .await
            .map_err(FilterActionError::from),
        None => Err(FilterActionError::NotFound),
    }
}
