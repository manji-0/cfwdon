use crate::RemoteActorProfile;
use worker::d1::D1Type;
use worker::{D1Database, Result};

fn json_boolish(value: Option<&serde_json::Value>) -> bool {
    value
        .and_then(|field| {
            field
                .as_bool()
                .or_else(|| field.as_i64().map(|number| number != 0))
        })
        .unwrap_or(false)
}

fn remote_actor_profile_from_value(value: &serde_json::Value) -> RemoteActorProfile {
    RemoteActorProfile {
        actor_uri: json_string(value, "actor_uri"),
        username: json_string(value, "username"),
        domain: json_string(value, "domain"),
        locked: json_boolish(value.get("locked")),
        bot: json_boolish(value.get("bot")),
        discoverable: json_boolish(value.get("discoverable")),
        indexable: json_boolish(value.get("indexable")),
        inbox_uri: json_string(value, "inbox_uri"),
        shared_inbox_uri: optional_json_string(value, "shared_inbox_uri"),
        public_key_id: json_string(value, "public_key_id"),
        public_key_pem: json_string(value, "public_key_pem"),
        display_name: json_string(value, "display_name"),
        summary_html: json_string(value, "summary_html"),
        profile_url: optional_json_string(value, "profile_url"),
        avatar_url: optional_json_string(value, "avatar_url"),
        header_url: optional_json_string(value, "header_url"),
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn optional_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(crate) async fn find_cached_remote_actor_profile_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<RemoteActorProfile>> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT actor_uri, username, domain, locked, bot, discoverable, indexable, inbox_uri, shared_inbox_uri, public_key_id, public_key_pem, display_name, summary_html, profile_url, avatar_url, header_url
             FROM remote_actors
             WHERE actor_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&actor_uri)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.map(|value| remote_actor_profile_from_value(&value)))
}

const UPSERT_REMOTE_ACTOR_SQL: &str = "INSERT INTO remote_actors (
    actor_uri,
    username,
    domain,
    locked,
    bot,
    discoverable,
    indexable,
    inbox_uri,
    shared_inbox_uri,
    public_key_id,
    public_key_pem,
    display_name,
    summary_html,
    profile_url,
    avatar_url,
    header_url,
    created_at,
    updated_at
) VALUES (
    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP
)
ON CONFLICT(actor_uri) DO UPDATE SET
    username = excluded.username,
    domain = excluded.domain,
    locked = excluded.locked,
    bot = excluded.bot,
    discoverable = excluded.discoverable,
    indexable = excluded.indexable,
    inbox_uri = excluded.inbox_uri,
    shared_inbox_uri = excluded.shared_inbox_uri,
    public_key_id = excluded.public_key_id,
    public_key_pem = excluded.public_key_pem,
    display_name = excluded.display_name,
    summary_html = excluded.summary_html,
    profile_url = excluded.profile_url,
    avatar_url = excluded.avatar_url,
    header_url = excluded.header_url,
    updated_at = CURRENT_TIMESTAMP";

fn remote_actor_bindings(actor: &RemoteActorProfile) -> [D1Type<'_>; 16] {
    [
        D1Type::Text(actor.actor_uri.as_str()),
        D1Type::Text(actor.username.as_str()),
        D1Type::Text(actor.domain.as_str()),
        D1Type::Integer(if actor.locked { 1 } else { 0 }),
        D1Type::Integer(if actor.bot { 1 } else { 0 }),
        D1Type::Integer(if actor.discoverable { 1 } else { 0 }),
        D1Type::Integer(if actor.indexable { 1 } else { 0 }),
        D1Type::Text(actor.inbox_uri.as_str()),
        match actor.shared_inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(actor.public_key_id.as_str()),
        D1Type::Text(actor.public_key_pem.as_str()),
        D1Type::Text(actor.display_name.as_str()),
        D1Type::Text(actor.summary_html.as_str()),
        match actor.profile_url.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match actor.avatar_url.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match actor.header_url.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ]
}

pub(crate) async fn upsert_remote_actor(db: &D1Database, actor: &RemoteActorProfile) -> Result<()> {
    let bindings = remote_actor_bindings(actor);
    db.prepare(UPSERT_REMOTE_ACTOR_SQL)
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    Ok(())
}

pub(crate) async fn upsert_remote_actors(
    db: &D1Database,
    actors: &[RemoteActorProfile],
) -> Result<()> {
    if actors.is_empty() {
        return Ok(());
    }

    let bindings = actors.iter().map(remote_actor_bindings).collect::<Vec<_>>();
    let statements = db
        .prepare(UPSERT_REMOTE_ACTOR_SQL)
        .batch_bind(bindings.iter().map(|binding| binding.iter()))?;
    db.batch(statements).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_actor_profile_from_value_maps_optional_and_boolish_fields() {
        let profile = remote_actor_profile_from_value(&serde_json::json!({
            "actor_uri": "https://remote.example/users/alice",
            "username": "alice",
            "domain": "remote.example",
            "locked": 1,
            "bot": false,
            "discoverable": true,
            "indexable": 0,
            "inbox_uri": "https://remote.example/users/alice/inbox",
            "shared_inbox_uri": "https://remote.example/inbox",
            "public_key_id": "https://remote.example/users/alice#main-key",
            "public_key_pem": "pem",
            "display_name": "Alice",
            "summary_html": "<p>Hello</p>",
            "profile_url": "https://remote.example/@alice",
            "avatar_url": "https://remote.example/avatar.png",
            "header_url": "https://remote.example/header.png"
        }));

        assert_eq!(profile.actor_uri, "https://remote.example/users/alice");
        assert_eq!(profile.username, "alice");
        assert_eq!(profile.domain, "remote.example");
        assert!(profile.locked);
        assert!(!profile.bot);
        assert!(profile.discoverable);
        assert!(!profile.indexable);
        assert_eq!(
            profile.inbox_uri,
            "https://remote.example/users/alice/inbox"
        );
        assert_eq!(
            profile.shared_inbox_uri.as_deref(),
            Some("https://remote.example/inbox")
        );
        assert_eq!(
            profile.public_key_id,
            "https://remote.example/users/alice#main-key"
        );
        assert_eq!(profile.public_key_pem, "pem");
        assert_eq!(profile.display_name, "Alice");
        assert_eq!(profile.summary_html, "<p>Hello</p>");
        assert_eq!(
            profile.profile_url.as_deref(),
            Some("https://remote.example/@alice")
        );
        assert_eq!(
            profile.avatar_url.as_deref(),
            Some("https://remote.example/avatar.png")
        );
        assert_eq!(
            profile.header_url.as_deref(),
            Some("https://remote.example/header.png")
        );
    }

    #[test]
    fn remote_actor_profile_from_value_uses_empty_and_false_defaults() {
        let profile = remote_actor_profile_from_value(&serde_json::json!({}));

        assert_eq!(profile.actor_uri, "");
        assert_eq!(profile.username, "");
        assert_eq!(profile.domain, "");
        assert!(!profile.locked);
        assert!(!profile.bot);
        assert!(!profile.discoverable);
        assert!(!profile.indexable);
        assert_eq!(profile.inbox_uri, "");
        assert!(profile.shared_inbox_uri.is_none());
        assert_eq!(profile.public_key_id, "");
        assert_eq!(profile.public_key_pem, "");
        assert_eq!(profile.display_name, "");
        assert_eq!(profile.summary_html, "");
        assert!(profile.profile_url.is_none());
        assert!(profile.avatar_url.is_none());
        assert!(profile.header_url.is_none());
    }
}
