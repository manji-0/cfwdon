use crate::RemoteActorRow;
use crate::account_store::{AccountRow, load_account_stats};
use crate::instance_host;
use crate::responses::MastodonAccountResponse;
use crate::search_text_match_rank;
use crate::{
    actor_url, find_account_by_username, find_remote_actor_by_username_domain,
    load_remote_actor_status_summary, parse_lookup_handle, strip_html_tags,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

pub(crate) fn normalized_account_search_query(query: &str) -> String {
    let query = query.trim().trim_start_matches('@');
    query
        .strip_prefix("acct:")
        .unwrap_or(query)
        .trim()
        .to_ascii_lowercase()
}

pub(crate) fn account_search_term(query: &str, config: &AppConfig) -> String {
    let normalized = normalized_account_search_query(query);
    let Some((username, domain)) = normalized.split_once('@') else {
        return normalized;
    };
    if domain == instance_host(config) {
        username.to_owned()
    } else {
        normalized
    }
}

pub(crate) fn account_search_is_complete_handle(query: &str, config: &AppConfig) -> bool {
    let query = query.trim().trim_start_matches('@');
    query.contains('@') && parse_lookup_handle(query, config).is_ok()
}

pub(crate) fn account_search_non_exact_limit(
    query: &str,
    viewer: Option<&LocalAccount>,
    limit: u32,
    exact_match_present: bool,
) -> u32 {
    if viewer.is_none() && normalized_account_search_query(query).len() < 3 {
        return 0;
    }

    if exact_match_present {
        limit.saturating_sub(1)
    } else {
        limit
    }
}

pub(crate) fn account_search_rank(
    query: &str,
    username: &str,
    acct: &str,
    display_name: &str,
    note: &str,
) -> (u8, u8, String) {
    let query = normalized_account_search_query(query);
    let candidates = if query.contains('@') {
        [
            (search_text_match_rank(&query, acct), 0u8),
            (search_text_match_rank(&query, username), 1u8),
            (search_text_match_rank(&query, display_name), 2u8),
            (search_text_match_rank(&query, note), 3u8),
        ]
    } else {
        [
            (search_text_match_rank(&query, username), 0u8),
            (search_text_match_rank(&query, acct), 1u8),
            (search_text_match_rank(&query, display_name), 2u8),
            (search_text_match_rank(&query, note), 3u8),
        ]
    };
    let (match_rank, field_rank) = candidates.into_iter().min().unwrap_or((3, 3));
    (match_rank, field_rank, acct.to_ascii_lowercase())
}

pub(crate) fn account_relationship_rank(is_self: bool, is_following: bool) -> u8 {
    if is_self {
        0
    } else if is_following {
        1
    } else {
        2
    }
}

pub(crate) fn account_search_sort_key(
    query: &str,
    username: &str,
    acct: &str,
    display_name: &str,
    note: &str,
    relationship_rank: u8,
    followers_count: u64,
    statuses_count: u64,
) -> (u8, u8, u8, u64, u64, String) {
    let (match_rank, field_rank, acct_sort_key) =
        account_search_rank(query, username, acct, display_name, note);
    (
        match_rank,
        relationship_rank,
        field_rank,
        u64::MAX - followers_count,
        u64::MAX - statuses_count,
        acct_sort_key,
    )
}

pub(crate) async fn search_local_accounts(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
    viewer_account_id: Option<&str>,
) -> Result<Vec<LocalAccount>> {
    let pattern = format!("%{}%", account_search_term(query, config));
    let local_host = instance_host(config);
    let sql = if following_only {
        "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.locked, a.bot, a.discoverable, a.default_post_visibility, a.default_quote_policy, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
         FROM accounts a
         JOIN follows f
           ON f.target_account_id = a.id
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE lower(a.username) LIKE ?2
            OR lower(a.display_name) LIKE ?2
            OR lower(a.bio_text) LIKE ?2
            OR lower(a.username || '@' || ?3) LIKE ?2
         ORDER BY a.username ASC
         LIMIT ?4
         OFFSET ?5"
    } else {
        "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
         FROM accounts
         WHERE lower(username) LIKE ?1
            OR lower(display_name) LIKE ?1
            OR lower(bio_text) LIKE ?1
            OR lower(username || '@' || ?2) LIKE ?1
         ORDER BY username ASC
         LIMIT ?3
         OFFSET ?4"
    };

    let result = if following_only {
        let bindings = [
            D1Type::Text(
                viewer_account_id
                    .ok_or_else(|| Error::RustError("missing viewer account id".to_owned()))?,
            ),
            D1Type::Text(pattern.as_str()),
            D1Type::Text(local_host.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Text(local_host.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    };

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

pub(crate) async fn search_remote_accounts(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
    viewer_account_id: Option<&str>,
) -> Result<Vec<RemoteActorRow>> {
    let pattern = format!("%{}%", account_search_term(query, config));
    let sql = if following_only {
        "SELECT ra.actor_uri, ra.username, ra.domain, ra.locked, ra.bot, ra.display_name, ra.summary_html, ra.profile_url, ra.avatar_url, ra.header_url
         FROM remote_actors ra
         JOIN follows f
           ON f.target_actor_uri = ra.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE lower(ra.username) LIKE ?2
            OR lower(ra.display_name) LIKE ?2
            OR lower(ra.summary_html) LIKE ?2
            OR lower(ra.domain) LIKE ?2
            OR lower(ra.username || '@' || ra.domain) LIKE ?2
         ORDER BY ra.username ASC, ra.domain ASC
         LIMIT ?3
         OFFSET ?4"
    } else {
        "SELECT actor_uri, username, domain, locked, bot, discoverable, indexable, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE lower(username) LIKE ?1
            OR lower(display_name) LIKE ?1
            OR lower(summary_html) LIKE ?1
            OR lower(domain) LIKE ?1
            OR lower(username || '@' || domain) LIKE ?1
         ORDER BY username ASC, domain ASC
         LIMIT ?2
         OFFSET ?3"
    };

    let result = if following_only {
        let bindings = [
            D1Type::Text(
                viewer_account_id
                    .ok_or_else(|| Error::RustError("missing viewer account id".to_owned()))?,
            ),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    };

    result.results::<RemoteActorRow>()
}

pub(crate) async fn search_cached_accounts(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
) -> Result<Vec<MastodonAccountResponse>> {
    let mut accounts = Vec::new();
    let viewer_account_id = viewer.map(|account| account.id.as_str());
    let query_limit = limit.saturating_add(offset).clamp(limit, 200);

    for account in search_local_accounts(
        db,
        config,
        query,
        query_limit,
        0,
        following_only,
        viewer_account_id,
    )
    .await?
    {
        let stats = load_account_stats(db, &account.id).await?;
        accounts.push(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }
    for actor in search_remote_accounts(
        db,
        config,
        query,
        query_limit,
        0,
        following_only,
        viewer_account_id,
    )
    .await?
    {
        let mut response = MastodonAccountResponse::from_remote_actor(&actor);
        let stats = load_remote_actor_status_summary(db, &actor.actor_uri).await?;
        response.statuses_count = stats.statuses_count;
        response.last_status_at = stats.last_status_at;
        accounts.push(response);
    }

    let rank_query = account_search_term(query, config);
    let mut ranked_accounts = Vec::with_capacity(accounts.len());
    for account in accounts {
        let relationship_rank = match viewer {
            Some(viewer) if account.id == viewer.id => account_relationship_rank(true, false),
            Some(viewer) => account_relationship_rank(
                false,
                crate::find_follow_by_target(db, &viewer.id, &account.uri)
                    .await?
                    .is_some_and(|follow| follow.state == "accepted"),
            ),
            None => account_relationship_rank(false, false),
        };
        ranked_accounts.push((
            account_search_sort_key(
                &rank_query,
                &account.username,
                &account.acct,
                &account.display_name,
                &strip_html_tags(&account.note),
                relationship_rank,
                account.followers_count,
                account.statuses_count,
            ),
            account,
        ));
    }
    ranked_accounts.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(ranked_accounts
        .into_iter()
        .map(|(_, account)| account)
        .skip(offset as usize)
        .take(limit as usize)
        .collect())
}

pub(crate) async fn resolve_cached_exact_search_account(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    query: &str,
    following_only: bool,
) -> Result<Option<MastodonAccountResponse>> {
    if !account_search_is_complete_handle(query, config) {
        return Ok(None);
    }

    let handle = parse_lookup_handle(query.trim().trim_start_matches('@'), config)?;
    let account = if handle.is_local_to(&config.instance_domain) {
        let Some(account) = find_account_by_username(db, &handle.username).await? else {
            return Ok(None);
        };
        let stats = load_account_stats(db, &account.id).await?;
        MastodonAccountResponse::from_account_with_stats(&account, config, &stats)
    } else {
        let Some(actor) = find_remote_actor_by_username_domain(
            db,
            &handle.username,
            handle.domain.as_deref().unwrap_or_default(),
        )
        .await?
        else {
            return Ok(None);
        };
        MastodonAccountResponse::from_remote_actor(&actor)
    };

    if following_only {
        let Some(viewer) = viewer else {
            return Ok(None);
        };
        let target_uri = if account.acct == account.username {
            actor_url(config, &account.username)
        } else {
            account.uri.clone()
        };
        let is_following = crate::find_follow_by_target(db, &viewer.id, &target_uri)
            .await?
            .is_some_and(|follow| follow.state == "accepted");
        if !is_following {
            return Ok(None);
        }
    }

    Ok(Some(account))
}
