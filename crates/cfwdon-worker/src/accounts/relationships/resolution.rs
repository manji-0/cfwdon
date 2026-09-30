use crate::auth::find_account_by_username;
use crate::federation::{ensure_remote_actor_username_matches_handle, resolve_webfinger_actor_uri};
use crate::identity::parse_lookup_handle;
use crate::observability::log_json_event;
use crate::remote::{AccountReference, resolve_account_reference_with_fetch};
use crate::response::{RemoteCollectionFetchContext, fetch_remote_actor_profile_with_context};
use crate::store::remote::{
    find_remote_actor_by_actor_uri, find_remote_actor_by_username_domain, upsert_remote_actor,
};
use crate::tracked_d1::D1Database;
use worker::Result;

pub(crate) async fn resolve_requested_account_reference(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Option<AccountReference>> {
    let fetch_context = RemoteCollectionFetchContext::public(config, db, None);
    if let Some(reference) =
        resolve_account_reference_with_fetch(db, account_id, Some(&fetch_context)).await?
    {
        return Ok(Some(reference));
    }

    let handle = match parse_lookup_handle(account_id, config) {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };

    if handle.is_local_to(&config.instance_domain) {
        return Ok(find_account_by_username(db, &handle.username)
            .await?
            .map(AccountReference::Local));
    }

    let Some(domain) = handle.domain.as_deref() else {
        return Ok(None);
    };
    if let Some(actor) = find_remote_actor_by_username_domain(db, &handle.username, domain).await? {
        return Ok(Some(AccountReference::Remote(actor)));
    }

    let actor_uri = match resolve_webfinger_actor_uri(&handle).await {
        Ok(actor_uri) => actor_uri,
        Err(error) => {
            log_json_event(serde_json::json!({
                "event": "remote_account_reference_fetch_failed",
                "acct": format!("{}@{}", handle.username, domain),
                "error": error.to_string(),
            }));
            return Ok(None);
        }
    };
    let fetched =
        match fetch_remote_actor_profile_with_context(&actor_uri, Some(&fetch_context)).await {
            Ok(fetched) => fetched,
            Err(error) => {
                log_json_event(serde_json::json!({
                    "event": "remote_account_reference_fetch_failed",
                    "actor_uri": actor_uri,
                    "error": error.to_string(),
                }));
                return Ok(None);
            }
        };
    if ensure_remote_actor_username_matches_handle(&fetched.profile, &handle.username).is_err() {
        log_json_event(serde_json::json!({
            "event": "remote_account_reference_fetch_failed",
            "actor_uri": actor_uri,
            "error": "preferredUsername did not match looked-up handle",
        }));
        return Ok(None);
    }
    upsert_remote_actor(db, &fetched.profile).await?;
    Ok(
        find_remote_actor_by_actor_uri(db, &fetched.profile.actor_uri)
            .await?
            .map(AccountReference::Remote),
    )
}
