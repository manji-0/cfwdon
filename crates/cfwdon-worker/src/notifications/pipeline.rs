//! Notifications are listed in two phases. Collectors first return light
//! candidates (internal key, creation time, what to render); the listing
//! filters, orders and cuts those, and only the candidates a response needs
//! are rendered, with one shared preload.

use super::{
    MastodonNotificationResponse, NotificationEntry, NotificationIdentity, NotificationsQuery,
    build_status_notification_entry, collect_notifications, load_dismissed_notification_ids,
    load_notification_clear_marker, notification_account_matches_filter,
    notification_api_numeric_id, notification_entry_matches_cursor_id,
    notification_group_key_from_parts, notification_timestamp_sort_token,
    preload_notification_statuses, push_notification_entry,
};
use crate::accounts::find_accounts_by_ids;
use crate::identity::{actor_url, remote_account_rest_id};
use crate::responses::MastodonAccountResponse;
use crate::statuses::{can_view_local_status, find_statuses_by_ids};
use crate::store::relationship::list_notification_muted_actor_uris_for_account;
use crate::store::remote::{RemoteActorRow, find_remote_actors_by_actor_uris};
use crate::time_html::timestamp_to_mastodon_iso8601;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, LocalStatus, RemoteStatus};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use worker::Result;

/// Who caused a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NotificationActorRef {
    /// A local account id.
    Local(String),
    /// A remote actor URI.
    Remote(String),
}

