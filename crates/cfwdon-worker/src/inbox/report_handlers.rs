//! Inbound ActivityPub `Flag`: a report another server files on local accounts.

use crate::activitypub::{collect_activitypub_uris, local_username_from_actor_uri};
use crate::auth::find_account_by_username;
use crate::federation::{RemoteActorProfile, remote_actor_domain_restrictions};
use crate::identity::remote_account_rest_id;
use crate::observability::log_federation_event;
use crate::reports::{
    InboundFlagReportInsert, insert_inbound_flag_report, notify_admins_of_report,
};
use crate::statuses::find_local_status_by_object_uri;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    FlaggedObject, INBOUND_FLAG_MAX_OBJECTS, inbound_flag_comment, inbound_flag_report_uri,
    inbound_flag_reports,
};
use worker::Result;

/// Files the reports a `Flag` makes on local accounts, filed by the sending
/// actor (an instance actor for an anonymous report). A server whose domain
/// block has `reject_reports` files nothing.
pub(crate) async fn handle_inbox_flag(
    db: &D1Database,
    config: &AppConfig,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let restrictions =
        remote_actor_domain_restrictions(&remote_actor.domain, &remote_actor.actor_uri).await?;
    if restrictions.reject_reports {
        log_federation_event(
            "inbox_flag_rejected",
            "skipped",
            format!(
                "inbox Flag dropped: domain block rejects reports from {}",
                remote_actor.domain
            ),
            serde_json::json!({
                "actor_uri": remote_actor.actor_uri,
                "domain": remote_actor.domain,
            }),
        );
        return Ok(());
    }

    let mut object_uris = Vec::new();
    collect_activitypub_uris(activity.get("object"), &mut object_uris);
    let mut objects = Vec::new();
    for uri in object_uris.iter().take(INBOUND_FLAG_MAX_OBJECTS) {
        if let Some(object) = resolve_flagged_object(db, config, uri).await? {
            objects.push(object);
        }
    }

    let reporter_account_id = remote_account_rest_id(&remote_actor.actor_uri);
    let uri = inbound_flag_report_uri(
        activity.get("id").and_then(serde_json::Value::as_str),
        &remote_actor.actor_uri,
    );
    let comment = inbound_flag_comment(activity.get("content").and_then(serde_json::Value::as_str));
    for report in &inbound_flag_reports(&objects) {
        let insert = InboundFlagReportInsert {
            reporter_account_id: &reporter_account_id,
            origin_domain: &remote_actor.domain,
            uri,
            comment: &comment,
            report,
        };
        if let Some(report_id) = insert_inbound_flag_report(db, &insert).await? {
            notify_admins_of_report(
                db,
                config,
                &report_id,
                &reporter_account_id,
                &report.target_account_id,
            )
            .await?;
        }
    }
    Ok(())
}

async fn resolve_flagged_object(
    db: &D1Database,
    config: &AppConfig,
    uri: &str,
) -> Result<Option<FlaggedObject>> {
    if let Some(username) = local_username_from_actor_uri(config, uri) {
        return Ok(find_account_by_username(db, &username)
            .await?
            .map(|account| FlaggedObject::LocalAccount {
                account_id: account.id().to_owned(),
            }));
    }
    Ok(find_local_status_by_object_uri(db, config, uri)
        .await?
        .map(|status| FlaggedObject::LocalStatus {
            status_id: status.id,
            account_id: status.account_id,
        }))
}
