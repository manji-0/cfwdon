use crate::auth::find_account_by_id;
use crate::content_helpers::{extract_hashtags_from_html, extract_hashtags_from_text};
use crate::conversation_store::{find_conversation_for_account, find_conversation_id_by_status_id};
use crate::conversations::conversation_document;
use crate::identity::actor_url;
use crate::lists::{
    list_membership_refs, list_membership_variants_for_local_account,
    list_membership_variants_for_remote_actor, list_row_by_id,
};
use crate::media::{find_media_attachments_by_status_id, remote_status_has_media};
use crate::notifications::{
    NotificationsQuery, collect_visible_notifications, filter_notification_entries_by_query,
};
use crate::statuses::{
    build_local_status_response, build_remote_status_response, is_local_status_thread_muted_by,
    list_local_direct_timeline_statuses, list_local_public_statuses_by_tag,
    list_local_public_timeline_statuses, list_remote_public_statuses_by_tag,
    list_remote_public_timeline_statuses, load_in_reply_to_account_id,
};
use crate::store::relationship::is_muted_actor;
use crate::store::remote::RemoteActorRow;
use crate::streaming_types::{
    StreamingBatch, StreamingEntry, StreamingEvent, StreamingPublicPlan,
    streaming_batch_from_entries,
};
use crate::timelines::{
    ResolvedTimelineCursor, TagTimelineQuery, TimelinePaginationQuery,
    matches_tag_timeline_filters, resolve_timeline_cursor, timeline_fetch_limit,
};
use crate::tracked_d1::D1Database;
use cfwdon_domain::{LocalStatus, RemoteStatus};
use std::collections::HashSet;
use worker::Result;

pub(super) async fn streaming_notification_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
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

pub(super) async fn append_streaming_local_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    status: LocalStatus,
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
        if !matches_tag_timeline_filters(&status_tags, tag, &TagTimelineQuery::default()) {
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

pub(super) async fn append_streaming_remote_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    status: RemoteStatus,
    actor: RemoteActorRow,
    only_media: bool,
    tag_filter: Option<&str>,
    payload_context: &str,
    entries: &mut Vec<StreamingEntry>,
    tracked_status_ids: &mut Vec<String>,
) -> Result<()> {
    if let Some(tag) = tag_filter {
        let status_tags = extract_hashtags_from_html(&status.content_html);
        if !matches_tag_timeline_filters(&status_tags, tag, &TagTimelineQuery::default()) {
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

pub(super) async fn streaming_public_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&cfwdon_domain::LocalAccount>,
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

pub(super) async fn append_streaming_hashtag_status_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    plan: StreamingPublicPlan,
    tag: &str,
    cursor: &ResolvedTimelineCursor,
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

pub(super) async fn append_streaming_public_status_entries(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    plan: StreamingPublicPlan,
    cursor: &ResolvedTimelineCursor,
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

pub(super) async fn streaming_direct_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
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
            serde_json::to_string(&conversation_document(db, config, viewer, &conversation).await?)
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

pub(super) async fn streaming_list_batch(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
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

pub(super) struct StreamingListBatchContext {
    cursor: ResolvedTimelineCursor,
    query_limit: u32,
    membership_refs: HashSet<String>,
    replies_policy: String,
}

pub(super) async fn streaming_list_batch_context(
    db: &D1Database,
    viewer: &cfwdon_domain::LocalAccount,
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

pub(super) async fn append_streaming_list_local_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    policy: &ListStreamStatusPolicy<'_>,
    status: LocalStatus,
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

pub(super) async fn append_streaming_list_remote_status_entry(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    policy: &ListStreamStatusPolicy<'_>,
    status: RemoteStatus,
    actor: RemoteActorRow,
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

pub(super) struct ListStreamStatusPolicy<'a> {
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

pub(super) fn list_stream_membership_refs_include_any(
    membership_refs: &HashSet<String>,
    candidates: impl IntoIterator<Item = String>,
) -> bool {
    candidates
        .into_iter()
        .any(|candidate| membership_refs.contains(&candidate))
}

pub(super) fn list_stream_excludes_reply(
    replies_policy: &str,
    reply_reference: Option<&str>,
) -> bool {
    replies_policy == "none" && reply_reference.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
