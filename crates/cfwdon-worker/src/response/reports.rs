use crate::{
    AccountReference, AppConfig, D1Database, MastodonAccountResponse, MastodonReportResponse,
    ReportRow, list_report_status_ids, load_account_stats, resolve_account_reference,
    timestamp_to_mastodon_iso8601,
};
use worker::{Error, Result};

pub(crate) async fn build_report_response(
    db: &D1Database,
    config: &AppConfig,
    report: &ReportRow,
) -> Result<MastodonReportResponse> {
    let target_account = match resolve_account_reference(db, &report.target_account_id).await? {
        Some(AccountReference::Local(account)) => {
            let stats = load_account_stats(db, account.id()).await?;
            MastodonAccountResponse::from_account_with_stats(&account, config, &stats)
        }
        Some(AccountReference::Remote(actor)) => MastodonAccountResponse::from_remote_actor(&actor),
        None => {
            return Err(Error::RustError(
                "reported account could not be resolved".to_owned(),
            ));
        }
    };

    Ok(MastodonReportResponse {
        id: report.id.clone(),
        action_taken: report.action_taken != 0,
        action_taken_at: report
            .action_taken_at
            .as_deref()
            .map(timestamp_to_mastodon_iso8601),
        category: report.category.clone(),
        comment: report.comment.clone(),
        forwarded: report.forward != 0,
        created_at: timestamp_to_mastodon_iso8601(&report.created_at),
        status_ids: {
            let status_ids = list_report_status_ids(db, &report.id).await?;
            if status_ids.is_empty() {
                None
            } else {
                Some(status_ids)
            }
        },
        collection_ids: Some(Vec::new()),
        target_account,
        rule_ids: None,
    })
}
