use super::collections::CollectionAccountEntry;
use super::remote_collections::remote_follow_collection_entries;
use crate::{
    MastodonAccountResponse, Result, fetch_remote_actor_profile, find_account_by_id,
    find_remote_actor_by_actor_uri, list_local_followers_for_account,
    list_local_followers_for_remote_actor, list_local_following_for_account,
    list_local_following_for_remote_actor, list_remote_followers_for_account,
    list_remote_following_for_account, load_account_stats, upsert_remote_actor,
};

async fn remote_follow_account_response(
    db: &worker::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    match fetch_remote_actor_profile(actor_uri).await {
        Ok(profile) => {
            upsert_remote_actor(db, &profile).await?;
            match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
                Some(actor) => Ok(Some(MastodonAccountResponse::from_remote_actor(&actor))),
                None => Ok(Some(MastodonAccountResponse::from_remote_actor_profile(
                    &profile,
                ))),
            }
        }
        Err(_) => Ok(find_remote_actor_by_actor_uri(db, actor_uri)
            .await?
            .map(|actor| MastodonAccountResponse::from_remote_actor(&actor))),
    }
}

pub(crate) async fn local_account_follower_entries(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Vec<CollectionAccountEntry>> {
    let mut entries = Vec::new();
    for follower in list_local_followers_for_account(db, account_id).await? {
        if let Some(author) = find_account_by_id(db, &follower.account_id).await? {
            let stats = load_account_stats(db, &author.id).await?;
            entries.push(CollectionAccountEntry {
                cursor_id: follower.cursor_id,
                created_at: follower.created_at,
                account: MastodonAccountResponse::from_account_with_stats(&author, config, &stats),
            });
        }
    }
    for follower in list_remote_followers_for_account(db, account_id).await? {
        if let Some(account) = remote_follow_account_response(db, &follower.actor_uri).await? {
            entries.push(CollectionAccountEntry {
                cursor_id: follower.cursor_id,
                created_at: follower.created_at,
                account,
            });
        }
    }
    Ok(entries)
}

pub(crate) async fn remote_actor_follower_entries(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<CollectionAccountEntry>> {
    if let Some(remote_entries) = remote_follow_collection_entries(
        db,
        config,
        actor_uri,
        "followers",
        limit,
        max_id,
        since_id,
    )
    .await?
    {
        return Ok(remote_entries);
    }

    let mut entries = Vec::new();
    for follower in list_local_followers_for_remote_actor(db, actor_uri).await? {
        if let Some(account) = find_account_by_id(db, &follower.account_id).await? {
            let stats = load_account_stats(db, &account.id).await?;
            entries.push(CollectionAccountEntry {
                cursor_id: follower.cursor_id,
                created_at: follower.created_at,
                account: MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
            });
        }
    }
    Ok(entries)
}

pub(crate) async fn local_account_following_entries(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Vec<CollectionAccountEntry>> {
    let mut entries = Vec::new();
    for followed in list_local_following_for_account(db, account_id).await? {
        if let Some(target) = find_account_by_id(db, &followed.account_id).await? {
            let stats = load_account_stats(db, &target.id).await?;
            entries.push(CollectionAccountEntry {
                cursor_id: followed.cursor_id,
                created_at: followed.created_at,
                account: MastodonAccountResponse::from_account_with_stats(&target, config, &stats),
            });
        }
    }
    for followed in list_remote_following_for_account(db, account_id).await? {
        if let Some(account) = remote_follow_account_response(db, &followed.actor_uri).await? {
            entries.push(CollectionAccountEntry {
                cursor_id: followed.cursor_id,
                created_at: followed.created_at,
                account,
            });
        }
    }
    Ok(entries)
}

pub(crate) async fn remote_actor_following_entries(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<CollectionAccountEntry>> {
    if let Some(remote_entries) = remote_follow_collection_entries(
        db,
        config,
        actor_uri,
        "following",
        limit,
        max_id,
        since_id,
    )
    .await?
    {
        return Ok(remote_entries);
    }

    let mut entries = Vec::new();
    for followed in list_local_following_for_remote_actor(db, actor_uri).await? {
        if let Some(account) = find_account_by_id(db, &followed.account_id).await? {
            let stats = load_account_stats(db, &account.id).await?;
            entries.push(CollectionAccountEntry {
                cursor_id: followed.cursor_id,
                created_at: followed.created_at,
                account: MastodonAccountResponse::from_account_with_stats(&account, config, &stats),
            });
        }
    }
    Ok(entries)
}
