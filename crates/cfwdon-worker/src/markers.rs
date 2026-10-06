use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::request_utils::query_array_param;
use crate::runtime_config::load_config;
use crate::time_html::{now_iso_string, timestamp_to_mastodon_iso8601};
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{Error, Request, Response, Result, RouteContext};

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
    db: &D1Database,
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
                "updated_at": timestamp_to_mastodon_iso8601(&row.updated_at),
            })
        }))
}

/// The notifications marker's `last_read_id`, which `unread_count` counts past.
pub(crate) async fn load_notifications_last_read_id(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<String>> {
    Ok(load_marker(db, account_id, NOTIFICATIONS_MARKER_SCOPE)
        .await?
        .and_then(|marker| {
            marker
                .get("last_read_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|value| !value.trim().is_empty()))
}

async fn save_marker(
    db: &D1Database,
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

    let updated_at = now_iso_string()?;
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
             version = timeline_markers.version + 1,
             updated_at = excluded.updated_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let version = load_marker(db, account_id, scope)
        .await?
        .and_then(|marker| marker.get("version").and_then(serde_json::Value::as_i64))
        .unwrap_or(1);

    Ok(serde_json::json!({
        "last_read_id": last_read_id,
        "version": version,
        "updated_at": updated_at,
    }))
}

fn requested_marker_scopes(req: &Request) -> Result<(bool, bool)> {
    let url = req.url()?;
    let Some(requested) = query_array_param(&url, "timeline") else {
        return Ok((true, true));
    };

    Ok((
        requested.iter().any(|scope| scope == HOME_MARKER_SCOPE),
        requested
            .iter()
            .any(|scope| scope == NOTIFICATIONS_MARKER_SCOPE),
    ))
}

/// Mastodon's marker document only carries the timelines that have a marker.
fn markers_document(
    home: Option<serde_json::Value>,
    notifications: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut document = serde_json::Map::new();
    if let Some(home) = home {
        document.insert(HOME_MARKER_SCOPE.to_owned(), home);
    }
    if let Some(notifications) = notifications {
        document.insert(NOTIFICATIONS_MARKER_SCOPE.to_owned(), notifications);
    }
    serde_json::Value::Object(document)
}

/// `POST /api/v1/markers` takes JSON or form params (`home[last_read_id]=…`).
async fn parse_save_markers_request(
    req: &mut Request,
) -> std::result::Result<SaveMarkersRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| error.to_string())?
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("application/json") {
        return req
            .json::<SaveMarkersRequest>()
            .await
            .map_err(|error| format!("invalid markers payload: {error}"));
    }
    let form = req
        .form_data()
        .await
        .map_err(|error| format!("invalid markers payload: {error}"))?;
    let marker = |scope: &str| {
        form.get_field(&format!("{scope}[last_read_id]"))
            .map(|last_read_id| MarkerUpdateRequest { last_read_id })
    };
    Ok(SaveMarkersRequest {
        home: marker(HOME_MARKER_SCOPE),
        notifications: marker(NOTIFICATIONS_MARKER_SCOPE),
    })
}
pub(crate) async fn markers_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let (wants_home, wants_notifications) = requested_marker_scopes(&req)?;
    let (home, notifications) = futures_util::try_join!(
        async {
            if wants_home {
                load_marker(&db, account.id(), HOME_MARKER_SCOPE).await
            } else {
                Ok(None)
            }
        },
        async {
            if wants_notifications {
                load_marker(&db, account.id(), NOTIFICATIONS_MARKER_SCOPE).await
            } else {
                Ok(None)
            }
        },
    )?;

    Response::from_json(&markers_document(home, notifications))
}

pub(crate) async fn save_markers_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);

    let db = bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let request = match parse_save_markers_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    if [&request.home, &request.notifications]
        .into_iter()
        .flatten()
        .any(|marker| marker.last_read_id.trim().is_empty())
    {
        return Response::error("Validation failed: Last read can't be blank", 422);
    }

    let (home, notifications) = futures_util::try_join!(
        async {
            match request.home {
                Some(home) => save_marker(&db, account.id(), HOME_MARKER_SCOPE, home)
                    .await
                    .map(Some),
                None => Ok(None),
            }
        },
        async {
            match request.notifications {
                Some(notifications) => {
                    save_marker(&db, account.id(), NOTIFICATIONS_MARKER_SCOPE, notifications)
                        .await
                        .map(Some)
                }
                None => Ok(None),
            }
        },
    )?;

    Response::from_json(&markers_document(home, notifications))
}
