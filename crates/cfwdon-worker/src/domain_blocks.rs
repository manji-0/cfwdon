use crate::app_cache::{invalidate_account_capabilities, load_account_capabilities};
use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::db_utils::d1_results;
use crate::federation::{
    invalidate_suspended_domains, suspended_instance_domains, url_host_within_blocked_domains,
};
use crate::request_utils::{
    InternalPaginationIds, build_internal_cursor_link_header, empty_page_response,
};
use crate::runtime_config::load_config;
use crate::store::instance::load_known_peer_domains;
use crate::tracked_d1::D1Database;
use cfwdon_domain::DomainBlockSeverity;
use serde::Deserialize;
use std::collections::HashSet;
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};
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

pub(crate) fn filter_delivery_inboxes_for_domain_blocks(
    inboxes: Vec<String>,
    blocked_domains: &[String],
) -> Vec<String> {
    inboxes
        .into_iter()
        .filter(|inbox| !url_host_within_blocked_domains(inbox, blocked_domains))
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

    d1_results::<DomainBlockEntryRow>(&result)
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
    invalidate_account_capabilities(account_id).await;
    Ok(())
}

pub(crate) async fn list_all_account_domain_blocks(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    if !load_account_capabilities(db, account_id)
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

    Ok(d1_results::<serde_json::Value>(&result)?
        .into_iter()
        .filter_map(|value| {
            value
                .get("domain")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect())
}

/// Domains `account_id` must not deliver to: its own domain blocks plus the
/// instance's suspended domains.
pub(crate) async fn list_delivery_blocked_domains(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    let mut domains = list_all_account_domain_blocks(db, account_id).await?;
    domains.extend(suspended_instance_domains().await?.iter().cloned());
    Ok(domains)
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
    invalidate_account_capabilities(account_id).await;
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
    let db = bind_request_d1(&ctx, &config)?;
    match find_authenticated_local_account(&req, &db, &config).await? {
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
    let db = bind_request_d1(&ctx, &config)?;
    match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => {
            let query: DomainBlocksQuery = req.query().unwrap_or_default();
            let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
            let Some(InternalPaginationIds {
                max_id,
                since_id,
                min_id,
            }) = InternalPaginationIds::parse(
                query.max_id.as_deref(),
                query.since_id.as_deref(),
                query.min_id.as_deref(),
            )
            else {
                return empty_page_response();
            };
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
    let db = bind_request_d1(&ctx, &config)?;
    match find_authenticated_local_account(&req, &db, &config).await? {
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
    let db = bind_request_d1(&ctx, &config)?;
    match find_authenticated_local_account(&req, &db, &config).await? {
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

/// An `instance_domain_blocks` row as the admin APIs read it.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct InstanceDomainBlockRow {
    pub(crate) id: i64,
    pub(crate) domain: String,
    pub(crate) created_at: String,
    pub(crate) severity: String,
    pub(crate) reject_media: i64,
    pub(crate) reject_reports: i64,
    pub(crate) obfuscate: i64,
    pub(crate) private_comment: Option<String>,
    pub(crate) public_comment: Option<String>,
}

/// Everything about a block except its domain, which never changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstanceDomainBlockInput {
    pub(crate) severity: DomainBlockSeverity,
    pub(crate) reject_media: bool,
    pub(crate) reject_reports: bool,
    pub(crate) obfuscate: bool,
    pub(crate) private_comment: Option<String>,
    pub(crate) public_comment: Option<String>,
}

impl Default for InstanceDomainBlockInput {
    /// Mastodon's defaults for a new block.
    fn default() -> Self {
        Self {
            severity: DomainBlockSeverity::Silence,
            reject_media: false,
            reject_reports: false,
            obfuscate: false,
            private_comment: None,
            public_comment: None,
        }
    }
}

const INSTANCE_DOMAIN_BLOCK_COLUMNS: &str = "id, domain, created_at, severity, reject_media,
    reject_reports, obfuscate, private_comment, public_comment";

pub(crate) fn normalize_instance_domain(domain: &str) -> Option<String> {
    let normalized = domain
        .trim()
        .trim_start_matches('@')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let valid = !normalized.is_empty()
        && !normalized
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '/' | '@' | ':' | '?' | '#'));
    valid.then_some(normalized)
}

fn invalid_domain_error() -> worker::Error {
    worker::Error::RustError("invalid domain".to_owned())
}

fn optional_id_binding(value: Option<i64>) -> D1Type<'static> {
    value.map_or(D1Type::Null, |id| {
        D1Type::Integer(i32::try_from(id).unwrap_or(if id < 0 { i32::MIN } else { i32::MAX }))
    })
}

/// A page of blocks, newest first, under Mastodon's id cursors. `min_id`
/// pages forward from the cursor and is still answered newest first.
pub(crate) async fn list_instance_domain_blocks(
    db: &D1Database,
    limit: u32,
    pagination: InternalPaginationIds,
) -> Result<Vec<InstanceDomainBlockRow>> {
    let limit = i32::try_from(limit.clamp(1, MAX_LIMIT)).unwrap_or(100);
    let order = if pagination.min_id.is_some() {
        "ASC"
    } else {
        "DESC"
    };
    let bindings = [
        optional_id_binding(pagination.max_id),
        optional_id_binding(pagination.since_id.or(pagination.min_id)),
        D1Type::Integer(limit),
    ];
    let result = db
        .prepare(format!(
            "SELECT {INSTANCE_DOMAIN_BLOCK_COLUMNS}
             FROM instance_domain_blocks
             WHERE (?1 IS NULL OR id < ?1)
               AND (?2 IS NULL OR id > ?2)
             ORDER BY id {order}
             LIMIT ?3"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    let mut rows = d1_results::<InstanceDomainBlockRow>(&result)?;
    if pagination.min_id.is_some() {
        rows.reverse();
    }
    Ok(rows)
}

pub(crate) async fn find_instance_domain_block(
    db: &D1Database,
    id: i64,
) -> Result<Option<InstanceDomainBlockRow>> {
    let bindings = [optional_id_binding(Some(id))];
    let result = db
        .prepare(format!(
            "SELECT {INSTANCE_DOMAIN_BLOCK_COLUMNS} FROM instance_domain_blocks WHERE id = ?1"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    Ok(d1_results::<InstanceDomainBlockRow>(&result)?
        .into_iter()
        .next())
}

pub(crate) async fn find_instance_domain_block_by_domain(
    db: &D1Database,
    domain: &str,
) -> Result<Option<InstanceDomainBlockRow>> {
    let bindings = [D1Type::Text(domain)];
    let result = db
        .prepare(format!(
            "SELECT {INSTANCE_DOMAIN_BLOCK_COLUMNS} FROM instance_domain_blocks WHERE domain = ?1"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    Ok(d1_results::<InstanceDomainBlockRow>(&result)?
        .into_iter()
        .next())
}

fn optional_text(value: Option<&str>) -> D1Type<'_> {
    value.map_or(D1Type::Null, D1Type::Text)
}

/// Write the block for `domain`. `on_conflict` decides what an existing row
/// for the domain becomes; the caller reads the row back by domain.
async fn write_instance_domain_block(
    db: &D1Database,
    domain: &str,
    input: &InstanceDomainBlockInput,
    created_by_account_id: Option<&str>,
    on_conflict: &str,
) -> Result<Option<InstanceDomainBlockRow>> {
    let domain = normalize_instance_domain(domain).ok_or_else(invalid_domain_error)?;
    let bindings = [
        D1Type::Text(&domain),
        optional_text(created_by_account_id),
        D1Type::Text(input.severity.as_str()),
        D1Type::Integer(i32::from(input.reject_media)),
        D1Type::Integer(i32::from(input.reject_reports)),
        D1Type::Integer(i32::from(input.obfuscate)),
        optional_text(input.private_comment.as_deref()),
        optional_text(input.public_comment.as_deref()),
    ];
    db.prepare(format!(
        "INSERT INTO instance_domain_blocks (
            domain, created_by_account_id, severity, reject_media, reject_reports,
            obfuscate, private_comment, public_comment
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(domain) {on_conflict}"
    ))
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    invalidate_suspended_domains();
    find_instance_domain_block_by_domain(db, &domain).await
}

/// Mastodon's create: `None` when the domain already has a block.
pub(crate) async fn insert_instance_domain_block(
    db: &D1Database,
    domain: &str,
    input: &InstanceDomainBlockInput,
    created_by_account_id: Option<&str>,
) -> Result<Option<InstanceDomainBlockRow>> {
    let domain = normalize_instance_domain(domain).ok_or_else(invalid_domain_error)?;
    if find_instance_domain_block_by_domain(db, &domain)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    write_instance_domain_block(db, &domain, input, created_by_account_id, "DO NOTHING").await
}

/// Create the block for `domain`, or replace an existing block's policy.
/// Comments left out keep their current text.
pub(crate) async fn upsert_instance_domain_block(
    db: &D1Database,
    domain: &str,
    input: &InstanceDomainBlockInput,
    created_by_account_id: Option<&str>,
) -> Result<Option<InstanceDomainBlockRow>> {
    write_instance_domain_block(
        db,
        domain,
        input,
        created_by_account_id,
        "DO UPDATE SET
            severity = excluded.severity,
            reject_media = excluded.reject_media,
            reject_reports = excluded.reject_reports,
            obfuscate = excluded.obfuscate,
            private_comment = COALESCE(excluded.private_comment, private_comment),
            public_comment = COALESCE(excluded.public_comment, public_comment)",
    )
    .await
}

/// Suspend `domain`, raising an existing block to `suspend` and keeping its
/// flags and comments (a report's "block domain" action).
pub(crate) async fn suspend_instance_domain(
    db: &D1Database,
    domain: &str,
    created_by_account_id: Option<&str>,
) -> Result<()> {
    let input = InstanceDomainBlockInput {
        severity: DomainBlockSeverity::Suspend,
        ..InstanceDomainBlockInput::default()
    };
    write_instance_domain_block(
        db,
        domain,
        &input,
        created_by_account_id,
        "DO UPDATE SET severity = excluded.severity",
    )
    .await
    .map(|_| ())
}

pub(crate) async fn update_instance_domain_block(
    db: &D1Database,
    id: i64,
    input: &InstanceDomainBlockInput,
) -> Result<Option<InstanceDomainBlockRow>> {
    let bindings = [
        optional_id_binding(Some(id)),
        D1Type::Text(input.severity.as_str()),
        D1Type::Integer(i32::from(input.reject_media)),
        D1Type::Integer(i32::from(input.reject_reports)),
        D1Type::Integer(i32::from(input.obfuscate)),
        optional_text(input.private_comment.as_deref()),
        optional_text(input.public_comment.as_deref()),
    ];
    db.prepare(
        "UPDATE instance_domain_blocks
         SET severity = ?2, reject_media = ?3, reject_reports = ?4, obfuscate = ?5,
             private_comment = ?6, public_comment = ?7
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    invalidate_suspended_domains();
    find_instance_domain_block(db, id).await
}

pub(crate) async fn delete_instance_domain_block_by_id(db: &D1Database, id: i64) -> Result<bool> {
    let bindings = [optional_id_binding(Some(id))];
    let result = db
        .prepare("DELETE FROM instance_domain_blocks WHERE id = ?1")
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    invalidate_suspended_domains();
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

pub(crate) async fn delete_instance_domain_block(db: &D1Database, domain: &str) -> Result<bool> {
    let domain = normalize_instance_domain(domain).ok_or_else(invalid_domain_error)?;
    let bindings = [D1Type::Text(&domain)];
    let result = db
        .prepare("DELETE FROM instance_domain_blocks WHERE domain = ?1")
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    invalidate_suspended_domains();
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

#[cfg(test)]
mod tests {
    use super::{domain_block_preview_candidates, filter_delivery_inboxes_for_domain_blocks};

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
    fn delivery_inbox_filter_drops_blocked_host_and_subdomain() {
        let blocked = vec!["blocked.example".to_owned()];
        assert_eq!(
            filter_delivery_inboxes_for_domain_blocks(
                vec![
                    "https://blocked.example/inbox".to_owned(),
                    "https://sub.blocked.example/users/a/inbox".to_owned(),
                    "https://safe.example/inbox".to_owned(),
                ],
                &blocked,
            ),
            vec!["https://safe.example/inbox".to_owned()]
        );
    }
}
