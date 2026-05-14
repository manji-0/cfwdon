use super::{
    AccountReference, CreateReportRequest, D1Database, Error, ReportRow, Result,
    generate_entity_id, remote_account_rest_id,
};
use worker::d1::D1Type;

#[derive(Debug)]
struct ReportInsertDraft {
    id: String,
    target_account_id: String,
    target_remote_actor_uri: Option<String>,
    comment: String,
    category: String,
    forward: bool,
}

impl ReportInsertDraft {
    fn new(id: String, request: &CreateReportRequest, target: &AccountReference) -> Self {
        let (target_account_id, target_remote_actor_uri) = match target {
            AccountReference::Local(account) => (account.id.clone(), None),
            AccountReference::Remote(actor) => (
                remote_account_rest_id(&actor.actor_uri),
                Some(actor.actor_uri.clone()),
            ),
        };
        Self {
            id,
            target_account_id,
            target_remote_actor_uri,
            comment: request.comment.clone().unwrap_or_default(),
            category: request
                .category
                .clone()
                .unwrap_or_else(|| "other".to_owned()),
            forward: request.forward.unwrap_or(false),
        }
    }
}

async fn insert_report_row(
    db: &D1Database,
    reporter_account_id: &str,
    draft: &ReportInsertDraft,
) -> Result<()> {
    let bindings = [
        D1Type::Text(draft.id.as_str()),
        D1Type::Text(reporter_account_id),
        D1Type::Text(draft.target_account_id.as_str()),
        draft
            .target_remote_actor_uri
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(draft.comment.as_str()),
        D1Type::Text(draft.category.as_str()),
        D1Type::Integer(if draft.forward { 1 } else { 0 }),
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
    let draft = ReportInsertDraft::new(report_id, request, target);
    insert_report_row(db, reporter_account_id, &draft).await?;
    insert_report_status_links(db, &draft.id, status_ids).await?;

    find_report_by_id(db, &draft.id)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn local_account(id: &str) -> crate::LocalAccount {
        crate::LocalAccount {
            id: id.to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            locked: false,
            bot: false,
            discoverable: true,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "public".to_owned(),
            default_sensitive: false,
            default_language: None,
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        }
    }

    fn remote_actor(actor_uri: &str) -> crate::RemoteActorRow {
        crate::RemoteActorRow {
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
        }
    }

    #[test]
    fn report_insert_draft_maps_local_target_and_defaults() {
        let request = CreateReportRequest {
            comment: Some("spam".to_owned()),
            ..CreateReportRequest::default()
        };
        let target = AccountReference::Local(local_account("acct-1"));

        let draft = ReportInsertDraft::new("report-1".to_owned(), &request, &target);

        assert_eq!(draft.id, "report-1");
        assert_eq!(draft.target_account_id, "acct-1");
        assert_eq!(draft.target_remote_actor_uri, None);
        assert_eq!(draft.comment, "spam");
        assert_eq!(draft.category, "other");
        assert!(!draft.forward);
    }

    #[test]
    fn report_insert_draft_maps_remote_target_and_request_fields() {
        let actor_uri = "https://remote.example/users/bob";
        let request = CreateReportRequest {
            category: Some("spam".to_owned()),
            forward: Some(true),
            ..CreateReportRequest::default()
        };
        let target = AccountReference::Remote(remote_actor(actor_uri));

        let draft = ReportInsertDraft::new("report-2".to_owned(), &request, &target);

        assert_eq!(draft.target_account_id, remote_account_rest_id(actor_uri));
        assert_eq!(draft.target_remote_actor_uri.as_deref(), Some(actor_uri));
        assert_eq!(draft.comment, "");
        assert_eq!(draft.category, "spam");
        assert!(draft.forward);
    }
}
