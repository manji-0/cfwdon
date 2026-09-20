use super::bindings::remote_status_object_uri_bindings;
use super::intents::serialize_remote_status_snapshot_json;
use super::records::{RemoteStatusRecord, RemoteStatusRow, remote_status_from_record};
use crate::{
    AppConfig, D1Database, build_remote_status_response, find_remote_actor_by_actor_uri,
    insert_remote_status_edit_snapshot, normalize_status_history_entry,
};
use cfwdon_domain::StoredRemoteStatusIntent;
use serde::Deserialize;
use worker::Result;

#[derive(Debug, Deserialize)]
pub(super) struct RemoteStatusEditStateRow {
    #[serde(flatten)]
    record: RemoteStatusRecord,
    pub(super) raw_object_json: String,
}

impl RemoteStatusEditStateRow {
    pub(super) fn status_row(&self) -> Result<RemoteStatusRow> {
        remote_status_from_record(self.record.clone())
    }
}

pub(super) async fn find_remote_status_edit_state_by_object_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Option<RemoteStatusEditStateRow>> {
    let bindings = remote_status_object_uri_bindings(object_uri);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, boost_of_uri, quote_of_uri,
                content_html, text_content, spoiler_text, visibility, sensitive, language, quote_state, published_at,
                edited_at, card_json, federated_emojis_json, in_reply_to_id,
                COALESCE(rsc.favourites_count, 0) AS favourites_count,
                COALESCE(rsc.reblogs_count, 0) AS reblogs_count,
                raw_object_json
         FROM remote_statuses
         LEFT JOIN remote_status_counts rsc ON rsc.remote_status_id = remote_statuses.id
         WHERE object_uri = ?1
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<RemoteStatusEditStateRow>(None)
    .await
}

pub(super) async fn insert_previous_remote_status_snapshot(
    db: &D1Database,
    config: &AppConfig,
    previous: &RemoteStatusEditStateRow,
    revision_at: &str,
) -> Result<()> {
    let Some(actor) = find_remote_actor_by_actor_uri(db, &previous.record.actor_uri).await? else {
        return Ok(());
    };
    let response =
        build_remote_status_response(db, config, None, &previous.status_row()?, &actor).await?;
    let mut snapshot = serde_json::to_value(response).unwrap_or_else(|_| serde_json::json!({}));
    snapshot["created_at"] = serde_json::json!(revision_at);
    let snapshot = normalize_status_history_entry(snapshot);
    let snapshot_json = serialize_remote_status_snapshot_json(&snapshot)?;
    insert_remote_status_edit_snapshot(db, &previous.record.id, &snapshot_json, revision_at).await
}

pub(super) async fn insert_previous_remote_status_snapshot_if_changed(
    db: &D1Database,
    config: &AppConfig,
    previous: Option<&RemoteStatusEditStateRow>,
    intent: &StoredRemoteStatusIntent,
) -> Result<()> {
    if let Some(previous) =
        previous.filter(|existing| existing.raw_object_json != intent.raw_object_json)
    {
        insert_previous_remote_status_snapshot(db, config, previous, &intent.revision_at).await?;
    }

    Ok(())
}
