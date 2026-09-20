use crate::{Result, StatusDraft};
use cfwdon_domain::{QuoteApprovalPolicy, Visibility};

mod documents;
mod publishing;
mod queries;
mod routes;

pub(crate) use documents::*;
pub(crate) use publishing::*;
pub(crate) use queries::*;
pub(crate) use routes::*;

#[derive(Clone, Debug)]
struct ScheduledStatus {
    cursor_id: i64,
    id: String,
    draft: StatusDraft,
    idempotency_key: Option<String>,
    application_id: Option<i64>,
    scheduled_at: String,
}

fn scheduled_status_from_value(
    value: &serde_json::Value,
) -> Result<ScheduledStatus, worker::Error> {
    Ok(ScheduledStatus {
        cursor_id: json_i64(value, "cursor_id").unwrap_or_default(),
        id: json_string(value, "id").unwrap_or_default(),
        draft: scheduled_status_draft_from_value(value)?,
        idempotency_key: json_string(value, "idempotency_key"),
        application_id: json_i64(value, "application_id"),
        scheduled_at: json_string(value, "scheduled_at")
            .unwrap_or_else(|| "2099-01-01T00:00:00.000Z".to_owned()),
    })
}

fn scheduled_status_draft_from_value(
    value: &serde_json::Value,
) -> Result<StatusDraft, worker::Error> {
    StatusDraft::try_from_persisted(
        json_string(value, "text_content").unwrap_or_default(),
        scheduled_status_visibility(value)?,
        json_string(value, "spoiler_text").unwrap_or_default(),
        json_boolish(value, "sensitive").unwrap_or(false),
        json_string(value, "language"),
        json_string(value, "quote_approval_policy")
            .as_deref()
            .and_then(|value| QuoteApprovalPolicy::parse(value).ok()),
        json_string(value, "in_reply_to_id"),
        scheduled_status_media_ids(value),
        scheduled_status_poll(value),
    )
    .map_err(|error| worker::Error::RustError(error.to_string()))
}

fn scheduled_status_visibility(value: &serde_json::Value) -> Result<Visibility, worker::Error> {
    let Some(raw) = value.get("visibility").and_then(serde_json::Value::as_str) else {
        return Err(worker::Error::RustError(
            "scheduled status visibility is missing".to_owned(),
        ));
    };
    Visibility::parse(raw)
        .map_err(|error| worker::Error::RustError(format!("invalid scheduled visibility: {error}")))
}

fn scheduled_status_media_ids(value: &serde_json::Value) -> Vec<String> {
    value
        .get("media_ids_json")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

fn scheduled_status_poll(value: &serde_json::Value) -> Option<cfwdon_domain::PollDraft> {
    value
        .get("poll_json")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| serde_json::from_str(value).ok())
}

/// A claim older than this is treated as abandoned, so a run that dies midway
/// is retried on a later sweep instead of stranding the scheduled status.
pub(crate) const SCHEDULED_STATUS_CLAIM_TTL_SECS: i64 = 300;

#[derive(Clone, Debug)]
pub(crate) struct DueScheduledStatus {
    pub(crate) id: String,
    pub(crate) account_id: String,
    pub(crate) draft: StatusDraft,
    pub(crate) application_id: Option<i64>,
    pub(crate) quote_of_uri: Option<String>,
    pub(crate) scheduled_at: String,
}

fn due_scheduled_status_from_value(
    value: &serde_json::Value,
) -> Result<DueScheduledStatus, worker::Error> {
    Ok(DueScheduledStatus {
        id: json_string(value, "id").unwrap_or_default(),
        account_id: json_string(value, "account_id").unwrap_or_default(),
        draft: scheduled_status_draft_from_value(value)?,
        application_id: json_i64(value, "application_id"),
        quote_of_uri: json_string(value, "quote_of_uri"),
        scheduled_at: json_string(value, "scheduled_at")
            .unwrap_or_else(|| "2099-01-01T00:00:00.000Z".to_owned()),
    })
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn json_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value.get(key).and_then(serde_json::Value::as_i64)
}

