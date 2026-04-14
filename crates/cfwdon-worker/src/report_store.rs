use super::{
    AccountReference, CreateReportRequest, D1Database, Error, ReportRow, Result,
    generate_entity_id, remote_account_rest_id,
};
use worker::d1::D1Type;

pub(crate) async fn insert_report(
    db: &D1Database,
    reporter_account_id: &str,
    request: &CreateReportRequest,
    target: &AccountReference,
    status_ids: &[String],
) -> Result<ReportRow> {
    let report_id = generate_entity_id(16)?;
    let target_account_id = match target {
        AccountReference::Local(account) => account.id.clone(),
        AccountReference::Remote(actor) => remote_account_rest_id(&actor.actor_uri),
    };
    let target_remote_actor_uri = match target {
        AccountReference::Local(_) => None,
        AccountReference::Remote(actor) => Some(actor.actor_uri.clone()),
    };
    let category = request
        .category
        .clone()
        .unwrap_or_else(|| "other".to_owned());
    let comment = request.comment.clone().unwrap_or_default();
    let bindings = [
        D1Type::Text(report_id.as_str()),
        D1Type::Text(reporter_account_id),
        D1Type::Text(target_account_id.as_str()),
        match target_remote_actor_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(comment.as_str()),
        D1Type::Text(category.as_str()),
        D1Type::Integer(if request.forward.unwrap_or(false) {
            1
        } else {
            0
        }),
    ];
    db.prepare(
        "INSERT INTO reports (
            id,
            account_id,
            target_account_id,
            target_remote_actor_uri,
            comment,
            category,
            forward,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    for status_id in status_ids {
        let bindings = [
            D1Type::Text(report_id.as_str()),
            D1Type::Text(status_id.as_str()),
        ];
        db.prepare(
            "INSERT INTO report_statuses (report_id, status_id)
             VALUES (?1, ?2)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    find_report_by_id(db, &report_id)
        .await?
        .ok_or_else(|| Error::RustError("failed to load created report".to_owned()))
}

pub(crate) async fn find_report_by_id(
    db: &D1Database,
    report_id: &str,
) -> Result<Option<ReportRow>> {
    let report_id = D1Type::Text(report_id);
    db.prepare(
        "SELECT id, account_id, target_account_id, target_remote_actor_uri, comment, category, forward, created_at
         FROM reports
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&report_id)?
    .first::<ReportRow>(None)
    .await
}

pub(crate) async fn list_reports(db: &D1Database, limit: u32) -> Result<Vec<ReportRow>> {
    let bindings = [D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT id, account_id, target_account_id, target_remote_actor_uri, comment, category, forward, created_at
             FROM reports
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<ReportRow>()
}

pub(crate) async fn list_report_status_ids(
    db: &D1Database,
    report_id: &str,
) -> Result<Vec<String>> {
    let bindings = [D1Type::Text(report_id)];
    let result = db
        .prepare(
            "SELECT status_id
             FROM report_statuses
             WHERE report_id = ?1
             ORDER BY status_id ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<serde_json::Value>()?
        .into_iter()
        .filter_map(|value| {
            value
                .get("status_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect())
}
