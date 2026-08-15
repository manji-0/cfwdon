//! Materialized JSON payloads for expensive anonymous public read endpoints.
//!
//! Populated by the hourly scheduled Worker cron so Mastodon web UI polling hits
//! a single-row D1 read instead of heavy scans.

use serde::Deserialize;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;

pub(crate) const PUBLIC_CACHE_INSTANCE_ACTIVITY: &str = "instance_activity";
pub(crate) const PUBLIC_CACHE_PUBLIC_TIMELINE: &str = "public_timeline";
/// Max public-timeline page is 40; keep one extra row so Link `next` stays accurate.
pub(crate) const PUBLIC_TIMELINE_CACHE_SIZE: u32 = 41;

#[derive(Debug, Deserialize)]
struct PublicEndpointCacheRow {
    payload_json: String,
}

pub(crate) async fn load_public_endpoint_cache(
    db: &D1Database,
    id: &str,
) -> Result<Option<serde_json::Value>> {
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

    serde_json::from_str(&row.payload_json)
        .map(Some)
        .map_err(|error| {
            worker::Error::RustError(format!("invalid public endpoint cache ({id}): {error}"))
        })
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
}
