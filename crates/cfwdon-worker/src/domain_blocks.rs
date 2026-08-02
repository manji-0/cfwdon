use crate::instance::load_known_peer_domains;
use crate::profile::require_authenticated_local_account;
use crate::request_utils::{build_internal_cursor_link_header, parse_internal_pagination_id};
use crate::runtime_config::load_config;
use serde::Deserialize;
use std::collections::HashSet;
use url::Url;
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};

use crate::D1Database;
const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 200;
const DEFAULT_PREVIEW_LIMIT: u32 = 20;
const MAX_PREVIEW_LIMIT: u32 = 100;

#[derive(Debug, Default, Deserialize)]
struct DomainBlocksQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DomainBlockRequest {
    domain: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DomainBlocksPreviewQuery {
    q: Option<String>,
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct DomainBlockEntryRow {
    id: i64,
    domain: String,
}

async fn parse_domain_block_request(req: &mut Request) -> std::result::Result<String, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let domain = if content_type.contains("application/json") {
        req.json::<DomainBlockRequest>()
            .await
            .map_err(|error| format!("invalid JSON domain block payload: {error}"))?
            .domain
    } else if content_type.trim().is_empty() {
        None
    } else {
        req.form_data()
            .await
            .map_err(|error| format!("invalid form domain block payload: {error}"))?
            .get_field("domain")
    };

    normalize_domain_block_value(domain.as_deref())
}

fn normalize_domain_block_value(value: Option<&str>) -> std::result::Result<String, String> {
    let Some(domain) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err("Validation failed: Domain can't be blank".to_owned());
    };
    if domain.chars().any(char::is_whitespace) {
        return Err("Validation failed: Domain is not a valid domain name".to_owned());
    }
    Ok(domain.to_ascii_lowercase())
}

pub(crate) fn delivery_inbox_blocked_by_domains(
    inbox_url: &str,
    blocked_domains: &[String],
) -> bool {
    if blocked_domains.is_empty() {
        return false;
    }
    let Ok(url) = Url::parse(inbox_url) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    blocked_domains.iter().any(|blocked| {
        let blocked = blocked.trim().to_ascii_lowercase();
        !blocked.is_empty() && (host == blocked || host.ends_with(&format!(".{blocked}")))
    })
}

pub(crate) fn filter_delivery_inboxes_for_domain_blocks(
    inboxes: Vec<String>,
    blocked_domains: &[String],
) -> Vec<String> {
    inboxes
        .into_iter()
        .filter(|inbox| !delivery_inbox_blocked_by_domains(inbox, blocked_domains))
        .collect()
}

