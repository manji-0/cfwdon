use crate::{Request, Response, Result, RouteContext, extract_authenticated_user, load_config};
use serde::Deserialize;
use worker::Error;
use worker::d1::D1Type;

const HOME_MARKER_SCOPE: &str = "home";
const NOTIFICATIONS_MARKER_SCOPE: &str = "notifications";

#[derive(Debug, Deserialize)]
struct MarkerRow {
    last_read_id: String,
    version: i32,
    updated_at: String,
}

#[derive(Debug, Default, Deserialize)]
struct MarkerUpdateRequest {
    last_read_id: String,
}

#[derive(Debug, Default, Deserialize)]
struct SaveMarkersRequest {
    home: Option<MarkerUpdateRequest>,
    notifications: Option<MarkerUpdateRequest>,
}

async fn load_marker(
    db: &worker::D1Database,
    account_id: &str,
    scope: &str,
) -> Result<Option<serde_json::Value>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(scope)];
    Ok(db
        .prepare(
            "SELECT last_read_id, version, updated_at
             FROM timeline_markers
             WHERE account_id = ?1
               AND scope = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<MarkerRow>(None)
        .await?
        .map(|row| {
            serde_json::json!({
                "last_read_id": row.last_read_id,
                "version": row.version,
                "updated_at": row.updated_at,
            })
        }))
}

async fn save_marker(
    db: &worker::D1Database,
    account_id: &str,
    scope: &str,
    marker: MarkerUpdateRequest,
) -> Result<serde_json::Value> {
    let last_read_id = marker.last_read_id.trim();
    if last_read_id.is_empty() {
        return Err(Error::RustError(
            "last_read_id must not be empty".to_owned(),
        ));
    }

    let updated_at = crate::now_iso_string()?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(scope),
        D1Type::Text(last_read_id),
        D1Type::Text(updated_at.as_str()),
    ];
    db.prepare(
        "INSERT INTO timeline_markers (account_id, scope, last_read_id, version, updated_at)
         VALUES (?1, ?2, ?3, 1, ?4)
         ON CONFLICT(account_id, scope) DO UPDATE SET
             last_read_id = excluded.last_read_id,
             version = excluded.version,
             updated_at = excluded.updated_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(serde_json::json!({
        "last_read_id": last_read_id,
        "version": 1,
        "updated_at": updated_at,
    }))
}

fn requested_marker_scopes(req: &Request) -> Result<(bool, bool)> {
    let url = req.url()?;
    let requested = url
        .query_pairs()
        .filter(|(key, _)| key == "timeline[]")
        .map(|(_, value)| value.into_owned())
        .collect::<Vec<_>>();
    if requested.is_empty() {
        return Ok((true, true));
    }

    Ok((
        requested.iter().any(|scope| scope == HOME_MARKER_SCOPE),
        requested
            .iter()
            .any(|scope| scope == NOTIFICATIONS_MARKER_SCOPE),
    ))
}

pub(crate) async fn markers_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let db = ctx.d1(&config.database_binding)?;
    let account = crate::resolve_local_account(&db, &user).await?;
    let (wants_home, wants_notifications) = requested_marker_scopes(&req)?;

    Response::from_json(&serde_json::json!({
        "home": if wants_home {
            load_marker(&db, &account.id, HOME_MARKER_SCOPE).await?
        } else {
            None
        },
        "notifications": if wants_notifications {
            load_marker(&db, &account.id, NOTIFICATIONS_MARKER_SCOPE).await?
        } else {
            None
        },
    }))
}

pub(crate) async fn save_markers_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let request = req
        .json::<SaveMarkersRequest>()
        .await
        .map_err(|error| worker::Error::RustError(format!("invalid markers payload: {error}")))?;
    let db = ctx.d1(&config.database_binding)?;
    let account = crate::resolve_local_account(&db, &user).await?;

    Response::from_json(&serde_json::json!({
        "home": match request.home {
            Some(home) => Some(save_marker(&db, &account.id, HOME_MARKER_SCOPE, home).await?),
            None => load_marker(&db, &account.id, HOME_MARKER_SCOPE).await?,
        },
        "notifications": match request.notifications {
            Some(notifications) => Some(
                save_marker(&db, &account.id, NOTIFICATIONS_MARKER_SCOPE, notifications).await?
            ),
            None => load_marker(&db, &account.id, NOTIFICATIONS_MARKER_SCOPE).await?,
        },
    }))
}
