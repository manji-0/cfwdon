use super::{
    DueScheduledStatus, ScheduledStatus, due_scheduled_status_from_value,
    scheduled_status_from_value,
};
use crate::{D1Database, Result, StatusDraft, generate_entity_id, now_iso_string};
use worker::{Error, d1::D1Type};

pub(crate) async fn list_due_scheduled_statuses(
    db: &D1Database,
    now_iso: &str,
    stale_claim_before_iso: &str,
    limit: u32,
) -> Result<Vec<DueScheduledStatus>> {
    let bindings = [
        D1Type::Text(now_iso),
        D1Type::Integer(limit as i32),
        D1Type::Text(stale_claim_before_iso),
    ];
    let rows = db
        .prepare(
            "SELECT
                id,
                account_id,
                text_content,
                visibility,
                spoiler_text,
                sensitive,
                language,
                quote_approval_policy,
                in_reply_to_id,
                media_ids_json,
                poll_json,
                idempotency_key,
                application_id,
                quote_of_uri,
                scheduled_at
             FROM scheduled_statuses
             WHERE scheduled_at <= ?1
               AND (claimed_at IS NULL OR claimed_at <= ?3)
             ORDER BY scheduled_at ASC, id ASC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<serde_json::Value>(&__d1))?;
    Ok(rows
        .iter()
        .filter_map(|row| due_scheduled_status_from_value(row).ok())
        .collect())
}
/// Marks the row as being published. Returns false when another sweep already
/// claimed it, or when the schedule changed after it was listed, so a status is
/// never published twice by concurrent runs.
pub(in crate::scheduled_statuses) async fn claim_scheduled_status(
    db: &D1Database,
    id: &str,
    scheduled_at: &str,
    now_iso: &str,
    stale_claim_before_iso: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(id),
        D1Type::Text(scheduled_at),
        D1Type::Text(now_iso),
        D1Type::Text(stale_claim_before_iso),
    ];
    let result = db
        .prepare(
            "UPDATE scheduled_statuses
             SET claimed_at = ?3
             WHERE id = ?1
               AND scheduled_at = ?2
               AND (claimed_at IS NULL OR claimed_at <= ?4)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    scheduled_status_did_change(&result)
}
pub(in crate::scheduled_statuses) async fn delete_scheduled_status_by_id(
    db: &D1Database,
    id: &str,
) -> Result<bool> {
    let id_binding = D1Type::Text(id);
    let result = db
        .prepare("DELETE FROM scheduled_statuses WHERE id = ?1")
        .bind_refs(&id_binding)?
        .run()
        .await?;
    scheduled_status_did_change(&result)
}
#[derive(Debug)]
struct ScheduledStatusInsertRow {
    id: String,
    created_at: String,
    media_ids_json: String,
    poll_json: Option<String>,
}

