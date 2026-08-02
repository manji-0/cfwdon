use super::{
    AccountRow, AppConfig, LocalAccount, REMOTE_ACTOR_ROW_COLUMNS, RemoteActorRow, actor_url,
    json_string_array, sql_in_json_each,
};
use cfwdon_domain::AccountHandle;
use std::collections::{HashMap, HashSet};
use worker::{Result, d1::D1Type};

use crate::D1Database;
pub(crate) async fn build_status_mentions(
    db: &D1Database,
    config: &AppConfig,
    text: &str,
) -> Result<Vec<serde_json::Value>> {
    build_status_mentions_with_preload(db, config, text, None).await
}

#[derive(Debug, Default)]
pub(crate) struct MentionAccountsPreload {
    local_accounts: HashMap<String, LocalAccount>,
    remote_actors: HashMap<(String, String), RemoteActorRow>,
}

pub(crate) async fn preload_mention_accounts_from_texts(
    db: &D1Database,
    config: &AppConfig,
    texts: &[&str],
) -> Result<MentionAccountsPreload> {
    let mut local_usernames = Vec::new();
    let mut remote_pairs = Vec::new();
    let mut seen_local = HashSet::new();
    let mut seen_remote = HashSet::new();

    for text in texts {
        let handles = crate::extract_account_handles_from_text(text, config);
        let keys = mention_lookup_keys(&handles, &config.instance_domain);
        for username in keys.local_usernames {
            if seen_local.insert(username.clone()) {
                local_usernames.push(username);
            }
        }
        for pair in keys.remote_pairs {
            if seen_remote.insert(pair.clone()) {
                remote_pairs.push(pair);
            }
        }
    }

    let (local_accounts, remote_actors) = futures_util::try_join!(
        load_mention_local_accounts(db, &local_usernames),
        load_mention_remote_actors(db, &remote_pairs),
    )?;

    Ok(MentionAccountsPreload {
        local_accounts,
        remote_actors,
    })
}

pub(crate) async fn build_status_mentions_with_preload(
    db: &D1Database,
    config: &AppConfig,
    text: &str,
    preload: Option<&MentionAccountsPreload>,
) -> Result<Vec<serde_json::Value>> {
    let handles = crate::extract_account_handles_from_text(text, config);
    if handles.is_empty() {
        return Ok(Vec::new());
    }

    if let Some(preload) = preload {
        return Ok(handles
            .iter()
            .filter_map(|handle| {
                mention_document_for_handle(
                    handle,
                    config,
                    &preload.local_accounts,
                    &preload.remote_actors,
                )
            })
            .collect());
    }

    let lookup_keys = mention_lookup_keys(&handles, &config.instance_domain);
    let local_accounts = load_mention_local_accounts(db, &lookup_keys.local_usernames).await?;
    let remote_actors = load_mention_remote_actors(db, &lookup_keys.remote_pairs).await?;

    Ok(handles
        .iter()
        .filter_map(|handle| {
            mention_document_for_handle(handle, config, &local_accounts, &remote_actors)
        })
        .collect())
}

#[derive(Debug, Default, PartialEq, Eq)]
struct MentionLookupKeys {
    local_usernames: Vec<String>,
    remote_pairs: Vec<(String, String)>,
}

fn mention_lookup_keys(handles: &[AccountHandle], instance_domain: &str) -> MentionLookupKeys {
    let mut keys = MentionLookupKeys::default();
    for handle in handles {
        if handle.is_local_to(instance_domain) {
            keys.local_usernames
                .push(handle.username.to_ascii_lowercase());
        } else if let Some(pair) = mention_remote_pair(handle) {
            keys.remote_pairs.push(pair);
        }
    }
    keys
}

fn mention_remote_pair(handle: &AccountHandle) -> Option<(String, String)> {
    handle.domain.as_deref().map(|domain| {
        (
            handle.username.to_ascii_lowercase(),
            domain.to_ascii_lowercase(),
        )
    })
}

fn mention_document_for_handle(
    handle: &AccountHandle,
    config: &AppConfig,
    local_accounts: &HashMap<String, LocalAccount>,
    remote_actors: &HashMap<(String, String), RemoteActorRow>,
) -> Option<serde_json::Value> {
    if handle.is_local_to(&config.instance_domain) {
        let account = local_accounts.get(&handle.username.to_ascii_lowercase())?;
        return Some(local_mention_document(config, account));
    }

    let key = mention_remote_pair(handle)?;
    let actor = remote_actors.get(&key)?;
    Some(remote_mention_document(actor))
}

fn local_mention_document(config: &AppConfig, account: &LocalAccount) -> serde_json::Value {
    serde_json::json!({
        "id": account.id().to_owned(),
        "username": account.username().to_owned(),
        "url": actor_url(config, account.username()),
        "acct": account.acct(),
    })
}

