use super::invalid_access_token_response;
use crate::StreamHubUpgradeParams;
use crate::timelines::{
    TimelinePaginationQuery, matches_tag_timeline_filters, resolve_timeline_cursor,
    timeline_fetch_limit,
};
use crate::{
    D1Database, LocalApiAuthentication, NotificationsQuery, Request, Response, Result,
    RouteContext, StreamingBatch, StreamingEntry, StreamingEvent, StreamingLoopState,
    StreamingPublicPlan, actor_url, authenticate_local_api_request, build_announcements_document,
    build_local_status_response, build_remote_status_response, collect_visible_notifications,
    connect_stream_hub_websocket, extract_hashtags_from_html, extract_hashtags_from_text,
    filter_notification_entries_by_query, find_account_by_id, find_conversation_for_account,
    find_conversation_id_by_status_id, find_media_attachments_by_status_id,
    find_oauth_access_token_with_account_by_bearer_token, find_remote_actor_by_actor_uri,
    find_remote_status_by_id, find_status_by_id, is_local_status_thread_muted_by, is_muted_actor,
    list_announcement_read_ids, list_local_direct_timeline_statuses,
    list_local_public_statuses_by_tag, list_local_public_timeline_statuses, list_membership_refs,
    list_membership_variants_for_local_account, list_membership_variants_for_remote_actor,
    list_remote_public_statuses_by_tag, list_remote_public_timeline_statuses, list_row_by_id,
    load_announcement_reaction_state, load_config, load_in_reply_to_account_id,
    load_latest_filter_updated_at, load_remote_status_updated_at, load_status_updated_at,
    now_iso_string, oauth_access_token_has_any_scope, remote_status_has_media,
    snapshot_d1_request_metrics, stream_hub_channel_id_name, stream_hub_session_id_name,
    streaming_batch_from_entries, streaming_home_batch, upgrade_stream_hub_websocket,
};
use async_stream::try_stream;
use futures_util::{FutureExt, StreamExt, pin_mut, select};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use worker::{
    Env, ResponseBody, WebSocket, WebSocketPair, console_error, console_log,
    ws_events::WebsocketEvent,
};

const STREAMING_POLL_INTERVAL_SECS: u64 = 3;
const STREAMING_HUB_BACKUP_POLL_INTERVAL_SECS: u64 = 30;
const STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION: u32 = 90;
const STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION: u32 = 200;
/// Cloudflare's per-invocation subrequest cap is 1000; recycle before D1 polling exhausts it.
const STREAMING_MAX_D1_SUBREQUESTS_PER_INVOCATION: u32 = 900;

#[derive(Debug, Default, Deserialize)]
struct StreamingQuery {
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamingWebSocketClientMessage {
    #[serde(rename = "type")]
    message_type: String,
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamingChannelValidationError {
    UnknownChannelRequested,
    MissingTag,
    MissingList,
}

pub(crate) fn normalize_streaming_channel(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
}

fn streaming_channel_requires_tag(stream: &str) -> bool {
    matches!(stream, "hashtag" | "hashtag:local")
}

fn streaming_channel_requires_list(stream: &str) -> bool {
    stream == "list"
}

fn normalize_streaming_path_channel(value: &str) -> Option<String> {
    let path = value.trim().trim_matches('/');
    if path.is_empty() {
        return None;
    }
    normalize_streaming_channel(Some(&path.replace('/', ":")))
}

pub(crate) fn streaming_channel_requires_auth(stream: &str) -> bool {
    matches!(stream, "user" | "user:notification" | "list" | "direct")
}

pub(crate) fn validate_streaming_channel_request(
    stream: Option<&str>,
    tag: Option<&str>,
    list: Option<&str>,
    extra_path: Option<&str>,
) -> std::result::Result<String, StreamingChannelValidationError> {
    let stream = match extra_path.map(str::trim).filter(|value| !value.is_empty()) {
        Some(_)
            if stream
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_some() =>
        {
            return Err(StreamingChannelValidationError::UnknownChannelRequested);
        }
        Some(path) => normalize_streaming_path_channel(path),
        None => normalize_streaming_channel(stream),
    };
    let Some(stream) = stream else {
        return Err(StreamingChannelValidationError::UnknownChannelRequested);
    };
    if !matches!(
        stream.as_str(),
        "public"
            | "public:media"
            | "public:local"
            | "public:local:media"
            | "public:remote"
            | "public:remote:media"
            | "hashtag"
            | "hashtag:local"
            | "user"
            | "user:notification"
            | "list"
            | "direct"
    ) {
        return Err(StreamingChannelValidationError::UnknownChannelRequested);
    }
    if streaming_channel_requires_tag(&stream)
        && tag
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(StreamingChannelValidationError::MissingTag);
    }
    if streaming_channel_requires_list(&stream)
        && list
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        return Err(StreamingChannelValidationError::MissingList);
    }
    Ok(stream)
}

fn streaming_bad_request_response(error: StreamingChannelValidationError) -> Result<Response> {
    let message = match error {
        StreamingChannelValidationError::UnknownChannelRequested => "Unknown channel requested",
        StreamingChannelValidationError::MissingTag => "Missing tag parameter",
        StreamingChannelValidationError::MissingList => "Missing list parameter",
    };
    Ok(Response::from_json(&serde_json::json!({
        "error": message,
    }))?
    .with_status(400))
}

fn websocket_protocol_access_token(req: &Request) -> Result<Option<String>> {
    let Some(protocols) = req.headers().get("Sec-WebSocket-Protocol")? else {
        return Ok(None);
    };

    Ok(protocols
        .split(',')
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned))
}

struct StreamingWebSocketSubscription {
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
    state: StreamingLoopState,
}

impl StreamingWebSocketSubscription {
    fn new(stream_name: String, tag: Option<String>, list: Option<String>) -> Self {
        Self {
            stream_name,
            tag,
            list,
            state: StreamingLoopState::new(),
        }
    }
}

fn sse_comment_bytes(value: &str) -> Vec<u8> {
    format!(": {value}\n\n").into_bytes()
}

fn sse_event_bytes(event: &StreamingEvent) -> Vec<u8> {
    format!("event: {}\ndata: {}\n\n", event.event, event.data).into_bytes()
}

fn sse_named_event_bytes(event: &str, data: &str) -> Vec<u8> {
    format!("event: {event}\ndata: {data}\n\n").into_bytes()
}

fn streaming_event_identity_from_payload(event_name: &str, data: &str) -> (String, String) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
        let id = value
            .get("id")
            .and_then(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| value.as_i64().map(|id| id.to_string()))
            })
            .unwrap_or_else(|| format!("{event_name}:{data}"));
        let created_at = value
            .get("created_at")
            .and_then(|value| value.as_str())
            .unwrap_or("1970-01-01T00:00:00Z")
            .to_owned();
        return (id, created_at);
    }

    (
        format!("{event_name}:{}", data.chars().take(64).collect::<String>()),
        "1970-01-01T00:00:00Z".to_owned(),
    )
}

fn stream_hub_websocket_text_to_sse_bytes(
    text: &str,
    state: &mut StreamingLoopState,
) -> Option<Vec<u8>> {
    let value = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) => value,
        Err(_) => return None,
    };
    if value.get("error").is_some() {
        return None;
    }
    let event_name = value
        .get("event")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if event_name.is_empty() {
        return None;
    }
    // `filters_changed` carries no payload to dedupe on, and the D1 catch-up
    // cursor only advances every backup poll, so keying it on that cursor would
    // drop every change after the first. Pass it through unconditionally.
    if event_name == "filters_changed" {
        return Some(sse_named_event_bytes(event_name, "undefined"));
    }

    let data = value
        .get("payload")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned();
    let (id, _created_at) = streaming_event_identity_from_payload(event_name, &data);
    let dedupe_key = format!("{event_name}:{id}");
    if !state.emitted_event_ids.insert(dedupe_key) {
        return None;
    }
    Some(sse_named_event_bytes(event_name, &data))
}

async fn yield_streaming_poll_round(
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

enum StreamingPollYield {
    Events(Vec<StreamingEvent>),
    PollFailed,
    Recycle,
}

fn streaming_websocket_subscription_key(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        stream_name,
        tag.unwrap_or_default(),
        list.unwrap_or_default()
    )
}

