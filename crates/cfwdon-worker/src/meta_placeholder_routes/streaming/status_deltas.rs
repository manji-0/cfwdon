use crate::{
    D1Database, Result, StreamingEvent, build_local_status_response, build_remote_status_response,
    find_account_by_id, find_media_attachments_by_status_id, find_remote_actor_by_actor_uri,
    find_remote_status_by_id, find_status_by_id, is_local_status_thread_muted_by, is_muted_actor,
    load_in_reply_to_account_id, load_remote_status_updated_at, load_status_updated_at,
    now_iso_string,
};
use std::collections::HashSet;

pub(super) async fn streaming_status_delta_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    tracked_status_ids: &[String],
    deleted_status_ids: &mut HashSet<String>,
    updated_status_ids: &mut HashSet<String>,
) -> Result<Vec<StreamingEvent>> {
    let mut events = Vec::new();

    for status_id in tracked_status_ids.iter().rev().take(200) {
        append_streaming_status_delta_event(
            db,
            config,
            viewer,
            status_id,
            deleted_status_ids,
            updated_status_ids,
            &mut events,
        )
        .await?;
    }

    Ok(events)
}

pub(super) async fn append_streaming_status_delta_event(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status_id: &str,
    deleted_status_ids: &mut HashSet<String>,
    updated_status_ids: &mut HashSet<String>,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    if streaming_status_delta_already_recorded(status_id, deleted_status_ids, updated_status_ids) {
        return Ok(());
    }

    if let Some(status) = find_status_by_id(db, status_id).await? {
        if let Some(event) =
            streaming_local_status_update_event(db, config, viewer, &status).await?
        {
            updated_status_ids.insert(status.id.clone());
            events.push(event);
        }
        return Ok(());
    }

    if let Some(status) = find_remote_status_by_id(db, status_id).await? {
        if let Some(event) =
            streaming_remote_status_update_event(db, config, viewer, &status).await?
        {
            updated_status_ids.insert(status.id.clone());
            events.push(event);
        }
        return Ok(());
    }

    deleted_status_ids.insert(status_id.to_owned());
    events.push(streaming_status_delete_event(status_id, now_iso_string()?));
    Ok(())
}

pub(super) fn streaming_status_delta_already_recorded(
    status_id: &str,
    deleted_status_ids: &HashSet<String>,
    updated_status_ids: &HashSet<String>,
) -> bool {
    deleted_status_ids.contains(status_id) || updated_status_ids.contains(status_id)
}

pub(super) async fn streaming_local_status_update_event(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: &crate::StatusRow,
) -> Result<Option<StreamingEvent>> {
    let Some(updated_at) = load_status_updated_at(db, &status.id).await? else {
        return Ok(None);
    };
    if updated_at == status.created_at {
        return Ok(None);
    }
    let Some(account) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(None);
    };
    if let Some(viewer) = viewer
        && is_local_status_thread_muted_by(db, viewer.id(), status).await?
    {
        return Ok(None);
    }
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    let payload = build_local_status_response(
        db,
        config,
        viewer,
        status,
        &account,
        load_in_reply_to_account_id(db, status).await?,
        media,
    )
    .await?;
    let data = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize streaming local status update payload: {error}"
        ))
    })?;

    Ok(Some(StreamingEvent {
        created_at: updated_at,
        id: status.id.clone(),
        event: "status.update",
        data,
    }))
}

pub(super) async fn streaming_remote_status_update_event(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: &crate::RemoteStatusRow,
) -> Result<Option<StreamingEvent>> {
    let Some(updated_at) = load_remote_status_updated_at(db, &status.id).await? else {
        return Ok(None);
    };
    if updated_at == status.published_at {
        return Ok(None);
    }
    if let Some(viewer) = viewer
        && is_muted_actor(db, viewer.id(), &status.actor_uri).await?
    {
        return Ok(None);
    }
    let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
        return Ok(None);
    };
    let payload = build_remote_status_response(db, config, viewer, status, &actor).await?;
    let data = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize streaming remote status update payload: {error}"
        ))
    })?;

    Ok(Some(StreamingEvent {
        created_at: updated_at,
        id: status.id.clone(),
        event: "status.update",
        data,
    }))
}

pub(super) fn streaming_status_delete_event(status_id: &str, created_at: String) -> StreamingEvent {
    StreamingEvent {
        created_at,
        id: status_id.to_owned(),
        event: "delete",
        data: status_id.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_status_delta_already_recorded_skips_deleted_or_updated_ids() {
        let deleted_status_ids = HashSet::from(["deleted-1".to_owned()]);
        let updated_status_ids = HashSet::from(["updated-1".to_owned()]);

        assert!(streaming_status_delta_already_recorded(
            "deleted-1",
            &deleted_status_ids,
            &updated_status_ids
        ));
        assert!(streaming_status_delta_already_recorded(
            "updated-1",
            &deleted_status_ids,
            &updated_status_ids
        ));
        assert!(!streaming_status_delta_already_recorded(
            "fresh-1",
            &deleted_status_ids,
            &updated_status_ids
        ));
    }

    #[test]
    fn streaming_status_delete_event_matches_mastodon_delete_shape() {
        let event = streaming_status_delete_event("status-1", "2026-05-01T00:00:00Z".to_owned());

        assert_eq!(event.created_at, "2026-05-01T00:00:00Z");
        assert_eq!(event.id, "status-1");
        assert_eq!(event.event, "delete");
        assert_eq!(event.data, "status-1");
    }
}
