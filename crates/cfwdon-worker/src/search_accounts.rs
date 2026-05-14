use crate::RemoteActorRow;
use crate::account_store::{AccountRow, AccountStats, load_account_stats, load_account_stats_map};
use crate::instance_host;
use crate::responses::MastodonAccountResponse;
use crate::search_text_match_rank;
use crate::{
    actor_url, apply_remote_actor_social_counts, fetch_remote_actor_profile_with_document,
    find_account_by_username, find_remote_actor_by_actor_uri, find_remote_actor_by_username_domain,
    list_accepted_follow_target_uris, load_remote_actor_social_counts_from_document,
    load_remote_actor_status_summaries, normalize_search_match_text, normalize_search_query_input,
    parse_lookup_handle, strip_html_tags, upsert_remote_actor,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

pub(crate) fn normalized_account_search_query(query: &str) -> String {
    let query = query.trim().trim_start_matches('@');
    let query = if query
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("acct:"))
    {
        &query[5..]
    } else {
        query
    };
    query.trim().to_ascii_lowercase()
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

fn tokenize_account_search_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in normalize_search_query_input(query).chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current);
                    current = String::new();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn account_query_terms(query: &str) -> Vec<String> {
    tokenize_account_search_query(query)
        .into_iter()
        .map(|value| normalize_search_match_text(value.trim_matches('"')))
        .filter(|value| !value.is_empty())
        .collect()
}

