use super::accounts::{
    account_search_non_exact_limit, account_search_resolve_enabled,
    resolve_cached_exact_search_account, search_cached_accounts,
};
use crate::accounts::{
    DirectoryOrder, directory_order, list_discoverable_accounts_with_sort_key,
    load_account_stats_map,
};
use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::db_utils::d1_results;
use crate::remote::resolve_search_account_with_viewer;
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::store::remote::{find_remote_actors_by_actor_uris, load_remote_actor_status_summaries};
use crate::tracked_d1::D1Database;
use cfwdon_domain::LocalAccount;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AccountSearchQuery {
    pub(crate) q: String,
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
    #[serde(
        default,
        deserialize_with = "crate::request_utils::deserialize_query_bool"
    )]
    pub(crate) resolve: Option<bool>,
    #[serde(
        default,
        deserialize_with = "crate::request_utils::deserialize_query_bool"
    )]
    pub(crate) following: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DirectoryQuery {
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
    #[serde(
        default,
        deserialize_with = "crate::request_utils::deserialize_query_bool"
    )]
    pub(crate) local: Option<bool>,
    pub(crate) order: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DirectoryRemoteActorRow {
    actor_uri: String,
    acct: String,
    sort_key: String,
}

enum DirectoryEntry {
    Local(Box<LocalAccount>),
    Remote(String),
}

async fn list_discoverable_remote_actor_rows(
    db: &D1Database,
    limit: u32,
    offset: u32,
    order: DirectoryOrder,
) -> Result<Vec<DirectoryRemoteActorRow>> {
    let sql = match order {
        DirectoryOrder::Active => {
            "SELECT ra.actor_uri,
                    ra.username || '@' || ra.domain AS acct,
                    COALESCE(
                      (SELECT MAX(rs.published_at)
                       FROM remote_statuses rs
                       WHERE rs.actor_uri = ra.actor_uri),
                      ra.created_at
                    ) AS sort_key
             FROM remote_actors ra
             WHERE ra.discoverable = 1
             ORDER BY sort_key DESC, ra.username ASC, ra.domain ASC
             LIMIT ?1
             OFFSET ?2"
        }
        DirectoryOrder::New => {
            "SELECT actor_uri,
                    username || '@' || domain AS acct,
                    created_at AS sort_key
             FROM remote_actors
             WHERE discoverable = 1
             ORDER BY created_at DESC, username ASC, domain ASC
             LIMIT ?1
             OFFSET ?2"
        }
    };
    let bindings = [
        D1Type::Integer(limit as i32),
        D1Type::Integer(offset as i32),
    ];
    db.prepare(sql)
        .bind_refs(bindings.iter())?
        .all()
        .await
        .and_then(|__d1| d1_results::<DirectoryRemoteActorRow>(&__d1))
}

pub(crate) async fn account_search(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountSearchQuery = req.query().unwrap_or_default();
    let q = query.q.trim();
    if q.is_empty() {
        return Response::from_json(&Vec::<MastodonAccountResponse>::new());
    }

    let db = bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let only_following = query.following.unwrap_or(false);
    let exact_account = if offset == 0 {
        resolve_cached_exact_search_account(&db, &config, Some(&viewer), q, only_following).await?
    } else {
        None
    };
    let non_exact_limit =
        account_search_non_exact_limit(q, Some(&viewer), limit, exact_account.is_some());
    let mut results = if non_exact_limit == 0 {
        Vec::new()
    } else {
        search_cached_accounts(
            &db,
            &config,
            Some(&viewer),
            q,
            non_exact_limit,
            offset,
            only_following,
        )
        .await?
    };
    if let Some(account) = exact_account
        && !results.iter().any(|candidate| candidate.id == account.id)
    {
        results.insert(0, account);
    }
    results.truncate(limit as usize);

    if account_search_resolve_enabled(query.resolve, q, &config)
        && results.is_empty()
        && let Some(account) =
            resolve_search_account_with_viewer(&db, &config, q, Some(&viewer)).await?
    {
        results.push(account);
    }

    Response::from_json(&results)
}

pub(crate) async fn account_directory(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: DirectoryQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let order = directory_order(query.order.as_deref());
    let db = bind_request_d1(&ctx, &config)?;
    let include_local = query.local.unwrap_or(true);
    let include_remote = !query.local.unwrap_or(false);
    let fetch_limit = limit.saturating_add(offset).clamp(limit, 1000);
    // Sort and cut the page on light rows first, then load stats for the page.
    let mut entries = Vec::<(String, String, DirectoryEntry)>::new();
    if include_local {
        for (account, sort_key) in
            list_discoverable_accounts_with_sort_key(&db, fetch_limit, 0, order).await?
        {
            let name = account.username().to_owned();
            entries.push((sort_key, name, DirectoryEntry::Local(Box::new(account))));
        }
    }
    if include_remote {
        for row in list_discoverable_remote_actor_rows(&db, fetch_limit, 0, order).await? {
            entries.push((
                row.sort_key,
                row.acct,
                DirectoryEntry::Remote(row.actor_uri),
            ));
        }
    }
    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let page = entries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|(_, _, entry)| entry)
        .collect::<Vec<_>>();

    let mut local_ids = Vec::new();
    let mut remote_uris = Vec::new();
    for entry in &page {
        match entry {
            DirectoryEntry::Local(account) => local_ids.push(account.id().to_owned()),
            DirectoryEntry::Remote(actor_uri) => remote_uris.push(actor_uri.clone()),
        }
    }
    let (stats_by_id, actors_by_uri, summaries_by_uri) = futures_util::try_join!(
        load_account_stats_map(&db, &local_ids),
        find_remote_actors_by_actor_uris(&db, &remote_uris),
        load_remote_actor_status_summaries(&db, &remote_uris),
    )?;
    let default_stats = Default::default();
    let response = page
        .into_iter()
        .filter_map(|entry| match entry {
            DirectoryEntry::Local(account) => {
                Some(MastodonAccountResponse::from_account_with_stats(
                    &account,
                    &config,
                    stats_by_id.get(account.id()).unwrap_or(&default_stats),
                ))
            }
            DirectoryEntry::Remote(actor_uri) => {
                let actor = actors_by_uri.get(&actor_uri)?;
                let mut response = MastodonAccountResponse::from_remote_actor(actor);
                if let Some(summary) = summaries_by_uri.get(&actor_uri) {
                    response.statuses_count = summary.statuses_count;
                    response.last_status_at = summary.last_status_at.clone();
                }
                Some(response)
            }
        })
        .collect::<Vec<_>>();

    Response::from_json(&response)
}
