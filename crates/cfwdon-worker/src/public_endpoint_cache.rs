//! Materialized JSON payloads for expensive anonymous public read endpoints.
//!
//! Hourly cron writes D1 as the source of truth and mirrors the payload into
//! `APP_CACHE` KV so user-facing reads skip D1 when the KV binding is present.

use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;
use crate::app_cache::app_cache_kv;

pub(crate) const PUBLIC_CACHE_INSTANCE_ACTIVITY: &str = "instance_activity";
pub(crate) const PUBLIC_CACHE_PUBLIC_TIMELINE: &str = "public_timeline";
/// Max public-timeline page is 40; keep one extra row so Link `next` stays accurate.
pub(crate) const PUBLIC_TIMELINE_CACHE_SIZE: u32 = 41;

const PUBLIC_ENDPOINT_CACHE_KEY_PREFIX: &str = "pec:v1:";
/// Hourly cron at minute 17; keep KV past one missed refresh.
const PUBLIC_ENDPOINT_CACHE_REFRESH_SECS: u64 = 3_600;
const PUBLIC_ENDPOINT_CACHE_TTL_GRACE_SECS: u64 = 3_600;

#[derive(Debug, Deserialize)]
struct PublicEndpointCacheRow {
    payload_json: String,
}

fn public_endpoint_cache_kv_key(id: &str) -> String {
    format!("{PUBLIC_ENDPOINT_CACHE_KEY_PREFIX}{id}")
}

fn public_endpoint_cache_ttl_secs() -> u64 {
    PUBLIC_ENDPOINT_CACHE_REFRESH_SECS.saturating_add(PUBLIC_ENDPOINT_CACHE_TTL_GRACE_SECS)
}

async fn kv_get_public_endpoint_cache(id: &str) -> Option<serde_json::Value> {
    let kv = app_cache_kv()?;
    let text = kv
        .get(&public_endpoint_cache_kv_key(id))
        .text()
        .await
        .ok()??;
    serde_json::from_str(&text).ok()
}

async fn kv_put_public_endpoint_cache_json(id: &str, payload_json: &str) {
    let Some(kv) = app_cache_kv() else {
        return;
    };
    let Ok(putter) = kv.put(&public_endpoint_cache_kv_key(id), payload_json.to_owned()) else {
        return;
    };
    let _ = putter
        .expiration_ttl(public_endpoint_cache_ttl_secs())
        .execute()
        .await;
}

pub(crate) async fn load_public_endpoint_cache(
    db: &D1Database,
    id: &str,
) -> Result<Option<serde_json::Value>> {
    if let Some(payload) = kv_get_public_endpoint_cache(id).await {
        return Ok(Some(payload));
    }

    let bindings = [D1Type::Text(id)];
    let Some(row) = db
        .prepare(
            "SELECT payload_json
             FROM public_endpoint_cache
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<PublicEndpointCacheRow>(None)
        .await?
    else {
        return Ok(None);
    };

    let payload = serde_json::from_str(&row.payload_json).map_err(|error| {
        worker::Error::RustError(format!("invalid public endpoint cache ({id}): {error}"))
    })?;
    kv_put_public_endpoint_cache_json(id, &row.payload_json).await;
    Ok(Some(payload))
}

pub(crate) async fn store_public_endpoint_cache(
    db: &D1Database,
    id: &str,
    payload: &serde_json::Value,
) -> Result<()> {
    let payload_json = serde_json::to_string(payload).map_err(|error| {
        worker::Error::RustError(format!("encode public cache ({id}): {error}"))
    })?;
    let now = crate::now_iso_string()?;
    let bindings = [
        D1Type::Text(id),
        D1Type::Text(&payload_json),
        D1Type::Text(&now),
    ];
    db.prepare(
        "INSERT INTO public_endpoint_cache (id, payload_json, computed_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
             payload_json = excluded.payload_json,
             computed_at = excluded.computed_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    kv_put_public_endpoint_cache_json(id, &payload_json).await;
    Ok(())
}

pub(crate) fn slice_json_array_cache(
    payload: serde_json::Value,
    offset: u32,
    limit: u32,
) -> Vec<serde_json::Value> {
    match payload {
        serde_json::Value::Array(items) => items
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect(),
        other => vec![other],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_json_array_cache_pages_array_payload() {
        let payload = serde_json::json!([1, 2, 3, 4, 5]);
        assert_eq!(
            slice_json_array_cache(payload, 1, 2),
            vec![serde_json::json!(2), serde_json::json!(3)]
        );
    }

    #[test]
    fn slice_json_array_cache_wraps_non_array() {
        let payload = serde_json::json!({"ok": true});
        assert_eq!(
            slice_json_array_cache(payload, 0, 10),
            vec![serde_json::json!({"ok": true})]
        );
    }

    #[test]
    fn public_endpoint_cache_kv_key_is_versioned() {
        assert_eq!(
            public_endpoint_cache_kv_key(PUBLIC_CACHE_PUBLIC_TIMELINE),
            "pec:v1:public_timeline"
        );
        assert_eq!(
            public_endpoint_cache_kv_key(PUBLIC_CACHE_INSTANCE_ACTIVITY),
            "pec:v1:instance_activity"
        );
    }

    #[test]
    fn public_endpoint_cache_ttl_covers_one_missed_hourly_refresh() {
        assert_eq!(public_endpoint_cache_ttl_secs(), 7_200);
    }
}