pub(crate) fn account_search_terms(query: &str, config: &AppConfig) -> Vec<String> {
    tokenize_account_search_query(query)
        .into_iter()
        .map(|value| account_search_term(&value, config))
        .map(|value| value.trim_matches('"').trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

fn account_search_seed_term(query: &str, config: &AppConfig) -> String {
    account_search_terms(query, config)
        .into_iter()
        .max_by_key(|value| value.len())
        .unwrap_or_else(|| account_search_term(query, config))
}

pub(crate) fn account_search_query_terms(query: &str, config: &AppConfig) -> Vec<String> {
    let mut terms = account_search_terms(query, config)
        .into_iter()
        .map(|term| term.trim().to_owned())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();

    if terms.is_empty() {
        let seed = account_search_seed_term(query, config).trim().to_owned();
        if !seed.is_empty() {
            terms.push(seed);
        }
        return terms;
    }

    let normalized = account_search_term(query, config).trim().to_owned();
    if !normalized.is_empty() && !terms.iter().any(|term| term == &normalized) {
        terms.push(normalized);
    }

    terms
}

pub(crate) fn account_matches_search_terms(
    terms: &[String],
    username: &str,
    acct: &str,
    display_name: &str,
    note: &str,
) -> bool {
    if terms.is_empty() {
        return true;
    }
    let username = normalize_search_match_text(username);
    let acct = normalize_search_match_text(acct);
    let display_name = normalize_search_match_text(display_name);
    let note = normalize_search_match_text(note);
    terms.iter().all(|term| {
        let term = normalize_search_match_text(term);
        !term.is_empty()
            && (username.contains(&term)
                || acct.contains(&term)
                || display_name.contains(&term)
                || note.contains(&term))
    })
}

pub(crate) fn account_search_is_complete_handle(query: &str, config: &AppConfig) -> bool {
    let query = query.trim().trim_start_matches('@');
    !query.is_empty()
        && !query.chars().any(char::is_whitespace)
        && query.contains('@')
        && parse_lookup_handle(query, config).is_ok()
}

pub(crate) fn account_search_non_exact_limit(
    query: &str,
    viewer: Option<&LocalAccount>,
    limit: u32,
    exact_match_present: bool,
) -> u32 {
    if query.trim_start().starts_with('#') {
        return 0;
    }
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
    let query = normalized_account_search_query(&normalize_search_query_input(query));
    let candidates = if query.contains('@') {
        [
            (account_text_match_rank(&query, acct), 0u8),
            (account_text_match_rank(&query, username), 1u8),
            (account_text_match_rank(&query, display_name), 2u8),
            (account_text_match_rank(&query, note), 3u8),
        ]
    } else {
        [
            (account_text_match_rank(&query, username), 0u8),
            (account_text_match_rank(&query, acct), 1u8),
            (account_text_match_rank(&query, display_name), 2u8),
            (account_text_match_rank(&query, note), 3u8),
        ]
    };
    let (match_rank, field_rank) = candidates.into_iter().min().unwrap_or((3, 3));
    (match_rank, field_rank, acct.to_ascii_lowercase())
}

fn account_text_match_rank(query: &str, candidate: &str) -> u8 {
    let phrase_rank = search_text_match_rank(query, candidate);
    if phrase_rank < 3 || !query.contains(char::is_whitespace) {
        return phrase_rank;
    }

    let candidate = normalize_search_match_text(candidate);
    if account_query_terms(query)
        .iter()
        .all(|term| candidate.contains(term))
    {
        2
    } else {
        3
    }
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
    let query_terms = account_search_query_terms(query, config);
    let patterns = query_terms
        .iter()
        .map(|term| format!("%{}%", term.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    let local_host = instance_host(config);
    let search_clause_list = if following_only {
        patterns
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let binding = index + 2;
                format!(
                    "lower(a.username) LIKE ?{binding} OR lower(a.display_name) LIKE ?{binding} OR lower(a.bio_text) LIKE ?{binding} OR lower(a.username || '@' || ?{}) LIKE ?{binding}",
                    patterns.len() + 2
                )
            })
            .collect::<Vec<_>>()
            .join(" OR ")
    } else {
        patterns
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let binding = index + 1;
                format!(
                    "lower(username) LIKE ?{binding} OR lower(display_name) LIKE ?{binding} OR lower(bio_text) LIKE ?{binding} OR lower(username || '@' || ?{}) LIKE ?{binding}",
                    patterns.len() + 1
                )
            })
            .collect::<Vec<_>>()
            .join(" OR ")
    };
    let sql = if following_only {
        format!(
            "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.locked, a.bot, a.discoverable, a.default_post_visibility, a.default_quote_policy, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
         FROM accounts a
         JOIN follows f
           ON f.target_account_id = a.id
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE ({search_clause_list})
         ORDER BY a.username ASC
         LIMIT ?{}
         OFFSET ?{}",
            patterns.len() + 3,
            patterns.len() + 4
        )
    } else {
        format!(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
         FROM accounts
         WHERE ({search_clause_list})
         ORDER BY username ASC
         LIMIT ?{}
         OFFSET ?{}",
            patterns.len() + 2,
            patterns.len() + 3
        )
    };

    let result = if following_only {
        let mut bindings = Vec::with_capacity(patterns.len() + 4);
        bindings.push(D1Type::Text(viewer_account_id.ok_or_else(|| {
            Error::RustError("missing viewer account id".to_owned())
        })?));
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(D1Type::Text(local_host.as_str()));
        bindings.push(D1Type::Integer(limit as i32));
        bindings.push(D1Type::Integer(offset as i32));
        db.prepare(&sql).bind_refs(bindings.iter())?.all().await?
    } else {
        let mut bindings = Vec::with_capacity(patterns.len() + 3);
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(D1Type::Text(local_host.as_str()));
        bindings.push(D1Type::Integer(limit as i32));
        bindings.push(D1Type::Integer(offset as i32));
        db.prepare(&sql).bind_refs(bindings.iter())?.all().await?
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
    let query_terms = account_search_query_terms(query, config);
    let patterns = query_terms
        .iter()
        .map(|term| format!("%{}%", term.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    let search_clause_list = if following_only {
        patterns
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let binding = index + 2;
                format!(
                    "lower(ra.username) LIKE ?{binding} OR lower(ra.display_name) LIKE ?{binding} OR lower(ra.summary_html) LIKE ?{binding} OR lower(ra.domain) LIKE ?{binding} OR lower(ra.username || '@' || ra.domain) LIKE ?{binding}"
                )
            })
            .collect::<Vec<_>>()
            .join(" OR ")
    } else {
        patterns
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let binding = index + 1;
                format!(
                    "lower(username) LIKE ?{binding} OR lower(display_name) LIKE ?{binding} OR lower(summary_html) LIKE ?{binding} OR lower(domain) LIKE ?{binding} OR lower(username || '@' || domain) LIKE ?{binding}"
                )
            })
            .collect::<Vec<_>>()
            .join(" OR ")
    };
    let sql = if following_only {
        format!(
            "SELECT ra.actor_uri, ra.username, ra.domain, ra.created_at, ra.locked, ra.bot, ra.discoverable, ra.indexable, ra.display_name, ra.summary_html, ra.profile_url, ra.avatar_url, ra.header_url
         FROM remote_actors ra
         JOIN follows f
           ON f.target_actor_uri = ra.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE ({search_clause_list})
         ORDER BY ra.username ASC, ra.domain ASC
         LIMIT ?{}
         OFFSET ?{}",
            patterns.len() + 2,
            patterns.len() + 3
        )
    } else {
        format!(
            "SELECT actor_uri, username, domain, created_at, locked, bot, discoverable, indexable, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE ({search_clause_list})
         ORDER BY username ASC, domain ASC
         LIMIT ?{}
         OFFSET ?{}",
            patterns.len() + 1,
            patterns.len() + 2
        )
    };

    let result = if following_only {
        let mut bindings = Vec::with_capacity(patterns.len() + 3);
        bindings.push(D1Type::Text(viewer_account_id.ok_or_else(|| {
            Error::RustError("missing viewer account id".to_owned())
        })?));
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(D1Type::Integer(limit as i32));
        bindings.push(D1Type::Integer(offset as i32));
        db.prepare(&sql).bind_refs(bindings.iter())?.all().await?
    } else {
        let mut bindings = Vec::with_capacity(patterns.len() + 2);
        bindings.extend(
            patterns
                .iter()
                .map(|pattern| D1Type::Text(pattern.as_str())),
        );
        bindings.push(D1Type::Integer(limit as i32));
        bindings.push(D1Type::Integer(offset as i32));
        db.prepare(&sql).bind_refs(bindings.iter())?.all().await?
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
    let viewer_account_id = viewer.map(|account| account.id.as_str());
    let query_limit = account_cache_query_limit(limit, offset);
    let search_terms = account_search_terms(query, config);
    let rank_query = account_search_term(query, config);

    let accounts = load_cached_account_search_candidates(
        db,
        config,
        query,
        query_limit,
        following_only,
        viewer_account_id,
        &search_terms,
    )
    .await?;
    rank_cached_account_search_results(db, viewer, &rank_query, accounts, limit, offset).await
}