impl ScheduledStatusInsertRow {
    fn new(id: String, created_at: String, draft: &StatusDraft) -> Result<Self> {
        let media_ids_json = serde_json::to_string(draft.media_ids()).map_err(|error| {
            Error::RustError(format!("failed to encode scheduled media ids: {error}"))
        })?;
        let poll_json = draft
            .poll()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| {
                Error::RustError(format!("failed to encode scheduled poll: {error}"))
            })?;
        Ok(Self {
            id,
            created_at,
            media_ids_json,
            poll_json,
        })
    }

    fn into_scheduled_status(
        self,
        draft: &StatusDraft,
        idempotency_key: Option<&str>,
        application_id: Option<i64>,
        scheduled_at: &str,
    ) -> ScheduledStatus {
        ScheduledStatus {
            cursor_id: 0,
            id: self.id,
            draft: draft.clone(),
            idempotency_key: idempotency_key.map(ToOwned::to_owned),
            application_id,
            scheduled_at: scheduled_at.to_owned(),
        }
    }
}
pub(in crate::scheduled_statuses) async fn insert_scheduled_status(
    db: &D1Database,
    account_id: &str,
    draft: &StatusDraft,
    idempotency_key: Option<&str>,
    application_id: Option<i64>,
    quote_of_uri: Option<&str>,
    scheduled_at: &str,
) -> Result<ScheduledStatus> {
    let row = ScheduledStatusInsertRow::new(generate_entity_id(16)?, now_iso_string()?, draft)?;
    let bindings = [
        D1Type::Text(row.id.as_str()),
        D1Type::Text(account_id),
        D1Type::Text(draft.text()),
        D1Type::Text(draft.visibility().as_str()),
        D1Type::Text(draft.spoiler_text()),
        D1Type::Integer(if draft.sensitive() { 1 } else { 0 }),
        draft.language().map_or(D1Type::Null, D1Type::Text),
        draft
            .quote_approval_policy()
            .map(|policy| D1Type::Text(policy.as_str()))
            .unwrap_or(D1Type::Null),
        draft.in_reply_to_id().map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(row.media_ids_json.as_str()),
        row.poll_json.as_deref().map_or(D1Type::Null, D1Type::Text),
        idempotency_key.map_or(D1Type::Null, D1Type::Text),
        application_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
        quote_of_uri.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(scheduled_at),
        D1Type::Text(row.created_at.as_str()),
    ];
    db.prepare(
        "INSERT INTO scheduled_statuses (
            id,
            account_id,
            text_content,
            visibility,
            spoiler_text,
            sensitive,
            language,
            quote_approval_policy,
            in_reply_to_id,
            media_ids_json,
            poll_json,
            idempotency_key,
            application_id,
            quote_of_uri,
            scheduled_at,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(row.into_scheduled_status(draft, idempotency_key, application_id, scheduled_at))
}

pub(in crate::scheduled_statuses) async fn list_scheduled_statuses_for_account(
    db: &D1Database,
    account_id: &str,
    request_application_id: Option<i64>,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    min_id: Option<i64>,
) -> Result<Vec<ScheduledStatus>> {
    let bindings = [
        D1Type::Text(account_id),
        max_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
        since_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
        min_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
        request_application_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
        D1Type::Integer(limit as i32),
    ];
    let query = if min_id.is_some() {
        "SELECT
            rowid AS cursor_id,
            id,
            account_id,
            text_content,
            visibility,
            spoiler_text,
            sensitive,
            language,
            quote_approval_policy,
            in_reply_to_id,
            media_ids_json,
            poll_json,
            idempotency_key,
            application_id,
            scheduled_at
         FROM scheduled_statuses
         WHERE account_id = ?1
           AND (?2 IS NULL OR rowid < ?2)
           AND (?4 IS NULL OR rowid > ?4)
           AND (?5 IS NULL OR application_id = ?5)
         ORDER BY rowid ASC
         LIMIT ?6"
    } else {
        "SELECT
            rowid AS cursor_id,
            id,
            account_id,
            text_content,
            visibility,
            spoiler_text,
            sensitive,
            language,
            quote_approval_policy,
            in_reply_to_id,
            media_ids_json,
            poll_json,
            idempotency_key,
            application_id,
            scheduled_at
         FROM scheduled_statuses
         WHERE account_id = ?1
           AND (?2 IS NULL OR rowid < ?2)
           AND (?3 IS NULL OR rowid > ?3)
           AND (?5 IS NULL OR application_id = ?5)
         ORDER BY rowid DESC
         LIMIT ?6"
    };
    let rows = db
        .prepare(query)
        .bind_refs(bindings.iter())?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<serde_json::Value>(&__d1))?;
    let mut statuses = rows
        .iter()
        .filter_map(|row| scheduled_status_from_value(row).ok())
        .collect::<Vec<_>>();
    if min_id.is_some() {
        statuses.reverse();
    }
    Ok(statuses)
}

pub(in crate::scheduled_statuses) async fn find_scheduled_status_for_account(
    db: &D1Database,
    account_id: &str,
    request_application_id: Option<i64>,
    id: &str,
) -> Result<Option<ScheduledStatus>> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(id),
        request_application_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
    ];
    let row = db
        .prepare(
            "SELECT
                rowid AS cursor_id,
                id,
                account_id,
                text_content,
                visibility,
                spoiler_text,
                sensitive,
                language,
                quote_approval_policy,
                in_reply_to_id,
                media_ids_json,
                poll_json,
                idempotency_key,
                application_id,
                scheduled_at
             FROM scheduled_statuses
             WHERE account_id = ?1
               AND id = ?2
               AND (?3 IS NULL OR application_id = ?3)
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    row.as_ref().map(scheduled_status_from_value).transpose()
}