fn json_boolish(value: &serde_json::Value, key: &str) -> Option<bool> {
    value
        .get(key)
        .and_then(|value| value.as_bool().or_else(|| value.as_i64().map(|n| n != 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_scheduled_status_from_value_maps_stored_fields() {
        let value = serde_json::json!({
            "id": "sched-due-1",
            "account_id": "acct-1",
            "text_content": "due later",
            "visibility": "public",
            "spoiler_text": "",
            "sensitive": 0,
            "language": "en",
            "quote_approval_policy": null,
            "in_reply_to_id": null,
            "media_ids_json": "[]",
            "poll_json": null,
            "idempotency_key": "idem-due",
            "application_id": 3,
            "quote_of_uri": "https://social.example/users/alice/statuses/42",
            "scheduled_at": "2026-01-01T00:00:00.000Z"
        });

        let due = due_scheduled_status_from_value(&value).expect("due scheduled status");

        assert_eq!(due.id, "sched-due-1");
        assert_eq!(due.account_id, "acct-1");
        assert_eq!(due.draft.text(), "due later");
        assert_eq!(due.application_id, Some(3));
        assert_eq!(
            due.quote_of_uri.as_deref(),
            Some("https://social.example/users/alice/statuses/42")
        );
        assert_eq!(due.scheduled_at, "2026-01-01T00:00:00.000Z");
    }

    #[test]
    fn scheduled_status_from_value_maps_stored_fields() {
        let poll = cfwdon_domain::PollDraft::try_new(
            vec!["yes".to_owned(), "no".to_owned()],
            3600,
            false,
            true,
        )
        .expect("poll draft");
        let poll_json = serde_json::to_string(&poll).expect("poll draft JSON");
        let value = serde_json::json!({
            "cursor_id": 42,
            "id": "sched-1",
            "text_content": "hello later",
            "visibility": "private",
            "spoiler_text": "cw",
            "sensitive": 1,
            "language": "ja",
            "quote_approval_policy": "followers",
            "in_reply_to_id": "status-1",
            "media_ids_json": "[]",
            "poll_json": poll_json,
            "idempotency_key": "idem-1",
            "application_id": 7,
            "scheduled_at": "2099-02-03T04:05:06.000Z"
        });

        let status = scheduled_status_from_value(&value).expect("scheduled status");

        assert_eq!(status.cursor_id, 42);
        assert_eq!(status.id, "sched-1");
        assert_eq!(status.draft.text(), "hello later");
        assert_eq!(status.draft.visibility(), Visibility::FollowersOnly);
        assert_eq!(status.draft.spoiler_text(), "cw");
        assert!(status.draft.sensitive());
        assert_eq!(status.draft.language(), Some("ja"));
        assert_eq!(
            status.draft.quote_approval_policy(),
            Some(QuoteApprovalPolicy::Followers)
        );
        assert_eq!(status.draft.in_reply_to_id(), Some("status-1"));
        assert!(status.draft.media_ids().is_empty());
        let poll = status.draft.poll().expect("poll draft");
        assert_eq!(poll.options(), &["yes", "no"]);
        assert_eq!(poll.expires_in_seconds(), 3600);
        assert!(!poll.multiple());
        assert!(poll.hide_totals());
        assert_eq!(status.idempotency_key.as_deref(), Some("idem-1"));
        assert_eq!(status.application_id, Some(7));
        assert_eq!(status.scheduled_at, "2099-02-03T04:05:06.000Z");
    }

    #[test]
    fn scheduled_status_from_value_rejects_invalid_visibility() {
        let value = serde_json::json!({
            "text_content": "hello",
            "visibility": "unknown",
            "sensitive": 0,
            "media_ids_json": "not-json",
            "poll_json": "not-json"
        });

        assert!(scheduled_status_from_value(&value).is_err());
    }

    #[test]
    fn scheduled_status_from_value_rejects_empty_payload() {
        let value = serde_json::json!({
            "visibility": "public",
            "sensitive": 0
        });

        assert!(scheduled_status_from_value(&value).is_err());
    }
}