fn streaming_websocket_stream_labels(
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) -> Vec<String> {
    let mut labels = vec![stream_name.to_owned()];
    if stream_name.starts_with("hashtag")
        && let Some(tag) = tag
    {
        labels.push(tag.to_owned());
    }
    if stream_name == "list"
        && let Some(list) = list
    {
        labels.push(list.to_owned());
    }
    labels
}

fn streaming_websocket_event_message(
    subscription: &StreamingWebSocketSubscription,
    event: &StreamingEvent,
) -> Result<String> {
    let mut payload = serde_json::json!({
        "stream": streaming_websocket_stream_labels(
            &subscription.stream_name,
            subscription.tag.as_deref(),
            subscription.list.as_deref(),
        ),
        "event": event.event,
    });
    if event.event != "filters_changed" {
        payload["payload"] = serde_json::Value::String(event.data.clone());
    }
    serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize websocket stream event: {error}"
        ))
    })
}

fn streaming_websocket_error_message(message: &str, status: u16) -> String {
    serde_json::json!({
        "error": message,
        "status": status,
    })
    .to_string()
}

fn streaming_poll_budget_exhausted(poll_rounds: u32, subscription_polls: u32) -> bool {
    poll_rounds >= STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION
        || subscription_polls >= STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION
        || snapshot_d1_request_metrics().query_count >= STREAMING_MAX_D1_SUBREQUESTS_PER_INVOCATION
}

fn streaming_error_is_subrequest_limit(error: &worker::Error) -> bool {
    error
        .to_string()
        .contains("Too many API requests by single Worker invocation")
}

fn announcement_reaction_entries_for_id(
    state: &HashMap<(String, String), (u64, bool)>,
    announcement_id: &str,
) -> BTreeMap<String, (u64, bool)> {
    state
        .iter()
        .filter(|((id, _), _)| id == announcement_id)
        .map(|((_, name), value)| (name.clone(), *value))
        .collect()
}

fn streaming_filter_update_changed(previous: Option<&str>, current: &str) -> bool {
    previous.map(|value| value != current).unwrap_or(false)
}

struct AnnouncementStreamEntry {
    id: String,
    payload: String,
    created_at: String,
}

struct CurrentAnnouncementStreamState {
    entries: Vec<AnnouncementStreamEntry>,
    reactions: HashMap<(String, String), (u64, bool)>,
}

fn announcement_stream_entries(
    announcements: Vec<serde_json::Value>,
) -> Result<Vec<AnnouncementStreamEntry>> {
    let mut entries = Vec::new();
    for announcement in announcements {
        if let Some(entry) = announcement_stream_entry(&announcement)? {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn announcement_stream_entry(
    announcement: &serde_json::Value,
) -> Result<Option<AnnouncementStreamEntry>> {
    let Some(id) = announcement
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return Ok(None);
    };
    let payload = serde_json::to_string(announcement).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to serialize announcement stream payload: {error}"
        ))
    })?;
    let created_at = announcement
        .get("published_at")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            announcement
                .get("updated_at")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or_default()
        .to_owned();

    Ok(Some(AnnouncementStreamEntry {
        id,
        payload,
        created_at,
    }))
}

async fn streaming_notification_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    since_id: Option<&str>,
    min_created_at: Option<&str>,
) -> Result<StreamingBatch> {
    let query = NotificationsQuery {
        since_id: since_id.map(str::to_owned),
        min_created_at: min_created_at.map(str::to_owned),
        limit: Some(40),
        ..NotificationsQuery::default()
    };
    let entries = collect_visible_notifications(db, config, viewer, &query, 160).await?;
    let filtered = filter_notification_entries_by_query(entries, &query);
    let last_id = filtered.first().map(|entry| entry.id.clone());
    let last_created_at = filtered.first().map(|entry| entry.created_at.clone());
    let mut events = Vec::with_capacity(filtered.len());

    for entry in filtered.into_iter().rev() {
        events.push(StreamingEvent {
            created_at: entry.created_at,
            id: entry.id,
            event: "notification",
            data: serde_json::to_string(&entry.value).map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize notification stream payload: {error}"
                ))
            })?,
        });
    }

    Ok(StreamingBatch {
        events,
        tracked_status_ids: Vec::new(),
        last_id,
        last_created_at,
    })
}

async fn append_streaming_local_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: crate::StatusRow,
    only_media: bool,
    mute_local_actor: bool,
    tag_filter: Option<&str>,
    include_reply_context: bool,
    payload_context: &str,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if let Some(tag) = tag_filter {
        let status_tags = extract_hashtags_from_text(&status.text);
        if !matches_tag_timeline_filters(&status_tags, tag, &crate::TagTimelineQuery::default()) {
            return Ok(());
        }
    }
    let Some(account) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(());
    };
    if mute_local_actor
        && let Some(viewer) = viewer
        && is_muted_actor(db, viewer.id(), &actor_url(config, account.username())).await?
    {
        return Ok(());
    }
    if let Some(viewer) = viewer
        && is_local_status_thread_muted_by(db, viewer.id(), &status).await?
    {
        return Ok(());
    }
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    if only_media && media.is_empty() {
        return Ok(());
    }
    let in_reply_to_account_id = if include_reply_context {
        load_in_reply_to_account_id(db, &status).await?
    } else {
        None
    };
    entries.push(StreamingEntry::new(
        status.created_at.clone(),
        status.id.clone(),
        serde_json::to_string(
            &build_local_status_response(
                db,
                config,
                viewer,
                &status,
                &account,
                in_reply_to_account_id,
                media,
            )
            .await?,
        )
        .map_err(|error| {
            worker::Error::RustError(format!(
                "failed to serialize {payload_context} stream payload: {error}"
            ))
        })?,
    ));
    tracked_status_ids.push(status.id);
    Ok(())
}

async fn append_streaming_remote_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    status: crate::RemoteStatusRow,
    actor: crate::RemoteActorRow,
    only_media: bool,
    tag_filter: Option<&str>,
    payload_context: &str,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if let Some(tag) = tag_filter {
        let status_tags = extract_hashtags_from_html(&status.content_html);
        if !matches_tag_timeline_filters(&status_tags, tag, &crate::TagTimelineQuery::default()) {
            return Ok(());
        }
    }
    if only_media && !remote_status_has_media(db, &status.id).await? {
        return Ok(());
    }
    if let Some(viewer) = viewer
        && is_muted_actor(db, viewer.id(), &actor.actor_uri).await?
    {
        return Ok(());
    }
    entries.push(StreamingEntry::new(
        status.published_at.clone(),
        status.id.clone(),
        serde_json::to_string(
            &build_remote_status_response(db, config, viewer, &status, &actor).await?,
        )
        .map_err(|error| {
            worker::Error::RustError(format!(
                "failed to serialize {payload_context} stream payload: {error}"
            ))
        })?,
    ));
    tracked_status_ids.push(status.id);
    Ok(())
}

