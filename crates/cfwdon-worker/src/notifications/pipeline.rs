//! Notifications are listed in two phases. Collectors first return light
//! candidates (internal key, creation time, what to render); the listing
//! filters, orders and cuts those, and only the candidates a response needs
//! are rendered, with one shared preload.

use super::{
    NotificationEntry, NotificationIdentity, NotificationsQuery, collect_notifications,
    load_dismissed_notification_ids, load_notification_clear_marker, notification_api_numeric_id,
    notification_entry_matches_cursor_id, notification_group_key_from_parts,
    notification_timestamp_sort_token,
};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use std::collections::HashSet;
use std::future::Future;
use worker::Result;

#[derive(Debug, Clone)]
pub(crate) enum NotificationCandidateSource {
    /// Rendered by its collector already: admin and collection notifications,
    /// which are rare, and kinds not yet split into two phases.
    Ready(NotificationEntry),
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
            source: NotificationCandidateSource::Ready(entry),
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
    let (entries, dismissed_ids, cleared_at) = futures_util::try_join!(
        collect_notifications(db, config, viewer, query, per_type_limit),
        load_dismissed_notification_ids(db, viewer.id()),
        load_notification_clear_marker(db, viewer.id()),
    )?;
    let mut candidates = entries
        .into_iter()
        .map(NotificationCandidate::ready)
        .collect::<Vec<_>>();
    retain_undismissed_candidates(&mut candidates, &dismissed_ids, cleared_at.as_deref());
    sort_candidates_newest_first(&mut candidates);
    Ok(candidates)
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

/// Phase two: render `candidates` in order. Candidates whose rows have gone
/// since phase one are dropped.
pub(crate) async fn hydrate_notification_candidates(
    _db: &D1Database,
    _config: &AppConfig,
    _viewer: &LocalAccount,
    candidates: Vec<NotificationCandidate>,
) -> Result<Vec<NotificationEntry>> {
    Ok(candidates
        .into_iter()
        .map(|candidate| match candidate.source {
            NotificationCandidateSource::Ready(entry) => entry,
        })
        .collect())
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
        fill_notification_page, retain_undismissed_candidates, sort_candidates_newest_first,
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
