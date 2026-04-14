use crate::RemoteActorRow;
use crate::account_store::{AccountRow, load_account_stats};
use crate::responses::MastodonAccountResponse;
use crate::search_text_match_rank;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use worker::d1::D1Type;
use worker::{D1Database, Error, Result};

pub(crate) fn account_search_rank(
    query: &str,
    username: &str,
    acct: &str,
    display_name: &str,
) -> (u8, u8, String) {
    let candidates = [
        (search_text_match_rank(query, username), 0u8),
        (search_text_match_rank(query, acct), 1u8),
        (search_text_match_rank(query, display_name), 2u8),
    ];
    let (match_rank, field_rank) = candidates.into_iter().min().unwrap_or((3, 3));
    (match_rank, field_rank, acct.to_ascii_lowercase())
}

pub(crate) async fn search_local_accounts(
    db: &D1Database,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
    viewer_account_id: Option<&str>,
) -> Result<Vec<LocalAccount>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let sql = if following_only {
        "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
         FROM accounts a
         JOIN follows f
           ON f.target_account_id = a.id
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE lower(a.username) LIKE ?2
            OR lower(a.display_name) LIKE ?2
         ORDER BY a.username ASC
         LIMIT ?3
         OFFSET ?4"
    } else {
        "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
         FROM accounts
         WHERE lower(username) LIKE ?1
            OR lower(display_name) LIKE ?1
         ORDER BY username ASC
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

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

pub(crate) async fn search_remote_accounts(
    db: &D1Database,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
    viewer_account_id: Option<&str>,
) -> Result<Vec<RemoteActorRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let sql = if following_only {
        "SELECT ra.actor_uri, ra.username, ra.domain, ra.display_name, ra.summary_html, ra.profile_url, ra.avatar_url, ra.header_url
         FROM remote_actors ra
         JOIN follows f
           ON f.target_actor_uri = ra.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE lower(ra.username) LIKE ?2
            OR lower(ra.display_name) LIKE ?2
            OR lower(ra.domain) LIKE ?2
         ORDER BY ra.username ASC, ra.domain ASC
         LIMIT ?3
         OFFSET ?4"
    } else {
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE lower(username) LIKE ?1
            OR lower(display_name) LIKE ?1
            OR lower(domain) LIKE ?1
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

    for account in
        search_local_accounts(db, query, query_limit, 0, following_only, viewer_account_id).await?
    {
        let stats = load_account_stats(db, &account.id).await?;
        accounts.push(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }
    for actor in
        search_remote_accounts(db, query, query_limit, 0, following_only, viewer_account_id).await?
    {
        accounts.push(MastodonAccountResponse::from_remote_actor(&actor));
    }

    accounts.sort_by_key(|account| {
        account_search_rank(
            query,
            &account.username,
            &account.acct,
            &account.display_name,
        )
    });
    Ok(accounts
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect())
}
