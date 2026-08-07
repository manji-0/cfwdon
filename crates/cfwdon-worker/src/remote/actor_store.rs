use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use worker::Result;
use worker::d1::D1Type;

use crate::{
    D1Database, RemoteActorSocialCounts, json_string_array, sql_in_json_each, unique_ordered_refs,
};

pub(crate) const REMOTE_ACTOR_ROW_COLUMNS: &str = "actor_uri, username, domain, created_at, locked, bot, discoverable, indexable, display_name, summary_html, profile_url, avatar_url, header_url, followers_count, following_count, statuses_count, social_counts_updated_at";

pub(crate) const REMOTE_ACTOR_ROW_COLUMNS_ALIASED: &str = "ra.actor_uri, ra.username, ra.domain, ra.created_at, ra.locked, ra.bot, ra.discoverable, ra.indexable, ra.display_name, ra.summary_html, ra.profile_url, ra.avatar_url, ra.header_url, ra.followers_count, ra.following_count, ra.statuses_count, ra.social_counts_updated_at";

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

fn json_u64(value: Option<&serde_json::Value>) -> u64 {
    value
        .and_then(|field| {
            field
                .as_u64()
                .or_else(|| {
                    field
                        .as_i64()
                        .filter(|number| *number >= 0)
                        .map(|number| number as u64)
                })
                .or_else(|| {
                    field
                        .as_f64()
                        .filter(|number| *number >= 0.0 && number.fract() == 0.0)
                        .map(|number| number as u64)
                })
                .or_else(|| field.as_str().and_then(|raw| raw.trim().parse().ok()))
        })
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
pub(crate) struct RemoteActorRow {
    pub(crate) actor_uri: String,
    pub(crate) username: String,
    pub(crate) domain: String,
    #[serde(default)]
    pub(crate) created_at: String,
    #[serde(default, deserialize_with = "deserialize_json_boolish")]
    pub(crate) locked: bool,
    #[serde(default, deserialize_with = "deserialize_json_boolish")]
    pub(crate) bot: bool,
    #[serde(default, deserialize_with = "deserialize_json_boolish")]
    pub(crate) discoverable: bool,
    #[serde(default, deserialize_with = "deserialize_json_boolish")]
    pub(crate) indexable: bool,
    pub(crate) display_name: String,
    pub(crate) summary_html: String,
    pub(crate) profile_url: Option<String>,
    pub(crate) avatar_url: Option<String>,
    pub(crate) header_url: Option<String>,
    #[serde(default)]
    pub(crate) followers_count: u64,
    #[serde(default)]
    pub(crate) following_count: u64,
    #[serde(default)]
    pub(crate) statuses_count: u64,
    #[serde(default)]
    pub(crate) social_counts_updated_at: Option<String>,
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
            created_at: value
                .get("created_at")
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
            followers_count: json_u64(value.get("followers_count")),
            following_count: json_u64(value.get("following_count")),
            statuses_count: json_u64(value.get("statuses_count")),
            social_counts_updated_at: value
                .get("social_counts_updated_at")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
        }
    }
}

fn d1_optional_count(count: Option<u64>) -> D1Type<'static> {
    match count {
        Some(value) if value <= i32::MAX as u64 => D1Type::Integer(value as i32),
        // D1Type::Integer is i32; keep large totals via REAL instead of silent clamp.
        Some(value) => D1Type::Real(value as f64),
        None => D1Type::Null,
    }
}

