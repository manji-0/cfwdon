use super::collections::CollectionAccountEntry;
use super::remote_collections::remote_follow_collection_entries;
use crate::accounts::{
    find_accounts_by_ids, load_account_stats_map, upserted_remote_actor_response,
};
use crate::custom_emojis::config_with_resolved_custom_emojis;
use crate::federation::fetch_remote_actor_profile;
use crate::relationship::{
    list_local_followers_for_account, list_local_followers_for_remote_actor,
    list_local_following_for_account, list_local_following_for_remote_actor,
    list_remote_followers_for_account, list_remote_following_for_account,
};
use crate::responses::MastodonAccountResponse;
use crate::store::remote::find_remote_actors_by_actor_uris;
use crate::tracked_d1::D1Database;
use cfwdon_domain::LocalAccount;
use std::collections::HashMap;
use worker::Result;

const FOLLOW_PAGE_LOAD_MARGIN: u32 = 8;

/// A follow row before its account is loaded.
struct FollowRow {
    cursor_id: i64,
    created_at: String,
    account: FollowAccountRef,
}

enum FollowAccountRef {
    Local(String),
    Remote(String),
}

/// Rows a few past what a page shows, so accounts that fail to load do not
/// shorten it, in the order `finalize_collection_response` sorts them; that
/// cuts the page and tells whether another follows.
fn page_follow_rows(
    mut rows: Vec<FollowRow>,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Vec<FollowRow> {
    rows.retain(|row| max_id.is_none_or(|value| row.cursor_id < value));
    rows.retain(|row| since_id.is_none_or(|value| row.cursor_id > value));
    rows.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.cursor_id.cmp(&left.cursor_id))
    });
    rows.truncate(limit.saturating_add(1 + FOLLOW_PAGE_LOAD_MARGIN) as usize);
    rows
}

/// Loads every row's account in batches. Remote accounts come from stored
/// actor rows; only actors never stored are fetched.
async fn hydrate_follow_rows(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    rows: Vec<FollowRow>,
) -> Result<Vec<CollectionAccountEntry>> {
    let mut local_ids = Vec::new();
    let mut remote_uris = Vec::new();
    for row in &rows {
        match &row.account {
            FollowAccountRef::Local(account_id) => local_ids.push(account_id.clone()),
            FollowAccountRef::Remote(actor_uri) => remote_uris.push(actor_uri.clone()),
        }
    }
    let (accounts_by_id, stats_by_id, actors_by_uri, config) = futures_util::try_join!(
        find_accounts_by_ids(db, &local_ids),
        load_account_stats_map(db, &local_ids),
        find_remote_actors_by_actor_uris(db, &remote_uris),
        config_with_resolved_custom_emojis(db, config),
    )?;
    let mut fetched = HashMap::new();
    for actor_uri in remote_uris
        .iter()
        .filter(|actor_uri| !actors_by_uri.contains_key(*actor_uri))
    {
        if let Ok(profile) = fetch_remote_actor_profile(actor_uri).await {
            fetched.insert(
                actor_uri.clone(),
                upserted_remote_actor_response(db, &profile).await?,
            );
        }
    }

    let default_stats = Default::default();
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let account = match &row.account {
                FollowAccountRef::Local(account_id) => {
                    let account = accounts_by_id.get(account_id)?;
                    MastodonAccountResponse::from_account_with_stats(
                        account,
                        &config,
                        stats_by_id.get(account_id).unwrap_or(&default_stats),
                    )
                }
                FollowAccountRef::Remote(actor_uri) => match actors_by_uri.get(actor_uri) {
                    Some(actor) => MastodonAccountResponse::from_remote_actor(actor),
                    None => fetched.remove(actor_uri)?,
                },
            };
            Some(CollectionAccountEntry {
                cursor_id: row.cursor_id,
                created_at: row.created_at,
                account,
            })
        })
        .collect())
}

pub(crate) async fn local_account_follower_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<CollectionAccountEntry>> {
    let (local, remote) = futures_util::try_join!(
        list_local_followers_for_account(db, account_id),
        list_remote_followers_for_account(db, account_id),
    )?;
    let rows = local
        .into_iter()
        .map(|entry| FollowRow {
            cursor_id: entry.cursor_id,
            created_at: entry.created_at,
            account: FollowAccountRef::Local(entry.account_id),
        })
        .chain(remote.into_iter().map(|entry| FollowRow {
            cursor_id: entry.cursor_id,
            created_at: entry.created_at,
            account: FollowAccountRef::Remote(entry.actor_uri),
        }))
        .collect();
    hydrate_follow_rows(db, config, page_follow_rows(rows, limit, max_id, since_id)).await
}

pub(crate) async fn local_account_following_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<CollectionAccountEntry>> {
    let (local, remote) = futures_util::try_join!(
        list_local_following_for_account(db, account_id),
        list_remote_following_for_account(db, account_id),
    )?;
    let rows = local
        .into_iter()
        .map(|entry| FollowRow {
            cursor_id: entry.cursor_id,
            created_at: entry.created_at,
            account: FollowAccountRef::Local(entry.account_id),
        })
        .chain(remote.into_iter().map(|entry| FollowRow {
            cursor_id: entry.cursor_id,
            created_at: entry.created_at,
            account: FollowAccountRef::Remote(entry.actor_uri),
        }))
        .collect();
    hydrate_follow_rows(db, config, page_follow_rows(rows, limit, max_id, since_id)).await
}

pub(crate) async fn remote_actor_follower_entries(
    db: &D1Database,
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

    let rows = list_local_followers_for_remote_actor(db, actor_uri)
        .await?
        .into_iter()
        .map(|entry| FollowRow {
            cursor_id: entry.cursor_id,
            created_at: entry.created_at,
            account: FollowAccountRef::Local(entry.account_id),
        })
        .collect();
    hydrate_follow_rows(db, config, page_follow_rows(rows, limit, max_id, since_id)).await
}

pub(crate) async fn remote_actor_following_entries(
    db: &D1Database,
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

    let rows = list_local_following_for_remote_actor(db, actor_uri)
        .await?
        .into_iter()
        .map(|entry| FollowRow {
            cursor_id: entry.cursor_id,
            created_at: entry.created_at,
            account: FollowAccountRef::Local(entry.account_id),
        })
        .collect();
    hydrate_follow_rows(db, config, page_follow_rows(rows, limit, max_id, since_id)).await
}
