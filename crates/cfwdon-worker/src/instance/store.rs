use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use super::{
    AppConfig, InstanceCapabilities, InstanceSummary, SoftwareInfo, build_metadata, instance_host,
    normalize_instance_domain, peer_authority_from_uri,
};
use serde::{Deserialize, Serialize};
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
use crate::app_cache::app_cache_kv;

const INSTANCE_SETTINGS_KV_KEY: &str = "instance_settings:v1";
/// Settings change only via migration/ops; keep KV past a missed hourly window.
const INSTANCE_SETTINGS_TTL_SECS: u64 = 21_600;

thread_local! {
    static INSTANCE_SETTINGS_L1: RefCell<Option<InstanceSettingsRow>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InstanceSettingsRow {
    pub(crate) domain: String,
    pub(crate) title: String,
    pub(crate) description: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ActiveMonthCountRow {
    pub(crate) count: u64,
}

#[derive(Debug, Deserialize)]
struct WeekOffsetCountRow {
    week_offset: i64,
    count: u64,
}

fn instance_settings_from_l1() -> Option<InstanceSettingsRow> {
    INSTANCE_SETTINGS_L1.with(|slot| slot.borrow().clone())
}

fn store_instance_settings_l1(settings: InstanceSettingsRow) {
    INSTANCE_SETTINGS_L1.with(|slot| {
        *slot.borrow_mut() = Some(settings);
    });
}

async fn kv_get_instance_settings() -> Option<InstanceSettingsRow> {
    let kv = app_cache_kv()?;
    let text = kv.get(INSTANCE_SETTINGS_KV_KEY).text().await.ok()??;
    serde_json::from_str(&text).ok()
}

async fn kv_put_instance_settings(settings: &InstanceSettingsRow) {
    let Some(kv) = app_cache_kv() else {
        return;
    };
    let Ok(body) = serde_json::to_string(settings) else {
        return;
    };
    let Ok(putter) = kv.put(INSTANCE_SETTINGS_KV_KEY, body) else {
        return;
    };
    let _ = putter
        .expiration_ttl(INSTANCE_SETTINGS_TTL_SECS)
        .execute()
        .await;
}

async fn load_instance_settings_row(db: &D1Database) -> Result<Option<InstanceSettingsRow>> {
    if let Some(settings) = instance_settings_from_l1() {
        return Ok(Some(settings));
    }
    if let Some(settings) = kv_get_instance_settings().await {
        store_instance_settings_l1(settings.clone());
        return Ok(Some(settings));
    }

    let settings = db
        .prepare(
            "SELECT domain, title, description
             FROM instance_settings
             WHERE id = 1
             LIMIT 1",
        )
        .first::<InstanceSettingsRow>(None)
        .await?;
    if let Some(settings) = settings.as_ref() {
        store_instance_settings_l1(settings.clone());
        kv_put_instance_settings(settings).await;
    }
    Ok(settings)
}

pub(crate) async fn load_instance_summary(
    db: &D1Database,
    config: AppConfig,
) -> Result<InstanceSummary> {
    let build = build_metadata();
    let settings = load_instance_settings_row(db).await?;

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

async fn count_rows_by_week_offset(
    db: &D1Database,
    table: &str,
    week_floor_rfc3339: &str,
    range_start: &str,
    range_end: &str,
) -> Result<HashMap<u32, u64>> {
    let bindings = [
        D1Type::Text(week_floor_rfc3339),
        D1Type::Text(range_start),
        D1Type::Text(range_end),
    ];
    let rows = db
        .prepare(format!(
            "SELECT
                CAST((strftime('%s', datetime(?1)) - strftime('%s', datetime(created_at))) / 604800 AS INTEGER) AS week_offset,
                COUNT(*) AS count
             FROM {table}
             WHERE datetime(created_at) >= datetime(?2)
               AND datetime(created_at) < datetime(?3)
             GROUP BY week_offset
             HAVING week_offset >= 0 AND week_offset < 12"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<WeekOffsetCountRow>(&__d1))?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            u32::try_from(row.week_offset)
                .ok()
                .map(|week_offset| (week_offset, row.count))
        })
        .collect())
}

pub(crate) async fn count_local_statuses_by_week_offset(
    db: &D1Database,
    week_floor_rfc3339: &str,
    range_start: &str,
    range_end: &str,
) -> Result<HashMap<u32, u64>> {
    count_rows_by_week_offset(db, "statuses", week_floor_rfc3339, range_start, range_end).await
}

pub(crate) async fn count_accounts_created_by_week_offset(
    db: &D1Database,
    week_floor_rfc3339: &str,
    range_start: &str,
    range_end: &str,
) -> Result<HashMap<u32, u64>> {
    count_rows_by_week_offset(db, "accounts", week_floor_rfc3339, range_start, range_end).await
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
        .await
        .and_then(|__d1| crate::d1_results::<serde_json::Value>(&__d1))?
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
            .await
            .and_then(|__d1| crate::d1_results::<serde_json::Value>(&__d1))?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_settings_kv_key_is_versioned() {
        assert_eq!(INSTANCE_SETTINGS_KV_KEY, "instance_settings:v1");
    }

    #[test]
    fn instance_settings_ttl_covers_several_hours() {
        assert_eq!(INSTANCE_SETTINGS_TTL_SECS, 21_600);
    }

    #[test]
    fn instance_settings_row_round_trips_json() {
        let settings = InstanceSettingsRow {
            domain: "fedi.example".to_owned(),
            title: "Example".to_owned(),
            description: "hello".to_owned(),
        };
        let encoded = serde_json::to_string(&settings).expect("encode");
        let decoded: InstanceSettingsRow = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, settings);
    }

    #[test]
    fn instance_settings_l1_round_trips() {
        INSTANCE_SETTINGS_L1.with(|slot| *slot.borrow_mut() = None);
        assert!(instance_settings_from_l1().is_none());
        let settings = InstanceSettingsRow {
            domain: "fedi.example".to_owned(),
            title: "Example".to_owned(),
            description: "hello".to_owned(),
        };
        store_instance_settings_l1(settings.clone());
        assert_eq!(instance_settings_from_l1(), Some(settings));
        INSTANCE_SETTINGS_L1.with(|slot| *slot.borrow_mut() = None);
    }
}