pub(in crate::scheduled_statuses) async fn update_scheduled_status_time(
    db: &D1Database,
    account_id: &str,
    request_application_id: Option<i64>,
    id: &str,
    scheduled_at: &str,
) -> Result<bool> {
    let updated_at = now_iso_string()?;
    let bindings = [
        D1Type::Text(scheduled_at),
        D1Type::Text(updated_at.as_str()),
        D1Type::Text(account_id),
        D1Type::Text(id),
        request_application_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
    ];
    let result = if request_application_id.is_some() {
        db.prepare(
            "UPDATE scheduled_statuses
             SET scheduled_at = ?1,
                 updated_at = ?2
             WHERE account_id = ?3
               AND id = ?4
               AND application_id = ?5",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?
    } else {
        db.prepare(
            "UPDATE scheduled_statuses
             SET scheduled_at = ?1,
                 updated_at = ?2
             WHERE account_id = ?3
               AND id = ?4",
        )
        .bind_refs(bindings[..4].iter())?
        .run()
        .await?
    };
    scheduled_status_did_change(&result)
}

pub(in crate::scheduled_statuses) async fn delete_scheduled_status(
    db: &D1Database,
    account_id: &str,
    request_application_id: Option<i64>,
    id: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(id),
        request_application_id.map_or(D1Type::Null, |value| {
            D1Type::Integer(i32::try_from(value).unwrap_or(i32::MAX))
        }),
    ];
    let result = if request_application_id.is_some() {
        db.prepare(
            "DELETE FROM scheduled_statuses
             WHERE account_id = ?1
               AND id = ?2
               AND application_id = ?3",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?
    } else {
        db.prepare(
            "DELETE FROM scheduled_statuses
             WHERE account_id = ?1
               AND id = ?2",
        )
        .bind_refs(bindings[..2].iter())?
        .run()
        .await?
    };
    scheduled_status_did_change(&result)
}
fn scheduled_status_did_change(result: &worker::d1::D1Result) -> Result<bool> {
    Ok(result
        .meta()?
        .and_then(|meta| {
            meta.changed_db
                .or_else(|| meta.changes.map(|changes| changes > 0))
        })
        .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::{QuoteApprovalPolicy, Visibility};

    #[test]
    fn scheduled_status_insert_row_encodes_storage_fields() {
        let draft = StatusDraft::try_from_persisted(
            "scheduled".to_owned(),
            Visibility::Unlisted,
            "cw".to_owned(),
            true,
            Some("ja".to_owned()),
            Some(QuoteApprovalPolicy::Followers),
            Some("reply-1".to_owned()),
            vec!["media-1".to_owned()],
            None,
        )
        .expect("draft");

        let row = ScheduledStatusInsertRow::new("sched-1".to_owned(), "now".to_owned(), &draft)
            .expect("insert row");

        assert_eq!(row.media_ids_json, "[\"media-1\"]");
        assert!(row.poll_json.is_none());
    }

    #[test]
    fn scheduled_status_insert_row_encodes_poll_fields() {
        let draft = StatusDraft::try_from_persisted(
            "scheduled".to_owned(),
            Visibility::Unlisted,
            "cw".to_owned(),
            true,
            Some("ja".to_owned()),
            Some(QuoteApprovalPolicy::Followers),
            None,
            Vec::new(),
            Some(
                cfwdon_domain::PollDraft::try_new(
                    vec!["yes".to_owned(), "no".to_owned()],
                    600,
                    true,
                    false,
                )
                .expect("poll draft"),
            ),
        )
        .expect("draft");

        let row = ScheduledStatusInsertRow::new("sched-1".to_owned(), "now".to_owned(), &draft)
            .expect("insert row");

        assert_eq!(row.media_ids_json, "[]");
        let poll: cfwdon_domain::PollDraft =
            serde_json::from_str(row.poll_json.as_deref().expect("poll JSON")).unwrap();
        assert_eq!(poll.options(), &["yes", "no"]);
        assert_eq!(poll.expires_in_seconds(), 600);
        assert!(poll.multiple());
        assert!(!poll.hide_totals());
    }
}
