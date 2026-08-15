//! Materialized trend payloads in `APP_CACHE` KV.
//!
//! Cron refreshes every 6 hours; reads avoid D1 except on cache miss.

use worker::Result;

use crate::app_cache::app_cache_kv;

pub(crate) const TRENDING_TAGS_CACHE_SIZE: u32 = 200;
pub(crate) const TRENDING_STATUSES_CACHE_SIZE: u32 = 40;

const TRENDS_TAGS_KEY: &str = "trends:tags:v1";
const TRENDS_STATUSES_KEY: &str = "trends:statuses:v1";
const TRENDS_CACHE_TTL_GRACE_SECS: u64 = 3_600;

fn trends_cache_ttl_secs() -> u64 {
    u64::from(crate::CACHE_TTL_TRENDS).saturating_add(TRENDS_CACHE_TTL_GRACE_SECS)
}

pub(crate) async fn load_trending_tags_cache() -> Option<Vec<serde_json::Value>> {
    kv_get_json_array(TRENDS_TAGS_KEY).await
}

pub(crate) async fn store_trending_tags_cache(documents: &[serde_json::Value]) -> Result<()> {
    kv_put_json_array(TRENDS_TAGS_KEY, documents).await
}

pub(crate) async fn load_trending_statuses_cache() -> Option<Vec<serde_json::Value>> {
    kv_get_json_array(TRENDS_STATUSES_KEY).await
}

pub(crate) async fn store_trending_statuses_cache(documents: &[serde_json::Value]) -> Result<()> {
    kv_put_json_array(TRENDS_STATUSES_KEY, documents).await
}

pub(crate) fn slice_trending_cache(
    documents: Vec<serde_json::Value>,
    offset: u32,
    limit: u32,
) -> Vec<serde_json::Value> {
    documents
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect()
}

async fn kv_get_json_array(key: &str) -> Option<Vec<serde_json::Value>> {
    let kv = app_cache_kv()?;
    let text = kv.get(key).text().await.ok()??;
    match serde_json::from_str::<serde_json::Value>(&text).ok()? {
        serde_json::Value::Array(items) => Some(items),
        _ => None,
    }
}

async fn kv_put_json_array(key: &str, documents: &[serde_json::Value]) -> Result<()> {
    let Some(kv) = app_cache_kv() else {
        return Ok(());
    };
    let body = serde_json::to_string(documents).map_err(|error| {
        worker::Error::RustError(format!("encode trends cache ({key}): {error}"))
    })?;
    kv.put(key, body)?
        .expiration_ttl(trends_cache_ttl_secs())
        .execute()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_trending_cache_applies_offset_and_limit() {
        let documents = vec![
            serde_json::json!({"name": "a"}),
            serde_json::json!({"name": "b"}),
            serde_json::json!({"name": "c"}),
        ];
        let sliced = slice_trending_cache(documents, 1, 1);
        assert_eq!(sliced.len(), 1);
        assert_eq!(sliced[0]["name"], "b");
    }

    #[test]
    fn trends_cache_ttl_extends_http_ttl() {
        assert_eq!(
            trends_cache_ttl_secs(),
            u64::from(crate::CACHE_TTL_TRENDS) + 3_600
        );
    }
}
