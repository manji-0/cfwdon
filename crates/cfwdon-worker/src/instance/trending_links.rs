use super::{
    CACHE_TTL_TRENDS, Request, Response, Result, RouteContext, TrendsQuery,
    build_status_card_value, cache_public_response, canonicalize_link_timeline_url, load_config,
    now_unix_timestamp,
};
use std::collections::{HashMap, HashSet};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use worker::d1::D1Type;

#[derive(Debug, serde::Deserialize)]
struct TrendingLocalLinkRow {
    text_content: String,
    created_at: String,
    account_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct TrendingRemoteLinkRow {
    text_content: String,
    published_at: String,
    actor_uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrendingLinkCandidate {
    url: String,
    account_key: String,
    published_at: String,
}

#[derive(Debug, Default)]
struct TrendingLinkAggregate {
    latest_timestamp: i64,
    total_uses: u64,
    accounts: HashSet<String>,
    uses_by_day: HashMap<i64, u64>,
    accounts_by_day: HashMap<i64, HashSet<String>>,
}

const TRENDING_LINK_HISTORY_DAYS: usize = 7;
pub(crate) async fn trending_links_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: TrendsQuery = req.query().unwrap_or_default();
    let db = crate::bind_request_d1(&ctx, &config)?;
    cache_public_response(
        Response::from_json(
            &list_trending_link_documents(
                &db,
                query.limit.unwrap_or(10).clamp(1, 20),
                query.offset.unwrap_or(0),
            )
            .await?,
        )?,
        CACHE_TTL_TRENDS,
    )
}
pub(crate) async fn trending_link_target_is_known(
    db: &crate::D1Database,
    target_urls: &[String],
) -> Result<bool> {
    let known = list_trending_link_entries(db).await?;
    let targets = target_urls
        .iter()
        .filter_map(|url| canonicalize_link_timeline_url(url))
        .collect::<HashSet<_>>();
    Ok(known.iter().any(|entry| targets.contains(&entry.url)))
}

async fn list_trending_link_documents(
    db: &crate::D1Database,
    limit: u32,
    offset: u32,
) -> Result<Vec<serde_json::Value>> {
    Ok(list_trending_link_entries(db)
        .await?
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|entry| entry.document)
        .collect())
}

async fn list_trending_link_entries(db: &crate::D1Database) -> Result<Vec<TrendingLinkEntry>> {
    // Prefer js_sys::Date via now_unix_timestamp — std SystemTime panics on wasm32.
    let now_ts = now_unix_timestamp();
    let now = OffsetDateTime::from_unix_timestamp(now_ts).map_err(|error| {
        worker::Error::RustError(format!("invalid current unix timestamp: {error}"))
    })?;
    let cutoff = (now - Duration::days(TRENDING_LINK_HISTORY_DAYS as i64))
        .format(&Rfc3339)
        .map_err(|error| {
            worker::Error::RustError(format!("failed to format trending link cutoff: {error}"))
        })?;
    let today = unix_day_bucket(now_ts);
    let mut candidates = Vec::new();

    for row in list_trending_local_link_rows(db, &cutoff).await? {
        if let Some(candidate) =
            build_trending_link_candidate(&row.text_content, &row.created_at, &row.account_id)
        {
            candidates.push(candidate);
        }
    }

    for row in list_trending_remote_link_rows(db, &cutoff).await? {
        if let Some(candidate) =
            build_trending_link_candidate(&row.text_content, &row.published_at, &row.actor_uri)
        {
            candidates.push(candidate);
        }
    }

    Ok(build_trending_link_entries(candidates, today))
}

async fn list_trending_local_link_rows(
    db: &crate::D1Database,
    cutoff: &str,
) -> Result<Vec<TrendingLocalLinkRow>> {
    let bindings = [D1Type::Text(cutoff)];
    db.prepare(
        "SELECT s.text_content, s.created_at, s.account_id
         FROM statuses s
         JOIN accounts a ON a.id = s.account_id
         WHERE s.visibility = 'public'
           AND a.discoverable = 1
           AND s.created_at >= ?1",
    )
    .bind_refs(&bindings)?
    .all()
    .await
    .and_then(|__d1| crate::d1_results::<TrendingLocalLinkRow>(&__d1))
}

async fn list_trending_remote_link_rows(
    db: &crate::D1Database,
    cutoff: &str,
) -> Result<Vec<TrendingRemoteLinkRow>> {
    let bindings = [D1Type::Text(cutoff)];
    db.prepare(
        "SELECT rs.text_content, rs.published_at, rs.actor_uri
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND ra.discoverable = 1
           AND rs.published_at >= ?1",
    )
    .bind_refs(&bindings)?
    .all()
    .await
    .and_then(|__d1| crate::d1_results::<TrendingRemoteLinkRow>(&__d1))
}

