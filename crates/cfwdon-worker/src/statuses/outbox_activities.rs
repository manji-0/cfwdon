use super::{
    AppConfig, D1Database, LocalAccount, Result, StatusRow, actor_url, build_activitypub_note,
};

pub(crate) async fn build_outbox_activities(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    statuses: &[StatusRow],
) -> Result<Vec<serde_json::Value>> {
    let mut items = Vec::with_capacity(statuses.len());

    for status in statuses {
        let note = build_activitypub_note(db, config, account, status, false).await?;
        let note_id = note
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let published = note
            .get("published")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::String(status.created_at.clone()));
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
            "actor": actor_url(config, &account.username),
            "published": published,
            "to": to,
            "cc": cc,
            "object": note,
        }));
    }

    Ok(items)
}