fn remote_mention_document(actor: &RemoteActorRow) -> serde_json::Value {
    serde_json::json!({
        "id": crate::remote_account_rest_id(&actor.actor_uri),
        "username": actor.username,
        "url": actor.profile_url.clone().unwrap_or_else(|| actor.actor_uri.clone()),
        "acct": format!("{}@{}", actor.username, actor.domain),
    })
}

fn mention_local_accounts_sql() -> String {
    format!(
        "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, locked, bot, discoverable, default_post_visibility, default_quote_policy, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, '' AS private_key_jwk, public_key_pem, created_at
         FROM accounts
         WHERE lower(username) {}",
        sql_in_json_each(1)
    )
}

async fn load_mention_local_accounts(
    db: &D1Database,
    usernames: &[String],
) -> Result<HashMap<String, LocalAccount>> {
    let usernames = crate::unique_ordered_refs(usernames);
    if usernames.is_empty() {
        return Ok(HashMap::new());
    }

    let usernames_json = json_string_array(&usernames);
    let binding = D1Type::Text(usernames_json.as_str());
    let result = db
        .prepare(mention_local_accounts_sql())
        .bind_refs(&binding)?
        .all()
        .await?;

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(|row| {
            (
                row.username.to_ascii_lowercase(),
                LocalAccount::from_record(row),
            )
        })
        .collect())
}

fn mention_remote_acct_key(username: &str, domain: &str) -> String {
    format!("{username}@{domain}")
}

fn mention_remote_actors_sql() -> String {
    format!(
        "SELECT {REMOTE_ACTOR_ROW_COLUMNS}
         FROM remote_actors
         WHERE (lower(username) || '@' || lower(domain)) {}",
        sql_in_json_each(1)
    )
}

async fn load_mention_remote_actors(
    db: &D1Database,
    pairs: &[(String, String)],
) -> Result<HashMap<(String, String), RemoteActorRow>> {
    let mut seen = HashSet::new();
    let pairs = pairs
        .iter()
        .filter(|(username, domain)| seen.insert((username.as_str(), domain.as_str())))
        .collect::<Vec<_>>();
    if pairs.is_empty() {
        return Ok(HashMap::new());
    }

    let acct_keys = pairs
        .iter()
        .map(|(username, domain)| mention_remote_acct_key(username, domain))
        .collect::<Vec<_>>();
    let keys_json = json_string_array(&acct_keys);
    let binding = D1Type::Text(keys_json.as_str());
    let result = db
        .prepare(mention_remote_actors_sql())
        .bind_refs(&binding)?
        .all()
        .await?;

    Ok(result
        .results::<RemoteActorRow>()?
        .into_iter()
        .map(|row| {
            (
                (
                    row.username.to_ascii_lowercase(),
                    row.domain.to_ascii_lowercase(),
                ),
                row,
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        MentionLookupKeys, mention_local_accounts_sql, mention_lookup_keys,
        mention_remote_acct_key, mention_remote_actors_sql, mention_remote_pair,
    };
    use cfwdon_domain::AccountHandle;

    #[test]
    fn mention_lookup_keys_partition_local_and_remote_handles() {
        let handles = vec![
            AccountHandle {
                username: "Alice".to_owned(),
                domain: Some("social.example".to_owned()),
            },
            AccountHandle {
                username: "Bob".to_owned(),
                domain: Some("Remote.Example".to_owned()),
            },
            AccountHandle::local("Carol"),
        ];

        let keys = mention_lookup_keys(&handles, "social.example");

        assert_eq!(
            keys,
            MentionLookupKeys {
                local_usernames: vec!["alice".to_owned(), "carol".to_owned()],
                remote_pairs: vec![("bob".to_owned(), "remote.example".to_owned())],
            }
        );
    }

    #[test]
    fn mention_remote_pair_lowercases_username_and_domain() {
        let handle = AccountHandle {
            username: "Bob".to_owned(),
            domain: Some("Remote.Example".to_owned()),
        };

        assert_eq!(
            mention_remote_pair(&handle),
            Some(("bob".to_owned(), "remote.example".to_owned()))
        );
        assert_eq!(mention_remote_pair(&AccountHandle::local("alice")), None);
    }

    #[test]
    fn mention_local_accounts_sql_uses_json_each() {
        let sql = mention_local_accounts_sql();

        assert!(sql.contains("WHERE lower(username) IN (SELECT value FROM json_each(?1))"));
        assert!(!sql.contains("?2"));
    }

    #[test]
    fn mention_remote_actors_sql_uses_json_each_composite_acct() {
        let sql = mention_remote_actors_sql();

        assert!(sql.contains(
            "WHERE (lower(username) || '@' || lower(domain)) IN (SELECT value FROM json_each(?1))"
        ));
        assert!(!sql.contains("lower(username) = ?"));
        assert_eq!(
            mention_remote_acct_key("bob", "remote.example"),
            "bob@remote.example"
        );
    }
}