fn build_trending_link_candidate(
    text: &str,
    published_at: &str,
    account_key: &str,
) -> Option<TrendingLinkCandidate> {
    let url = build_status_card_value(text)?
        .get("url")
        .and_then(serde_json::Value::as_str)
        .and_then(canonicalize_link_timeline_url)?;
    Some(TrendingLinkCandidate {
        url,
        account_key: account_key.to_owned(),
        published_at: published_at.to_owned(),
    })
}

#[derive(Debug, Clone)]
struct TrendingLinkEntry {
    url: String,
    latest_timestamp: i64,
    total_uses: u64,
    total_accounts: usize,
    document: serde_json::Value,
}

fn build_trending_link_entries(
    candidates: Vec<TrendingLinkCandidate>,
    today_bucket: i64,
) -> Vec<TrendingLinkEntry> {
    let mut aggregates = HashMap::<String, TrendingLinkAggregate>::new();

    for candidate in candidates {
        let Some(published_at) = parse_unix_timestamp(&candidate.published_at) else {
            continue;
        };
        let day = unix_day_bucket(published_at);
        let entry = aggregates.entry(candidate.url).or_default();
        entry.latest_timestamp = entry.latest_timestamp.max(published_at);
        entry.total_uses += 1;
        entry.accounts.insert(candidate.account_key.clone());
        *entry.uses_by_day.entry(day).or_insert(0) += 1;
        entry
            .accounts_by_day
            .entry(day)
            .or_default()
            .insert(candidate.account_key);
    }

    let mut entries = aggregates
        .into_iter()
        .filter_map(|(url, aggregate)| {
            let mut document = build_status_card_value(&url)?;
            document["history"] =
                serde_json::json!(build_trending_link_history(&aggregate, today_bucket,));
            Some(TrendingLinkEntry {
                url,
                latest_timestamp: aggregate.latest_timestamp,
                total_uses: aggregate.total_uses,
                total_accounts: aggregate.accounts.len(),
                document,
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        right
            .total_uses
            .cmp(&left.total_uses)
            .then_with(|| right.total_accounts.cmp(&left.total_accounts))
            .then_with(|| right.latest_timestamp.cmp(&left.latest_timestamp))
            .then_with(|| left.url.cmp(&right.url))
    });
    entries
}

fn build_trending_link_history(
    aggregate: &TrendingLinkAggregate,
    today_bucket: i64,
) -> Vec<serde_json::Value> {
    (0..TRENDING_LINK_HISTORY_DAYS)
        .map(|offset| {
            let day = today_bucket - (offset as i64 * 86_400);
            let uses = aggregate.uses_by_day.get(&day).copied().unwrap_or(0);
            let accounts = aggregate
                .accounts_by_day
                .get(&day)
                .map(|accounts| accounts.len() as u64)
                .unwrap_or(0);
            serde_json::json!({
                "day": day.to_string(),
                "accounts": accounts.to_string(),
                "uses": uses.to_string(),
            })
        })
        .collect()
}

fn parse_unix_timestamp(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|timestamp| timestamp.unix_timestamp())
}

fn unix_day_bucket(timestamp: i64) -> i64 {
    timestamp.div_euclid(86_400) * 86_400
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_trending_link_candidate_normalizes_tracking_and_fragment() {
        let candidate = build_trending_link_candidate(
            "see https://example.com/post?utm_source=feed#part",
            "2026-04-21T12:00:00Z",
            "acct-1",
        )
        .unwrap();
        assert_eq!(candidate.url, "https://example.com/post");
    }

    #[test]
    fn build_trending_link_entries_sorts_by_usage_then_accounts() {
        let today_bucket = unix_day_bucket(parse_unix_timestamp("2026-04-21T12:00:00Z").unwrap());
        let entries = build_trending_link_entries(
            vec![
                TrendingLinkCandidate {
                    url: "https://example.com/b".to_owned(),
                    account_key: "acct-1".to_owned(),
                    published_at: "2026-04-21T12:00:00Z".to_owned(),
                },
                TrendingLinkCandidate {
                    url: "https://example.com/a".to_owned(),
                    account_key: "acct-1".to_owned(),
                    published_at: "2026-04-21T11:00:00Z".to_owned(),
                },
                TrendingLinkCandidate {
                    url: "https://example.com/a".to_owned(),
                    account_key: "acct-2".to_owned(),
                    published_at: "2026-04-21T10:00:00Z".to_owned(),
                },
            ],
            today_bucket,
        );

        assert_eq!(entries[0].url, "https://example.com/a");
        assert_eq!(entries[0].total_uses, 2);
        assert_eq!(
            entries[0].document["history"][0]["uses"],
            serde_json::json!("2")
        );
        assert_eq!(entries[1].url, "https://example.com/b");
    }
}