async fn streaming_public_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    stream: &str,
    tag: Option<&str>,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let plan = StreamingPublicPlan::from_stream(stream);
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);
    let mut entries = Vec::new();
    let mut tracked_status_ids = Vec::new();

    if plan.hashtag_stream {
        let Some(tag) = tag else {
            return Ok(StreamingBatch::empty());
        };
        append_streaming_hashtag_status_entries(
            db,
            config,
            viewer,
            plan,
            tag,
            &cursor,
            query_limit,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    } else {
        append_streaming_public_status_entries(
            db,
            config,
            viewer,
            plan,
            &cursor,
            query_limit,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    Ok(streaming_batch_from_entries(
        entries,
        tracked_status_ids,
        "conversation",
    ))
}

async fn append_streaming_hashtag_status_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    plan: StreamingPublicPlan,
    tag: &str,
    cursor: &crate::ResolvedTimelineCursor,
    query_limit: u32,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if plan.include_local {
        for status in list_local_public_statuses_by_tag(db, tag, cursor, query_limit).await? {
            append_streaming_local_status_entry(
                db,
                config,
                viewer,
                status,
                plan.only_media,
                false,
                Some(tag),
                true,
                "hashtag",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    if plan.include_remote {
        for (status, actor) in
            list_remote_public_statuses_by_tag(db, tag, cursor, query_limit).await?
        {
            append_streaming_remote_status_entry(
                db,
                config,
                viewer,
                status,
                actor,
                plan.only_media,
                Some(tag),
                "hashtag",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    Ok(())
}

async fn append_streaming_public_status_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    plan: StreamingPublicPlan,
    cursor: &crate::ResolvedTimelineCursor,
    query_limit: u32,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if plan.include_local {
        for status in list_local_public_timeline_statuses(db, cursor, query_limit).await? {
            append_streaming_local_status_entry(
                db,
                config,
                viewer,
                status,
                plan.only_media,
                false,
                None,
                false,
                "public",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    if plan.include_remote {
        for (status, actor) in list_remote_public_timeline_statuses(db, cursor, query_limit).await?
        {
            append_streaming_remote_status_entry(
                db,
                config,
                viewer,
                status,
                actor,
                plan.only_media,
                None,
                "public",
                entries,
                tracked_status_ids,
            )
            .await?;
        }
    }
    Ok(())
}

async fn streaming_direct_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);
    let mut entries = Vec::new();
    let mut tracked_conversation_ids = Vec::new();
    let mut seen_conversation_ids = HashSet::new();

    for status in list_local_direct_timeline_statuses(db, viewer.id(), &cursor, query_limit).await?
    {
        let Some(conversation_id) = find_conversation_id_by_status_id(db, &status.id).await? else {
            continue;
        };
        if !seen_conversation_ids.insert(conversation_id.clone()) {
            continue;
        }
        let Some(conversation) =
            find_conversation_for_account(db, viewer.id(), &conversation_id).await?
        else {
            continue;
        };
        entries.push(StreamingEntry::new(
            status.created_at.clone(),
            conversation.id.clone(),
            serde_json::to_string(
                &crate::conversation_document(db, config, viewer, &conversation).await?,
            )
            .map_err(|error| {
                worker::Error::RustError(format!(
                    "failed to serialize direct stream payload: {error}"
                ))
            })?,
        ));
        tracked_conversation_ids.push(conversation.id.clone());
    }

    Ok(streaming_batch_from_entries(
        entries,
        tracked_conversation_ids,
        "update",
    ))
}

async fn streaming_list_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    list_id: &str,
    since_id: Option<&str>,
) -> Result<StreamingBatch> {
    let Some(context) = streaming_list_batch_context(db, viewer, list_id, since_id).await? else {
        return Ok(StreamingBatch::empty());
    };
    let StreamingListBatchContext {
        cursor,
        query_limit,
        membership_refs,
        replies_policy,
    } = context;
    let mut entries = Vec::new();
    let mut tracked_status_ids = Vec::new();
    let policy = ListStreamStatusPolicy::new(&membership_refs, &replies_policy);

    for status in list_local_public_timeline_statuses(db, &cursor, query_limit).await? {
        append_streaming_list_local_status_entry(
            db,
            config,
            viewer,
            &policy,
            status,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    for (status, actor) in list_remote_public_timeline_statuses(db, &cursor, query_limit).await? {
        append_streaming_list_remote_status_entry(
            db,
            config,
            viewer,
            &policy,
            status,
            actor,
            &mut entries,
            &mut tracked_status_ids,
        )
        .await?;
    }

    Ok(streaming_batch_from_entries(
        entries,
        tracked_status_ids,
        "update",
    ))
}

struct StreamingListBatchContext {
    cursor: crate::ResolvedTimelineCursor,
    query_limit: u32,
    membership_refs: HashSet<String>,
    replies_policy: String,
}

async fn streaming_list_batch_context(
    db: &D1Database,
    viewer: &crate::LocalAccount,
    list_id: &str,
    since_id: Option<&str>,
) -> Result<Option<StreamingListBatchContext>> {
    let cursor = resolve_timeline_cursor(
        db,
        &TimelinePaginationQuery {
            since_id: since_id.map(str::to_owned),
            limit: Some(40),
            ..TimelinePaginationQuery::default()
        },
    )
    .await?;
    let query_limit = timeline_fetch_limit(40);
    let Some(list) = list_row_by_id(db, viewer.id(), list_id).await? else {
        return Ok(None);
    };
    let membership_refs = list_membership_refs(db, list_id)
        .await?
        .into_iter()
        .map(|row| row.target_account_ref)
        .collect::<HashSet<_>>();
    Ok(Some(StreamingListBatchContext {
        cursor,
        query_limit,
        membership_refs,
        replies_policy: list.replies_policy,
    }))
}

async fn append_streaming_list_local_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    policy: &ListStreamStatusPolicy<'_>,
    status: crate::StatusRow,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    let Some(author) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(());
    };
    if !policy.matches(
        list_membership_variants_for_local_account(&author, config),
        status.in_reply_to_id.as_deref(),
    ) {
        return Ok(());
    }
    if is_local_status_thread_muted_by(db, viewer.id(), &status).await? {
        return Ok(());
    }
    let media = find_media_attachments_by_status_id(db, &status.id).await?;
    entries.push(StreamingEntry::new(
        status.created_at.clone(),
        status.id.clone(),
        serde_json::to_string(
            &build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &author,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?,
        )
        .map_err(|error| {
            worker::Error::RustError(format!("failed to serialize list stream payload: {error}"))
        })?,
    ));
    tracked_status_ids.push(status.id.clone());
    Ok(())
}

async fn append_streaming_list_remote_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &crate::LocalAccount,
    policy: &ListStreamStatusPolicy<'_>,
    status: crate::RemoteStatusRow,
    actor: crate::RemoteActorRow,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if !policy.matches(
        list_membership_variants_for_remote_actor(&actor),
        status.in_reply_to_uri.as_deref(),
    ) {
        return Ok(());
    }
    if is_muted_actor(db, viewer.id(), &actor.actor_uri).await? {
        return Ok(());
    }
    entries.push(StreamingEntry::new(
        status.published_at.clone(),
        status.id.clone(),
        serde_json::to_string(
            &build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
        )
        .map_err(|error| {
            worker::Error::RustError(format!("failed to serialize list stream payload: {error}"))
        })?,
    ));
    tracked_status_ids.push(status.id.clone());
    Ok(())
}

struct ListStreamStatusPolicy<'a> {
    membership_refs: &'a HashSet<String>,
    replies_policy: &'a str,
}

impl<'a> ListStreamStatusPolicy<'a> {
    fn new(membership_refs: &'a HashSet<String>, replies_policy: &'a str) -> Self {
        Self {
            membership_refs,
            replies_policy,
        }
    }

    fn matches(
        &self,
        candidates: impl IntoIterator<Item = String>,
        reply_reference: Option<&str>,
    ) -> bool {
        list_stream_membership_refs_include_any(self.membership_refs, candidates)
            && !list_stream_excludes_reply(self.replies_policy, reply_reference)
    }
}

fn list_stream_membership_refs_include_any(
    membership_refs: &HashSet<String>,
    candidates: impl IntoIterator<Item = String>,
) -> bool {
    candidates
        .into_iter()
        .any(|candidate| membership_refs.contains(&candidate))
}

fn list_stream_excludes_reply(replies_policy: &str, reply_reference: Option<&str>) -> bool {
    replies_policy == "none" && reply_reference.is_some()
}

async fn streaming_status_delta_events(
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

async fn append_streaming_status_delta_event(
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

fn streaming_status_delta_already_recorded(
    status_id: &str,
    deleted_status_ids: &HashSet<String>,
    updated_status_ids: &HashSet<String>,
) -> bool {
    deleted_status_ids.contains(status_id) || updated_status_ids.contains(status_id)
}

async fn streaming_local_status_update_event(
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

async fn streaming_remote_status_update_event(
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

fn streaming_status_delete_event(status_id: &str, created_at: String) -> StreamingEvent {
    StreamingEvent {
        created_at,
        id: status_id.to_owned(),
        event: "delete",
        data: status_id.to_owned(),
    }
}

fn apply_streaming_batch_to_state(
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

fn streaming_event_key(event: &StreamingEvent) -> String {
    format!("{}:{}", event.event, event.id)
}

async fn append_user_stream_state_events(
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

async fn append_user_filter_state_events(
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

async fn append_user_announcement_state_events(
    config: &cfwdon_core::AppConfig,
    db: &D1Database,
    viewer: &crate::LocalAccount,
    state: &mut StreamingLoopState,
    is_initial_poll: bool,
    events: &mut Vec<StreamingEvent>,
) -> Result<()> {
    let current_state = load_current_announcement_stream_state(config, db, viewer).await?;
    let mut current_announcements = HashMap::<String, String>::new();

    for entry in current_state.entries {
        append_current_announcement_stream_entry_events(
            &entry,
            is_initial_poll,
            &state.last_announcements,
            &state.last_announcement_reactions,
            &current_state.reactions,
            events,
        );
        current_announcements.insert(entry.id, entry.payload);
    }

    if !is_initial_poll {
        for removed_id in
            removed_announcement_ids(&state.last_announcements, &current_announcements)
        {
            events.push(announcement_delete_event(removed_id, now_iso_string()?));
        }
    }
    state.last_announcement_reactions = current_state.reactions;
    state.last_announcements = current_announcements;
    Ok(())
}

async fn load_current_announcement_stream_state(
    config: &cfwdon_core::AppConfig,
    db: &D1Database,
    viewer: &crate::LocalAccount,
) -> Result<CurrentAnnouncementStreamState> {
    let read_ids = list_announcement_read_ids(db, viewer.id()).await?;
    let reactions = load_announcement_reaction_state(db, viewer.id()).await?;
    let announcements = build_announcements_document(config, &read_ids, &reactions);

    Ok(CurrentAnnouncementStreamState {
        entries: announcement_stream_entries(announcements)?,
        reactions,
    })
}

fn append_current_announcement_stream_entry_events(
    entry: &AnnouncementStreamEntry,
    is_initial_poll: bool,
    previous_announcements: &HashMap<String, String>,
    previous_reactions_state: &HashMap<(String, String), (u64, bool)>,
    current_reactions_state: &HashMap<(String, String), (u64, bool)>,
    events: &mut Vec<StreamingEvent>,
) {
    let current_reactions =
        announcement_reaction_entries_for_id(current_reactions_state, &entry.id);
    let previous_reactions =
        announcement_reaction_entries_for_id(previous_reactions_state, &entry.id);
    match announcement_stream_entry_action(
        is_initial_poll,
        previous_announcements.get(&entry.id).map(String::as_str),
        &entry.payload,
        &current_reactions,
        &previous_reactions,
    ) {
        AnnouncementStreamEntryAction::Reaction => {
            append_announcement_reaction_events(
                entry,
                &current_reactions,
                previous_reactions_state,
                events,
            );
        }
        AnnouncementStreamEntryAction::Announcement => {
            events.push(announcement_stream_event(entry));
        }
        AnnouncementStreamEntryAction::None => {}
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AnnouncementStreamEntryAction {
    None,
    Reaction,
    Announcement,
}

fn announcement_stream_entry_action(
    is_initial_poll: bool,
    previous_payload: Option<&str>,
    current_payload: &str,
    current_reactions: &BTreeMap<String, (u64, bool)>,
    previous_reactions: &BTreeMap<String, (u64, bool)>,
) -> AnnouncementStreamEntryAction {
    if is_initial_poll {
        return AnnouncementStreamEntryAction::None;
    }
    if current_reactions != previous_reactions {
        return AnnouncementStreamEntryAction::Reaction;
    }
    if previous_payload != Some(current_payload) {
        return AnnouncementStreamEntryAction::Announcement;
    }
    AnnouncementStreamEntryAction::None
}

fn announcement_stream_event(entry: &AnnouncementStreamEntry) -> StreamingEvent {
    StreamingEvent {
        created_at: entry.created_at.clone(),
        id: entry.id.clone(),
        event: "announcement",
        data: entry.payload.clone(),
    }
}

fn removed_announcement_ids(
    previous_announcements: &HashMap<String, String>,
    current_announcements: &HashMap<String, String>,
) -> Vec<String> {
    previous_announcements
        .keys()
        .filter(|id| !current_announcements.contains_key(*id))
        .cloned()
        .collect()
}

fn announcement_delete_event(removed_id: String, created_at: String) -> StreamingEvent {
    StreamingEvent {
        created_at,
        id: removed_id.clone(),
        event: "announcement.delete",
        data: removed_id,
    }
}

fn append_announcement_reaction_events(
    entry: &AnnouncementStreamEntry,
    current_reactions: &BTreeMap<String, (u64, bool)>,
    last_announcement_reactions: &HashMap<(String, String), (u64, bool)>,
    events: &mut Vec<StreamingEvent>,
) {
    for (name, (count, me)) in current_reactions {
        let previous = last_announcement_reactions
            .get(&(entry.id.clone(), name.clone()))
            .copied();
        if previous != Some((*count, *me)) {
            events.push(StreamingEvent {
                created_at: entry.created_at.clone(),
                id: format!("{}:{name}", entry.id),
                event: "announcement.reaction",
                data: serde_json::json!({
                    "name": name,
                    "count": count,
                    "announcement_id": entry.id,
                })
                .to_string(),
            });
        }
    }
}

async fn poll_streaming_events(
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

async fn streaming_batch_for_stream(
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

fn required_streaming_viewer<'a>(
    viewer: Option<&'a crate::LocalAccount>,
    stream_label: &str,
) -> Result<&'a crate::LocalAccount> {
    viewer.ok_or_else(|| {
        worker::Error::RustError(format!(
            "missing authenticated viewer for {stream_label} stream"
        ))
    })
}

fn required_streaming_list_id(list: Option<&str>) -> Result<&str> {
    list.filter(|value| !value.trim().is_empty())
        .ok_or_else(|| worker::Error::RustError("missing list id for list stream".to_owned()))
}

async fn append_streaming_poll_side_effect_events(
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

fn retain_new_streaming_events(state: &mut StreamingLoopState, events: &mut Vec<StreamingEvent>) {
    events.retain(|event| state.emitted_event_ids.insert(streaming_event_key(event)));
}

fn build_streaming_event_stream(
    env: Option<Env>,
    db: D1Database,
    config: cfwdon_core::AppConfig,
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
    viewer: Option<crate::LocalAccount>,
    hub_target: Option<(String, Option<String>)>,
) -> impl futures_util::TryStream<
    Ok = Vec<u8>,
    Error = worker::Error,
    Item = std::result::Result<Vec<u8>, worker::Error>,
> + 'static {
    try_stream! {
        yield sse_comment_bytes(&format!("stream={stream_name}"));
        let mut state = StreamingLoopState::new();
        let mut poll_rounds = 0_u32;
        let hub_websocket = if let (Some(env), Some((hub_name, account_id))) =
            (env.as_ref(), hub_target.as_ref())
        {
            yield sse_comment_bytes("source=stream-hub");
            match connect_stream_hub_websocket(
                env,
                &config.stream_hub_binding,
                hub_name,
                &stream_name,
                tag.as_deref(),
                list.as_deref(),
                account_id.as_deref(),
            )
            .await
            {
                Ok(websocket) => {
                    if let Err(error) = poll_streaming_events(
                        &db,
                        &config,
                        &stream_name,
                        tag.as_deref(),
                        list.as_deref(),
                        viewer.as_ref(),
                        &mut state,
                    )
                    .await
                    {
                        console_log!(
                            "stream hub sse initial sync failed stream={} error={}",
                            stream_name,
                            error
                        );
                    }
                    Some(websocket)
                }
                Err(error) => {
                    console_log!(
                        "stream hub sse connect failed for hub {}: {:?}; falling back to d1 poll",
                        hub_name,
                        error
                    );
                    None
                }
            }
        } else {
            None
        };

        let mut hub_events = hub_websocket
            .as_ref()
            .and_then(|websocket| websocket.events().ok());
        while hub_events.is_some() {
            let backup_tick =
                worker::Delay::from(Duration::from_secs(STREAMING_HUB_BACKUP_POLL_INTERVAL_SECS))
                    .fuse();
            pin_mut!(backup_tick);
            select! {
                event = hub_events.as_mut().unwrap().next().fuse() => {
                    match event {
                        Some(Ok(WebsocketEvent::Message(message))) => {
                            if let Some(bytes) = message
                                .text()
                                .and_then(|text| {
                                    stream_hub_websocket_text_to_sse_bytes(&text, &mut state)
                                })
                            {
                                yield bytes;
                            }
                        }
                        Some(Ok(WebsocketEvent::Close(_))) | None => {
                            if let Some((hub_name, _)) = hub_target.as_ref() {
                                console_log!(
                                    "stream hub sse websocket closed for hub {}; falling back to d1 poll",
                                    hub_name
                                );
                            }
                            hub_events = None;
                        }
                        Some(Err(error)) => {
                            if let Some((hub_name, _)) = hub_target.as_ref() {
                                console_error!(
                                    "stream hub sse websocket error for hub {}: {}",
                                    hub_name,
                                    error
                                );
                            }
                            hub_events = None;
                        }
                    }
                }
                _ = backup_tick => {
                    match yield_streaming_poll_round(
                        &db,
                        &config,
                        &stream_name,
                        tag.as_deref(),
                        list.as_deref(),
                        viewer.as_ref(),
                        &mut state,
                        &mut poll_rounds,
                    )
                    .await
                    {
                        StreamingPollYield::Events(events) => {
                            if events.is_empty() {
                                yield sse_comment_bytes("thump");
                            } else {
                                for event in events {
                                    yield sse_event_bytes(&event);
                                }
                            }
                        }
                        StreamingPollYield::PollFailed => {
                            yield sse_comment_bytes("error=streaming_poll_failed");
                        }
                        StreamingPollYield::Recycle => {
                            yield sse_comment_bytes("stream=recycle");
                            return;
                        }
                    }
                }
            }
        }

        loop {
            match yield_streaming_poll_round(
                &db,
                &config,
                &stream_name,
                tag.as_deref(),
                list.as_deref(),
                viewer.as_ref(),
                &mut state,
                &mut poll_rounds,
            )
            .await
            {
                StreamingPollYield::Events(events) => {
                    if events.is_empty() {
                        yield sse_comment_bytes("thump");
                    } else {
                        for event in events {
                            yield sse_event_bytes(&event);
                        }
                    }
                }
                StreamingPollYield::PollFailed => {
                    yield sse_comment_bytes("error=streaming_poll_failed");
                    worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).await;
                    continue;
                }
                StreamingPollYield::Recycle => {
                    yield sse_comment_bytes("stream=recycle");
                    break;
                }
            }
            worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).await;
        }
    }
}

fn add_streaming_websocket_subscription(
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    stream_name: String,
    tag: Option<String>,
    list: Option<String>,
) {
    let key = streaming_websocket_subscription_key(&stream_name, tag.as_deref(), list.as_deref());
    subscriptions
        .entry(key)
        .or_insert_with(|| StreamingWebSocketSubscription::new(stream_name, tag, list));
}

fn remove_streaming_websocket_subscription(
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    stream_name: &str,
    tag: Option<&str>,
    list: Option<&str>,
) {
    let key = streaming_websocket_subscription_key(stream_name, tag, list);
    subscriptions.remove(&key);
}

fn handle_streaming_websocket_client_message(
    websocket: &WebSocket,
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
    text: &str,
    viewer: Option<&crate::LocalAccount>,
) -> bool {
    let message = match serde_json::from_str::<StreamingWebSocketClientMessage>(text) {
        Ok(message) => message,
        Err(error) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                &format!("Malformed streaming message: {error}"),
                400,
            ));
            return true;
        }
    };
    if !matches!(message.message_type.as_str(), "subscribe" | "unsubscribe") {
        let _ = websocket.send_with_str(streaming_websocket_error_message(
            "Unknown streaming message type",
            400,
        ));
        return true;
    }
    let stream_name = match validate_streaming_channel_request(
        message.stream.as_deref(),
        message.tag.as_deref(),
        message.list.as_deref(),
        None,
    ) {
        Ok(stream_name) => stream_name,
        Err(StreamingChannelValidationError::UnknownChannelRequested) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Unknown stream type",
                400,
            ));
            return true;
        }
        Err(StreamingChannelValidationError::MissingTag) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Missing tag parameter",
                400,
            ));
            return true;
        }
        Err(StreamingChannelValidationError::MissingList) => {
            let _ = websocket.send_with_str(streaming_websocket_error_message(
                "Missing list parameter",
                400,
            ));
            return true;
        }
    };
    if streaming_channel_requires_auth(&stream_name) && viewer.is_none() {
        let _ = websocket.send_with_str(streaming_websocket_error_message(
            "The access token is invalid",
            401,
        ));
        return true;
    }
    if message.message_type == "subscribe" {
        add_streaming_websocket_subscription(subscriptions, stream_name, message.tag, message.list);
    } else {
        remove_streaming_websocket_subscription(
            subscriptions,
            &stream_name,
            message.tag.as_deref(),
            message.list.as_deref(),
        );
    }
    true
}

async fn poll_streaming_websocket_subscriptions(
    websocket: &WebSocket,
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&crate::LocalAccount>,
    subscriptions: &mut HashMap<String, StreamingWebSocketSubscription>,
) -> bool {
    for subscription in subscriptions.values_mut() {
        if streaming_poll_budget_exhausted(0, 0) {
            return false;
        }
        let events = match poll_streaming_events(
            db,
            config,
            &subscription.stream_name,
            subscription.tag.as_deref(),
            subscription.list.as_deref(),
            viewer,
            &mut subscription.state,
        )
        .await
        {
            Ok(events) => events,
            Err(error) => {
                console_error!(
                    "websocket streaming poll failed stream={} tag={} list={} error={}",
                    subscription.stream_name,
                    subscription.tag.as_deref().unwrap_or(""),
                    subscription.list.as_deref().unwrap_or(""),
                    error
                );
                if streaming_error_is_subrequest_limit(&error) {
                    return false;
                }
                continue;
            }
        };
        for event in events {
            let message = match streaming_websocket_event_message(subscription, &event) {
                Ok(message) => message,
                Err(error) => {
                    console_error!(
                        "websocket streaming event serialization failed stream={} error={}",
                        subscription.stream_name,
                        error
                    );
                    continue;
                }
            };
            if websocket.send_with_str(message).is_err() {
                return false;
            }
        }
    }
    true
}

async fn run_streaming_websocket(
    websocket: WebSocket,
    db: D1Database,
    config: cfwdon_core::AppConfig,
    initial_stream: Option<String>,
    initial_tag: Option<String>,
    initial_list: Option<String>,
    viewer: Option<crate::LocalAccount>,
) {
    let mut subscriptions = HashMap::<String, StreamingWebSocketSubscription>::new();
    if let Some(stream_name) = initial_stream {
        add_streaming_websocket_subscription(
            &mut subscriptions,
            stream_name,
            initial_tag,
            initial_list,
        );
    }
    {
        let mut websocket_events = match websocket.events() {
            Ok(events) => events,
            Err(error) => {
                console_error!("failed to attach websocket event stream: {}", error);
                let _ = websocket.close(Some(1011), Some("stream failed"));
                return;
            }
        };
        let mut poll_rounds = 0_u32;
        let mut subscription_polls = 0_u32;
        loop {
            let tick =
                worker::Delay::from(Duration::from_secs(STREAMING_POLL_INTERVAL_SECS)).fuse();
            pin_mut!(tick);
            select! {
                event = websocket_events.next().fuse() => {
                    match event {
                        Some(Ok(WebsocketEvent::Message(message))) => {
                            let Some(text) = message.text() else {
                                let _ = websocket.send_with_str(streaming_websocket_error_message(
                                    "Only text websocket messages are supported",
                                    400,
                                ));
                                continue;
                            };
                            if !handle_streaming_websocket_client_message(
                                &websocket,
                                &mut subscriptions,
                                &text,
                                viewer.as_ref(),
                            ) {
                                break;
                            }
                        }
                        Some(Ok(WebsocketEvent::Close(_))) | None => break,
                        Some(Err(error)) => {
                            console_error!("websocket stream failed: {}", error);
                            break;
                        }
                    }
                }
                _ = tick => {
                    if subscriptions.is_empty() {
                        continue;
                    }
                    let next_subscription_polls = subscription_polls
                        .saturating_add(subscriptions.len() as u32);
                    if streaming_poll_budget_exhausted(poll_rounds, next_subscription_polls) {
                        console_log!(
                            "websocket streaming recycled before subrequest limit rounds={} subscription_polls={} d1_queries={}",
                            poll_rounds,
                            next_subscription_polls,
                            snapshot_d1_request_metrics().query_count
                        );
                        break;
                    }
                    poll_rounds = poll_rounds.saturating_add(1);
                    subscription_polls = next_subscription_polls;
                    if !poll_streaming_websocket_subscriptions(
                        &websocket,
                        &db,
                        &config,
                        viewer.as_ref(),
                        &mut subscriptions,
                    )
                    .await
                    {
                        break;
                    }
                    if streaming_poll_budget_exhausted(poll_rounds, subscription_polls) {
                        console_log!(
                            "websocket streaming recycled before subrequest limit rounds={} subscription_polls={} d1_queries={}",
                            poll_rounds,
                            subscription_polls,
                            snapshot_d1_request_metrics().query_count
                        );
                        break;
                    }
                }
            }
        }
    }
    let _ = websocket.close(Some(1000), Some("stream closed"));
}

enum StreamingAuthOutcome {
    Viewer(Option<cfwdon_domain::LocalAccount>),
    InvalidToken,
}

fn streaming_channel_supports_live_events(stream: &str) -> bool {
    matches!(
        stream,
        "public"
            | "public:media"
            | "public:local"
            | "public:local:media"
            | "public:remote"
            | "public:remote:media"
            | "hashtag"
            | "hashtag:local"
            | "user"
            | "user:notification"
            | "list"
            | "direct"
    )
}

async fn resolve_streaming_auth(
    req: &Request,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    query_access_token: Option<&str>,
    websocket_protocol_token: Option<&str>,
) -> Result<StreamingAuthOutcome> {
    match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::Auth0(viewer) => Ok(StreamingAuthOutcome::Viewer(Some(viewer))),
        LocalApiAuthentication::OAuthToken(auth) => {
            Ok(StreamingAuthOutcome::Viewer(Some(auth.account)))
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            Ok(StreamingAuthOutcome::InvalidToken)
        }
        LocalApiAuthentication::None => {
            let token = query_access_token
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| websocket_protocol_token.map(ToOwned::to_owned));
            match token {
                Some(token) => {
                    let Some(auth) =
                        find_oauth_access_token_with_account_by_bearer_token(db, &token).await?
                    else {
                        return Ok(StreamingAuthOutcome::InvalidToken);
                    };
                    if !oauth_access_token_has_any_scope(
                        &auth.token,
                        &["read", "read:statuses", "read:notifications"],
                    ) {
                        return Ok(StreamingAuthOutcome::InvalidToken);
                    }
                    Ok(StreamingAuthOutcome::Viewer(auth.account))
                }
                None => Ok(StreamingAuthOutcome::Viewer(None)),
            }
        }
    }
}