async fn list_account_domain_blocks(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<DomainBlockEntryRow>> {
    let bindings = [
        D1Type::Text(account_id),
        max_id
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
        since_id
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, domain
             FROM account_domain_blocks
             WHERE account_id = ?1
               AND (?2 IS NULL OR id < ?2)
               AND (?3 IS NULL OR id > ?3)
             ORDER BY id DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<DomainBlockEntryRow>()
}

async fn insert_account_domain_block(
    db: &D1Database,
    account_id: &str,
    domain: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(domain)];
    db.prepare(
        "INSERT INTO account_domain_blocks (account_id, domain)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, domain) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    crate::invalidate_account_capabilities(account_id).await;
    Ok(())
}

pub(crate) async fn list_all_account_domain_blocks(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    if !crate::load_account_capabilities(db, account_id)
        .await?
        .has_domain_blocks
    {
        return Ok(Vec::new());
    }

    let bindings = [D1Type::Text(account_id)];
    let result = db
        .prepare(
            "SELECT domain
             FROM account_domain_blocks
             WHERE account_id = ?1",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<serde_json::Value>()?
        .into_iter()
        .filter_map(|value| {
            value
                .get("domain")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect())
}

async fn delete_account_domain_block(
    db: &D1Database,
    account_id: &str,
    domain: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(domain)];
    db.prepare(
        "DELETE FROM account_domain_blocks
         WHERE account_id = ?1
           AND domain = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    crate::invalidate_account_capabilities(account_id).await;
    Ok(())
}

fn domain_block_preview_candidates(
    known_peer_domains: Vec<String>,
    blocked_domains: Vec<String>,
    q: Option<&str>,
    limit: u32,
) -> Vec<String> {
    let blocked = blocked_domains
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let query = q
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);

    known_peer_domains
        .into_iter()
        .filter(|domain| !blocked.contains(&domain.to_ascii_lowercase()))
        .filter(|domain| {
            query
                .as_ref()
                .is_none_or(|needle| domain.to_ascii_lowercase().contains(needle))
        })
        .take(limit as usize)
        .collect()
}

pub(crate) async fn domain_blocks_preview_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => {
            let query: DomainBlocksPreviewQuery = req.query().unwrap_or_default();
            let limit = query
                .limit
                .unwrap_or(DEFAULT_PREVIEW_LIMIT)
                .clamp(1, MAX_PREVIEW_LIMIT);
            let blocked_domains = list_all_account_domain_blocks(&db, account.id()).await?;
            let candidates = domain_block_preview_candidates(
                load_known_peer_domains(&db, &config).await?,
                blocked_domains,
                query.q.as_deref(),
                limit,
            );
            Response::from_json(&candidates)
        }
        None => Response::error("Auth0 authentication required", 401),
    }
}

pub(crate) async fn domain_blocks_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => {
            let query: DomainBlocksQuery = req.query().unwrap_or_default();
            let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
            let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
            let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
            let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
            let since_id = since_id.or(min_id);
            let blocks =
                list_account_domain_blocks(&db, account.id(), limit, max_id, since_id).await?;
            let domains = blocks
                .iter()
                .map(|entry| entry.domain.clone())
                .collect::<Vec<_>>();

            let mut builder = Response::builder();
            if let Some(link_header) = build_internal_cursor_link_header(
                &req,
                limit,
                blocks.first().map(|entry| entry.id),
                blocks.last().map(|entry| entry.id),
                blocks.len() as u32 >= limit,
                max_id.is_some() || since_id.is_some(),
            )? {
                builder = builder.with_header("Link", &link_header)?;
            }

            builder.from_json(&domains)
        }
        None => Response::error("Auth0 authentication required", 401),
    }
}

pub(crate) async fn create_domain_block_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => {
            let domain = match parse_domain_block_request(&mut req).await {
                Ok(domain) => domain,
                Err(message) => return Response::error(&message, 422),
            };
            insert_account_domain_block(&db, account.id(), &domain).await?;
            Response::from_json(&serde_json::json!({}))
        }
        None => Response::error("Auth0 authentication required", 401),
    }
}

pub(crate) async fn delete_domain_block_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => {
            let domain = match parse_domain_block_request(&mut req).await {
                Ok(domain) => domain,
                Err(message) => return Response::error(&message, 422),
            };
            delete_account_domain_block(&db, account.id(), &domain).await?;
            Response::from_json(&serde_json::json!({}))
        }
        None => Response::error("Auth0 authentication required", 401),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        delivery_inbox_blocked_by_domains, domain_block_preview_candidates,
        filter_delivery_inboxes_for_domain_blocks,
    };

    #[test]
    fn domain_block_preview_excludes_blocked_domains() {
        let candidates = domain_block_preview_candidates(
            vec![
                "alpha.example".to_owned(),
                "beta.example".to_owned(),
                "gamma.example".to_owned(),
            ],
            vec!["beta.example".to_owned()],
            None,
            20,
        );

        assert_eq!(candidates, vec!["alpha.example", "gamma.example"]);
    }

    #[test]
    fn domain_block_preview_filters_by_query_and_limit() {
        let candidates = domain_block_preview_candidates(
            vec![
                "alpha.example".to_owned(),
                "alpaca.social".to_owned(),
                "beta.example".to_owned(),
            ],
            Vec::new(),
            Some("ALP"),
            1,
        );

        assert_eq!(candidates, vec!["alpha.example"]);
    }

    #[test]
    fn delivery_inbox_domain_block_matches_host_and_subdomain() {
        let blocked = vec!["blocked.example".to_owned()];
        assert!(delivery_inbox_blocked_by_domains(
            "https://blocked.example/inbox",
            &blocked
        ));
        assert!(delivery_inbox_blocked_by_domains(
            "https://sub.blocked.example/users/a/inbox",
            &blocked
        ));
        assert!(!delivery_inbox_blocked_by_domains(
            "https://safe.example/inbox",
            &blocked
        ));
        assert_eq!(
            filter_delivery_inboxes_for_domain_blocks(
                vec![
                    "https://blocked.example/inbox".to_owned(),
                    "https://safe.example/inbox".to_owned(),
                ],
                &blocked,
            ),
            vec!["https://safe.example/inbox".to_owned()]
        );
    }
}
