use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteActorRow {
    pub(crate) actor_uri: String,
    pub(crate) username: String,
    pub(crate) domain: String,
    pub(crate) display_name: String,
    pub(crate) summary_html: String,
    pub(crate) profile_url: Option<String>,
    pub(crate) avatar_url: Option<String>,
    pub(crate) header_url: Option<String>,
}

impl RemoteActorRow {
    pub(crate) fn from_value(value: &serde_json::Value) -> Self {
        Self {
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
}

pub(crate) async fn find_remote_actor_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<RemoteActorRow>> {
    let actor_uri = D1Type::Text(actor_uri);
    db.prepare(
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE actor_uri = ?1
         LIMIT 1",
    )
    .bind_refs(&actor_uri)?
    .first::<RemoteActorRow>(None)
    .await
}

pub(crate) async fn find_remote_actor_by_profile_url_or_actor_uri(
    db: &D1Database,
    value: &str,
) -> Result<Option<RemoteActorRow>> {
    let value = D1Type::Text(value);
    db.prepare(
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE actor_uri = ?1
            OR profile_url = ?1
         LIMIT 1",
    )
    .bind_refs(&value)?
    .first::<RemoteActorRow>(None)
    .await
}

pub(crate) async fn find_remote_actor_by_username_domain(
    db: &D1Database,
    username: &str,
    domain: &str,
) -> Result<Option<RemoteActorRow>> {
    let username = username.to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    let bindings = [D1Type::Text(&username), D1Type::Text(&domain)];
    db.prepare(
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE lower(username) = ?1
           AND lower(domain) = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<RemoteActorRow>(None)
    .await
}
