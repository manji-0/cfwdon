use super::response_mentions::{
    load_mention_local_accounts, load_mention_remote_actors, mention_lookup_keys,
};
use super::{
    AppConfig, D1Database, actor_url, extract_account_handles_from_text,
    find_remote_actor_by_actor_uri,
};
use worker::{Result, d1::D1Type};

struct MentionRow {
    mention_key: String,
    account_id: Option<String>,
    actor_uri: Option<String>,
    username: String,
    acct: String,
    url: String,
}

/// Writes `status_mentions` rows for a local status. Replaces all existing
/// rows for the status with the newly resolved set.
pub(crate) async fn replace_local_status_mentions(
    db: &D1Database,
    config: &AppConfig,
    status_id: &str,
    created_at: &str,
    text: &str,
) -> Result<()> {
    let rows = resolve_rows_from_text(db, config, text).await?;
    replace_status_mention_rows(db, status_id, created_at, &rows).await
}

/// Writes `remote_status_mentions` rows for a remote status. Prefers explicit
/// AP `tag` entries of type `Mention`; falls back to text-extracted handles.
pub(crate) async fn replace_remote_status_mentions(
    db: &D1Database,
    config: &AppConfig,
    status_id: &str,
    published_at: &str,
    object: &serde_json::Value,
    text: &str,
) -> Result<()> {
    let rows = if let Some(rows) = extract_ap_mention_rows(db, config, object).await? {
        rows
    } else {
        resolve_rows_from_text(db, config, text).await?
    };
    replace_remote_status_mention_rows(db, status_id, published_at, &rows).await
}

async fn resolve_rows_from_text(
    db: &D1Database,
    config: &AppConfig,
    text: &str,
) -> Result<Vec<MentionRow>> {
    let handles = extract_account_handles_from_text(text, config);
    let keys = mention_lookup_keys(&handles, &config.instance_domain);
    let (local_accounts, remote_actors) = futures_util::try_join!(
        load_mention_local_accounts(db, &keys.local_usernames),
        load_mention_remote_actors(db, &keys.remote_pairs),
    )?;

    let mut rows: Vec<MentionRow> = Vec::new();
    for handle in &handles {
        if handle.is_local_to(&config.instance_domain) {
            let key = handle.username.to_ascii_lowercase();
            if let Some(account) = local_accounts.get(&key) {
                rows.push(MentionRow {
                    mention_key: account.acct().to_ascii_lowercase(),
                    account_id: Some(account.id().to_owned()),
                    actor_uri: None,
                    username: account.username().to_owned(),
                    acct: account.acct().to_owned(),
                    url: actor_url(config, account.username()),
                });
            }
        } else if let Some(domain) = handle.domain.as_deref() {
            let pair = (
                handle.username.to_ascii_lowercase(),
                domain.to_ascii_lowercase(),
            );
            if let Some(actor) = remote_actors.get(&pair) {
                let acct = format!("{}@{}", actor.username, actor.domain);
                rows.push(MentionRow {
                    mention_key: acct.to_ascii_lowercase(),
                    account_id: None,
                    actor_uri: Some(actor.actor_uri.clone()),
                    username: actor.username.clone(),
                    acct,
                    url: actor
                        .profile_url
                        .clone()
                        .unwrap_or_else(|| actor.actor_uri.clone()),
                });
            }
        }
    }
    Ok(rows)
}

/// Extract mentions from the AP `tag` array. Returns `None` if no Mention
/// entries are found, signalling fallback to text parsing.
async fn extract_ap_mention_rows(
    db: &D1Database,
    config: &AppConfig,
    object: &serde_json::Value,
) -> Result<Option<Vec<MentionRow>>> {
    let Some(tags) = object.get("tag").and_then(serde_json::Value::as_array) else {
        return Ok(None);
    };

    let mention_entries: Vec<(String, String)> = tags
        .iter()
        .filter(|tag| {
            tag.get("type")
                .and_then(serde_json::Value::as_str)
                .map(|t| t.eq_ignore_ascii_case("Mention"))
                .unwrap_or(false)
        })
        .filter_map(|tag| {
            let href = tag.get("href").and_then(serde_json::Value::as_str)?;
            let name = tag.get("name").and_then(serde_json::Value::as_str)?;
            Some((href.to_owned(), name.trim_start_matches('@').to_owned()))
        })
        .collect();

    if mention_entries.is_empty() {
        return Ok(None);
    }

    let mut rows: Vec<MentionRow> = Vec::new();
    for (href, name) in &mention_entries {
        // Try exact actor_uri match first.
        if let Some(actor) = find_remote_actor_by_actor_uri(db, href).await? {
            let acct = format!("{}@{}", actor.username, actor.domain);
            rows.push(MentionRow {
                mention_key: acct.to_ascii_lowercase(),
                account_id: None,
                actor_uri: Some(actor.actor_uri.clone()),
                username: actor.username.clone(),
                acct,
                url: actor
                    .profile_url
                    .clone()
                    .unwrap_or_else(|| actor.actor_uri.clone()),
            });
            continue;
        }

        // Detect if this is a local actor URL and resolve by username.
        let local_actor_base = format!("{}/users/", config.instance_domain);
        let local_actor_base_https = format!("https://{}/users/", config.instance_domain);
        let maybe_username =
            if href.starts_with(&local_actor_base) || href.starts_with(&local_actor_base_https) {
                href.rsplit('/').next().map(str::to_owned)
            } else {
                None
            };

        if let Some(username) = maybe_username {
            let usernames = vec![username.to_ascii_lowercase()];
            if let Some(account) = load_mention_local_accounts(db, &usernames)
                .await?
                .into_values()
                .next()
            {
                rows.push(MentionRow {
                    mention_key: account.acct().to_ascii_lowercase(),
                    account_id: Some(account.id().to_owned()),
                    actor_uri: None,
                    username: account.username().to_owned(),
                    acct: account.acct().to_owned(),
                    url: actor_url(config, account.username()),
                });
                continue;
            }
        }

        // Fall back: synthesize from href/name without DB lookup.
        let (username, acct) = if let Some((user, domain)) = name.split_once('@') {
            (user.to_owned(), format!("{user}@{domain}"))
        } else {
            (name.clone(), name.clone())
        };
        rows.push(MentionRow {
            mention_key: acct.to_ascii_lowercase(),
            account_id: None,
            actor_uri: Some(href.clone()),
            username,
            acct,
            url: href.clone(),
        });
    }

    Ok(Some(rows))
}

