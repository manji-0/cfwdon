use super::{build_local_status_response, find_owned_local_status_response_subject};
use crate::db_session::bind_request_d1;
use crate::db_utils::d1_results;
use crate::delivery::{
    enqueue_add_featured_status_activity, enqueue_remove_featured_status_activity,
};
use crate::responses::MastodonStatusResponse;
use crate::runtime_config::load_config;
use crate::statuses::{find_authenticated_local_account, statuses_from_records};
use crate::tracked_d1::D1Database;
use cfwdon_domain::{LocalStatus, LocalStatusRecord};
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn is_local_status_pinned_by(
    db: &D1Database,
    account_id: &str,
    status_id: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(status_id)];
    Ok(db
        .prepare(
            "SELECT status_id
             FROM status_pins
             WHERE account_id = ?1
               AND status_id = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?
        .is_some())
}

/// Mastodon's `StatusPinValidator::PIN_LIMIT` for local accounts.
const STATUS_PIN_LIMIT: u64 = 5;

/// Mastodon's `StatusPinValidator` messages (`statuses.pin_errors`).
fn pin_validation_error(status: &LocalStatus, pinned_count: u64) -> Option<&'static str> {
    if status.boost_of_uri.is_some() {
        Some("A boost cannot be pinned")
    } else if status.visibility == cfwdon_domain::Visibility::Direct {
        Some("Posts that are only visible to mentioned users cannot be pinned")
    } else if pinned_count >= STATUS_PIN_LIMIT {
        Some("You have already pinned the maximum number of posts")
    } else {
        None
    }
}

async fn count_pinned_statuses(db: &D1Database, account_id: &str) -> Result<u64> {
    #[derive(serde::Deserialize)]
    struct CountRow {
        count: u64,
    }
    let account_id = D1Type::Text(account_id);
    Ok(db
        .prepare("SELECT COUNT(*) AS count FROM status_pins WHERE account_id = ?1")
        .bind_refs(&account_id)?
        .first::<CountRow>(None)
        .await?
        .map(|row| row.count)
        .unwrap_or(0))
}

pub(crate) async fn pin_local_status(
    db: &D1Database,
    account_id: &str,
    status_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(status_id)];
    db.prepare(
        "INSERT INTO status_pins (account_id, status_id)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, status_id) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn unpin_local_status(
    db: &D1Database,
    account_id: &str,
    status_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(status_id)];
    db.prepare(
        "DELETE FROM status_pins
         WHERE account_id = ?1
           AND status_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn list_pinned_statuses_for_account(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<LocalStatus>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.boost_of_uri, s.quote_of_uri, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.quote_state, s.created_at
             FROM status_pins sp
             JOIN statuses s
               ON s.id = sp.status_id
             WHERE sp.account_id = ?1
             ORDER BY sp.created_at DESC, s.created_at DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    d1_results::<LocalStatusRecord>(&result).and_then(statuses_from_records)
}

async fn pinned_status_response(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    subject: super::LoadedLocalStatusResponseSubject,
) -> Result<MastodonStatusResponse> {
    let super::LoadedLocalStatusResponseSubject {
        status,
        account,
        preload,
    } = subject;
    build_local_status_response(
        db,
        config,
        Some(viewer),
        &status,
        &account,
        preload.in_reply_to_account_id,
        preload.media,
    )
    .await
}

pub(crate) async fn pin_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(subject) = find_owned_local_status_response_subject(&db, &status_id, &viewer).await?
    else {
        return Response::error("status not found", 404);
    };

    if !is_local_status_pinned_by(&db, viewer.id(), &subject.status.id).await? {
        let pinned_count = count_pinned_statuses(&db, viewer.id()).await?;
        if let Some(message) = pin_validation_error(&subject.status, pinned_count) {
            return Response::error(format!("Validation failed: {message}"), 422);
        }
    }
    pin_local_status(&db, viewer.id(), &subject.status.id).await?;
    enqueue_add_featured_status_activity(&db, &config, &viewer, &subject.status).await?;
    Response::from_json(&pinned_status_response(&db, &config, &viewer, subject).await?)
}

pub(crate) async fn unpin_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = bind_request_d1(&ctx, &config)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(subject) = find_owned_local_status_response_subject(&db, &status_id, &viewer).await?
    else {
        return Response::error("status not found", 404);
    };

    unpin_local_status(&db, viewer.id(), &subject.status.id).await?;
    enqueue_remove_featured_status_activity(&db, &config, &viewer, &subject.status).await?;
    Response::from_json(&pinned_status_response(&db, &config, &viewer, subject).await?)
}

#[cfg(test)]
mod tests {
    use super::{STATUS_PIN_LIMIT, pin_validation_error};
    use cfwdon_domain::{LocalStatus, LocalStatusRecord};

    fn status(visibility: &str, boost_of_uri: Option<&str>) -> LocalStatus {
        let record: LocalStatusRecord = serde_json::from_value(serde_json::json!({
            "id": "status-1",
            "account_id": "acct-1",
            "boost_of_uri": boost_of_uri,
            "content_html": "<p>hi</p>",
            "text_content": "hi",
            "spoiler_text": "",
            "visibility": visibility,
            "sensitive": 0,
            "quote_state": "accepted",
            "created_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap();
        LocalStatus::try_from_record(record).unwrap()
    }

    #[test]
    fn pin_validation_matches_mastodon_status_pin_validator() {
        assert_eq!(pin_validation_error(&status("public", None), 0), None);
        assert!(pin_validation_error(&status("public", Some("https://x/1")), 0).is_some());
        assert!(pin_validation_error(&status("direct", None), 0).is_some());
        assert!(pin_validation_error(&status("private", None), STATUS_PIN_LIMIT).is_some());
        assert_eq!(
            pin_validation_error(&status("private", None), STATUS_PIN_LIMIT - 1),
            None
        );
    }
}