impl NotificationActorRef {
    /// The `local-<account id>` / `remote-<account id>` part of internal keys.
    fn key_part(&self) -> String {
        match self {
            Self::Local(account_id) => format!("local-{account_id}"),
            Self::Remote(actor_uri) => format!("remote-{}", remote_account_rest_id(actor_uri)),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum NotificationCandidateSource {
    /// Rendered by its collector already: admin and collection notifications,
    /// which are rare, and kinds not yet split into two phases.
    Ready(NotificationEntry),
    /// A favourite or reblog of one of the viewer's statuses.
    ViewerStatusInteraction { status_id: String },
    /// A follow or follow request.
    Account,
    /// A status a local account wrote: a mention, a subscribed post, a quote.
    AuthoredLocalStatus(Box<LocalStatus>),
    /// A local account's status loaded by id: an ended poll.
    AuthoredLocalStatusById { status_id: String },
    /// A status a remote actor wrote.
    AuthoredRemoteStatus(Box<RemoteStatus>),
}

#[derive(Debug, Clone)]
pub(crate) struct NotificationCandidate {
    /// The internal key dismissals, cursors and API ids are derived from. It
    /// is fixed here and reused by the rendered entry, never re-derived.
    pub(crate) id: String,
    /// Mastodon-format creation time, identical to the rendered entry's.
    pub(crate) created_at: String,
    pub(crate) notification_type: String,
    /// The status a favourite or reblog targets, for group keys.
    pub(crate) status_id: Option<String>,
    /// Who caused it, for mutes and `account_id`; `None` for ready
    /// candidates, whose collectors applied both.
    pub(crate) actor: Option<NotificationActorRef>,
    pub(crate) source: NotificationCandidateSource,
}

impl NotificationIdentity for NotificationCandidate {
    fn notification_key(&self) -> &str {
        &self.id
    }

    fn notification_created_at(&self) -> &str {
        &self.created_at
    }
}

impl NotificationCandidate {
    pub(crate) fn ready(entry: NotificationEntry) -> Self {
        let notification_type = entry
            .value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let status_id = entry
            .value
            .get("status")
            .and_then(|status| status.get("id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        Self {
            id: entry.id.clone(),
            created_at: entry.created_at.clone(),
            notification_type,
            status_id,
            actor: None,
            source: NotificationCandidateSource::Ready(entry),
        }
    }

    /// `favourite-local-<account>-<status>` and the like.
    pub(crate) fn viewer_status_interaction(
        notification_type: &str,
        actor: NotificationActorRef,
        status_id: String,
        created_at: &str,
    ) -> Self {
        Self {
            id: format!("{notification_type}-{}-{status_id}", actor.key_part()),
            created_at: timestamp_to_mastodon_iso8601(created_at),
            notification_type: notification_type.to_owned(),
            status_id: Some(status_id.clone()),
            actor: Some(actor),
            source: NotificationCandidateSource::ViewerStatusInteraction { status_id },
        }
    }

    /// `mention-local-<author>-<status>` and the like.
    pub(crate) fn authored_local_status(notification_type: &str, status: LocalStatus) -> Self {
        let actor = NotificationActorRef::Local(status.account_id.clone());
        Self {
            id: format!("{notification_type}-{}-{}", actor.key_part(), status.id),
            created_at: timestamp_to_mastodon_iso8601(&status.created_at),
            notification_type: notification_type.to_owned(),
            status_id: Some(status.id.clone()),
            actor: Some(actor),
            source: NotificationCandidateSource::AuthoredLocalStatus(Box::new(status)),
        }
    }

    /// `mention-remote-<author>-<status>` and the like.
    pub(crate) fn authored_remote_status(notification_type: &str, status: RemoteStatus) -> Self {
        let actor = NotificationActorRef::Remote(status.actor_uri.clone());
        Self {
            id: format!("{notification_type}-{}-{}", actor.key_part(), status.id),
            created_at: timestamp_to_mastodon_iso8601(&status.published_at),
            notification_type: notification_type.to_owned(),
            status_id: Some(status.id.clone()),
            actor: Some(actor),
            source: NotificationCandidateSource::AuthoredRemoteStatus(Box::new(status)),
        }
    }

    /// An event on one of the viewer's statuses under a caller-built key,
    /// such as a quoted post's edit.
    pub(crate) fn viewer_status_event(
        notification_type: &str,
        id: String,
        actor: NotificationActorRef,
        status_id: String,
        created_at: &str,
    ) -> Self {
        Self {
            id,
            created_at: timestamp_to_mastodon_iso8601(created_at),
            notification_type: notification_type.to_owned(),
            status_id: Some(status_id.clone()),
            actor: Some(actor),
            source: NotificationCandidateSource::ViewerStatusInteraction { status_id },
        }
    }

    /// An event on a remote status under a caller-built key, such as an edit.
    pub(crate) fn authored_remote_status_event(
        notification_type: &str,
        id: String,
        created_at: &str,
        status: RemoteStatus,
    ) -> Self {
        Self {
            id,
            created_at: timestamp_to_mastodon_iso8601(created_at),
            notification_type: notification_type.to_owned(),
            status_id: Some(status.id.clone()),
            actor: Some(NotificationActorRef::Remote(status.actor_uri.clone())),
            source: NotificationCandidateSource::AuthoredRemoteStatus(Box::new(status)),
        }
    }

    /// An event on a local account's status, loaded by id when rendered.
    pub(crate) fn authored_local_status_event(
        notification_type: &str,
        id: String,
        author_account_id: String,
        status_id: String,
        created_at: &str,
    ) -> Self {
        Self {
            id,
            created_at: timestamp_to_mastodon_iso8601(created_at),
            notification_type: notification_type.to_owned(),
            status_id: Some(status_id.clone()),
            actor: Some(NotificationActorRef::Local(author_account_id)),
            source: NotificationCandidateSource::AuthoredLocalStatusById { status_id },
        }
    }

    /// `follow-local-<account>`, `follow-request-remote-<account>` and the like.
    pub(crate) fn account(
        notification_type: &str,
        id_prefix: &str,
        actor: NotificationActorRef,
        created_at: &str,
    ) -> Self {
        Self {
            id: format!("{id_prefix}-{}", actor.key_part()),
            created_at: timestamp_to_mastodon_iso8601(created_at),
            notification_type: notification_type.to_owned(),
            status_id: None,
            actor: Some(actor),
            source: NotificationCandidateSource::Account,
        }
    }

    pub(crate) fn group_key(&self, grouped_types: &[String]) -> String {
        notification_group_key_from_parts(
            &self.notification_type,
            self.status_id.as_deref(),
            &self.created_at,
            notification_api_numeric_id(self),
            grouped_types,
        )
    }
}

/// Phase one: every source's candidates the viewer has not dismissed or
/// cleared, newest first by API id.
pub(crate) async fn collect_notification_candidates(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationCandidate>> {
    let (mut candidates, dismissed_ids, cleared_at) = futures_util::try_join!(
        collect_notifications(db, config, viewer, query, per_type_limit),
        load_dismissed_notification_ids(db, viewer.id()),
        load_notification_clear_marker(db, viewer.id()),
    )?;
    retain_undismissed_candidates(&mut candidates, &dismissed_ids, cleared_at.as_deref());
    retain_candidates_from_unmuted_actors(db, config, viewer, query, &mut candidates).await?;
    sort_candidates_newest_first(&mut candidates);
    Ok(candidates)
}

/// Drops candidates from actors the viewer muted notifications from, from
/// local accounts that no longer exist, and from anyone but `account_id`.
async fn retain_candidates_from_unmuted_actors(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    candidates: &mut Vec<NotificationCandidate>,
) -> Result<()> {
    if candidates.iter().all(|candidate| candidate.actor.is_none()) {
        return Ok(());
    }
    let local_account_ids = unique_local_actor_ids(candidates.iter());
    let (accounts_by_id, muted_actor_uris) = futures_util::try_join!(
        find_accounts_by_ids(db, &local_account_ids),
        list_notification_muted_actor_uris_for_account(db, viewer.id()),
    )?;
    candidates.retain(|candidate| match &candidate.actor {
        None => true,
        Some(NotificationActorRef::Local(account_id)) => {
            accounts_by_id.get(account_id).is_some_and(|account| {
                !muted_actor_uris.contains(&actor_url(config, account.username()))
                    && notification_account_matches_filter(
                        query.account_id.as_deref(),
                        account.id(),
                        None,
                    )
            })
        }
        Some(NotificationActorRef::Remote(actor_uri)) => {
            !muted_actor_uris.contains(actor_uri)
                && notification_account_matches_filter(
                    query.account_id.as_deref(),
                    &remote_account_rest_id(actor_uri),
                    Some(actor_uri),
                )
        }
    });
    Ok(())
}

fn unique_local_actor_ids<'a>(
    candidates: impl Iterator<Item = &'a NotificationCandidate>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    candidates
        .filter_map(|candidate| match &candidate.actor {
            Some(NotificationActorRef::Local(account_id)) => Some(account_id.clone()),
            _ => None,
        })
        .filter(|account_id| seen.insert(account_id.clone()))
        .collect()
}

fn unique_remote_actor_uris<'a>(
    candidates: impl Iterator<Item = &'a NotificationCandidate>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    candidates
        .filter_map(|candidate| match &candidate.actor {
            Some(NotificationActorRef::Remote(actor_uri)) => Some(actor_uri.clone()),
            _ => None,
        })
        .filter(|actor_uri| seen.insert(actor_uri.clone()))
        .collect()
}

pub(crate) fn retain_undismissed_candidates<T: NotificationIdentity>(
    candidates: &mut Vec<T>,
    dismissed_ids: &HashSet<String>,
    cleared_at: Option<&str>,
) {
    let cleared_at_token = cleared_at.and_then(notification_timestamp_sort_token);
    candidates.retain(|candidate| {
        if dismissed_ids.contains(candidate.notification_key()) {
            return false;
        }
        match (
            cleared_at_token.as_deref(),
            notification_timestamp_sort_token(candidate.notification_created_at()),
        ) {
            (Some(cleared_at), Some(created_at)) => created_at.as_str() > cleared_at,
            _ => true,
        }
    });
}

pub(crate) fn sort_candidates_newest_first<T: NotificationIdentity>(candidates: &mut [T]) {
    candidates
        .sort_by_cached_key(|candidate| std::cmp::Reverse(notification_api_numeric_id(candidate)));
}

/// Candidates in one group. `ungrouped-<id>` names one notification whatever
/// `grouped_types[]` the listing used, so it matches by id.
pub(crate) fn notification_group_candidates(
    candidates: Vec<NotificationCandidate>,
    group_key: &str,
    grouped_types: &[String],
) -> Vec<NotificationCandidate> {
    if let Some(notification_id) = group_key.strip_prefix("ungrouped-") {
        return candidates
            .into_iter()
            .filter(|candidate| notification_entry_matches_cursor_id(candidate, notification_id))
            .collect();
    }
    candidates
        .into_iter()
        .filter(|candidate| candidate.group_key(grouped_types) == group_key)
        .collect()
}

/// Phase two: render `candidates` in order with one shared preload.
/// Candidates whose rows have gone since phase one are dropped.
pub(crate) async fn hydrate_notification_candidates(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    candidates: Vec<NotificationCandidate>,
) -> Result<Vec<NotificationEntry>> {
    if candidates.iter().all(|candidate| candidate.actor.is_none()) {
        return Ok(candidates
            .into_iter()
            .filter_map(|candidate| match candidate.source {
                NotificationCandidateSource::Ready(entry) => Some(entry),
                _ => None,
            })
            .collect());
    }

    let mut status_ids_to_load = Vec::new();
    let mut seen_status_ids = HashSet::new();
    let mut authored_local_statuses = Vec::new();
    let mut authored_remote_statuses = Vec::new();
    for candidate in &candidates {
        match &candidate.source {
            NotificationCandidateSource::ViewerStatusInteraction { status_id }
            | NotificationCandidateSource::AuthoredLocalStatusById { status_id }
                if seen_status_ids.insert(status_id.as_str()) =>
            {
                status_ids_to_load.push(status_id.clone());
            }
            NotificationCandidateSource::AuthoredLocalStatus(status) => {
                authored_local_statuses.push(status.as_ref().clone());
            }
            NotificationCandidateSource::AuthoredRemoteStatus(status) => {
                authored_remote_statuses.push(status.as_ref().clone());
            }
            _ => {}
        }
    }
    let local_account_ids = unique_local_actor_ids(candidates.iter());
    let remote_actor_uris = unique_remote_actor_uris(candidates.iter());
    let (loaded_statuses, accounts_by_id, actors_by_uri) = futures_util::try_join!(
        find_statuses_by_ids(db, &status_ids_to_load),
        find_accounts_by_ids(db, &local_account_ids),
        find_remote_actors_by_actor_uris(db, &remote_actor_uris),
    )?;
    let mut local_statuses = loaded_statuses;
    let loaded_statuses_by_id = local_statuses
        .iter()
        .map(|status| (status.id.clone(), status.clone()))
        .collect::<HashMap<_, _>>();
    local_statuses.extend(authored_local_statuses);
    let preloads = preload_notification_statuses(
        db,
        config,
        viewer,
        &local_statuses,
        &authored_remote_statuses,
    )
    .await?;
    let context = HydrationContext {
        db,
        config,
        viewer,
        accounts_by_id: &accounts_by_id,
        actors_by_uri: &actors_by_uri,
        loaded_statuses_by_id: &loaded_statuses_by_id,
        preloads: &preloads,
    };

    let rendered = futures_util::future::try_join_all(
        candidates
            .into_iter()
            .map(|candidate| context.render(candidate)),
    )
    .await?;
    Ok(rendered.into_iter().flatten().collect())
}

struct HydrationContext<'a> {
    db: &'a D1Database,
    config: &'a AppConfig,
    viewer: &'a LocalAccount,
    accounts_by_id: &'a HashMap<String, LocalAccount>,
    actors_by_uri: &'a HashMap<String, RemoteActorRow>,
    loaded_statuses_by_id: &'a HashMap<String, LocalStatus>,
    preloads: &'a super::status_preload::NotificationStatusPreloads,
}

impl HydrationContext<'_> {
    fn actor_account(
        &self,
        actor: Option<&NotificationActorRef>,
    ) -> Option<MastodonAccountResponse> {
        match actor? {
            NotificationActorRef::Local(account_id) => self
                .accounts_by_id
                .get(account_id)
                .map(|account| MastodonAccountResponse::from_account(account, self.config)),
            NotificationActorRef::Remote(actor_uri) => self
                .actors_by_uri
                .get(actor_uri)
                .map(MastodonAccountResponse::from_remote_actor),
        }
    }

    async fn render_authored_local_status(
        &self,
        id: String,
        notification_type: &str,
        created_at: String,
        actor: Option<&NotificationActorRef>,
        status: &LocalStatus,
    ) -> Result<Option<NotificationEntry>> {
        let Some(NotificationActorRef::Local(account_id)) = actor else {
            return Ok(None);
        };
        let Some(author) = self.accounts_by_id.get(account_id) else {
            return Ok(None);
        };
        if !can_view_local_status(self.db, status, Some(self.viewer), author).await? {
            return Ok(None);
        }
        let status_response = self
            .preloads
            .build_local_status_response(
                self.db,
                self.config,
                self.viewer,
                status,
                author,
                self.preloads.local_media(&status.id),
            )
            .await?;
        Ok(Some(build_status_notification_entry(
            id,
            notification_type,
            created_at,
            MastodonAccountResponse::from_account(author, self.config),
            status_response,
        )))
    }

    async fn render(&self, candidate: NotificationCandidate) -> Result<Option<NotificationEntry>> {
        let NotificationCandidate {
            id,
            created_at,
            notification_type,
            actor,
            source,
            ..
        } = candidate;
        match source {
            NotificationCandidateSource::Ready(entry) => Ok(Some(entry)),
            NotificationCandidateSource::ViewerStatusInteraction { status_id } => {
                let (Some(account), Some(status)) = (
                    self.actor_account(actor.as_ref()),
                    self.loaded_statuses_by_id.get(&status_id),
                ) else {
                    return Ok(None);
                };
                let status_response = self
                    .preloads
                    .build_local_status_response(
                        self.db,
                        self.config,
                        self.viewer,
                        status,
                        self.viewer,
                        self.preloads.local_media(&status.id),
                    )
                    .await?;
                Ok(Some(build_status_notification_entry(
                    id,
                    &notification_type,
                    created_at,
                    account,
                    status_response,
                )))
            }
            NotificationCandidateSource::AuthoredLocalStatus(status) => {
                self.render_authored_local_status(
                    id,
                    &notification_type,
                    created_at,
                    actor.as_ref(),
                    &status,
                )
                .await
            }
            NotificationCandidateSource::AuthoredLocalStatusById { status_id } => {
                let Some(status) = self.loaded_statuses_by_id.get(&status_id) else {
                    return Ok(None);
                };
                self.render_authored_local_status(
                    id,
                    &notification_type,
                    created_at,
                    actor.as_ref(),
                    status,
                )
                .await
            }
            NotificationCandidateSource::AuthoredRemoteStatus(status) => {
                let Some(author) = self.actors_by_uri.get(&status.actor_uri) else {
                    return Ok(None);
                };
                let status_response = self
                    .preloads
                    .build_remote_status_response(
                        self.db,
                        self.config,
                        self.viewer,
                        &status,
                        author,
                        self.preloads.remote_media(&status.id),
                    )
                    .await?;
                Ok(Some(build_status_notification_entry(
                    id,
                    &notification_type,
                    created_at,
                    MastodonAccountResponse::from_remote_actor(author),
                    status_response,
                )))
            }
            NotificationCandidateSource::Account => {
                let Some(account) = self.actor_account(actor.as_ref()) else {
                    return Ok(None);
                };
                let mut entries = Vec::with_capacity(1);
                push_notification_entry(
                    &mut entries,
                    MastodonNotificationResponse {
                        group_key: id.clone(),
                        id,
                        notification_type,
                        created_at,
                        account,
                        status: None,
                        report: None,
                    },
                );
                Ok(entries.pop())
            }
        }
    }
}

/// Render one page from newest-first `candidates`: the newest `limit`, or
/// with `forward` (`min_id`) the oldest `limit`, returned newest first.
pub(crate) async fn hydrate_notification_page(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    candidates: Vec<NotificationCandidate>,
    limit: usize,
    forward: bool,
) -> Result<Vec<NotificationEntry>> {
    fill_notification_page(candidates, limit, forward, |chunk| {
        hydrate_notification_candidates(db, config, viewer, chunk)
    })
    .await
}

/// Rendering can drop candidates, so the page is filled in chunks: render a
/// little more than is missing, and render more only if the page is short.
pub(crate) async fn fill_notification_page<C, E, F, Fut>(
    candidates: Vec<C>,
    limit: usize,
    forward: bool,
    mut hydrate: F,
) -> Result<Vec<E>>
where
    F: FnMut(Vec<C>) -> Fut,
    Fut: Future<Output = Result<Vec<E>>>,
{
    let mut remaining = if forward {
        candidates.into_iter().rev().collect::<Vec<_>>().into_iter()
    } else {
        candidates.into_iter()
    };
    let mut page = Vec::new();
    while page.len() < limit {
        let missing = limit - page.len();
        let chunk = remaining
            .by_ref()
            .take(missing + (missing / 4).max(8))
            .collect::<Vec<_>>();
        if chunk.is_empty() {
            break;
        }
        page.extend(hydrate(chunk).await?);
    }
    page.truncate(limit);
    if forward {
        page.reverse();
    }
    Ok(page)
}

#[cfg(test)]
mod tests {
    use super::{
        NotificationActorRef, NotificationCandidate, fill_notification_page,
        retain_undismissed_candidates, sort_candidates_newest_first,
    };
    use crate::notifications::{NotificationEntry, notification_api_numeric_id};
    use futures_util::FutureExt;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn entry(id: &str, created_at: &str) -> NotificationEntry {
        NotificationEntry {
            id: id.to_owned(),
            created_at: created_at.to_owned(),
            value: serde_json::json!({ "id": id }),
        }
    }

