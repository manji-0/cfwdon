use crate::{
    LocalAccount, MastodonAccountResponse, RemoteActorProfile, RemoteActorRow, Result,
    fetch_remote_actor_profile, find_account_by_id, find_account_by_username,
    find_remote_actor_by_actor_uri, load_account_stats, local_username_from_actor_uri,
    upsert_remote_actor,
};
use worker::D1Database;

pub(crate) async fn build_local_account_response(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account: &LocalAccount,
) -> Result<MastodonAccountResponse> {
    let stats = load_account_stats(db, account.id()).await?;
    Ok(MastodonAccountResponse::from_account_with_stats(
        account, config, &stats,
    ))
}

pub(crate) async fn find_local_account_response(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let Some(account) = find_account_by_id(db, account_id).await? else {
        return Ok(None);
    };
    Ok(Some(
        build_local_account_response(db, config, &account).await?,
    ))
}

pub(crate) async fn find_local_account_response_by_actor_uri(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let Some(username) = local_username_from_actor_uri(config, actor_uri) else {
        return Ok(None);
    };
    let Some(account) = find_account_by_username(db, &username).await? else {
        return Ok(None);
    };
    Ok(Some(
        build_local_account_response(db, config, &account).await?,
    ))
}

pub(crate) async fn upserted_remote_actor_response(
    db: &D1Database,
    profile: &RemoteActorProfile,
) -> Result<MastodonAccountResponse> {
    upsert_remote_actor(db, profile).await?;
    Ok(
        match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
            Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
            None => MastodonAccountResponse::from_remote_actor_profile(profile),
        },
    )
}

pub(crate) async fn refreshed_remote_actor_response(
    db: &D1Database,
    actor: &RemoteActorRow,
) -> Result<MastodonAccountResponse> {
    match fetch_remote_actor_profile(&actor.actor_uri).await {
        Ok(profile) => upserted_remote_actor_response(db, &profile).await,
        Err(_) => Ok(MastodonAccountResponse::from_remote_actor(actor)),
    }
}