fn account_cache_query_limit(limit: u32, offset: u32) -> u32 {
    limit
        .saturating_add(offset)
        .saturating_mul(4)
        .clamp(limit, 200)
}

async fn load_cached_account_search_candidates(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
    query_limit: u32,
    following_only: bool,
    viewer_account_id: Option<&str>,
    search_terms: &[String],
) -> Result<Vec<MastodonAccountResponse>> {
    let (local_accounts, remote_actors) = futures_util::try_join!(
        search_local_accounts(
            db,
            config,
            query,
            query_limit,
            0,
            following_only,
            viewer_account_id,
        ),
        search_remote_accounts(
            db,
            config,
            query,
            query_limit,
            0,
            following_only,
            viewer_account_id,
        ),
    )?;
    let local_account_ids = local_accounts
        .iter()
        .map(|account| account.id.clone())
        .collect::<Vec<_>>();
    let remote_actor_uris = remote_actors
        .iter()
        .map(|actor| actor.actor_uri.clone())
        .collect::<Vec<_>>();
    let (local_stats, remote_stats) = futures_util::try_join!(
        load_account_stats_map(db, &local_account_ids),
        load_remote_actor_status_summaries(db, &remote_actor_uris),
    )?;

    let mut accounts = Vec::with_capacity(local_accounts.len() + remote_actors.len());
    for account in local_accounts {
        if let Some(response) = local_search_account_response(
            &account,
            config,
            local_stats.get(&account.id),
            search_terms,
        ) {
            accounts.push(response);
        }
    }
    for actor in remote_actors {
        if let Some(response) = remote_search_account_response(
            db,
            &actor,
            remote_stats.get(&actor.actor_uri),
            search_terms,
        )
        .await?
        {
            accounts.push(response);
        }
    }
    Ok(accounts)
}