    fn newest_first(count: usize) -> Vec<NotificationEntry> {
        let mut entries = (0..count)
            .map(|index| {
                entry(
                    &format!("n{index}"),
                    &format!("2026-10-07T05:{:02}:00.000Z", index),
                )
            })
            .collect::<Vec<_>>();
        sort_candidates_newest_first(&mut entries);
        entries
    }

    fn ids(entries: &[NotificationEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.id.as_str()).collect()
    }

    #[test]
    fn candidate_keys_match_the_keys_collectors_rendered_before() {
        // Dismissals are stored under these keys, so they must not change.
        let remote = "https://remote.example/users/alice";
        let rest_id = crate::identity::remote_account_rest_id(remote);
        let favourite = NotificationCandidate::viewer_status_interaction(
            "favourite",
            NotificationActorRef::Remote(remote.to_owned()),
            "s1".to_owned(),
            "2026-10-07 05:00:00",
        );
        assert_eq!(favourite.id, format!("favourite-remote-{rest_id}-s1"));
        assert_eq!(favourite.created_at, "2026-10-07T05:00:00.000Z");
        assert_eq!(favourite.status_id.as_deref(), Some("s1"));

        let reblog = NotificationCandidate::viewer_status_interaction(
            "reblog",
            NotificationActorRef::Local("a1".to_owned()),
            "s1".to_owned(),
            "2026-10-07T05:00:00.000Z",
        );
        assert_eq!(reblog.id, "reblog-local-a1-s1");

        let follow_request = NotificationCandidate::account(
            "follow_request",
            "follow-request",
            NotificationActorRef::Remote(remote.to_owned()),
            "2026-10-07 05:00:00",
        );
        assert_eq!(
            follow_request.id,
            format!("follow-request-remote-{rest_id}")
        );
        assert_eq!(follow_request.notification_type, "follow_request");
    }