fn stream_hub_proxy_target(
    stream: &str,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    tag: Option<&str>,
    list: Option<&str>,
) -> Option<(String, Option<String>)> {
    let tag = tag.map(str::trim).filter(|value| !value.is_empty());
    let list = list.map(str::trim).filter(|value| !value.is_empty());

    match stream {
        // Authenticated channels all live on the viewer's own session hub, so a
        // list id from another account can never reach that account's events.
        "user" | "user:notification" | "direct" => viewer.map(|viewer| {
            (
                stream_hub_session_id_name(viewer.id()),
                Some(viewer.id().to_owned()),
            )
        }),
        "list" => match (viewer, list) {
            (Some(viewer), Some(_)) => Some((
                stream_hub_session_id_name(viewer.id()),
                Some(viewer.id().to_owned()),
            )),
            _ => None,
        },
        // Authenticated clients always land on their session hub; the open
        // channel hub forwards matching events there, so one socket can mix
        // session and open channels.
        "public"
        | "public:media"
        | "public:local"
        | "public:local:media"
        | "public:remote"
        | "public:remote:media" => {
            if let Some(viewer) = viewer {
                return Some((
                    stream_hub_session_id_name(viewer.id()),
                    Some(viewer.id().to_owned()),
                ));
            }
            Some((stream_hub_channel_id_name(stream, None), None))
        }
        "hashtag" | "hashtag:local" => {
            let tag_value = tag?;
            if let Some(viewer) = viewer {
                return Some((
                    stream_hub_session_id_name(viewer.id()),
                    Some(viewer.id().to_owned()),
                ));
            }
            Some((stream_hub_channel_id_name(stream, Some(tag_value)), None))
        }
        _ => None,
    }
}

