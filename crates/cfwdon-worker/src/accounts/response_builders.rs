use crate::{
    LocalAccount, MastodonAccountResponse, RemoteActorProfile, RemoteActorRow,
    RemoteCollectionFetchContext, Result, config_with_resolved_custom_emojis,
    enrich_remote_account_response, fetch_remote_actor_profile_with_context, find_account_by_id,
    find_account_by_username, find_remote_actor_by_actor_uri, load_account_stats,
    local_username_from_actor_uri, log_json_event, upsert_remote_actor,
};

use crate::D1Database;
pub(crate) async fn build_local_account_response(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account: &LocalAccount,
) -> Result<MastodonAccountResponse> {
    let stats = load_account_stats(db, account.id()).await?;
    let config = config_with_resolved_custom_emojis(db, config).await?;
    Ok(MastodonAccountResponse::from_account_with_stats(
        account, &config, &stats,
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

pub(crate) async fn upserted_remote_actor_response_with_document(
    db: &D1Database,
    profile: &RemoteActorProfile,
    document: &serde_json::Value,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<MastodonAccountResponse> {
    upsert_remote_actor(db, profile).await?;
    let cached = find_remote_actor_by_actor_uri(db, &profile.actor_uri).await?;
    let social_counts_updated_at = cached
        .as_ref()
        .and_then(|row| row.social_counts_updated_at.clone());
    let mut response = match cached {
        Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
        None => MastodonAccountResponse::from_remote_actor_profile(profile),
    };
    enrich_remote_account_response(
        db,
        &profile.actor_uri,
        social_counts_updated_at.as_deref(),
        &mut response,
        document,
        fetch_context,
    )
    .await?;
    Ok(response)
}

pub(crate) async fn refreshed_remote_actor_response(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    actor: &RemoteActorRow,
    viewer: Option<&LocalAccount>,
) -> Result<MastodonAccountResponse> {
    let fetch_context = RemoteCollectionFetchContext::public(config, db, viewer);
    match fetch_remote_actor_profile_with_context(&actor.actor_uri, Some(&fetch_context)).await {
        Ok(fetched) => {
            upserted_remote_actor_response_with_document(
                db,
                &fetched.profile,
                &fetched.document,
                Some(&fetch_context),
            )
            .await
        }
        Err(error) => {
            log_json_event(serde_json::json!({
                "event": "remote_actor_refresh_failed",
                "actor_uri": actor.actor_uri,
                "error": error.to_string(),
            }));
            Ok(MastodonAccountResponse::from_remote_actor(actor))
        }
    }
}
