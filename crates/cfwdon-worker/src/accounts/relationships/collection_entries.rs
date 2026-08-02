use super::collections::CollectionAccountEntry;
use super::remote_collections::remote_follow_collection_entries;
use crate::{
    LocalAccount, MastodonAccountResponse, Result, fetch_remote_actor_profile,
    find_local_account_response, find_remote_actor_by_actor_uri, list_local_followers_for_account,
    list_local_followers_for_remote_actor, list_local_following_for_account,
    list_local_following_for_remote_actor, list_remote_followers_for_account,
    list_remote_following_for_account, upserted_remote_actor_response,
};

async fn remote_follow_account_response(
    db: &crate::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    match fetch_remote_actor_profile(actor_uri).await {
        Ok(profile) => Ok(Some(upserted_remote_actor_response(db, &profile).await?)),
        Err(_) => Ok(find_remote_actor_by_actor_uri(db, actor_uri)
            .await?
            .map(|actor| MastodonAccountResponse::from_remote_actor(&actor))),
    }
}

async fn local_follow_account_entry(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    cursor_id: i64,
    created_at: &str,
    account_id: &str,
) -> Result<Option<CollectionAccountEntry>> {
    let Some(account) = find_local_account_response(db, config, account_id).await? else {
        return Ok(None);
    };
    Ok(Some(CollectionAccountEntry {
        cursor_id,
        created_at: created_at.to_owned(),
        account,
    }))
}

async fn build_local_follow_entries<T, I, FCursorId, FCreatedAt, FAccountId>(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    entries: I,
    cursor_id: FCursorId,
    created_at: FCreatedAt,
    account_id: FAccountId,
) -> Result<Vec<CollectionAccountEntry>>
where
    I: IntoIterator<Item = T>,
    FCursorId: Fn(&T) -> i64,
    FCreatedAt: Fn(&T) -> &str,
    FAccountId: Fn(&T) -> &str,
{
    let mut collected = Vec::new();
    for entry in entries {
        if let Some(account_entry) = local_follow_account_entry(
            db,
            config,
            cursor_id(&entry),
            created_at(&entry),
            account_id(&entry),
        )
        .await?
        {
            collected.push(account_entry);
        }
    }
    Ok(collected)
}

async fn append_remote_follow_entries<T, I, FCursorId, FCreatedAt, FActorUri>(
    entries: &mut Vec<CollectionAccountEntry>,
    db: &crate::D1Database,
    records: I,
    cursor_id: FCursorId,
    created_at: FCreatedAt,
    actor_uri: FActorUri,
) -> Result<()>
where
    I: IntoIterator<Item = T>,
    FCursorId: Fn(&T) -> i64,
    FCreatedAt: Fn(&T) -> &str,
    FActorUri: Fn(&T) -> &str,
{
    for record in records {
        if let Some(account) = remote_follow_account_response(db, actor_uri(&record)).await? {
            entries.push(CollectionAccountEntry {
                cursor_id: cursor_id(&record),
                created_at: created_at(&record).to_owned(),
                account,
            });
        }
    }
    Ok(())
}

pub(crate) async fn local_account_follower_entries(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Vec<CollectionAccountEntry>> {
    let mut entries = build_local_follow_entries(
        db,
        config,
        list_local_followers_for_account(db, account_id).await?,
        |entry| entry.cursor_id,
        |entry| &entry.created_at,
        |entry| &entry.account_id,
    )
    .await?;
    append_remote_follow_entries(
        &mut entries,
        db,
        list_remote_followers_for_account(db, account_id).await?,
        |entry| entry.cursor_id,
        |entry| &entry.created_at,
        |entry| &entry.actor_uri,
    )
    .await?;
    Ok(entries)
}

pub(crate) async fn remote_actor_follower_entries(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    actor_uri: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<CollectionAccountEntry>> {
    if let Some(remote_entries) = remote_follow_collection_entries(
        db,
        config,
        viewer,
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

    build_local_follow_entries(
        db,
        config,
        list_local_followers_for_remote_actor(db, actor_uri).await?,
        |entry| entry.cursor_id,
        |entry| &entry.created_at,
        |entry| &entry.account_id,
    )
    .await
}

pub(crate) async fn local_account_following_entries(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
) -> Result<Vec<CollectionAccountEntry>> {
    let mut entries = build_local_follow_entries(
        db,
        config,
        list_local_following_for_account(db, account_id).await?,
        |entry| entry.cursor_id,
        |entry| &entry.created_at,
        |entry| &entry.account_id,
    )
    .await?;
    append_remote_follow_entries(
        &mut entries,
        db,
        list_remote_following_for_account(db, account_id).await?,
        |entry| entry.cursor_id,
        |entry| &entry.created_at,
        |entry| &entry.actor_uri,
    )
    .await?;
    Ok(entries)
}

pub(crate) async fn remote_actor_following_entries(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&LocalAccount>,
    actor_uri: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<CollectionAccountEntry>> {
    if let Some(remote_entries) = remote_follow_collection_entries(
        db,
        config,
        viewer,
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

    build_local_follow_entries(
        db,
        config,
        list_local_following_for_remote_actor(db, actor_uri).await?,
        |entry| entry.cursor_id,
        |entry| &entry.created_at,
        |entry| &entry.account_id,
    )
    .await
}
