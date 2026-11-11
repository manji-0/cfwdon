//! Admin IP blocks (`ip_blocks`, Mastodon's `IpBlock`) and their enforcement.
//!
//! The client address is `CF-Connecting-IP`, which Cloudflare sets on every
//! request it forwards to the Worker and overwrites when a client sends its
//! own. A request without it (a local `wrangler dev` call) has no known address
//! and no block applies. `X-Forwarded-For` is never read: on Workers it is
//! whatever the client sent.
//!
//! Each isolate keeps the compiled block list for [`IP_BLOCKS_L1_TTL_MS`]; an
//! admin edit drops this isolate's copy at once and other isolates pick it up
//! within the TTL. A failed D1 read fails open, so an outage cannot lock every
//! client out.

use crate::db_utils::d1_results;
use crate::time_html::{
    add_seconds_to_iso_string, normalize_rfc3339_millis_utc, now_iso_string, now_unix_timestamp,
};
use crate::tracked_d1::D1Database;
use cfwdon_domain::{
    IpBlockRule, IpBlockSeverity, IpRange, IpSignUpDecision, parse_client_ip, strictest_ip_block,
};
use serde::Deserialize;
use std::cell::RefCell;
use std::net::IpAddr;
use std::rc::Rc;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use worker::d1::D1Type;
use worker::{Request, Response, Result, console_error};

const IP_BLOCKS_L1_TTL_MS: f64 = 60_000.0;
const MAX_LIMIT: u32 = 200;
/// Answered without a block check so uptime probes keep working.
const UNBLOCKED_PATHS: &[&str] = &["/healthz"];

thread_local! {
    /// `(loaded_at_ms, rules)`.
    static IP_BLOCKS_L1: RefCell<Option<(f64, Rc<[IpBlockRule]>)>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IpBlockRow {
    pub(crate) id: i64,
    pub(crate) ip: String,
    pub(crate) severity: String,
    pub(crate) comment: String,
    pub(crate) expires_at: Option<String>,
    pub(crate) created_at: String,
}

/// A block as an admin writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IpBlockInput {
    pub(crate) range: IpRange,
    pub(crate) severity: IpBlockSeverity,
    pub(crate) comment: String,
    pub(crate) expiry: IpBlockExpiry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IpBlockExpiry {
    Permanent,
    /// RFC3339 UTC, millisecond precision.
    At(String),
}

impl IpBlockExpiry {
    /// The expiry `seconds` from now.
    pub(crate) fn in_seconds(seconds: u64) -> Result<Self> {
        let at = add_seconds_to_iso_string(&now_iso_string()?, seconds)?;
        Ok(Self::At(normalize_rfc3339_millis_utc(&at).unwrap_or(at)))
    }

    pub(crate) fn from_column(value: Option<&str>) -> Self {
        value
            .filter(|value| !value.is_empty())
            .map_or(Self::Permanent, |value| Self::At(value.to_owned()))
    }

    fn as_column(&self) -> Option<&str> {
        match self {
            Self::Permanent => None,
            Self::At(at) => Some(at),
        }
    }
}

fn iso_to_unix(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(OffsetDateTime::unix_timestamp)
}

/// The rule a row enforces. A row whose range or severity does not parse is
/// left out, so a hand-edited row cannot break enforcement; an unreadable
/// expiry keeps the block in force.
fn rule_from_row(row: &IpBlockRow) -> Option<IpBlockRule> {
    Some(IpBlockRule {
        range: row.ip.parse().ok()?,
        severity: IpBlockSeverity::from_name(&row.severity)?,
        expires_at_unix: row.expires_at.as_deref().and_then(iso_to_unix),
    })
}

fn ip_blocks_from_l1(now_ms: f64) -> Option<Rc<[IpBlockRule]>> {
    IP_BLOCKS_L1.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|(loaded_at_ms, _)| now_ms - loaded_at_ms < IP_BLOCKS_L1_TTL_MS)
            .map(|(_, rules)| Rc::clone(rules))
    })
}

fn store_ip_blocks_l1(now_ms: f64, rules: Rc<[IpBlockRule]>) {
    IP_BLOCKS_L1.with(|slot| *slot.borrow_mut() = Some((now_ms, rules)));
}

