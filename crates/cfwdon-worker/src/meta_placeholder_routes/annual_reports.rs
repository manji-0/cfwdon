use crate::accounts::load_account_stats;
use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::db_utils::d1_results;
use crate::id_utils::generate_entity_id;
use crate::runtime_config::load_config;
use crate::time_html::{now_iso_string, timestamp_to_mastodon_iso8601};
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use worker::{Request, Response, Result, RouteContext, d1::D1Type};

#[derive(Debug, Deserialize)]
struct AnnualReportRow {
    account_id: String,
    year: i32,
    data_json: String,
    schema_version: i32,
    share_key: Option<String>,
    viewed_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CountRow {
    count: u64,
}

#[derive(Debug, Deserialize)]
struct StatusIdRow {
    id: String,
}

fn current_campaign_year() -> i32 {
    js_sys::Date::new_0().get_utc_full_year() as i32
}

fn annual_report_bounds(year: i32) -> (String, String) {
    (
        format!("{year:04}-01-01T00:00:00Z"),
        format!("{:04}-01-01T00:00:00Z", year + 1),
    )
}

fn annual_report_document(row: &AnnualReportRow) -> serde_json::Value {
    let data = serde_json::from_str::<serde_json::Value>(&row.data_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    serde_json::json!({
        "year": row.year,
        "data": data,
        "schema_version": row.schema_version,
        "share_url": row
            .share_key
            .as_ref()
            .map(|value| format!("/annual_reports/{}/{}", row.year, value))
            .unwrap_or_default(),
        "account_id": row.account_id,
    })
}

async fn list_generated_annual_reports(
    db: &D1Database,
    account_id: &str,
    pending_only: bool,
) -> Result<Vec<AnnualReportRow>> {
    let sql = if pending_only {
        "SELECT account_id, year, data_json, schema_version, share_key, viewed_at
         FROM generated_annual_reports
         WHERE account_id = ?1
           AND viewed_at IS NULL
         ORDER BY year DESC"
    } else {
        "SELECT account_id, year, data_json, schema_version, share_key, viewed_at
         FROM generated_annual_reports
         WHERE account_id = ?1
         ORDER BY year DESC"
    };

    db.prepare(sql)
        .bind_refs(&D1Type::Text(account_id))?
        .all()
        .await
        .and_then(|__d1| d1_results::<AnnualReportRow>(&__d1))
}

async fn find_generated_annual_report(
    db: &D1Database,
    account_id: &str,
    year: i32,
) -> Result<Option<AnnualReportRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(year)];
    db.prepare(
        "SELECT account_id, year, data_json, schema_version, share_key, viewed_at
         FROM generated_annual_reports
         WHERE account_id = ?1
           AND year = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<AnnualReportRow>(None)
    .await
}

async fn count_account_statuses_between(
    db: &D1Database,
    account_id: &str,
    start: &str,
    end: &str,
) -> Result<u64> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(start),
        D1Type::Text(end),
    ];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM statuses
             WHERE account_id = ?1
               AND datetime(created_at) >= datetime(?2)
               AND datetime(created_at) < datetime(?3)",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|value| value.count).unwrap_or(0))
}

async fn list_recent_public_status_ids_between(
    db: &D1Database,
    account_id: &str,
    start: &str,
    end: &str,
    limit: u32,
) -> Result<Vec<String>> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(start),
        D1Type::Text(end),
        D1Type::Integer(limit as i32),
    ];
    Ok(db
        .prepare(
            "SELECT id
             FROM statuses
             WHERE account_id = ?1
               AND datetime(created_at) >= datetime(?2)
               AND datetime(created_at) < datetime(?3)
               AND visibility IN ('public', 'unlisted')
             ORDER BY created_at DESC, id DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await
        .and_then(|__d1| d1_results::<StatusIdRow>(&__d1))?
        .into_iter()
        .map(|row| row.id)
        .collect())
}

