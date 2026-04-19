use serde::{Deserialize, Deserializer};
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

fn deserialize_json_boolish<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(json_boolish(Some(&value)))
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteActorRow {
    pub(crate) actor_uri: String,
    pub(crate) username: String,
    pub(crate) domain: String,
    #[serde(deserialize_with = "deserialize_json_boolish")]
    pub(crate) locked: bool,
    #[serde(deserialize_with = "deserialize_json_boolish")]
    pub(crate) bot: bool,
    #[serde(deserialize_with = "deserialize_json_boolish")]
    pub(crate) discoverable: bool,
    #[serde(deserialize_with = "deserialize_json_boolish")]
    pub(crate) indexable: bool,
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
            locked: json_boolish(value.get("locked")),
            bot: json_boolish(value.get("bot")),
            discoverable: json_boolish(value.get("discoverable")),
            indexable: json_boolish(value.get("indexable")),
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
        "SELECT actor_uri, username, domain, locked, bot, discoverable, indexable, display_name, summary_html, profile_url, avatar_url, header_url
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
        "SELECT actor_uri, username, domain, locked, bot, discoverable, indexable, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE actor_uri = ?1
            OR profile_url = ?1
         LIMIT 1",
    )
    .bind_refs(&value)?
    .first::<RemoteActorRow>(None)
    .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteActorStatusSummary {
    pub(crate) statuses_count: u64,
    pub(crate) last_status_at: Option<String>,
}

pub(crate) async fn load_remote_actor_status_summary(
    db: &D1Database,
    actor_uri: &str,
) -> Result<RemoteActorStatusSummary> {
    let actor_uri = D1Type::Text(actor_uri);
    Ok(db
        .prepare(
            "SELECT COUNT(*) AS statuses_count,
                    MAX(substr(published_at, 1, 10)) AS last_status_at
             FROM remote_statuses
             WHERE actor_uri = ?1",
        )
        .bind_refs(&actor_uri)?
        .first::<RemoteActorStatusSummary>(None)
        .await?
        .unwrap_or(RemoteActorStatusSummary {
            statuses_count: 0,
            last_status_at: None,
        }))
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
        "SELECT actor_uri, username, domain, locked, bot, discoverable, indexable, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE lower(username) = ?1
           AND lower(domain) = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<RemoteActorRow>(None)
    .await
}