fn local_search_account_response(
    account: &LocalAccount,
    config: &AppConfig,
    stats: Option<&AccountStats>,
    search_terms: &[String],
) -> Option<MastodonAccountResponse> {
    let default_stats;
    let stats = match stats {
        Some(stats) => stats,
        None => {
            default_stats = Default::default();
            &default_stats
        }
    };
    let response = MastodonAccountResponse::from_account_with_stats(account, config, stats);
    search_account_response_matches(search_terms, response)
}

async fn remote_search_account_response(
    db: &D1Database,
    actor: &RemoteActorRow,
    stats: Option<&crate::RemoteActorStatusSummary>,
    search_terms: &[String],
) -> Result<Option<MastodonAccountResponse>> {
    let default_stats;
    let stats = match stats {
        Some(stats) => stats,
        None => {
            default_stats = crate::RemoteActorStatusSummary {
                statuses_count: 0,
                last_status_at: None,
            };
            &default_stats
        }
    };
    let mut fallback_response = MastodonAccountResponse::from_remote_actor(actor);
    fallback_response.statuses_count = stats.statuses_count;
    fallback_response.last_status_at = stats.last_status_at.clone();
    let Some(_) = search_account_response_matches(search_terms, fallback_response.clone()) else {
        return Ok(None);
    };

    let mut response = match fresh_remote_search_account_response(db, actor).await {
        Ok(fresh_response) => fresh_response,
        Err(_) => fallback_response,
    };
    if stats.statuses_count > 0 {
        response.statuses_count = stats.statuses_count;
    }
    response.last_status_at = stats.last_status_at.clone();
    Ok(Some(response))
}

fn search_account_response_matches(
    search_terms: &[String],
    response: MastodonAccountResponse,
) -> Option<MastodonAccountResponse> {
    if account_matches_search_terms(
        search_terms,
        &response.username,
        &response.acct,
        &response.display_name,
        &strip_html_tags(&response.note),
    ) {
        Some(response)
    } else {
        None
    }
}

async fn rank_cached_account_search_results(
    db: &D1Database,
    viewer: Option<&LocalAccount>,
    rank_query: &str,
    accounts: Vec<MastodonAccountResponse>,
    limit: u32,
    offset: u32,
) -> Result<Vec<MastodonAccountResponse>> {
    let followed_target_uris = match viewer {
        Some(viewer) => {
            let target_uris = accounts
                .iter()
                .map(|account| account.uri.clone())
                .collect::<Vec<_>>();
            list_accepted_follow_target_uris(db, &viewer.id, &target_uris).await?
        }
        None => Default::default(),
    };
    let mut ranked_accounts = Vec::with_capacity(accounts.len());
    for account in accounts {
        let relationship_rank = match viewer {
            Some(viewer) if account.id == viewer.id => account_relationship_rank(true, false),
            Some(_) => {
                account_relationship_rank(false, followed_target_uris.contains(&account.uri))
            }
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
        fresh_remote_search_account_response(db, &actor).await?
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

async fn fresh_remote_search_account_response(
    db: &D1Database,
    actor: &RemoteActorRow,
) -> Result<MastodonAccountResponse> {
    let fetched = match fetch_remote_actor_profile_with_document(&actor.actor_uri).await {
        Ok(fetched) => fetched,
        Err(_) => return Ok(MastodonAccountResponse::from_remote_actor(actor)),
    };
    let profile = fetched.profile;
    upsert_remote_actor(db, &profile).await?;
    let mut response = match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
        Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
        None => MastodonAccountResponse::from_remote_actor_profile(&profile),
    };
    if let Ok(counts) = load_remote_actor_social_counts_from_document(&fetched.document).await {
        apply_remote_actor_social_counts(&mut response, counts);
    }
    Ok(response)
}