async fn replace_status_mention_rows(
    db: &D1Database,
    status_id: &str,
    created_at: &str,
    rows: &[MentionRow],
) -> Result<()> {
    let status_binding = D1Type::Text(status_id);
    db.prepare("DELETE FROM status_mentions WHERE status_id = ?1")
        .bind_refs(&status_binding)?
        .run()
        .await?;

    for row in rows {
        let account_id_val: D1Type<'_> = match &row.account_id {
            Some(id) => D1Type::Text(id.as_str()),
            None => D1Type::Null,
        };
        let actor_uri_val: D1Type<'_> = match &row.actor_uri {
            Some(uri) => D1Type::Text(uri.as_str()),
            None => D1Type::Null,
        };
        let bindings: [D1Type<'_>; 8] = [
            D1Type::Text(status_id),
            D1Type::Text(&row.mention_key),
            account_id_val,
            actor_uri_val,
            D1Type::Text(&row.username),
            D1Type::Text(&row.acct),
            D1Type::Text(&row.url),
            D1Type::Text(created_at),
        ];
        db.prepare(
            "INSERT OR REPLACE INTO status_mentions
             (status_id, mention_key, account_id, actor_uri, username, acct, url, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn replace_remote_status_mention_rows(
    db: &D1Database,
    status_id: &str,
    published_at: &str,
    rows: &[MentionRow],
) -> Result<()> {
    let status_binding = D1Type::Text(status_id);
    db.prepare("DELETE FROM remote_status_mentions WHERE status_id = ?1")
        .bind_refs(&status_binding)?
        .run()
        .await?;

    for row in rows {
        let account_id_val: D1Type<'_> = match &row.account_id {
            Some(id) => D1Type::Text(id.as_str()),
            None => D1Type::Null,
        };
        let actor_uri_val: D1Type<'_> = match &row.actor_uri {
            Some(uri) => D1Type::Text(uri.as_str()),
            None => D1Type::Null,
        };
        let bindings: [D1Type<'_>; 8] = [
            D1Type::Text(status_id),
            D1Type::Text(&row.mention_key),
            account_id_val,
            actor_uri_val,
            D1Type::Text(&row.username),
            D1Type::Text(&row.acct),
            D1Type::Text(&row.url),
            D1Type::Text(published_at),
        ];
        db.prepare(
            "INSERT OR REPLACE INTO remote_status_mentions
             (status_id, mention_key, account_id, actor_uri, username, acct, url, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

/// Load mention JSON documents for a status from the pre-computed table.
/// Returns `None` if no rows exist (caller should fall back to text parse).
pub(crate) async fn load_stored_status_mentions(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<Vec<serde_json::Value>>> {
    let binding = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT account_id, actor_uri, username, acct, url
             FROM status_mentions
             WHERE status_id = ?1",
        )
        .bind_refs(&binding)?
        .all()
        .await?;
    let rows = result.results::<StoredMentionRow>()?;
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(rows.into_iter().map(stored_mention_to_json).collect()))
}

/// Load mention JSON documents for a remote status from the pre-computed table.
/// Returns `None` if no rows exist.
pub(crate) async fn load_stored_remote_status_mentions(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<Vec<serde_json::Value>>> {
    let binding = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT account_id, actor_uri, username, acct, url
             FROM remote_status_mentions
             WHERE status_id = ?1",
        )
        .bind_refs(&binding)?
        .all()
        .await?;
    let rows = result.results::<StoredMentionRow>()?;
    if rows.is_empty() {
        return Ok(None);
    }
    Ok(Some(rows.into_iter().map(stored_mention_to_json).collect()))
}

#[derive(serde::Deserialize)]
struct StoredMentionRow {
    account_id: Option<String>,
    actor_uri: Option<String>,
    username: String,
    acct: String,
    url: String,
}

fn stored_mention_to_json(row: StoredMentionRow) -> serde_json::Value {
    let id = row
        .account_id
        .clone()
        .or_else(|| row.actor_uri.as_deref().map(crate::remote_account_rest_id))
        .unwrap_or_default();
    serde_json::json!({
        "id": id,
        "username": row.username,
        "url": row.url,
        "acct": row.acct,
    })
}
