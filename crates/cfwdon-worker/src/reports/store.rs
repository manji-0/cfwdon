use super::{CreateReportRequest, ReportRow};
use crate::db_utils::{d1_results, json_string_array};
use crate::id_utils::generate_entity_id;
use crate::identity::remote_account_rest_id;
use crate::remote::AccountReference;
use crate::tracked_d1::D1Database;
use cfwdon_domain::{InboundFlagReport, StoredReportIntent};
use worker::d1::D1Type;
use worker::{Error, Result};
fn stored_report_intent(
    report_id: String,
    request: &CreateReportRequest,
    target: &AccountReference,
) -> StoredReportIntent {
    let (target_account_id, target_remote_actor_uri) = match target {
        AccountReference::Local(account) => (account.id().to_owned(), None),
        AccountReference::Remote(actor) => (
            remote_account_rest_id(&actor.actor_uri),
            Some(actor.actor_uri.clone()),
        ),
    };
    StoredReportIntent::new(
        report_id,
        target_account_id,
        target_remote_actor_uri,
        request.comment.clone().unwrap_or_default(),
        request
            .category
            .clone()
            .unwrap_or_else(|| "other".to_owned()),
        request.forward.unwrap_or(false),
    )
}

async fn insert_report_row(
    db: &D1Database,
    reporter_account_id: &str,
    intent: &StoredReportIntent,
) -> Result<()> {
    let bindings = [
        D1Type::Text(intent.report_id.as_str()),
        D1Type::Text(reporter_account_id),
        D1Type::Text(intent.target_account_id.as_str()),
        intent
            .target_remote_actor_uri
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(intent.comment.as_str()),
        D1Type::Text(intent.category.as_str()),
        D1Type::Integer(if intent.forward { 1 } else { 0 }),
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
    Ok(())
}

async fn insert_report_status_links(
    db: &D1Database,
    report_id: &str,
    status_ids: &[String],
) -> Result<()> {
    for status_id in status_ids {
        let bindings = [D1Type::Text(report_id), D1Type::Text(status_id.as_str())];
        db.prepare(
            "INSERT INTO report_statuses (report_id, status_id)
             VALUES (?1, ?2)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    Ok(())
}

pub(crate) async fn insert_report(
    db: &D1Database,
    reporter_account_id: &str,
    request: &CreateReportRequest,
    target: &AccountReference,
    status_ids: &[String],
) -> Result<ReportRow> {
    let report_id = generate_entity_id(16)?;
    let intent = stored_report_intent(report_id, request, target);
    insert_report_row(db, reporter_account_id, &intent).await?;
    insert_report_status_links(db, &intent.report_id, status_ids).await?;

    find_report_by_id(db, &intent.report_id)
        .await?
        .ok_or_else(|| Error::RustError("failed to load created report".to_owned()))
}

/// Files a remote report unless the same `Flag` already filed one on this
/// target. `scripts/check_migrations.py` runs this text against the schema.
const INSERT_INBOUND_FLAG_REPORT_SQL: &str = "INSERT INTO reports (
            id, account_id, target_account_id, comment, category, forward,
            uri, origin_domain, created_at
        ) VALUES (?1, ?2, ?3, ?4, 'other', 0, ?5, ?6, CURRENT_TIMESTAMP)
        ON CONFLICT (uri, target_account_id) WHERE uri IS NOT NULL DO NOTHING";

/// Links the flagged statuses to the report just filed (`?1`) or, on
/// redelivery, to the one the same `Flag` filed before, so a retry after a
/// partial apply still converges.
const LINK_INBOUND_FLAG_REPORT_STATUSES_SQL: &str =
    "INSERT OR IGNORE INTO report_statuses (report_id, status_id)
        SELECT r.id, j.value
        FROM reports r, json_each(?4) j
        WHERE r.id = ?1 OR (r.uri = ?2 AND r.target_account_id = ?3)";

/// A report another server filed through an ActivityPub `Flag`.
pub(crate) struct InboundFlagReportInsert<'a> {
    /// The REST id of the remote actor that sent the `Flag`.
    pub(crate) reporter_account_id: &'a str,
    pub(crate) origin_domain: &'a str,
    /// The trusted `Flag` id, when it has one.
    pub(crate) uri: Option<&'a str>,
    pub(crate) comment: &'a str,
    pub(crate) report: &'a InboundFlagReport,
}

/// Files one inbound `Flag` report and its statuses in one transaction.
/// Returns the new report id, or `None` when the `Flag` filed it before.
pub(crate) async fn insert_inbound_flag_report(
    db: &D1Database,
    insert: &InboundFlagReportInsert<'_>,
) -> Result<Option<String>> {
    let report_id = generate_entity_id(16)?;
    let uri = || insert.uri.map_or(D1Type::Null, D1Type::Text);
    let target = || D1Type::Text(insert.report.target_account_id.as_str());
    let status_ids = json_string_array(&insert.report.status_ids);
    let report_bindings = [
        D1Type::Text(report_id.as_str()),
        D1Type::Text(insert.reporter_account_id),
        target(),
        D1Type::Text(insert.comment),
        uri(),
        D1Type::Text(insert.origin_domain),
    ];
    let link_bindings = [
        D1Type::Text(report_id.as_str()),
        uri(),
        target(),
        D1Type::Text(status_ids.as_str()),
    ];
    let results = db
        .batch(vec![
            db.prepare(INSERT_INBOUND_FLAG_REPORT_SQL)
                .bind_refs(report_bindings.iter())?,
            db.prepare(LINK_INBOUND_FLAG_REPORT_STATUSES_SQL)
                .bind_refs(link_bindings.iter())?,
        ])
        .await?;
    let filed = results
        .first()
        .map(|result| result.meta())
        .transpose()?
        .flatten()
        .and_then(|meta| meta.changes)
        .unwrap_or(0)
        > 0;
    Ok(filed.then_some(report_id))
}

const REPORT_SELECT_COLUMNS: &str = "id, account_id, target_account_id, target_remote_actor_uri, comment, category, forward, action_taken, action_taken_at, action_taken_by_account_id, origin_domain, created_at";

pub(crate) async fn find_report_by_id(
    db: &D1Database,
    report_id: &str,
) -> Result<Option<ReportRow>> {
    let report_id = D1Type::Text(report_id);
    db.prepare(format!(
        "SELECT {REPORT_SELECT_COLUMNS}
         FROM reports
         WHERE id = ?1
         LIMIT 1"
    ))
    .bind_refs(&report_id)?
    .first::<ReportRow>(None)
    .await
}

pub(crate) async fn list_reports(db: &D1Database, limit: u32) -> Result<Vec<ReportRow>> {
    list_reports_filtered(db, limit, false).await
}

pub(crate) async fn list_reports_filtered(
    db: &D1Database,
    limit: u32,
    pending_only: bool,
) -> Result<Vec<ReportRow>> {
    let bindings = [D1Type::Integer(limit as i32)];
    let where_clause = if pending_only {
        "WHERE action_taken = 0"
    } else {
        ""
    };
    let result = db
        .prepare(format!(
            "SELECT {REPORT_SELECT_COLUMNS}
             FROM reports
             {where_clause}
             ORDER BY created_at DESC
             LIMIT ?1"
        ))
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    d1_results::<ReportRow>(&result)
}

pub(crate) async fn resolve_report(
    db: &D1Database,
    report_id: &str,
    admin_account_id: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(report_id), D1Type::Text(admin_account_id)];
    let result = db
        .prepare(
            "UPDATE reports
             SET action_taken = 1,
                 action_taken_at = CURRENT_TIMESTAMP,
                 action_taken_by_account_id = ?2
             WHERE id = ?1
               AND action_taken = 0",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    Ok(result.meta()?.and_then(|meta| meta.changes).unwrap_or(0) > 0)
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

    Ok(d1_results::<serde_json::Value>(&result)?
        .into_iter()
        .filter_map(|value| {
            value
                .get("status_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::remote::RemoteActorRow;
    use cfwdon_domain::{LocalAccount, LocalAccountRecord};

    fn local_account() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice"))
    }

    fn remote_actor(actor_uri: &str) -> RemoteActorRow {
        RemoteActorRow {
            actor_uri: actor_uri.to_owned(),
            username: "bob".to_owned(),
            domain: "remote.example".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            display_name: "Bob".to_owned(),
            summary_html: String::new(),
            profile_url: Some("https://remote.example/@bob".to_owned()),
            avatar_url: None,
            header_url: None,
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            social_counts_updated_at: None,
        }
    }

    #[test]
    fn report_insert_draft_maps_local_target_and_defaults() {
        let request = CreateReportRequest {
            comment: Some("spam".to_owned()),
            ..CreateReportRequest::default()
        };
        let target = AccountReference::Local(local_account());

        let intent = stored_report_intent("report-1".to_owned(), &request, &target);

        assert_eq!(intent.report_id, "report-1");
        assert_eq!(intent.target_account_id, "acct-1");
        assert_eq!(intent.target_remote_actor_uri, None);
        assert_eq!(intent.comment, "spam");
        assert_eq!(intent.category, "other");
        assert!(!intent.forward);
    }

    #[test]
    fn stored_report_intent_maps_remote_target_and_request_fields() {
        let actor_uri = "https://remote.example/users/bob";
        let request = CreateReportRequest {
            category: Some("spam".to_owned()),
            forward: Some(true),
            ..CreateReportRequest::default()
        };
        let target = AccountReference::Remote(remote_actor(actor_uri));

        let intent = stored_report_intent("report-2".to_owned(), &request, &target);

        assert_eq!(intent.target_account_id, remote_account_rest_id(actor_uri));
        assert_eq!(intent.target_remote_actor_uri.as_deref(), Some(actor_uri));
        assert_eq!(intent.comment, "");
        assert_eq!(intent.category, "spam");
        assert!(intent.forward);
    }
}
