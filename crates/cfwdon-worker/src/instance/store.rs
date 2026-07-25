use std::collections::BTreeSet;

use super::{
    AppConfig, D1Database, InstanceCapabilities, InstanceSummary, SoftwareInfo, build_metadata,
    instance_host, normalize_instance_domain, peer_authority_from_uri,
};
use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

#[derive(Debug, Deserialize)]
pub(crate) struct InstanceSettingsRow {
    pub(crate) domain: String,
    pub(crate) title: String,
    pub(crate) description: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ActiveMonthCountRow {
    pub(crate) count: u64,
}

pub(crate) async fn load_instance_summary(
    db: &D1Database,
    config: AppConfig,
) -> Result<InstanceSummary> {
    let build = build_metadata();
    let settings = db
        .prepare(
            "SELECT domain, title, description
             FROM instance_settings
             WHERE id = 1
             LIMIT 1",
        )
        .first::<InstanceSettingsRow>(None)
        .await?;

    let (domain, title, description) = match settings {
        Some(settings) => (settings.domain, settings.title, settings.description),
        None => (
            config.instance_domain,
            config.instance_name,
            config.instance_description,
        ),
    };

    Ok(InstanceSummary {
        domain: normalize_instance_domain(&domain),
        title,
        description,
        software: SoftwareInfo {
            name: build.service_name.to_owned(),
            version: build.version.to_owned(),
        },
        capabilities: InstanceCapabilities {
            federation: true,
            local_timeline: true,
            media_uploads: true,
        },
    })
}

pub(crate) async fn load_active_month_users(db: &D1Database) -> Result<u64> {
    let row = db
        .prepare(
            "SELECT COUNT(DISTINCT account_id) AS count
             FROM statuses
             WHERE created_at >= datetime('now', '-28 days')",
        )
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

pub(crate) async fn load_active_halfyear_users(db: &D1Database) -> Result<u64> {
    let row = db
        .prepare(
            "SELECT COUNT(DISTINCT account_id) AS count
             FROM statuses
             WHERE created_at >= datetime('now', '-180 days')",
        )
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

pub(crate) async fn load_total_local_accounts(db: &D1Database) -> Result<u64> {
    let row = db
        .prepare("SELECT COUNT(*) AS count FROM accounts")
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

pub(crate) async fn load_total_local_statuses(db: &D1Database) -> Result<u64> {
    let row = db
        .prepare("SELECT COUNT(*) AS count FROM statuses")
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

pub(crate) async fn count_local_statuses_between(
    db: &D1Database,
    start: &str,
    end: &str,
) -> Result<u64> {
    let bindings = [D1Type::Text(start), D1Type::Text(end)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM statuses
             WHERE datetime(created_at) >= datetime(?1)
               AND datetime(created_at) < datetime(?2)",
        )
        .bind_refs(bindings.iter())?
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

pub(crate) async fn count_accounts_created_between(
    db: &D1Database,
    start: &str,
    end: &str,
) -> Result<u64> {
    let bindings = [D1Type::Text(start), D1Type::Text(end)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM accounts
             WHERE datetime(created_at) >= datetime(?1)
               AND datetime(created_at) < datetime(?2)",
        )
        .bind_refs(bindings.iter())?
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

pub(crate) async fn load_known_peer_domains(
    db: &D1Database,
    config: &AppConfig,
) -> Result<Vec<String>> {
    let mut peers = BTreeSet::new();

    for value in db
        .prepare(
            "SELECT DISTINCT domain
             FROM remote_actors
             WHERE domain IS NOT NULL
               AND trim(domain) != ''",
        )
        .all()
        .await?
        .results::<serde_json::Value>()?
    {
        if let Some(domain) = value.get("domain").and_then(serde_json::Value::as_str) {
            let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
            if !domain.is_empty() && domain != instance_host(config) {
                peers.insert(domain);
            }
        }
    }

    for (sql, field) in [
        (
            "SELECT DISTINCT target_actor_uri AS actor_uri
             FROM follows
             WHERE target_actor_uri IS NOT NULL
               AND trim(target_actor_uri) != ''",
            "actor_uri",
        ),
        (
            "SELECT DISTINCT actor_uri
             FROM followers
             WHERE actor_uri IS NOT NULL
               AND trim(actor_uri) != ''",
            "actor_uri",
        ),
    ] {
        for value in db
            .prepare(sql)
            .all()
            .await?
            .results::<serde_json::Value>()?
        {
            if let Some(uri) = value.get(field).and_then(serde_json::Value::as_str)
                && let Some(peer) = peer_authority_from_uri(config, uri)
            {
                peers.insert(peer);
            }
        }
    }

    Ok(peers.into_iter().collect())
}
