use crate::{d1_in_value_chunk_size, sql_placeholders, unique_ordered_refs};
use cfwdon_domain::{LocalAccount, LocalAccountRecord};
use serde::Deserialize;
use std::collections::HashMap;
use worker::d1::D1Type;
use worker::{D1Database, Result};

pub(crate) type AccountRow = LocalAccountRecord;

#[derive(Debug, Deserialize)]
struct DiscoverableAccountRow {
    id: String,
    username: String,
    access_email: String,
    display_name: String,
    bio_html: String,
    bio_text: String,
    fields_json: String,
    locked: i32,
    bot: i32,
    discoverable: i32,
    default_post_visibility: String,
    #[serde(default = "default_quote_policy")]
    default_quote_policy: String,
    default_sensitive: i32,
    default_language: Option<String>,
    avatar_object_key: Option<String>,
    avatar_content_type: Option<String>,
    header_object_key: Option<String>,
    header_content_type: Option<String>,
    public_key_pem: String,
    created_at: String,
    sort_key: String,
}

#[derive(Debug, Default)]
pub(crate) struct AccountStats {
    pub(crate) followers_count: u64,
    pub(crate) following_count: u64,
    pub(crate) statuses_count: u64,
    pub(crate) last_status_at: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectoryOrder {
    Active,
    New,
}

pub(crate) fn directory_order(value: Option<&str>) -> DirectoryOrder {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("new") => DirectoryOrder::New,
        _ => DirectoryOrder::Active,
    }
}

fn default_quote_policy() -> String {
    "public".to_owned()
}

pub(crate) async fn list_discoverable_accounts_with_sort_key(
    db: &D1Database,
    limit: u32,
    offset: u32,
    order: DirectoryOrder,
) -> Result<Vec<(LocalAccount, String)>> {
    let sql = match order {
        DirectoryOrder::Active => {
            "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.locked, a.bot, a.discoverable, a.default_post_visibility, a.default_quote_policy, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.public_key_pem, a.created_at,
                    COALESCE(MAX(s.created_at), a.created_at) AS sort_key
             FROM accounts a
             LEFT JOIN statuses s
               ON s.account_id = a.id
             WHERE a.discoverable = 1
             GROUP BY a.id
             ORDER BY sort_key DESC, a.username ASC
             LIMIT ?1
             OFFSET ?2"
        }
        DirectoryOrder::New => {
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, public_key_pem, created_at,
                    created_at AS sort_key
             FROM accounts
             WHERE discoverable = 1
             ORDER BY sort_key DESC, username ASC
             LIMIT ?1
             OFFSET ?2"
        }
    };

    let bindings = [
        D1Type::Integer(limit as i32),
        D1Type::Integer(offset as i32),
    ];
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<DiscoverableAccountRow>()?
        .into_iter()
        .map(|row| {
            let sort_key = row.sort_key.clone();
            (
                LocalAccount::from_record(AccountRow {
                    id: row.id,
                    username: row.username,
                    access_email: row.access_email,
                    display_name: row.display_name,
                    bio_html: row.bio_html,
                    bio_text: row.bio_text,
                    fields_json: row.fields_json,
                    locked: row.locked,
                    bot: row.bot,
                    discoverable: row.discoverable,
                    default_post_visibility: row.default_post_visibility,
                    default_quote_policy: row.default_quote_policy,
                    default_sensitive: row.default_sensitive,
                    default_language: row.default_language,
                    avatar_object_key: row.avatar_object_key,
                    avatar_content_type: row.avatar_content_type,
                    header_object_key: row.header_object_key,
                    header_content_type: row.header_content_type,
                    private_key_jwk: String::new(),
                    public_key_pem: row.public_key_pem,
                    created_at: row.created_at,
                }),
                sort_key,
            )
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct AccountStatsRow {
    followers_count: u64,
    following_count: u64,
    statuses_count: u64,
    last_status_at: Option<String>,
}

pub(crate) async fn load_account_stats(db: &D1Database, account_id: &str) -> Result<AccountStats> {
    let account_id_binding = D1Type::Text(account_id);
    let stats = db
        .prepare(
            "SELECT followers_count, following_count, statuses_count, last_status_at
             FROM account_stats
             WHERE account_id = ?1",
        )
        .bind_refs(&account_id_binding)?
        .first::<AccountStatsRow>(None)
        .await?
        .unwrap_or(AccountStatsRow {
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            last_status_at: None,
        });

    Ok(AccountStats {
        followers_count: stats.followers_count,
        following_count: stats.following_count,
        statuses_count: stats.statuses_count,
        last_status_at: stats.last_status_at,
    })
}

pub(crate) async fn load_account_stats_map(
    db: &D1Database,
    account_ids: &[String],
) -> Result<HashMap<String, AccountStats>> {
    let ids = unique_ordered_refs(account_ids);
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    #[derive(Debug, Deserialize)]
    struct AccountStatsMapRow {
        account_id: String,
        followers_count: u64,
        following_count: u64,
        statuses_count: u64,
        last_status_at: Option<String>,
    }

    let placeholders = sql_placeholders(1, ids.len());
    let sql = format!(
        "SELECT account_id, followers_count, following_count, statuses_count, last_status_at
         FROM account_stats
         WHERE account_id IN ({placeholders})"
    );
    let bindings = ids
        .iter()
        .map(|id| D1Type::Text(id.as_str()))
        .collect::<Vec<_>>();
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<AccountStatsMapRow>()?
        .into_iter()
        .map(|row| {
            (
                row.account_id,
                AccountStats {
                    followers_count: row.followers_count,
                    following_count: row.following_count,
                    statuses_count: row.statuses_count,
                    last_status_at: row.last_status_at,
                },
            )
        })
        .collect())
}

pub(crate) async fn find_accounts_by_ids(
    db: &D1Database,
    account_ids: &[String],
) -> Result<HashMap<String, LocalAccount>> {
    let ids = unique_ordered_refs(account_ids);
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut accounts = HashMap::new();
    for chunk in ids.chunks(d1_in_value_chunk_size(0)) {
        let placeholders = sql_placeholders(1, chunk.len());
        let sql = format!(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE id IN ({placeholders})"
        );
        let bindings = chunk
            .iter()
            .map(|id| D1Type::Text(id.as_str()))
            .collect::<Vec<_>>();
        let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;
        accounts.extend(result.results::<AccountRow>()?.into_iter().map(|row| {
            let id = row.id.clone();
            (id, LocalAccount::from_record(row))
        }));
    }

    Ok(accounts)
}