pub(crate) fn invalidate_ip_blocks() {
    IP_BLOCKS_L1.with(|slot| *slot.borrow_mut() = None);
}

const IP_BLOCK_COLUMNS: &str = "id, ip, severity, comment, expires_at, created_at";

async fn current_ip_blocks(db: &D1Database) -> Rc<[IpBlockRule]> {
    let now_ms = js_sys::Date::now();
    if let Some(rules) = ip_blocks_from_l1(now_ms) {
        return rules;
    }
    let rows = match db
        .prepare(format!("SELECT {IP_BLOCK_COLUMNS} FROM ip_blocks"))
        .all()
        .await
        .and_then(|result| d1_results::<IpBlockRow>(&result))
    {
        Ok(rows) => rows,
        Err(error) => {
            console_error!("ip block list failed to load: {error}");
            return Rc::from([]);
        }
    };
    let rules: Rc<[IpBlockRule]> = rows.iter().filter_map(rule_from_row).collect();
    store_ip_blocks_l1(now_ms, Rc::clone(&rules));
    rules
}

fn client_ip(req: &Request) -> Option<IpAddr> {
    req.headers()
        .get("CF-Connecting-IP")
        .ok()
        .flatten()
        .and_then(|value| parse_client_ip(&value))
}

async fn strictest_block_for_request(req: &Request, db: &D1Database) -> Option<IpBlockSeverity> {
    let address = client_ip(req)?;
    let rules = current_ip_blocks(db).await;
    strictest_ip_block(&rules, address, now_unix_timestamp())
}

/// 403 for a client under a `no_access` block, as Mastodon's Rack::Attack
/// blocklist answers before routing.
pub(crate) async fn ip_access_denied_response(
    req: &Request,
    db: &D1Database,
    path: &str,
) -> Result<Option<Response>> {
    if UNBLOCKED_PATHS.contains(&path) {
        return Ok(None);
    }
    if strictest_block_for_request(req, db).await == Some(IpBlockSeverity::NoAccess) {
        return Response::error("Forbidden", 403).map(Some);
    }
    Ok(None)
}

/// What a sign-up from this request's address may do.
pub(crate) async fn ip_sign_up_decision(req: &Request, db: &D1Database) -> IpSignUpDecision {
    IpSignUpDecision::for_block(strictest_block_for_request(req, db).await)
}

fn optional_id_binding(id: Option<i64>) -> D1Type<'static> {
    id.and_then(|id| i32::try_from(id).ok())
        .map_or(D1Type::Null, D1Type::Integer)
}