    #[test]
    fn page_keeps_the_newest_or_with_forward_the_oldest_newest_first() {
        let page = fill_notification_page(newest_first(5), 2, false, |chunk| async move {
            Ok::<_, worker::Error>(chunk)
        })
        .now_or_never()
        .unwrap()
        .unwrap();
        assert_eq!(ids(&page), ["n4", "n3"]);

        let page = fill_notification_page(newest_first(5), 2, true, |chunk| async move {
            Ok::<_, worker::Error>(chunk)
        })
        .now_or_never()
        .unwrap()
        .unwrap();
        assert_eq!(ids(&page), ["n1", "n0"]);
    }

    #[test]
    fn page_renders_more_chunks_when_rendering_drops_candidates() {
        let calls = AtomicUsize::new(0);
        // Every candidate but the oldest three fails to render.
        let page = fill_notification_page(newest_first(40), 3, false, |chunk| {
            calls.fetch_add(1, Ordering::Relaxed);
            async move {
                Ok::<_, worker::Error>(
                    chunk
                        .into_iter()
                        .filter(|entry| ["n0", "n1", "n2"].contains(&entry.id.as_str()))
                        .collect::<Vec<_>>(),
                )
            }
        })
        .now_or_never()
        .unwrap()
        .unwrap();
        assert_eq!(ids(&page), ["n2", "n1", "n0"]);
        assert!(calls.load(Ordering::Relaxed) > 1);
    }