/// Resolved StreamHub websocket upgrade target for a Mastodon client.
struct StreamHubWebSocketUpgradePlan {
    hub_name: String,
    stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    account_id: Option<String>,
}

/// Resolve a StreamHub websocket upgrade target.
///
/// When `initial_stream` is set, route using the per-channel mapping. When it is
/// unset but the client is authenticated, route to the account session hub so
/// Mastodon-style post-connect `subscribe` messages are handled by StreamHub.
fn stream_hub_websocket_upgrade_plan(
    initial_stream: Option<&str>,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    tag: Option<&str>,
    list: Option<&str>,
) -> Option<StreamHubWebSocketUpgradePlan> {
    if let Some(stream) = initial_stream {
        return stream_hub_proxy_target(stream, viewer, tag, list).map(|(hub_name, account_id)| {
            StreamHubWebSocketUpgradePlan {
                hub_name,
                stream: Some(stream.to_owned()),
                tag: tag.map(str::to_owned),
                list: list.map(str::to_owned),
                account_id,
            }
        });
    }

    viewer.map(|viewer| StreamHubWebSocketUpgradePlan {
        hub_name: stream_hub_session_id_name(viewer.id()),
        stream: None,
        tag: None,
        list: None,
        account_id: Some(viewer.id().to_owned()),
    })
}