/// A page of blocks, newest first, under Mastodon's id cursors.
pub(crate) async fn list_ip_block_page(
    db: &D1Database,
    limit: u32,
    max_id: Option<i64>,
    after_id: Option<i64>,
    ascending: bool,
) -> Result<Vec<IpBlockRow>> {
    let limit = i32::try_from(limit.clamp(1, MAX_LIMIT)).unwrap_or(100);
    let order = if ascending { "ASC" } else { "DESC" };
    let bindings = [
        optional_id_binding(max_id),
        optional_id_binding(after_id),
        D1Type::Integer(limit),
    ];
    let result = db
        .prepare(format!(
            "SELECT {IP_BLOCK_COLUMNS}
             FROM ip_blocks
             WHERE (?1 IS NULL OR id < ?1)
               AND (?2 IS NULL OR id > ?2)
             ORDER BY id {order}
             LIMIT ?3"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    let mut rows = d1_results::<IpBlockRow>(&result)?;
    if ascending {
        rows.reverse();
    }
    Ok(rows)
}

pub(crate) async fn find_ip_block(db: &D1Database, id: i64) -> Result<Option<IpBlockRow>> {
    let bindings = [optional_id_binding(Some(id))];
    let result = db
        .prepare(format!(
            "SELECT {IP_BLOCK_COLUMNS} FROM ip_blocks WHERE id = ?1"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    Ok(d1_results::<IpBlockRow>(&result)?.into_iter().next())
}

pub(crate) async fn find_ip_block_by_range(
    db: &D1Database,
    range: IpRange,
) -> Result<Option<IpBlockRow>> {
    let range = range.to_string();
    let bindings = [D1Type::Text(&range)];
    let result = db
        .prepare(format!(
            "SELECT {IP_BLOCK_COLUMNS} FROM ip_blocks WHERE ip = ?1"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    Ok(d1_results::<IpBlockRow>(&result)?.into_iter().next())
}

pub(crate) async fn insert_ip_block(
    db: &D1Database,
    input: &IpBlockInput,
) -> Result<Option<IpBlockRow>> {
    let range = input.range.to_string();
    let now = now_iso_string()?;
    let bindings = [
        D1Type::Text(&range),
        D1Type::Text(input.severity.as_str()),
        D1Type::Text(&input.comment),
        input.expiry.as_column().map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(&now),
    ];
    db.prepare(
        "INSERT INTO ip_blocks (ip, severity, comment, expires_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(ip) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    invalidate_ip_blocks();
    find_ip_block_by_range(db, input.range).await
}

pub(crate) async fn update_ip_block(
    db: &D1Database,
    id: i64,
    input: &IpBlockInput,
) -> Result<Option<IpBlockRow>> {
    let range = input.range.to_string();
    let now = now_iso_string()?;
    let bindings = [
        optional_id_binding(Some(id)),
        D1Type::Text(&range),
        D1Type::Text(input.severity.as_str()),
        D1Type::Text(&input.comment),
        input.expiry.as_column().map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(&now),
    ];
    db.prepare(
        "UPDATE ip_blocks
         SET ip = ?2, severity = ?3, comment = ?4, expires_at = ?5, updated_at = ?6
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    invalidate_ip_blocks();
    find_ip_block(db, id).await
}

pub(crate) async fn delete_ip_block(db: &D1Database, id: i64) -> Result<bool> {
    let bindings = [optional_id_binding(Some(id))];
    let result = db
        .prepare("DELETE FROM ip_blocks WHERE id = ?1")
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    invalidate_ip_blocks();
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

#[cfg(test)]
mod tests {
    use super::{
        IP_BLOCKS_L1_TTL_MS, IpBlockRow, invalidate_ip_blocks, ip_blocks_from_l1, rule_from_row,
        store_ip_blocks_l1,
    };
    use cfwdon_domain::{IpBlockSeverity, parse_client_ip, strictest_ip_block};
    use std::rc::Rc;

    fn row(ip: &str, severity: &str, expires_at: Option<&str>) -> IpBlockRow {
        IpBlockRow {
            id: 1,
            ip: ip.to_owned(),
            severity: severity.to_owned(),
            comment: String::new(),
            expires_at: expires_at.map(str::to_owned),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn rows_compile_into_enforced_rules() {
        let rule = rule_from_row(&row(
            "192.0.2.0/24",
            "no_access",
            Some("2026-01-01T00:01:00.000Z"),
        ))
        .unwrap();
        let address = parse_client_ip("192.0.2.9").unwrap();
        // 2026-01-01T00:00:00Z
        let start = 1_767_225_600;
        assert_eq!(
            strictest_ip_block(&[rule], address, start),
            Some(IpBlockSeverity::NoAccess)
        );
        assert_eq!(strictest_ip_block(&[rule], address, start + 60), None);
    }

    #[test]
    fn malformed_rows_are_skipped() {
        assert!(rule_from_row(&row("not-an-ip", "no_access", None)).is_none());
        assert!(rule_from_row(&row("192.0.2.0/24", "suspend", None)).is_none());
        let permanent = rule_from_row(&row("192.0.2.0/24", "sign_up_block", Some("garbage")));
        assert_eq!(permanent.unwrap().expires_at_unix, None);
    }

    #[test]
    fn ip_blocks_l1_expires_and_invalidates() {
        let rule = rule_from_row(&row("192.0.2.0/24", "no_access", None)).unwrap();
        store_ip_blocks_l1(1_000.0, Rc::from([rule]));
        assert_eq!(ip_blocks_from_l1(1_000.0).map(|rules| rules.len()), Some(1));
        assert!(ip_blocks_from_l1(1_000.0 + IP_BLOCKS_L1_TTL_MS).is_none());
        store_ip_blocks_l1(2_000.0, Rc::from([]));
        invalidate_ip_blocks();
        assert!(ip_blocks_from_l1(2_000.0).is_none());
    }
}
