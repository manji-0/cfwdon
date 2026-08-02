use std::collections::{HashMap, HashSet};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct StreamingEvent {
    pub(crate) created_at: String,
    pub(crate) id: String,
    pub(crate) event: &'static str,
    pub(crate) data: String,
}

#[derive(Debug)]
pub(crate) struct StreamingBatch {
    pub(crate) events: Vec<StreamingEvent>,
    pub(crate) tracked_status_ids: Vec<String>,
    pub(crate) last_id: Option<String>,
    pub(crate) last_created_at: Option<String>,
}

impl StreamingBatch {
    pub(crate) fn empty() -> Self {
        Self {
            events: Vec::new(),
            tracked_status_ids: Vec::new(),
            last_id: None,
            last_created_at: None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct StreamingEntry {
    created_at: String,
    id: String,
    data: String,
}

impl StreamingEntry {
    pub(crate) fn new(created_at: String, id: String, data: String) -> Self {
        Self {
            created_at,
            id,
            data,
        }
    }
}

pub(crate) fn streaming_batch_from_entries(
    mut entries: Vec<StreamingEntry>,
    tracked_status_ids: Vec<String>,
    event: &'static str,
) -> StreamingBatch {
    entries.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let last_id = entries.last().map(|entry| entry.id.clone());
    let last_created_at = entries.last().map(|entry| entry.created_at.clone());
    let events = entries
        .into_iter()
        .map(|entry| StreamingEvent {
            created_at: entry.created_at,
            id: entry.id,
            event,
            data: entry.data,
        })
        .collect::<Vec<_>>();

    StreamingBatch {
        events,
        tracked_status_ids,
        last_id,
        last_created_at,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamingPublicPlan {
    pub(crate) include_local: bool,
    pub(crate) include_remote: bool,
    pub(crate) only_media: bool,
    pub(crate) hashtag_stream: bool,
}

impl StreamingPublicPlan {
    pub(crate) fn from_stream(stream: &str) -> Self {
        let hashtag_stream = stream.starts_with("hashtag");
        Self {
            include_local: matches!(
                stream,
                "public"
                    | "public:media"
                    | "public:local"
                    | "public:local:media"
                    | "hashtag"
                    | "hashtag:local"
            ) || hashtag_stream,
            include_remote: matches!(
                stream,
                "public" | "public:media" | "public:remote" | "public:remote:media" | "hashtag"
            ),
            only_media: stream.ends_with(":media"),
            hashtag_stream,
        }
    }
}

pub(crate) struct StreamingLoopState {
    pub(crate) since_id: Option<String>,
    pub(crate) notification_min_created_at: Option<String>,
    pub(crate) tracked_status_ids: Vec<String>,
    pub(crate) tracked_status_id_set: HashSet<String>,
    pub(crate) deleted_status_ids: HashSet<String>,
    pub(crate) updated_status_ids: HashSet<String>,
    pub(crate) emitted_event_ids: HashSet<String>,
    pub(crate) last_filter_updated_at: Option<String>,
    pub(crate) last_announcements: HashMap<String, String>,
    pub(crate) last_announcement_reactions: HashMap<(String, String), (u64, bool)>,
    pub(crate) initialized: bool,
}

impl StreamingLoopState {
    pub(crate) fn new() -> Self {
        Self {
            since_id: None,
            notification_min_created_at: None,
            tracked_status_ids: Vec::new(),
            tracked_status_id_set: HashSet::new(),
            deleted_status_ids: HashSet::new(),
            updated_status_ids: HashSet::new(),
            emitted_event_ids: HashSet::new(),
            last_filter_updated_at: None,
            last_announcements: HashMap::new(),
            last_announcement_reactions: HashMap::new(),
            initialized: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{StreamingEntry, StreamingPublicPlan, streaming_batch_from_entries};

    #[test]
    fn streaming_batch_from_entries_orders_and_tracks_cursor() {
        let batch = streaming_batch_from_entries(
            vec![
                StreamingEntry::new(
                    "2025-01-02T00:00:00Z".to_owned(),
                    "b".to_owned(),
                    "2".to_owned(),
                ),
                StreamingEntry::new(
                    "2025-01-01T00:00:00Z".to_owned(),
                    "a".to_owned(),
                    "1".to_owned(),
                ),
            ],
            vec!["a".to_owned(), "b".to_owned()],
            "update",
        );

        assert_eq!(batch.events[0].id, "a");
        assert_eq!(batch.events[1].id, "b");
        assert_eq!(batch.last_id.as_deref(), Some("b"));
        assert_eq!(
            batch.last_created_at.as_deref(),
            Some("2025-01-02T00:00:00Z")
        );
        assert_eq!(batch.tracked_status_ids, vec!["a", "b"]);
    }

    #[test]
    fn streaming_batch_from_entries_handles_empty_input() {
        let batch = streaming_batch_from_entries(Vec::new(), Vec::new(), "update");

        assert!(batch.events.is_empty());
        assert!(batch.tracked_status_ids.is_empty());
        assert!(batch.last_id.is_none());
        assert!(batch.last_created_at.is_none());
    }

    #[test]
    fn streaming_batch_from_entries_breaks_ties_by_id() {
        let batch = streaming_batch_from_entries(
            vec![
                StreamingEntry::new(
                    "2025-01-01T00:00:00Z".to_owned(),
                    "b".to_owned(),
                    "2".to_owned(),
                ),
                StreamingEntry::new(
                    "2025-01-01T00:00:00Z".to_owned(),
                    "a".to_owned(),
                    "1".to_owned(),
                ),
            ],
            Vec::new(),
            "update",
        );

        assert_eq!(batch.events[0].id, "a");
        assert_eq!(batch.events[1].id, "b");
        assert_eq!(batch.last_id.as_deref(), Some("b"));
        assert_eq!(
            batch.last_created_at.as_deref(),
            Some("2025-01-01T00:00:00Z")
        );
    }

    #[test]
    fn streaming_public_plan_classifies_public_and_hashtag_streams() {
        assert_eq!(
            StreamingPublicPlan::from_stream("public"),
            StreamingPublicPlan {
                include_local: true,
                include_remote: true,
                only_media: false,
                hashtag_stream: false,
            }
        );
        assert_eq!(
            StreamingPublicPlan::from_stream("public:local:media"),
            StreamingPublicPlan {
                include_local: true,
                include_remote: false,
                only_media: true,
                hashtag_stream: false,
            }
        );
        assert_eq!(
            StreamingPublicPlan::from_stream("hashtag:local"),
            StreamingPublicPlan {
                include_local: true,
                include_remote: false,
                only_media: false,
                hashtag_stream: true,
            }
        );
    }
}
