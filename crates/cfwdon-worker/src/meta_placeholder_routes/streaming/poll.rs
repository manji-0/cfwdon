use super::announcements::append_user_announcement_state_events;
use super::batches::{
    streaming_direct_batch, streaming_list_batch, streaming_notification_batch,
    streaming_public_batch,
};
use super::budget::{streaming_error_is_subrequest_limit, streaming_poll_budget_exhausted};
use super::status_deltas::streaming_status_delta_events;
use crate::{
    D1Database, Result, StreamingBatch, StreamingEvent, StreamingLoopState,
    load_latest_filter_updated_at, streaming_home_batch,
};
use worker::console_error;

pub(super) async fn yield_streaming_poll_round(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
    viewer: Option<&crate::LocalAccount>,
    state: &mut StreamingLoopState,
    poll_rounds: &mut u32,
) -> StreamingPollYield {
    if streaming_poll_budget_exhausted(*poll_rounds, *poll_rounds) {
        return StreamingPollYield::Recycle;
    }
    *poll_rounds = poll_rounds.saturating_add(1);
    let events =
        match poll_streaming_events(db, config, stream_name, tag, list, viewer, state).await {
            Ok(events) => events,
            Err(error) => {
                console_error!(
                    "streaming poll failed stream={} tag={} list={} error={}",
                    stream_name,
                    tag.unwrap_or_default(),
                    list.unwrap_or_default(),
                    error
                );
                if streaming_error_is_subrequest_limit(&error)
                    || streaming_poll_budget_exhausted(*poll_rounds, *poll_rounds)
                {
                    return StreamingPollYield::Recycle;
                }
                return StreamingPollYield::PollFailed;
            }
        };
    if streaming_poll_budget_exhausted(*poll_rounds, *poll_rounds) {
        return StreamingPollYield::Recycle;
    }
    StreamingPollYield::Events(events)
}

pub(super) enum StreamingPollYield {
    Events(Vec<StreamingEvent>),
    PollFailed,
    Recycle,
}

pub(super) fn streaming_filter_update_changed(previous: Option<&str>, current: &str) -> bool {
    previous.map(|value| value != current).unwrap_or(false)
}

pub(super) fn apply_streaming_batch_to_state(
    stream_name: &str,
    batch: StreamingBatch,
    is_initial_poll: bool,
    state: &mut StreamingLoopState,
) -> Vec<StreamingEvent> {
    if let Some(next_since_id) = batch.last_id {
        state.since_id = Some(next_since_id);
    }
    if stream_name == "user:notification"
        && let Some(next_min_created_at) = batch.last_created_at
    {
        state.notification_min_created_at = Some(next_min_created_at);
    }
    for status_id in batch.tracked_status_ids {
        if state.tracked_status_id_set.insert(status_id.clone()) {
            state.tracked_status_ids.push(status_id);
        }
    }
    while state.tracked_status_ids.len() > 200 {
        let removed = state.tracked_status_ids.remove(0);
        state.tracked_status_id_set.remove(&removed);
    }

    if is_initial_poll {
        for event in &batch.events {
            state.emitted_event_ids.insert(streaming_event_key(event));
        }
        Vec::new()
    } else {
        batch.events
    }
}

pub(super) fn streaming_event_key(event: &StreamingEvent) -> String {
    format!("{}:{}", event.event, event.id)
}

pub(super) async fn append_user_stream_state_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    append_user_filter_state_events(db, viewer, state, is_initial_poll, events).await?;
    append_user_announcement_state_events(config, db, viewer, state, is_initial_poll, events).await
}

pub(super) async fn append_user_filter_state_events(
    db: &D1Database,
    viewer: &crate::LocalAccount,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    let current_filter_updated_at = load_latest_filter_updated_at(db, viewer.id()).await?;
    if let Some(current_filter_updated_at) = current_filter_updated_at {
        let changed = streaming_filter_update_changed(
            state.last_filter_updated_at.as_deref(),
            &current_filter_updated_at,
        );
        if !is_initial_poll && changed {
            events.push(StreamingEvent {
                created_at: current_filter_updated_at.clone(),
                id: current_filter_updated_at.clone(),
                event: "filters_changed",
                data: "undefined".to_owned(),
            });
        }
        state.last_filter_updated_at = Some(current_filter_updated_at);
    }

    Ok(())
}

