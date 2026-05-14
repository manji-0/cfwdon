use super::{
    LocalAccount, RemoteActorRow, RemoteStatusRow, StatusRow, find_account_by_id,
    find_local_status_by_object_uri, find_remote_actor_by_actor_uri, find_remote_status_by_id,
    find_remote_status_by_url_or_object_uri, find_status_by_id, resolve_remote_status_by_url,
};
use serde::Deserialize;
use worker::{D1Database, Error, Result};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StatusActionQuery {
    pub(crate) uri: Option<String>,
}

pub(crate) enum ResolvedActionStatus {
    Local(StatusRow, LocalAccount),
    Remote(RemoteStatusRow, RemoteActorRow),
}

pub(crate) fn normalized_action_uri(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }

    Some(
        urlencoding::decode(value)
            .map(|decoded| decoded.into_owned())
            .unwrap_or_else(|_| value.to_owned()),
    )
}

pub(crate) async fn resolve_action_status(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    status_id: &str,
    action_uri: Option<&str>,
) -> Result<Option<ResolvedActionStatus>> {
    if action_uri.is_some() && normalized_action_uri(action_uri).is_none() {
        return Err(Error::RustError(
            "uri query parameter must not be empty".to_owned(),
        ));
    }

    if let Some(action_uri) = normalized_action_uri(action_uri) {
        return resolve_action_uri_reference(db, config, &action_uri).await;
    }

    if let Some(status) = find_status_by_id(db, status_id).await?
        && let Some(account) = find_account_by_id(db, &status.account_id).await?
    {
        return Ok(Some(ResolvedActionStatus::Local(status, account)));
    }

    if let Some(status) = find_remote_status_by_id(db, status_id).await?
        && let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await?
    {
        return Ok(Some(ResolvedActionStatus::Remote(status, actor)));
    }

    let decoded_status_id = urlencoding::decode(status_id)
        .map(|decoded| decoded.into_owned())
        .unwrap_or_else(|_| status_id.to_owned());
    resolve_action_uri_reference(db, config, &decoded_status_id).await
}

async fn resolve_action_uri_reference(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    value: &str,
) -> Result<Option<ResolvedActionStatus>> {
    if let Some(status) = find_local_status_by_object_uri(db, config, value).await?
        && let Some(account) = find_account_by_id(db, &status.account_id).await?
    {
        return Ok(Some(ResolvedActionStatus::Local(status, account)));
    }

    if let Some(status) = find_remote_status_by_url_or_object_uri(db, value).await?
        && let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await?
    {
        return Ok(Some(ResolvedActionStatus::Remote(status, actor)));
    }

    if let Some((status, actor)) = resolve_remote_status_by_url(db, config, value).await? {
        return Ok(Some(ResolvedActionStatus::Remote(status, actor)));
    }

    Ok(None)
}
