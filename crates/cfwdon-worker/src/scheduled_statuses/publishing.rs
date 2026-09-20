use super::{
    DueScheduledStatus, SCHEDULED_STATUS_CLAIM_TTL_SECS, claim_scheduled_status,
    delete_scheduled_status_by_id, list_due_scheduled_statuses,
};
use crate::auth::extract_authenticated_user;
use crate::{
    AppConfig, CreatePublishedStatusInput, D1Database, Env, Request, Response, Result,
    RouteContext, create_published_status_and_response, find_account_by_id, load_config,
    now_iso_string, resolve_attachable_media, resolve_in_reply_to_account_id,
    subtract_seconds_from_iso_string, validate_local_quote_creation,
};
use serde::Serialize;
use worker::console_error;

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
/// Claims older than this timestamp are considered abandoned.
pub(in crate::scheduled_statuses) fn scheduled_status_stale_claim_threshold(
    now_iso: &str,
) -> Result<String> {
    subtract_seconds_from_iso_string(now_iso, SCHEDULED_STATUS_CLAIM_TTL_SECS)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StatusDraft;
    use cfwdon_domain::Visibility;

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
}
