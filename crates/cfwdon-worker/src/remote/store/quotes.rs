use super::bindings::remote_status_quote_state_update_bindings;
use super::lookups::find_remote_status_by_id;
use super::records::RemoteStatusRow;
use crate::{
    AppConfig, D1Database, RemoteActorProfile, count_followers_by_actor, find_account_by_id,
    find_local_status_by_object_uri,
};
use cfwdon_domain::{QuoteState, RemoteQuoteLocalTarget, RemoteQuoteResolution};
use worker::{Error, Result};

pub(super) async fn resolve_remote_quote_resolution(
    db: &D1Database,
    config: &AppConfig,
    actor: &RemoteActorProfile,
    quote_of_uri: Option<&str>,
) -> Result<RemoteQuoteResolution> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(RemoteQuoteResolution::without_quote());
    };
    let Some(status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? else {
        return Ok(RemoteQuoteResolution::accepted_quote(
            quote_of_uri.to_owned(),
        ));
    };
    let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(RemoteQuoteResolution::accepted_quote(
            quote_of_uri.to_owned(),
        ));
    };
    let remote_actor_follows_owner =
        count_followers_by_actor(db, owner.id(), &actor.actor_uri).await? > 0;
    let blocked_by_owner = crate::is_blocking_actor(db, owner.id(), &actor.actor_uri).await?;
    let policy = status.effective_quote_approval_policy();
    Ok(RemoteQuoteResolution::with_local_target(
        quote_of_uri.to_owned(),
        RemoteQuoteLocalTarget {
            blocked_by_owner,
            policy_allows: policy.allows_quote(false, remote_actor_follows_owner),
        },
    ))
}

pub(crate) async fn clear_remote_status_quote(
    db: &D1Database,
    status: &RemoteStatusRow,
) -> Result<RemoteStatusRow> {
    update_remote_status_quote_state(
        db,
        &status.id,
        QuoteState::quote_state_after_revoke(status.quote_state),
    )
    .await
}

pub(crate) async fn update_remote_status_quote_state(
    db: &D1Database,
    status_id: &str,
    quote_state: QuoteState,
) -> Result<RemoteStatusRow> {
    let bindings = remote_status_quote_state_update_bindings(quote_state.as_str(), status_id);
    db.prepare(
        "UPDATE remote_statuses
         SET quote_state = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    find_remote_status_by_id(db, status_id)
        .await?
        .ok_or_else(|| Error::RustError("remote status not found".to_owned()))
}
