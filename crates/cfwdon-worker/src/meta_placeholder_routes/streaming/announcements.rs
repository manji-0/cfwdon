use crate::{
    D1Database, Result, StreamingEvent, StreamingLoopState, build_announcements_document,
    list_announcement_read_ids, load_announcement_reaction_state, now_iso_string,
};
use std::collections::{BTreeMap, HashMap};

pub(super) fn announcement_reaction_entries_for_id(
    state: &HashMap<(String, String), (u64, bool)>,
    announcement_id: &str,
) -> BTreeMap<String, (u64, bool)> {
    state
        .iter()
        .filter(|((id, _), _)| id == announcement_id)
        .map(|((_, name), value)| (name.clone(), *value))
        .collect()
}

pub(super) struct AnnouncementStreamEntry {
    id: String,
    payload: String,
    created_at: String,
}

pub(super) struct CurrentAnnouncementStreamState {
    entries: Vec<AnnouncementStreamEntry>,
    reactions: HashMap<(String, String), (u64, bool)>,
}

pub(super) fn announcement_stream_entries(
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

pub(super) fn announcement_stream_entry(
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

pub(super) async fn append_user_announcement_state_events(
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

pub(super) async fn load_current_announcement_stream_state(
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

pub(super) fn append_current_announcement_stream_entry_events(
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
pub(super) enum AnnouncementStreamEntryAction {
    None,
    Reaction,
    Announcement,
}

pub(super) fn announcement_stream_entry_action(
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

pub(super) fn announcement_stream_event(entry: &AnnouncementStreamEntry) -> StreamingEvent {
    StreamingEvent {
        created_at: entry.created_at.clone(),
        id: entry.id.clone(),
        event: "announcement",
        data: entry.payload.clone(),
    }
}

pub(super) fn removed_announcement_ids(
    previous_announcements: &HashMap<String, String>,
    current_announcements: &HashMap<String, String>,
) -> Vec<String> {
    previous_announcements
        .keys()
        .filter(|id| !current_announcements.contains_key(*id))
        .cloned()
        .collect()
}

pub(super) fn announcement_delete_event(removed_id: String, created_at: String) -> StreamingEvent {
    StreamingEvent {
        created_at,
        id: removed_id.clone(),
        event: "announcement.delete",
        data: removed_id,
    }
}

pub(super) fn append_announcement_reaction_events(
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
