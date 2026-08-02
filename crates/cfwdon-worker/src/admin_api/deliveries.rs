use super::guard::{AdminAuthorization, authorize_admin_request};
use crate::{Response, Result, RouteContext};
use serde::Serialize;
use worker::{Request, d1::D1Type};

#[derive(Debug, Serialize)]
struct AdminDeliveryResponse {
    id: String,
    source: String,
    account_id: String,
    activity_type: String,
    state: String,
    attempt_count: i32,
    target_inbox: Option<String>,
    last_attempt_at: Option<String>,
    next_attempt_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, serde::Deserialize)]
struct AdminDeliveryRow {
    id: String,
    account_id: String,
    activity_type: String,
    state: String,
    attempt_count: i32,
    target_inbox: Option<String>,
    last_attempt_at: Option<String>,
    next_attempt_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Default, serde::Deserialize)]
struct AdminDeliveriesQuery {
    state: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct AdminDeliveryRetryQuery {
    source: Option<String>,
}

pub(crate) async fn admin_deliveries_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let query: AdminDeliveriesQuery = req.query().unwrap_or_default();
    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let state_filter = normalize_delivery_state_filter(query.state.as_deref());
    let mut deliveries = list_outbound_admin_deliveries(&db, state_filter).await?;
    deliveries.extend(list_outbox_admin_deliveries(&db, state_filter).await?);
    deliveries.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    deliveries.truncate(100);
    Response::from_json(&deliveries)
}

pub(crate) async fn admin_retry_delivery_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    match authorize_admin_request(&req, &ctx).await? {
        AdminAuthorization::Authorized(_) => {}
        AdminAuthorization::Denied(response) => return Ok(response),
    }

    let delivery_id = ctx
        .param("id")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing delivery id route parameter".to_owned()))?
        .to_owned();
    let query: AdminDeliveryRetryQuery = req.query().unwrap_or_default();
    let source = query
        .source
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("outbound");
    let config = crate::load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;

    let retried = match source {
        "outbox" => retry_outbox_delivery(&db, &delivery_id).await?,
        "outbound" => retry_outbound_delivery(&db, &delivery_id).await?,
        _ => return Response::error("source must be outbound or outbox", 422),
    };
    if retried {
        Response::from_json(
            &serde_json::json!({ "id": delivery_id, "source": source, "retried": true }),
        )
    } else {
        Response::error("delivery not found or not in failed state", 404)
    }
}

fn normalize_delivery_state_filter(value: Option<&str>) -> Option<&'static str> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some("queued") => Some("queued"),
        Some("in_flight") => Some("in_flight"),
        Some("failed") => Some("failed"),
        Some("delivered") => Some("delivered"),
        _ => None,
    }
}

async fn list_outbound_admin_deliveries(
    db: &crate::D1Database,
    state_filter: Option<&str>,
) -> Result<Vec<AdminDeliveryResponse>> {
    let (sql, bindings) = if let Some(state) = state_filter {
        (
            "SELECT id, account_id, activity_type, state, attempt_count, target_inbox, last_attempt_at, next_attempt_at, created_at, updated_at
             FROM outbound_activities
             WHERE state = ?1
             ORDER BY updated_at DESC
             LIMIT 100",
            vec![D1Type::Text(state)],
        )
    } else {
        (
            "SELECT id, account_id, activity_type, state, attempt_count, target_inbox, last_attempt_at, next_attempt_at, created_at, updated_at
             FROM outbound_activities
             WHERE state IN ('queued', 'in_flight', 'failed')
             ORDER BY updated_at DESC
             LIMIT 100",
            Vec::new(),
        )
    };

    let result = if bindings.is_empty() {
        db.prepare(sql).all().await?
    } else {
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    };

    Ok(result
        .results::<AdminDeliveryRow>()?
        .into_iter()
        .map(|row| row_to_admin_delivery(row, "outbound"))
        .collect())
}

async fn list_outbox_admin_deliveries(
    db: &crate::D1Database,
    state_filter: Option<&str>,
) -> Result<Vec<AdminDeliveryResponse>> {
    let (sql, bindings) = if let Some(state) = state_filter {
        (
            "SELECT id, account_id, activity_type, state, attempt_count, target_inbox, last_attempt_at, next_attempt_at, created_at, updated_at
             FROM outbox_deliveries
             WHERE state = ?1
             ORDER BY updated_at DESC
             LIMIT 100",
            vec![D1Type::Text(state)],
        )
    } else {
        (
            "SELECT id, account_id, activity_type, state, attempt_count, target_inbox, last_attempt_at, next_attempt_at, created_at, updated_at
             FROM outbox_deliveries
             WHERE state IN ('queued', 'in_flight', 'failed')
             ORDER BY updated_at DESC
             LIMIT 100",
            Vec::new(),
        )
    };

    let result = if bindings.is_empty() {
        db.prepare(sql).all().await?
    } else {
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    };

    Ok(result
        .results::<AdminDeliveryRow>()?
        .into_iter()
        .map(|row| row_to_admin_delivery(row, "outbox"))
        .collect())
}

async fn retry_outbound_delivery(db: &crate::D1Database, delivery_id: &str) -> Result<bool> {
    let bindings = [D1Type::Text(delivery_id)];
    let result = db
        .prepare(
            "UPDATE outbound_activities
             SET state = 'queued',
                 next_attempt_at = CURRENT_TIMESTAMP,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1
               AND state = 'failed'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

async fn retry_outbox_delivery(db: &crate::D1Database, delivery_id: &str) -> Result<bool> {
    let bindings = [D1Type::Text(delivery_id)];
    let result = db
        .prepare(
            "UPDATE outbox_deliveries
             SET state = 'queued',
                 next_attempt_at = CURRENT_TIMESTAMP,
                 updated_at = CURRENT_TIMESTAMP
             WHERE id = ?1
               AND state = 'failed'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
}

fn row_to_admin_delivery(row: AdminDeliveryRow, source: &str) -> AdminDeliveryResponse {
    AdminDeliveryResponse {
        id: row.id,
        source: source.to_owned(),
        account_id: row.account_id,
        activity_type: row.activity_type,
        state: row.state,
        attempt_count: row.attempt_count,
        target_inbox: row.target_inbox,
        last_attempt_at: row.last_attempt_at,
        next_attempt_at: row.next_attempt_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_delivery_state_filter;

    #[test]
    fn delivery_state_filter_accepts_known_states() {
        assert_eq!(
            normalize_delivery_state_filter(Some("failed")),
            Some("failed")
        );
        assert_eq!(normalize_delivery_state_filter(Some("")), None);
        assert_eq!(normalize_delivery_state_filter(None), None);
    }
}
