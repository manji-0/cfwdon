use super::{
    Request, Response, Result, RouteContext, load_config, require_authenticated_local_account,
};
use serde::{Deserialize, Serialize};
use worker::d1::D1Type;

use crate::D1Database;
pub(crate) const ASYNC_REFRESH_RETRY_SECONDS: u32 = 3;

#[derive(Debug, Deserialize)]
struct AsyncRefreshRow {
    id: String,
    status: String,
    result_count: Option<u64>,
}

#[derive(Debug, Serialize)]
struct AsyncRefreshDocument {
    async_refresh: AsyncRefreshState,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AsyncRefreshState {
    pub(crate) id: String,
    pub(crate) status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result_count: Option<u64>,
}

pub(crate) fn context_async_refresh_id(status_id: &str) -> String {
    format!("context:{status_id}:refresh")
}

pub(crate) fn format_async_refresh_header_value(
    id: &str,
    retry: u32,
    result_count: Option<u64>,
) -> String {
    let mut parts = vec![format!("id=\"{id}\""), format!("retry={retry}")];
    if let Some(result_count) = result_count {
        parts.push(format!("result_count={result_count}"));
    }
    parts.join(", ")
}

async fn upsert_async_refresh(
    db: &D1Database,
    id: &str,
    status: &str,
    result_count: Option<u64>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(id),
        D1Type::Text(status),
        result_count.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
    ];
    db.prepare(
        "INSERT INTO async_refreshes (
            id,
            status,
            result_count,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(id) DO UPDATE SET
            status = excluded.status,
            result_count = excluded.result_count,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn build_finished_context_async_refresh_header(
    db: &D1Database,
    status_id: &str,
) -> Result<String> {
    let id = context_async_refresh_id(status_id);
    upsert_async_refresh(db, &id, "finished", Some(0)).await?;
    Ok(format_async_refresh_header_value(
        &id,
        ASYNC_REFRESH_RETRY_SECONDS,
        Some(0),
    ))
}

pub(crate) async fn find_async_refresh_state(
    db: &D1Database,
    id: &str,
) -> Result<Option<AsyncRefreshState>> {
    let row = db
        .prepare(
            "SELECT id, status, result_count
             FROM async_refreshes
             WHERE id = ?1",
        )
        .bind_refs(&[D1Type::Text(id)])?
        .first::<AsyncRefreshRow>(None)
        .await?;

    Ok(row.map(|row| AsyncRefreshState {
        id: row.id,
        status: row.status,
        result_count: row.result_count,
    }))
}

pub(crate) async fn async_refresh_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(_viewer) = require_authenticated_local_account(&req, &db, &config).await? else {
        return Response::error("Auth0 authentication required", 401);
    };
    let refresh_id = match ctx.param("id") {
        Some(id) if !id.is_empty() => id,
        _ => return Response::error("missing async refresh id route parameter", 400),
    };
    let Some(async_refresh) = find_async_refresh_state(&db, refresh_id).await? else {
        return Response::error("Not Found", 404);
    };
    Response::from_json(&AsyncRefreshDocument { async_refresh })
}
