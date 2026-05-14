use crate::AccountReference;
use crate::{
    Result, find_account_by_username, find_remote_actor_by_username_domain, parse_lookup_handle,
    resolve_account_reference,
};

pub(crate) async fn resolve_requested_account_reference(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Option<AccountReference>> {
    if let Some(reference) = resolve_account_reference(db, account_id).await? {
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
    Ok(
        find_remote_actor_by_username_domain(db, &handle.username, domain)
            .await?
            .map(AccountReference::Remote),
    )
}