pub(crate) async fn update_remote_actor_social_counts(
    db: &D1Database,
    actor_uri: &str,
    counts: &RemoteActorSocialCounts,
) -> Result<()> {
    if !counts.has_any() {
        return Ok(());
    }

    let bindings = [
        D1Type::Text(actor_uri),
        d1_optional_count(counts.followers_count),
        d1_optional_count(counts.following_count),
        d1_optional_count(counts.statuses_count),
    ];
    db.prepare(
        "UPDATE remote_actors
         SET followers_count = COALESCE(?2, followers_count),
             following_count = COALESCE(?3, following_count),
             statuses_count = COALESCE(?4, statuses_count),
             social_counts_updated_at = CURRENT_TIMESTAMP
         WHERE actor_uri = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

pub(crate) async fn find_remote_actor_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<RemoteActorRow>> {
    let actor_uri = D1Type::Text(actor_uri);
    db.prepare(format!(
        "SELECT {REMOTE_ACTOR_ROW_COLUMNS}
         FROM remote_actors
         WHERE actor_uri = ?1
         LIMIT 1"
    ))
    .bind_refs(&actor_uri)?
    .first::<RemoteActorRow>(None)
    .await
}

pub(crate) async fn find_remote_actors_by_actor_uris(
    db: &D1Database,
    actor_uris: &[String],
) -> Result<HashMap<String, RemoteActorRow>> {
    let uris = unique_ordered_refs(actor_uris);
    if uris.is_empty() {
        return Ok(HashMap::new());
    }

    let uris_json = json_string_array(&uris);
    let sql = format!(
        "SELECT {REMOTE_ACTOR_ROW_COLUMNS}
         FROM remote_actors
         WHERE actor_uri {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(uris_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;

    Ok(crate::d1_results::<RemoteActorRow>(&result)?
        .into_iter()
        .map(|row| (row.actor_uri.clone(), row))
        .collect())
}

pub(crate) async fn find_remote_actor_by_profile_url_or_actor_uri(
    db: &D1Database,
    value: &str,
) -> Result<Option<RemoteActorRow>> {
    let value = D1Type::Text(value);
    db.prepare(format!(
        "SELECT {REMOTE_ACTOR_ROW_COLUMNS}
         FROM remote_actors
         WHERE actor_uri = ?1
            OR profile_url = ?1
         ORDER BY CASE WHEN actor_uri = ?1 THEN 0 ELSE 1 END, updated_at DESC
         LIMIT 1"
    ))
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

pub(crate) async fn load_remote_actor_status_summaries(
    db: &D1Database,
    actor_uris: &[String],
) -> Result<HashMap<String, RemoteActorStatusSummary>> {
    let uris = unique_ordered_refs(actor_uris);
    if uris.is_empty() {
        return Ok(HashMap::new());
    }

    #[derive(Debug, Deserialize)]
    struct RemoteActorStatusSummaryMapRow {
        actor_uri: String,
        statuses_count: u64,
        last_status_at: Option<String>,
    }

    let uris_json = json_string_array(&uris);
    let sql = format!(
        "SELECT actor_uri,
                COUNT(*) AS statuses_count,
                MAX(substr(published_at, 1, 10)) AS last_status_at
         FROM remote_statuses
         WHERE actor_uri {}
         GROUP BY actor_uri",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(uris_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;

    Ok(
        crate::d1_results::<RemoteActorStatusSummaryMapRow>(&result)?
            .into_iter()
            .map(|row| {
                (
                    row.actor_uri,
                    RemoteActorStatusSummary {
                        statuses_count: row.statuses_count,
                        last_status_at: row.last_status_at,
                    },
                )
            })
            .collect(),
    )
}

pub(crate) async fn find_remote_actor_by_username_domain(
    db: &D1Database,
    username: &str,
    domain: &str,
) -> Result<Option<RemoteActorRow>> {
    let username = username.to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    let bindings = [D1Type::Text(&username), D1Type::Text(&domain)];
    let row = db
        .prepare(format!(
            "SELECT {REMOTE_ACTOR_ROW_COLUMNS}
             FROM remote_actors
             WHERE lower(username) = ?1
               AND lower(domain) = ?2
             ORDER BY updated_at DESC, actor_uri ASC
             LIMIT 1"
        ))
        .bind_refs(bindings.iter())?
        .first::<RemoteActorRow>(None)
        .await?;

    Ok(row.filter(|actor| {
        cfwdon_domain::remote_actor_cached_handle_allowed(
            &actor.actor_uri,
            &actor.username,
            &actor.domain,
            &username,
            &domain,
        )
    }))
}
