use crate::auth::LocalApiAuthentication;
use crate::auth::extract_authenticated_user;
use crate::{
    AppConfig, CreatePublishedStatusInput, D1Database, Env, MastodonMediaAttachmentResponse,
    Request, Response, Result, RouteContext, StatusDraft, app_bearer_token_from_request,
    authenticate_local_api_request, build_internal_cursor_link_for_url_with_min_id,
    create_published_status_and_response, find_account_by_id, find_media_attachment_by_id,
    find_oauth_app_id_by_bearer_token, generate_entity_id, load_config, normalize_scheduled_at,
    now_iso_string, oauth_access_token_has_any_scope, parse_internal_pagination_id,
    require_authenticated_local_account, resolve_attachable_media, resolve_in_reply_to_account_id,
    subtract_seconds_from_iso_string, validate_local_quote_creation,
    validate_scheduled_at_minimum_offset,
};
use cfwdon_domain::{QuoteApprovalPolicy, Visibility};
use serde::{Deserialize, Serialize};
use worker::{Error, console_error, d1::D1Type};

#[derive(Clone, Debug)]
struct ScheduledStatus {
    cursor_id: i64,
    id: String,
    draft: StatusDraft,
    idempotency_key: Option<String>,
    application_id: Option<i64>,
    scheduled_at: String,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn scheduled_status_document(id: &str) -> serde_json::Value {
    scheduled_status_document_with_params(id, "2099-01-01T00:00:00.000Z", None)
}

pub(crate) fn scheduled_status_document_with_params(
    id: &str,
    scheduled_at: &str,
    draft: Option<&StatusDraft>,
) -> serde_json::Value {
    let poll = draft
        .and_then(|draft| draft.poll().map(serde_json::to_value).transpose().ok())
        .flatten()
        .unwrap_or(serde_json::Value::Null);
    let media_ids = draft
        .map(|draft| draft.media_ids().to_vec())
        .filter(|media_ids| !media_ids.is_empty())
        .map(serde_json::Value::from)
        .unwrap_or(serde_json::Value::Null);
    let language = draft
        .and_then(|draft| draft.language().map(str::to_owned))
        .map(serde_json::Value::from)
        .unwrap_or(serde_json::Value::Null);
    let sensitive = draft
        .map(|draft| serde_json::Value::Bool(draft.sensitive()))
        .unwrap_or(serde_json::Value::Null);
    let visibility = draft
        .map(|draft| serde_json::Value::from(draft.visibility().as_str()))
        .unwrap_or(serde_json::Value::Null);
    let spoiler_text = draft
        .map(|draft| serde_json::Value::from(draft.spoiler_text()))
        .unwrap_or(serde_json::Value::Null);
    let in_reply_to_id = draft
        .and_then(|draft| draft.in_reply_to_id().map(str::to_owned))
        .map(serde_json::Value::from)
        .unwrap_or(serde_json::Value::Null);
    let text = draft
        .map(|draft| draft.text().to_owned())
        .unwrap_or_default();

    serde_json::json!({
        "id": id,
        "scheduled_at": scheduled_at,
        "params": {
            "poll": poll,
            "text": text,
            "language": language,
            "media_ids": media_ids,
            "sensitive": sensitive,
            "visibility": visibility,
            "idempotency": serde_json::Value::Null,
            "scheduled_at": serde_json::Value::Null,
            "spoiler_text": spoiler_text,
            "application_id": 0,
            "in_reply_to_id": in_reply_to_id,
            "with_rate_limit": false,
        },
        "media_attachments": [],
    })
}

fn build_scheduled_status_document_with_media(
    status: &ScheduledStatus,
    media_attachments: Vec<serde_json::Value>,
) -> serde_json::Value {
    let mut document = scheduled_status_document_with_params(
        &status.id,
        &status.scheduled_at,
        Some(&status.draft),
    );
    if let Some(params) = document
        .get_mut("params")
        .and_then(serde_json::Value::as_object_mut)
    {
        params.insert(
            "idempotency".to_owned(),
            status
                .idempotency_key
                .as_ref()
                .map(|value| serde_json::Value::from(value.clone()))
                .unwrap_or(serde_json::Value::Null),
        );
        params.insert(
            "application_id".to_owned(),
            status
                .application_id
                .map(serde_json::Value::from)
                .unwrap_or(serde_json::json!(0)),
        );
    }
    if let Some(object) = document.as_object_mut() {
        object.insert(
            "media_attachments".to_owned(),
            serde_json::Value::Array(media_attachments),
        );
    }
    document
}

async fn load_scheduled_media_attachments(
    db: &D1Database,
    config: &AppConfig,
    media_ids: &[String],
) -> Result<Vec<serde_json::Value>> {
    let mut attachments = Vec::new();
    for media_id in media_ids {
        let Some(media) = find_media_attachment_by_id(db, media_id).await? else {
            continue;
        };
        attachments.push(
            serde_json::to_value(MastodonMediaAttachmentResponse::from_row(&media, config))
                .unwrap_or(serde_json::Value::Null),
        );
    }
    Ok(attachments)
}

async fn build_scheduled_status_document(
    db: &D1Database,
    config: &AppConfig,
    status: &ScheduledStatus,
) -> Result<serde_json::Value> {
    let attachments =
        load_scheduled_media_attachments(db, config, status.draft.media_ids()).await?;
    Ok(build_scheduled_status_document_with_media(
        status,
        attachments,
    ))
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

#[cfg(test)]
fn is_scheduled_status_due(scheduled_at: &str, now_iso: &str) -> bool {
    scheduled_at <= now_iso
}

#[cfg(test)]
fn compare_due_scheduled_status_order(
    left: &DueScheduledStatus,
    right: &DueScheduledStatus,
) -> std::cmp::Ordering {
    left.scheduled_at
        .cmp(&right.scheduled_at)
        .then_with(|| left.id.cmp(&right.id))
}

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

/// Claims older than this timestamp are considered abandoned.
fn scheduled_status_stale_claim_threshold(now_iso: &str) -> Result<String> {
    subtract_seconds_from_iso_string(now_iso, SCHEDULED_STATUS_CLAIM_TTL_SECS)
}

/// Marks the row as being published. Returns false when another sweep already
/// claimed it, or when the schedule changed after it was listed, so a status is
/// never published twice by concurrent runs.
async fn claim_scheduled_status(
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

async fn delete_scheduled_status_by_id(db: &D1Database, id: &str) -> Result<bool> {
    let id_binding = D1Type::Text(id);
    let result = db
        .prepare("DELETE FROM scheduled_statuses WHERE id = ?1")
        .bind_refs(&id_binding)?
        .run()
        .await?;
    scheduled_status_did_change(&result)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PublishDueScheduledStatusOutcome {
    Published,
    Skipped,
    Failed,
}

pub(crate) async fn publish_due_scheduled_status(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    due: &DueScheduledStatus,
    now_iso: &str,
    stale_claim_before_iso: &str,
) -> PublishDueScheduledStatusOutcome {
    match claim_scheduled_status(
        db,
        &due.id,
        &due.scheduled_at,
        now_iso,
        stale_claim_before_iso,
    )
    .await
    {
        Ok(true) => {}
        Ok(false) => return PublishDueScheduledStatusOutcome::Skipped,
        Err(error) => {
            console_error!(
                "scheduled status publish failed: could not claim scheduled status {}: {error}",
                due.id
            );
            return PublishDueScheduledStatusOutcome::Failed;
        }
    }

    let account = match find_account_by_id(db, &due.account_id).await {
        Ok(Some(account)) => account,
        Ok(None) => {
            console_error!(
                "scheduled status publish skipped: account {} not found for scheduled status {}",
                due.account_id,
                due.id
            );
            return PublishDueScheduledStatusOutcome::Skipped;
        }
        Err(error) => {
            console_error!(
                "scheduled status publish failed: could not load account {} for scheduled status {}: {error}",
                due.account_id,
                due.id
            );
            return PublishDueScheduledStatusOutcome::Failed;
        }
    };

    let pending_media = match resolve_attachable_media(db, &account, due.draft.media_ids()).await {
        Ok(media) => media,
        Err(message) => {
            console_error!(
                "scheduled status publish failed: media resolution for scheduled status {}: {message}",
                due.id
            );
            return PublishDueScheduledStatusOutcome::Failed;
        }
    };

    let in_reply_to_account_id = if let Some(status_id) = due.draft.in_reply_to_id() {
        match resolve_in_reply_to_account_id(db, status_id).await {
            Ok(Some(account_id)) => Some(account_id),
            Ok(None) => {
                console_error!(
                    "scheduled status publish failed: in_reply_to_id {} references unknown status for scheduled status {}",
                    status_id,
                    due.id
                );
                return PublishDueScheduledStatusOutcome::Failed;
            }
            Err(error) => {
                console_error!(
                    "scheduled status publish failed: could not resolve in_reply_to for scheduled status {}: {error}",
                    due.id
                );
                return PublishDueScheduledStatusOutcome::Failed;
            }
        }
    } else {
        None
    };

    if let Some(quote_of_uri) = due.quote_of_uri.as_deref() {
        match validate_local_quote_creation(db, config, &account, &due.draft, quote_of_uri).await {
            Ok(None) => {}
            Ok(Some(message)) => {
                console_error!(
                    "scheduled status publish failed: quote validation for scheduled status {}: {message}",
                    due.id
                );
                return PublishDueScheduledStatusOutcome::Failed;
            }
            Err(error) => {
                console_error!(
                    "scheduled status publish failed: quote validation error for scheduled status {}: {error}",
                    due.id
                );
                return PublishDueScheduledStatusOutcome::Failed;
            }
        }
    }

    match create_published_status_and_response(
        db,
        config,
        env,
        CreatePublishedStatusInput {
            account: &account,
            application_id: due.application_id,
            draft: &due.draft,
            pending_media: &pending_media,
            in_reply_to_account_id,
            quote_of_uri: due.quote_of_uri.as_deref(),
        },
    )
    .await
    {
        Ok(_) => {
            if let Err(error) = delete_scheduled_status_by_id(db, &due.id).await {
                console_error!(
                    "scheduled status publish warning: published scheduled status {} but failed to delete row: {error}",
                    due.id
                );
            }
            PublishDueScheduledStatusOutcome::Published
        }
        Err(error) => {
            console_error!(
                "scheduled status publish failed: could not publish scheduled status {}: {error}",
                due.id
            );
            PublishDueScheduledStatusOutcome::Failed
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct DueScheduledStatusProcessSummary {
    pub(crate) published: u32,
    pub(crate) skipped: u32,
    pub(crate) failed: u32,
}

pub(crate) async fn process_due_scheduled_statuses_for_config(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    limit: u32,
) -> Result<DueScheduledStatusProcessSummary> {
    let now_iso = now_iso_string()?;
    let stale_claim_before_iso = scheduled_status_stale_claim_threshold(&now_iso)?;
    let mut summary = DueScheduledStatusProcessSummary::default();
    for due in list_due_scheduled_statuses(db, &now_iso, &stale_claim_before_iso, limit).await? {
        match publish_due_scheduled_status(db, config, env, &due, &now_iso, &stale_claim_before_iso)
            .await
        {
            PublishDueScheduledStatusOutcome::Published => summary.published += 1,
            PublishDueScheduledStatusOutcome::Skipped => summary.skipped += 1,
            PublishDueScheduledStatusOutcome::Failed => summary.failed += 1,
        }
    }
    Ok(summary)
}

pub(crate) async fn process_due_scheduled_statuses(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Auth0 authentication required", 401),
    }

    let db = crate::bind_request_d1(&ctx, &config)?;
    let summary =
        process_due_scheduled_statuses_for_config(&db, &config, Some(&ctx.env), 32).await?;
    Response::from_json(&summary)
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
    fn scheduled_status_stale_claim_threshold_is_one_ttl_back() {
        use time::{OffsetDateTime, format_description::well_known::Rfc3339};

        let now = "2026-01-01T00:10:00Z";
        let threshold = scheduled_status_stale_claim_threshold(now).expect("threshold");
        let parsed_now = OffsetDateTime::parse(now, &Rfc3339).expect("now");
        let parsed_threshold = OffsetDateTime::parse(&threshold, &Rfc3339).expect("threshold");
        assert_eq!(
            (parsed_now - parsed_threshold).whole_seconds(),
            SCHEDULED_STATUS_CLAIM_TTL_SECS
        );
    }

    #[test]
    fn scheduled_status_stale_claim_threshold_precedes_now() {
        let now = "2026-01-01T00:10:00Z";
        let threshold = scheduled_status_stale_claim_threshold(now).expect("threshold");
        assert!(threshold.as_str() < now);
    }

    #[test]
    fn is_scheduled_status_due_compares_iso_timestamps() {
        assert!(is_scheduled_status_due(
            "2026-01-01T00:00:00.000Z",
            "2026-01-02T00:00:00.000Z"
        ));
        assert!(is_scheduled_status_due(
            "2026-01-02T00:00:00.000Z",
            "2026-01-02T00:00:00.000Z"
        ));
        assert!(!is_scheduled_status_due(
            "2026-01-03T00:00:00.000Z",
            "2026-01-02T00:00:00.000Z"
        ));
    }

    #[test]
    fn compare_due_scheduled_status_order_sorts_by_scheduled_at_then_id() {
        let earlier = DueScheduledStatus {
            id: "b".to_owned(),
            account_id: "acct".to_owned(),
            draft: StatusDraft::try_from_persisted(
                "a".to_owned(),
                Visibility::Public,
                "".to_owned(),
                false,
                None,
                None,
                None,
                Vec::new(),
                None,
            )
            .expect("draft"),
            application_id: None,
            quote_of_uri: None,
            scheduled_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };
        let later_same_time = DueScheduledStatus {
            id: "c".to_owned(),
            account_id: "acct".to_owned(),
            draft: earlier.draft.clone(),
            application_id: None,
            quote_of_uri: None,
            scheduled_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };
        let much_later = DueScheduledStatus {
            id: "a".to_owned(),
            account_id: "acct".to_owned(),
            draft: earlier.draft.clone(),
            application_id: None,
            quote_of_uri: None,
            scheduled_at: "2026-01-02T00:00:00.000Z".to_owned(),
        };

        assert_eq!(
            compare_due_scheduled_status_order(&earlier, &later_same_time),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_due_scheduled_status_order(&later_same_time, &much_later),
            std::cmp::Ordering::Less
        );

        let mut ordered = [much_later, later_same_time, earlier];
        ordered.sort_by(compare_due_scheduled_status_order);
        assert_eq!(ordered[0].id, "b");
        assert_eq!(ordered[1].id, "c");
        assert_eq!(ordered[2].id, "a");
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

async fn insert_scheduled_status(
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

async fn list_scheduled_statuses_for_account(
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

async fn find_scheduled_status_for_account(
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

async fn update_scheduled_status_time(
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

async fn delete_scheduled_status(
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

async fn require_scheduled_status_id(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing scheduled status id".to_owned()))
}

async fn require_authenticated_scheduled_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<crate::LocalAccount>> {
    require_authenticated_local_account(req, db, config).await
}

#[derive(Debug)]
struct ScheduledStatusRequestAccess {
    viewer: crate::LocalAccount,
    application_id: Option<i64>,
}

async fn request_scheduled_status_application_id(
    req: &Request,
    db: &D1Database,
) -> Result<Option<i64>> {
    let Some(token) = app_bearer_token_from_request(req)? else {
        return Ok(None);
    };
    let app_id = find_oauth_app_id_by_bearer_token(db, &token)
        .await?
        .ok_or_else(|| Error::RustError("invalid scheduled status app bearer token".to_owned()))?;
    Ok(Some(app_id))
}

fn scheduled_statuses_unauthorized_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401))
}

fn scheduled_statuses_not_found_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "Record not found",
    }))?
    .with_status(404))
}

fn scheduled_statuses_outside_authorized_scopes_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

#[derive(Debug, Default, Deserialize)]
struct ScheduledStatusesQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateScheduledStatusRequest {
    scheduled_at: Option<String>,
}

async fn parse_scheduled_status_update_request(
    req: &mut Request,
) -> std::result::Result<UpdateScheduledStatusRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        req.json::<UpdateScheduledStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON scheduled status payload: {error}"))
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form scheduled status payload: {error}"))?;
        Ok(UpdateScheduledStatusRequest {
            scheduled_at: form.get_field("scheduled_at"),
        })
    }
}

async fn resolve_scheduled_status_request_access(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    scopes: &[&str],
) -> Result<Option<ScheduledStatusRequestAccess>> {
    match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, scopes) {
                return Err(Error::RustError(
                    "scheduled status token outside authorized scopes".to_owned(),
                ));
            }
            Ok(Some(ScheduledStatusRequestAccess {
                viewer: auth.account,
                application_id: Some(auth.token.oauth_app_id),
            }))
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => Ok(None),
        LocalApiAuthentication::Auth0(viewer) => {
            let application_id = match request_scheduled_status_application_id(req, db).await {
                Ok(value) => value,
                Err(Error::RustError(message))
                    if message == "invalid scheduled status app bearer token" =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            Ok(Some(ScheduledStatusRequestAccess {
                viewer,
                application_id,
            }))
        }
        LocalApiAuthentication::None => {
            Ok(require_authenticated_scheduled_account(req, db, config)
                .await?
                .map(|viewer| ScheduledStatusRequestAccess {
                    viewer,
                    application_id: None,
                }))
        }
    }
}

fn build_scheduled_statuses_link_header(
    req: &Request,
    limit: u32,
    first_cursor: Option<i64>,
    last_cursor: Option<i64>,
    has_next: bool,
) -> Result<Option<String>> {
    let url = req.url()?;
    let mut links = Vec::new();

    if has_next && let Some(cursor) = last_cursor {
        links.push(build_internal_cursor_link_for_url_with_min_id(
            &url,
            limit,
            Some(cursor),
            None,
            None,
            "next",
        )?);
    }

    if let Some(cursor) = first_cursor {
        links.push(build_internal_cursor_link_for_url_with_min_id(
            &url,
            limit,
            None,
            None,
            Some(cursor),
            "prev",
        )?);
    }

    if links.is_empty() {
        return Ok(None);
    }

    Ok(Some(links.join(", ")))
}

pub(crate) async fn scheduled_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["read:statuses", "read"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let query: ScheduledStatusesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let mut statuses = list_scheduled_statuses_for_account(
        &db,
        access.viewer.id(),
        access.application_id,
        limit.saturating_add(1),
        max_id,
        since_id,
        min_id,
    )
    .await?;
    let has_next = statuses.len() as u32 > limit;
    if has_next {
        statuses.truncate(limit as usize);
    }
    let mut documents = Vec::new();
    for status in &statuses {
        documents.push(build_scheduled_status_document(&db, &config, status).await?);
    }
    let mut builder = Response::builder();
    if let Some(link_header) = build_scheduled_statuses_link_header(
        &req,
        limit,
        statuses.first().map(|status| status.cursor_id),
        statuses.last().map(|status| status.cursor_id),
        has_next,
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }
    builder.from_json(&documents)
}

pub(crate) async fn scheduled_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["read:statuses", "read"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let id = require_scheduled_status_id(&ctx).await?;
    let Some(status) =
        find_scheduled_status_for_account(&db, access.viewer.id(), access.application_id, &id)
            .await?
    else {
        return scheduled_statuses_not_found_response();
    };
    Response::from_json(&build_scheduled_status_document(&db, &config, &status).await?)
}

pub(crate) async fn update_scheduled_status_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["write:statuses", "write"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let id = require_scheduled_status_id(&ctx).await?;
    let Some(current) =
        find_scheduled_status_for_account(&db, access.viewer.id(), access.application_id, &id)
            .await?
    else {
        return scheduled_statuses_not_found_response();
    };
    let request = match parse_scheduled_status_update_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let scheduled_at = match normalize_scheduled_at(request.scheduled_at.as_deref()) {
        Ok(Some(value)) => value,
        Ok(None) => current.scheduled_at.clone(),
        Err(message) => return Response::error(message, 422),
    };
    if let Err(message) = validate_scheduled_at_minimum_offset(&scheduled_at) {
        return Response::error(message, 422);
    }
    update_scheduled_status_time(
        &db,
        access.viewer.id(),
        access.application_id,
        &id,
        &scheduled_at,
    )
    .await?;
    let Some(updated) =
        find_scheduled_status_for_account(&db, access.viewer.id(), access.application_id, &id)
            .await?
    else {
        return scheduled_statuses_not_found_response();
    };
    Response::from_json(&build_scheduled_status_document(&db, &config, &updated).await?)
}

pub(crate) async fn create_scheduled_status(
    db: &D1Database,
    config: &AppConfig,
    account_id: &str,
    draft: &StatusDraft,
    idempotency_key: Option<&str>,
    application_id: Option<i64>,
    quote_of_uri: Option<&str>,
    scheduled_at: &str,
) -> Result<serde_json::Value> {
    let status = insert_scheduled_status(
        db,
        account_id,
        draft,
        idempotency_key,
        application_id,
        quote_of_uri,
        scheduled_at,
    )
    .await?;
    build_scheduled_status_document(db, config, &status).await
}

pub(crate) async fn delete_scheduled_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let access = match resolve_scheduled_status_request_access(
        &req,
        &db,
        &config,
        &["write:statuses", "write"],
    )
    .await
    {
        Ok(Some(access)) => access,
        Ok(None) => return scheduled_statuses_unauthorized_response(),
        Err(Error::RustError(message))
            if message == "scheduled status token outside authorized scopes" =>
        {
            return scheduled_statuses_outside_authorized_scopes_response();
        }
        Err(error) => return Err(error),
    };
    let id = require_scheduled_status_id(&ctx).await?;
    if !delete_scheduled_status(&db, access.viewer.id(), access.application_id, &id).await? {
        return scheduled_statuses_not_found_response();
    }
    Response::from_json(&serde_json::json!({}))
}