async fn streaming_websocket_upgrade_response(
    env: &Env,
    req: Request,
    db: crate::D1Database,
    config: cfwdon_core::AppConfig,
    initial_stream: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    viewer: Option<cfwdon_domain::LocalAccount>,
    websocket_protocol_token: Option<&str>,
) -> Result<Response> {
    if let Some(plan) = stream_hub_websocket_upgrade_plan(
        initial_stream.as_deref(),
        viewer.as_ref(),
        tag.as_deref(),
        list.as_deref(),
    ) {
        let params = StreamHubUpgradeParams {
            stream: plan.stream.as_deref(),
            tag: plan.tag.as_deref(),
            list: plan.list.as_deref(),
            account_id: plan.account_id.as_deref(),
        };
        // The hub reads the subscription from these params, not from anything the
        // client sent, so a forged X-Account-Id or X-Stream cannot take effect.
        match upgrade_stream_hub_websocket(
            env,
            &config.stream_hub_binding,
            &plan.hub_name,
            req,
            &params,
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(error) => {
                console_log!(
                    "stream hub websocket upgrade failed for hub {}: {:?}; falling back to worker poll",
                    plan.hub_name,
                    error
                );
            }
        }
    } else {
        drop(req);
    }

    let pair = WebSocketPair::new()?;
    pair.server.accept()?;
    let websocket = pair.server.clone();
    spawn_local(async move {
        run_streaming_websocket(websocket, db, config, initial_stream, tag, list, viewer).await;
    });
    let mut response = Response::from_websocket(pair.client)?;
    if let Some(protocol) = websocket_protocol_token {
        response
            .headers_mut()
            .set("Sec-WebSocket-Protocol", protocol)?;
    }
    Ok(response)
}

fn streaming_sse_response(
    env: &Env,
    db: crate::D1Database,
    config: cfwdon_core::AppConfig,
    stream: String,
    tag: Option<String>,
    list: Option<String>,
    viewer: Option<cfwdon_domain::LocalAccount>,
) -> Result<Response> {
    if streaming_channel_supports_live_events(&stream) {
        let hub_target =
            stream_hub_proxy_target(&stream, viewer.as_ref(), tag.as_deref(), list.as_deref());
        let env_for_hub = if hub_target.is_some() {
            Some(env.clone())
        } else {
            None
        };
        let stream_body = build_streaming_event_stream(
            env_for_hub,
            db,
            config,
            stream,
            tag,
            list,
            viewer,
            hub_target,
        );
        let mut response = Response::from_stream(stream_body)?;
        response
            .headers_mut()
            .set("Content-Type", "text/event-stream")?;
        response.headers_mut().set("Cache-Control", "no-cache")?;
        return Ok(response);
    }

    let mut body = format!(": cfwdon-placeholder stream={stream}\n");
    if let Some(tag) = tag
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.push_str(&format!(": tag={tag}\n"));
    }
    if let Some(list) = list
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        body.push_str(&format!(": list={list}\n"));
    }
    body.push('\n');
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "text/event-stream")?;
    response.headers_mut().set("Cache-Control", "no-cache")?;
    Ok(response)
}