pub(super) async fn poll_streaming_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
    viewer: Option<&crate::LocalAccount>,
    state: &mut StreamingLoopState,
) -> Result<Vec<StreamingEvent>> {
    let is_initial_poll = !state.initialized;
    let batch =
        streaming_batch_for_stream(db, config, stream_name, tag, list, viewer, state).await?;
    let mut events = apply_streaming_batch_to_state(stream_name, batch, is_initial_poll, state);
    append_streaming_poll_side_effect_events(
        db,
        config,
        stream_name,
        viewer,
        state,
        is_initial_poll,
        &mut events,
    )
    .await?;
    state.initialized = true;
    retain_new_streaming_events(state, &mut events);
    Ok(events)
}

pub(super) async fn streaming_batch_for_stream(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
    viewer: Option<&crate::LocalAccount>,
    state: &StreamingLoopState,
) -> Result<StreamingBatch> {
    match stream_name {
        "user" => {
            let viewer = required_streaming_viewer(viewer, "user")?;
            streaming_home_batch(db, config, viewer, state.since_id.as_deref()).await
        }
        "user:notification" => {
            let viewer = required_streaming_viewer(viewer, "notification")?;
            streaming_notification_batch(
                db,
                config,
                viewer,
                state.since_id.as_deref(),
                state.notification_min_created_at.as_deref(),
            )
            .await
        }
        "list" => {
            let viewer = required_streaming_viewer(viewer, "list")?;
            let list_id = required_streaming_list_id(list)?;
            streaming_list_batch(db, config, viewer, list_id, state.since_id.as_deref()).await
        }
        "direct" => {
            let viewer = required_streaming_viewer(viewer, "direct")?;
            streaming_direct_batch(db, config, viewer, state.since_id.as_deref()).await
        }
        _ => {
            streaming_public_batch(
                db,
                config,
                viewer,
                stream_name,
                tag,
                state.since_id.as_deref(),
            )
            .await
        }
    }
}

pub(super) fn required_streaming_viewer<'a>(
    viewer: Option<&'a crate::LocalAccount>,
    stream_label: &str,
) -> Result<&'a crate::LocalAccount> {
    viewer.ok_or_else(|| {
        worker::Error::RustError(format!(
            "missing authenticated viewer for {stream_label} stream"
        ))
    })
}

pub(super) fn required_streaming_list_id(list: Option<&str>) -> Result<&str> {
    list.filter(|value| !value.trim().is_empty())
        .ok_or_else(|| worker::Error::RustError("missing list id for list stream".to_owned()))
}

pub(super) async fn append_streaming_poll_side_effect_events(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream_name: &str,
    viewer: Option<&crate::LocalAccount>,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    if !is_initial_poll && stream_name != "user:notification" {
        let delta_events = streaming_status_delta_events(
            db,
            config,
            viewer,
            &state.tracked_status_ids,
            &mut state.deleted_status_ids,
            &mut state.updated_status_ids,
        )
        .await?;
        events.extend(delta_events);
    }
    if stream_name == "user" {
        let viewer = required_streaming_viewer(viewer, "user")?;
        append_user_stream_state_events(db, config, viewer, state, is_initial_poll, events).await?;
    }

    Ok(())
}

pub(super) fn retain_new_streaming_events(
    state: &mut StreamingLoopState,
    events: &mut Vec<StreamingEvent>,
) {
    events.retain(|event| state.emitted_event_ids.insert(streaming_event_key(event)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_filter_update_changed_only_after_initial_state() {
        assert!(!streaming_filter_update_changed(
            None,
            "2026-05-01T00:00:00Z"
        ));
        assert!(!streaming_filter_update_changed(
            Some("2026-05-01T00:00:00Z"),
            "2026-05-01T00:00:00Z"
        ));
        assert!(streaming_filter_update_changed(
            Some("2026-05-01T00:00:00Z"),
            "2026-05-02T00:00:00Z"
        ));
    }

    #[test]
    fn required_streaming_list_id_rejects_missing_or_blank_values() {
        assert!(required_streaming_list_id(None).is_err());
        assert!(required_streaming_list_id(Some("   ")).is_err());
        assert_eq!(
            required_streaming_list_id(Some("list-1")).unwrap(),
            "list-1"
        );
    }

    #[test]
    fn streaming_event_key_combines_event_type_and_id() {
        let event = StreamingEvent {
            created_at: "2026-05-01T00:00:00Z".to_owned(),
            id: "status-1".to_owned(),
            event: "update",
            data: "{}".to_owned(),
        };

        assert_eq!(streaming_event_key(&event), "update:status-1");
    }
}