    #[test]
    fn page_renders_only_a_margin_beyond_the_limit() {
        let rendered = AtomicUsize::new(0);
        let page = fill_notification_page(newest_first(60), 20, false, |chunk| {
            rendered.fetch_add(chunk.len(), Ordering::Relaxed);
            async move { Ok::<_, worker::Error>(chunk) }
        })
        .now_or_never()
        .unwrap()
        .unwrap();
        assert_eq!(page.len(), 20);
        assert_eq!(rendered.load(Ordering::Relaxed), 28);
    }

    #[test]
    fn dismissed_and_cleared_candidates_are_dropped() {
        let mut entries = vec![
            entry("kept", "2026-10-07T06:00:00.000Z"),
            entry("dismissed", "2026-10-07T06:00:00.000Z"),
            entry("cleared", "2026-10-07T04:00:00.000Z"),
        ];
        let dismissed = HashSet::from(["dismissed".to_owned()]);
        retain_undismissed_candidates(&mut entries, &dismissed, Some("2026-10-07 05:00:00"));
        assert_eq!(ids(&entries), ["kept"]);
    }

    #[test]
    fn candidates_in_one_millisecond_sort_by_api_id() {
        let mut entries = vec![
            entry("a", "2026-10-07T05:00:00.000Z"),
            entry("b", "2026-10-07T05:00:00.000Z"),
            entry("c", "2026-10-07T05:00:00.000Z"),
        ];
        sort_candidates_newest_first(&mut entries);
        let api_ids = entries
            .iter()
            .map(notification_api_numeric_id)
            .collect::<Vec<_>>();
        assert!(api_ids.windows(2).all(|pair| pair[0] > pair[1]));
    }
}
