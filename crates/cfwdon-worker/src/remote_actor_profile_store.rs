use crate::RemoteActorProfile;
use worker::d1::D1Type;
use worker::{D1Database, Result};

fn remote_actor_profile_from_value(value: &serde_json::Value) -> RemoteActorProfile {
    RemoteActorProfile {
        actor_uri: value
            .get("actor_uri")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        username: value
            .get("username")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        domain: value
            .get("domain")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        inbox_uri: value
            .get("inbox_uri")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        shared_inbox_uri: value
            .get("shared_inbox_uri")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        public_key_id: value
            .get("public_key_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        public_key_pem: value
            .get("public_key_pem")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        display_name: value
            .get("display_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        summary_html: value
            .get("summary_html")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        profile_url: value
            .get("profile_url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        avatar_url: value
            .get("avatar_url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        header_url: value
            .get("header_url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    }
}

pub(crate) async fn find_cached_remote_actor_profile_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<RemoteActorProfile>> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT actor_uri, username, domain, inbox_uri, shared_inbox_uri, public_key_id, public_key_pem, display_name, summary_html, profile_url, avatar_url, header_url
             FROM remote_actors
             WHERE actor_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&actor_uri)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.map(|value| remote_actor_profile_from_value(&value)))
}

pub(crate) async fn upsert_remote_actor(db: &D1Database, actor: &RemoteActorProfile) -> Result<()> {
    let bindings = [
        D1Type::Text(actor.actor_uri.as_str()),
        D1Type::Text(actor.username.as_str()),
        D1Type::Text(actor.domain.as_str()),
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
    ];
    db.prepare(
        "INSERT INTO remote_actors (
            actor_uri,
            username,
            domain,
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
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(actor_uri) DO UPDATE SET
            username = excluded.username,
            domain = excluded.domain,
            inbox_uri = excluded.inbox_uri,
            shared_inbox_uri = excluded.shared_inbox_uri,
            public_key_id = excluded.public_key_id,
            public_key_pem = excluded.public_key_pem,
            display_name = excluded.display_name,
            summary_html = excluded.summary_html,
            profile_url = excluded.profile_url,
            avatar_url = excluded.avatar_url,
            header_url = excluded.header_url,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}
