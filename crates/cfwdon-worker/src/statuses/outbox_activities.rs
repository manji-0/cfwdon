use super::LocalAccount;
use crate::activitypub::build_activitypub_note;
use crate::identity::actor_url;
use crate::time_html::activitypub_datetime_string;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalStatus;
use worker::Result;
pub(crate) async fn build_outbox_activities(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    statuses: &[LocalStatus],
) -> Result<Vec<serde_json::Value>> {
    let mut items = Vec::with_capacity(statuses.len());

    for status in statuses {
        let note = build_activitypub_note(db, config, account, status, false, None).await?;
        let note_id = note
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let published = note.get("published").cloned().unwrap_or_else(|| {
            serde_json::Value::String(activitypub_datetime_string(&status.created_at))
        });
        let to = note
            .get("to")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let cc = note
            .get("cc")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));

        items.push(serde_json::json!({
            "type": "Create",
            "id": format!("{note_id}/activity"),
            "actor": actor_url(config, account.username()),
            "published": published,
            "to": to,
            "cc": cc,
            "object": note,
        }));
    }

    Ok(items)
}
