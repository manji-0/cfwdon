use crate::account_store::{
    DirectoryOrder, directory_order, list_discoverable_accounts_with_sort_key, load_account_stats,
};
use crate::auth::find_authenticated_local_account;
use crate::responses::MastodonAccountResponse;
use crate::runtime_config::load_config;
use crate::{
    account_search_non_exact_limit, find_remote_actor_by_actor_uri,
    load_remote_actor_status_summary, resolve_cached_exact_search_account, resolve_search_account,
    search_cached_accounts,
};
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AccountSearchQuery {
    pub(crate) q: String,
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
    pub(crate) resolve: Option<bool>,
    pub(crate) following: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DirectoryQuery {
    pub(crate) limit: Option<u32>,
    pub(crate) offset: Option<u32>,
    pub(crate) local: Option<bool>,
    pub(crate) order: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DirectoryRemoteActorRow {
    actor_uri: String,
    sort_key: String,
}

async fn list_discoverable_remote_actor_rows(
    db: &worker::D1Database,
    limit: u32,
    offset: u32,
    order: DirectoryOrder,
) -> Result<Vec<DirectoryRemoteActorRow>> {
    let sql = match order {
        DirectoryOrder::Active => {
            "SELECT ra.actor_uri,
                    COALESCE(MAX(rs.published_at), ra.created_at) AS sort_key
             FROM remote_actors ra
             LEFT JOIN remote_statuses rs
               ON rs.actor_uri = ra.actor_uri
             WHERE ra.discoverable = 1
             GROUP BY ra.actor_uri
             ORDER BY sort_key DESC, ra.username ASC, ra.domain ASC
             LIMIT ?1
             OFFSET ?2"
        }
        DirectoryOrder::New => {
            "SELECT actor_uri,
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
    Ok(db
        .prepare(sql)
        .bind_refs(bindings.iter())?
        .all()
        .await?
        .results::<DirectoryRemoteActorRow>()?)
}

pub(crate) async fn account_search(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: AccountSearchQuery = req.query().unwrap_or_default();
    let q = query.q.trim();
    if q.is_empty() {
        return Response::from_json(&Vec::<MastodonAccountResponse>::new());
    }

    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Cloudflare Access authentication required", 401),
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

    if query.resolve.unwrap_or(false)
        && results.is_empty()
        && let Some(account) = resolve_search_account(&db, &config, q).await?
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
    let db = ctx.d1(&config.database_binding)?;
    let include_local = query.local.unwrap_or(true);
    let include_remote = !query.local.unwrap_or(false);
    let fetch_limit = limit.saturating_add(offset).clamp(limit, 1000);
    let mut entries = Vec::<(String, String, MastodonAccountResponse)>::new();

    if include_local {
        for (account, sort_key) in
            list_discoverable_accounts_with_sort_key(&db, fetch_limit, 0, order).await?
        {
            let stats = load_account_stats(&db, &account.id).await?;
            entries.push((
                sort_key,
                account.username.clone(),
                MastodonAccountResponse::from_account_with_stats(&account, &config, &stats),
            ));
        }
    }

    if include_remote {
        for row in list_discoverable_remote_actor_rows(&db, fetch_limit, 0, order).await? {
            let Some(actor) = find_remote_actor_by_actor_uri(&db, &row.actor_uri).await? else {
                continue;
            };
            let mut response = MastodonAccountResponse::from_remote_actor(&actor);
            let stats = load_remote_actor_status_summary(&db, &actor.actor_uri).await?;
            response.statuses_count = stats.statuses_count;
            response.last_status_at = stats.last_status_at;
            entries.push((row.sort_key, response.acct.clone(), response));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let response = entries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|(_, _, response)| response)
        .collect::<Vec<_>>();

    Response::from_json(&response)
}