async fn create_generated_annual_report(
    db: &D1Database,
    account: &cfwdon_domain::LocalAccount,
    year: i32,
) -> Result<AnnualReportRow> {
    let (start, end) = annual_report_bounds(year);
    let stats = load_account_stats(db, account.id()).await?;
    let posts_count = count_account_statuses_between(db, account.id(), &start, &end).await?;
    let top_statuses =
        list_recent_public_status_ids_between(db, account.id(), &start, &end, 3).await?;
    let share_key = generate_entity_id(12)?;
    let data_json = serde_json::json!({
        "display_name": if account.display_name().trim().is_empty() {
            account.username().to_owned()
        } else {
            account.display_name().to_owned()
        },
        "username": account.username(),
        "joined_at": timestamp_to_mastodon_iso8601(account.created_at()),
        "posts_count": posts_count,
        "followers_count": stats.followers_count,
        "following_count": stats.following_count,
        "top_statuses": {
            "first": top_statuses.first().cloned(),
            "second": top_statuses.get(1).cloned(),
            "third": top_statuses.get(2).cloned(),
        },
    });
    let data_json_string = serde_json::to_string(&data_json).map_err(|error| {
        worker::Error::RustError(format!("failed to serialize annual report data: {error}"))
    })?;
    let now = now_iso_string()?;
    let bindings = [
        D1Type::Text(account.id()),
        D1Type::Integer(year),
        D1Type::Text(data_json_string.as_str()),
        D1Type::Text(share_key.as_str()),
        D1Type::Text(now.as_str()),
        D1Type::Text(now.as_str()),
    ];
    db.prepare(
        "INSERT OR REPLACE INTO generated_annual_reports (
            account_id,
            year,
            data_json,
            schema_version,
            share_key,
            viewed_at,
            created_at,
            updated_at
         ) VALUES (?1, ?2, ?3, 1, ?4, NULL, ?5, ?6)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    find_generated_annual_report(db, account.id(), year)
        .await?
        .ok_or_else(|| {
            worker::Error::RustError("generated annual report was not persisted".to_owned())
        })
}

async fn mark_generated_annual_report_viewed(
    db: &D1Database,
    account_id: &str,
    year: i32,
) -> Result<()> {
    let now = now_iso_string()?;
    let bindings = [
        D1Type::Text(now.as_str()),
        D1Type::Text(account_id),
        D1Type::Integer(year),
    ];
    db.prepare(
        "UPDATE generated_annual_reports
         SET viewed_at = COALESCE(viewed_at, ?1),
             updated_at = ?1
         WHERE account_id = ?2
           AND year = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn annual_reports_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let reports = list_generated_annual_reports(&db, account.id(), true).await?;
    let response = reports
        .iter()
        .map(annual_report_document)
        .collect::<Vec<_>>();
    Response::from_json(&response)
}

pub(crate) async fn annual_report_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::error("annual report not found", 404);
    };
    let Some(report) = find_generated_annual_report(&db, account.id(), year).await? else {
        return Response::error("annual report not found", 404);
    };
    Response::from_json(&annual_report_document(&report))
}

pub(crate) async fn annual_report_action_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::empty();
    };
    let url = req.url()?;

    if url.path().ends_with("/read") {
        mark_generated_annual_report_viewed(&db, account.id(), year).await?;
        return Response::empty();
    }

    if year != current_campaign_year() {
        return Response::empty();
    }
    if find_generated_annual_report(&db, account.id(), year)
        .await?
        .is_some()
    {
        return Response::empty();
    }

    create_generated_annual_report(&db, &account, year).await?;
    Ok(Response::empty()?.with_status(202))
}

pub(crate) async fn annual_report_state_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(year) = ctx.param("id").and_then(|value| value.parse::<i32>().ok()) else {
        return Response::from_json(&serde_json::json!({
            "state": "unavailable",
            "available": false,
        }));
    };
    if let Some(report) = find_generated_annual_report(&db, account.id(), year).await? {
        return Response::from_json(&serde_json::json!({
            "state": if report.viewed_at.is_some() { "viewed" } else { "ready" },
            "available": true,
        }));
    }

    Response::from_json(&serde_json::json!({
        "state": if year == current_campaign_year() { "not_generated" } else { "unavailable" },
        "available": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annual_report_bounds_cover_exact_calendar_year() {
        assert_eq!(
            annual_report_bounds(2025),
            (
                "2025-01-01T00:00:00Z".to_owned(),
                "2026-01-01T00:00:00Z".to_owned()
            )
        );
    }
}