pub(crate) async fn streaming_placeholder_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let query: StreamingQuery = req.query().unwrap_or_default();
    let wants_websocket = req
        .headers()
        .get("Upgrade")
        .ok()
        .flatten()
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let websocket_protocol_token = if wants_websocket {
        websocket_protocol_access_token(&req)?
    } else {
        None
    };
    let extra_path = ctx
        .param("any")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let initial_stream = if wants_websocket && query.stream.is_none() && extra_path.is_none() {
        None
    } else {
        match validate_streaming_channel_request(
            query.stream.as_deref(),
            query.tag.as_deref(),
            query.list.as_deref(),
            extra_path,
        ) {
            Ok(stream) => Some(stream),
            Err(error) => return streaming_bad_request_response(error),
        }
    };
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let authenticated = match resolve_streaming_auth(
        &req,
        &db,
        &config,
        query.access_token.as_deref(),
        websocket_protocol_token.as_deref(),
    )
    .await?
    {
        StreamingAuthOutcome::InvalidToken => return invalid_access_token_response(),
        StreamingAuthOutcome::Viewer(viewer) => viewer,
    };

    if initial_stream
        .as_deref()
        .is_some_and(streaming_channel_requires_auth)
        && authenticated.is_none()
    {
        return invalid_access_token_response();
    }

    if wants_websocket {
        return streaming_websocket_upgrade_response(
            &ctx.env,
            req,
            db,
            config,
            initial_stream,
            query.tag.clone(),
            query.list.clone(),
            authenticated,
            websocket_protocol_token.as_deref(),
        )
        .await;
    }

    let Some(stream) = initial_stream else {
        return streaming_bad_request_response(
            StreamingChannelValidationError::UnknownChannelRequested,
        );
    };

    streaming_sse_response(
        &ctx.env,
        db,
        config,
        stream,
        query.tag,
        query.list,
        authenticated,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{record_d1_query_duration, reset_d1_request_metrics};

    #[test]
    fn sse_event_bytes_match_event_stream_format() {
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "event-1".to_owned(),
            event: "update",
            data: "{\"id\":\"status-1\"}".to_owned(),
        };

        assert_eq!(
            sse_event_bytes(&event),
            b"event: update\ndata: {\"id\":\"status-1\"}\n\n".to_vec()
        );
    }

    #[test]
    fn stream_hub_websocket_text_to_sse_bytes_matches_event_stream_format() {
        let mut state = StreamingLoopState::new();
        let message = serde_json::json!({
            "stream": ["user"],
            "event": "update",
            "payload": "{\"id\":\"status-1\"}",
        });

        assert_eq!(
            stream_hub_websocket_text_to_sse_bytes(&message.to_string(), &mut state),
            Some(b"event: update\ndata: {\"id\":\"status-1\"}\n\n".to_vec())
        );
    }

    #[test]
    fn stream_hub_websocket_text_to_sse_bytes_dedupes_repeated_events() {
        let mut state = StreamingLoopState::new();
        let message = serde_json::json!({
            "stream": ["user"],
            "event": "update",
            "payload": "{\"id\":\"status-1\"}",
        })
        .to_string();

        assert!(stream_hub_websocket_text_to_sse_bytes(&message, &mut state).is_some());
        assert!(stream_hub_websocket_text_to_sse_bytes(&message, &mut state).is_none());
    }

    #[test]
    fn stream_hub_websocket_text_to_sse_bytes_handles_filters_changed() {
        let mut state = StreamingLoopState::new();
        state.last_filter_updated_at = Some("2025-01-02T00:00:00Z".to_owned());
        let message = serde_json::json!({
            "stream": ["user"],
            "event": "filters_changed",
        });

        assert_eq!(
            stream_hub_websocket_text_to_sse_bytes(&message.to_string(), &mut state),
            Some(b"event: filters_changed\ndata: undefined\n\n".to_vec())
        );
        // Repeated filter edits must keep arriving.
        assert_eq!(
            stream_hub_websocket_text_to_sse_bytes(&message.to_string(), &mut state),
            Some(b"event: filters_changed\ndata: undefined\n\n".to_vec())
        );
    }

    #[test]
    fn streaming_websocket_event_message_matches_mastodon_shape() {
        let subscription =
            StreamingWebSocketSubscription::new("user:notification".to_owned(), None, None);
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "notification-1".to_owned(),
            event: "notification",
            data: "{\"id\":\"notification-1\"}".to_owned(),
        };

        let message = streaming_websocket_event_message(&subscription, &event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["stream"], serde_json::json!(["user:notification"]));
        assert_eq!(value["event"], "notification");
        assert_eq!(value["payload"], "{\"id\":\"notification-1\"}");
    }

    #[test]
    fn streaming_websocket_filter_change_omits_payload() {
        let subscription = StreamingWebSocketSubscription::new("user".to_owned(), None, None);
        let event = StreamingEvent {
            created_at: "2025-01-01T00:00:00Z".to_owned(),
            id: "filter-change".to_owned(),
            event: "filters_changed",
            data: "undefined".to_owned(),
        };

        let message = streaming_websocket_event_message(&subscription, &event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["stream"], serde_json::json!(["user"]));
        assert_eq!(value["event"], "filters_changed");
        assert!(value.get("payload").is_none());
    }

    #[test]
    fn streaming_websocket_stream_labels_include_subscription_params() {
        assert_eq!(
            streaming_websocket_stream_labels("hashtag", Some("rust"), None),
            vec!["hashtag".to_owned(), "rust".to_owned()]
        );
        assert_eq!(
            streaming_websocket_stream_labels("list", None, Some("list-1")),
            vec!["list".to_owned(), "list-1".to_owned()]
        );
    }

    fn stream_hub_proxy_test_account() -> cfwdon_domain::LocalAccount {
        cfwdon_domain::LocalAccount::from_record(cfwdon_domain::LocalAccountRecord::test_fixture(
            "acct-1", "alice",
        ))
    }

    #[test]
    fn stream_hub_proxy_target_maps_authenticated_channels() {
        let viewer = stream_hub_proxy_test_account();
        assert_eq!(
            stream_hub_proxy_target("user", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert_eq!(
            stream_hub_proxy_target("user:notification", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert_eq!(
            stream_hub_proxy_target("direct", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        // A list id from another account still resolves to the viewer's own hub.
        assert_eq!(
            stream_hub_proxy_target("list", Some(&viewer), None, Some("list-of-someone-else")),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert!(stream_hub_proxy_target("user", None, None, None).is_none());
        assert!(stream_hub_proxy_target("list", Some(&viewer), None, None).is_none());
        assert!(stream_hub_proxy_target("list", None, None, Some("list-1")).is_none());
    }

    #[test]
    fn stream_hub_proxy_target_maps_public_and_hashtag_channels() {
        let viewer = stream_hub_proxy_test_account();
        assert_eq!(
            stream_hub_proxy_target("public:local", None, None, None),
            Some(("public:local".to_owned(), None))
        );
        assert_eq!(
            stream_hub_proxy_target("public", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert_eq!(
            stream_hub_proxy_target("hashtag", None, Some("rust"), None),
            Some(("hashtag:rust".to_owned(), None))
        );
        assert_eq!(
            stream_hub_proxy_target("hashtag:local", Some(&viewer), Some("rust"), None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert!(stream_hub_proxy_target("hashtag", None, None, None).is_none());
        assert!(stream_hub_proxy_target("unknown", None, None, None).is_none());
    }

    #[test]
    fn stream_hub_websocket_upgrade_plan_maps_deferred_authenticated_subscribe() {
        let viewer = stream_hub_proxy_test_account();
        let plan = stream_hub_websocket_upgrade_plan(None, Some(&viewer), None, None).unwrap();
        assert_eq!(plan.hub_name, "user:acct-1");
        assert!(plan.stream.is_none());
        assert!(plan.tag.is_none());
        assert!(plan.list.is_none());
        assert_eq!(plan.account_id.as_deref(), Some("acct-1"));
    }

    #[test]
    fn stream_hub_websocket_upgrade_plan_keeps_explicit_stream_mapping() {
        let viewer = stream_hub_proxy_test_account();
        let plan =
            stream_hub_websocket_upgrade_plan(Some("user"), Some(&viewer), None, None).unwrap();
        assert_eq!(plan.hub_name, "user:acct-1");
        assert_eq!(plan.stream.as_deref(), Some("user"));
        assert_eq!(plan.account_id.as_deref(), Some("acct-1"));
    }

    #[test]
    fn stream_hub_websocket_upgrade_plan_defers_anonymous_subscribe_to_worker_poll() {
        assert!(stream_hub_websocket_upgrade_plan(None, None, None, None).is_none());
    }

    #[test]
    fn streaming_channel_validation_accepts_query_and_path_channels() {
        assert_eq!(
            validate_streaming_channel_request(Some("public"), None, None, None).unwrap(),
            "public"
        );
        assert_eq!(
            validate_streaming_channel_request(None, None, None, Some("user")).unwrap(),
            "user"
        );
        assert_eq!(
            validate_streaming_channel_request(None, None, None, Some("public/local/media"))
                .unwrap(),
            "public:local:media"
        );
        assert_eq!(
            validate_streaming_channel_request(None, Some("rust"), None, Some("hashtag")).unwrap(),
            "hashtag"
        );
    }

    #[test]
    fn streaming_channel_supports_live_events_for_validated_channels() {
        assert!(streaming_channel_supports_live_events("public"));
        assert!(streaming_channel_supports_live_events("user:notification"));
        assert!(streaming_channel_supports_live_events("direct"));
        assert!(!streaming_channel_supports_live_events("unknown"));
    }

    #[test]
    fn streaming_channel_validation_rejects_conflicting_query_and_path_channels() {
        assert!(matches!(
            validate_streaming_channel_request(Some("public"), None, None, Some("user")),
            Err(StreamingChannelValidationError::UnknownChannelRequested)
        ));
    }

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
    fn announcement_stream_entry_action_prioritizes_reaction_delta() {
        let previous_reactions = BTreeMap::from([("wave".to_owned(), (1, false))]);
        let current_reactions = BTreeMap::from([("wave".to_owned(), (2, true))]);

        assert_eq!(
            announcement_stream_entry_action(
                true,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-2\"}",
                &current_reactions,
                &previous_reactions,
            ),
            AnnouncementStreamEntryAction::None
        );
        assert_eq!(
            announcement_stream_entry_action(
                false,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-2\"}",
                &current_reactions,
                &previous_reactions,
            ),
            AnnouncementStreamEntryAction::Reaction
        );
    }

    #[test]
    fn announcement_stream_entry_action_detects_payload_delta_after_reactions() {
        let reactions = BTreeMap::from([("wave".to_owned(), (1, false))]);

        assert_eq!(
            announcement_stream_entry_action(
                false,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-2\"}",
                &reactions,
                &reactions,
            ),
            AnnouncementStreamEntryAction::Announcement
        );
        assert_eq!(
            announcement_stream_entry_action(
                false,
                Some("{\"id\":\"announcement-1\"}"),
                "{\"id\":\"announcement-1\"}",
                &reactions,
                &reactions,
            ),
            AnnouncementStreamEntryAction::None
        );
    }

    #[test]
    fn append_current_announcement_stream_entry_events_prioritizes_reaction_events() {
        let entry = AnnouncementStreamEntry {
            id: "announcement-1".to_owned(),
            payload: "{\"id\":\"announcement-1\",\"content\":\"new\"}".to_owned(),
            created_at: "2026-05-01T00:00:00Z".to_owned(),
        };
        let previous_announcements = HashMap::from([(
            "announcement-1".to_owned(),
            "{\"id\":\"announcement-1\",\"content\":\"old\"}".to_owned(),
        )]);
        let previous_reactions =
            HashMap::from([(("announcement-1".to_owned(), "wave".to_owned()), (1, false))]);
        let current_reactions =
            HashMap::from([(("announcement-1".to_owned(), "wave".to_owned()), (2, true))]);
        let mut events = Vec::new();

        append_current_announcement_stream_entry_events(
            &entry,
            false,
            &previous_announcements,
            &previous_reactions,
            &current_reactions,
            &mut events,
        );

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "announcement.reaction");
        assert_eq!(events[0].id, "announcement-1:wave");
    }

    #[test]
    fn removed_announcement_ids_returns_only_missing_current_ids() {
        let previous = HashMap::from([
            ("announcement-1".to_owned(), "{}".to_owned()),
            ("announcement-2".to_owned(), "{}".to_owned()),
        ]);
        let current = HashMap::from([("announcement-2".to_owned(), "{}".to_owned())]);

        assert_eq!(
            removed_announcement_ids(&previous, &current),
            vec!["announcement-1".to_owned()]
        );
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

    #[test]
    fn list_stream_membership_refs_include_any_accepts_any_candidate_variant() {
        let membership_refs = HashSet::from(["alice@example.com".to_owned()]);

        assert!(list_stream_membership_refs_include_any(
            &membership_refs,
            vec![
                "acct:alice@example.com".to_owned(),
                "alice@example.com".to_owned()
            ]
        ));
        assert!(!list_stream_membership_refs_include_any(
            &membership_refs,
            vec!["bob@example.com".to_owned()]
        ));
    }

    #[test]
    fn list_stream_status_policy_requires_membership_and_allowed_reply() {
        let membership_refs = HashSet::from(["alice@example.com".to_owned()]);
        let allow_replies = ListStreamStatusPolicy::new(&membership_refs, "list");
        let exclude_replies = ListStreamStatusPolicy::new(&membership_refs, "none");

        assert!(allow_replies.matches(
            vec![
                "acct:alice@example.com".to_owned(),
                "alice@example.com".to_owned()
            ],
            Some("status-1"),
        ));
        assert!(!allow_replies.matches(vec!["bob@example.com".to_owned()], None,));
        assert!(!exclude_replies.matches(vec!["alice@example.com".to_owned()], Some("status-1"),));
    }

    #[test]
    fn list_stream_excludes_reply_only_when_policy_blocks_replies() {
        assert!(list_stream_excludes_reply("none", Some("status-1")));
        assert!(!list_stream_excludes_reply("list", Some("status-1")));
        assert!(!list_stream_excludes_reply("none", None));
    }

    #[test]
    fn announcement_stream_entry_extracts_payload_identity_and_time() {
        let announcement = serde_json::json!({
            "id": "announcement-1",
            "published_at": "2026-05-01T00:00:00Z",
            "content": "<p>Hello</p>"
        });

        let entry = announcement_stream_entry(&announcement).unwrap().unwrap();

        assert_eq!(entry.id, "announcement-1");
        assert_eq!(entry.created_at, "2026-05-01T00:00:00Z");
        assert!(entry.payload.contains("\"announcement-1\""));
    }

    #[test]
    fn announcement_stream_entry_uses_updated_at_fallback() {
        let announcement = serde_json::json!({
            "id": "announcement-1",
            "updated_at": "2026-05-02T00:00:00Z",
        });

        let entry = announcement_stream_entry(&announcement).unwrap().unwrap();

        assert_eq!(entry.created_at, "2026-05-02T00:00:00Z");
    }

    #[test]
    fn announcement_stream_entries_skips_documents_without_stream_identity() {
        let announcements = vec![
            serde_json::json!({
                "id": "announcement-1",
                "published_at": "2026-05-01T00:00:00Z",
            }),
            serde_json::json!({
                "content": "<p>missing id</p>",
            }),
        ];

        let entries = announcement_stream_entries(announcements).unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "announcement-1");
    }

    #[test]
    fn streaming_poll_budget_exhausts_before_cloudflare_subrequest_limit() {
        reset_d1_request_metrics();
        assert!(!streaming_poll_budget_exhausted(
            STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION - 1,
            STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION - 1
        ));
        assert!(streaming_poll_budget_exhausted(
            STREAMING_MAX_POLL_ROUNDS_PER_INVOCATION,
            1
        ));
        assert!(streaming_poll_budget_exhausted(
            1,
            STREAMING_MAX_SUBSCRIPTION_POLLS_PER_INVOCATION
        ));
        reset_d1_request_metrics();
    }

    #[test]
    fn streaming_poll_budget_exhausts_on_d1_subrequest_count() {
        reset_d1_request_metrics();
        for _ in 0..STREAMING_MAX_D1_SUBREQUESTS_PER_INVOCATION {
            record_d1_query_duration(1);
        }
        assert!(streaming_poll_budget_exhausted(1, 1));
        reset_d1_request_metrics();
    }

    #[test]
    fn streaming_error_detects_cloudflare_subrequest_limit() {
        let error = worker::Error::RustError(
            "Error: Too many API requests by single Worker invocation".to_owned(),
        );

        assert!(streaming_error_is_subrequest_limit(&error));
    }
}
