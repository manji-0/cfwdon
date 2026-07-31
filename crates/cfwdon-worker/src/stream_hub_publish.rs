use crate::{
    AppConfig, D1Database, Env, STREAM_HUB_FOLLOWER_FANOUT_LIMIT, STREAM_HUB_LIST_FANOUT_LIMIT,
    StatusRow, extract_hashtags_from_html, extract_hashtags_from_text, extract_mentions_from_text,
    find_account_by_id, find_account_by_username, list_local_account_list_stream_fanout,
    list_local_follower_account_ids_for_stream_fanout, list_membership_variants_for_local_account,
    local_status_visible_on_list_timeline, publish_stream_hub_event_soft,
    publish_user_stream_hub_event_soft, stream_hub_id_name,
};
use cfwdon_domain::Visibility;
use std::collections::HashSet;
use worker::console_error;

const HASHTAG_STREAMS: [&str; 2] = ["hashtag", "hashtag:local"];

fn status_hashtag_tags(status: &StatusRow) -> Vec<String> {
    let mut tags = extract_hashtags_from_text(&status.text);
    for tag in extract_hashtags_from_html(&status.content_html) {
        if !tags.iter().any(|existing| existing == &tag) {
            tags.push(tag);
        }
    }
    tags
}

fn public_timeline_streams(has_media: bool) -> Vec<&'static str> {
    let mut streams = vec!["public", "public:local"];
    if has_media {
        streams.push("public:media");
        streams.push("public:local:media");
    }
    streams
}

fn visibility_reaches_follower_home_timelines(visibility: Visibility) -> bool {
    matches!(
        visibility,
        Visibility::Public | Visibility::Unlisted | Visibility::FollowersOnly
    )
}

async fn publish_to_hub_streams_soft(
    env: &Env,
    binding: &str,
    hub_name: &str,
    streams: &[&str],
    account_id: Option<&str>,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    for stream in streams {
        publish_stream_hub_event_soft(
            env, binding, hub_name, stream, account_id, event, payload, event_id,
        )
        .await;
    }
}

async fn publish_follower_home_timeline_events_soft(
    env: &Env,
    db: &D1Database,
    binding: &str,
    author_account_id: &str,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    let fanout = match list_local_follower_account_ids_for_stream_fanout(db, author_account_id)
        .await
    {
        Ok(fanout) => fanout,
        Err(error) => {
            console_error!(
                "failed to list local followers for stream fan-out (author {author_account_id}): {error}"
            );
            return;
        }
    };

    if fanout.truncated {
        console_error!(
            "stream hub follower fan-out truncated to {} for author {}",
            STREAM_HUB_FOLLOWER_FANOUT_LIMIT,
            author_account_id
        );
    }

    for follower_id in fanout.account_ids {
        if follower_id == author_account_id {
            continue;
        }
        publish_user_stream_hub_event_soft(env, binding, &follower_id, event, payload, event_id)
            .await;
    }
}

async fn publish_public_timeline_events_soft(
    env: &Env,
    binding: &str,
    has_media: bool,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    for stream in public_timeline_streams(has_media) {
        let hub_name = stream_hub_id_name(stream, None, None, None);
        publish_stream_hub_event_soft(
            env, binding, &hub_name, stream, None, event, payload, event_id,
        )
        .await;
    }
}

async fn publish_hashtag_timeline_events_soft(
    env: &Env,
    binding: &str,
    tags: &[String],
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    for tag in tags {
        let hub_name = stream_hub_id_name("hashtag", None, Some(tag), None);
        publish_to_hub_streams_soft(
            env,
            binding,
            &hub_name,
            &HASHTAG_STREAMS,
            None,
            event,
            payload,
            event_id,
        )
        .await;
    }
}

async fn publish_list_timeline_events_soft(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    binding: &str,
    author_account_id: &str,
    status: &StatusRow,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    if status.visibility != Visibility::Public {
        return;
    }

    let author = match find_account_by_id(db, author_account_id).await {
        Ok(Some(author)) => author,
        Ok(None) => return,
        Err(error) => {
            console_error!(
                "failed to load author for list stream fan-out (author {author_account_id}): {error}"
            );
            return;
        }
    };

    let membership_refs = list_membership_variants_for_local_account(&author, config);
    let fanout = match list_local_account_list_stream_fanout(db, &membership_refs).await {
        Ok(fanout) => fanout,
        Err(error) => {
            console_error!(
                "failed to list memberships for list stream fan-out (author {author_account_id}): {error}"
            );
            return;
        }
    };

    if fanout.truncated {
        console_error!(
            "stream hub list fan-out truncated to {} for author {}",
            STREAM_HUB_LIST_FANOUT_LIMIT,
            author_account_id
        );
    }

    for list in fanout.lists {
        if !local_status_visible_on_list_timeline(
            status.visibility,
            &list.replies_policy,
            status.in_reply_to_id.as_deref(),
        ) {
            continue;
        }
        let hub_name = stream_hub_id_name("list", None, None, Some(&list.list_id));
        publish_stream_hub_event_soft(
            env, binding, &hub_name, "list", None, event, payload, event_id,
        )
        .await;
    }
}

async fn publish_direct_timeline_events_soft(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    binding: &str,
    author_account_id: &str,
    status: &StatusRow,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    let mut recipient_ids = HashSet::new();
    recipient_ids.insert(author_account_id.to_owned());

    for handle in extract_mentions_from_text(&status.text, config) {
        if let Ok(Some(account)) = find_account_by_username(db, &handle.username).await {
            recipient_ids.insert(account.id().to_owned());
        }
    }

    for recipient_id in recipient_ids {
        let hub_name = stream_hub_id_name("direct", Some(&recipient_id), None, None);
        publish_stream_hub_event_soft(
            env,
            binding,
            &hub_name,
            "direct",
            Some(&recipient_id),
            event,
            payload,
            event_id,
        )
        .await;
    }
}

pub(crate) async fn publish_local_status_create_stream_fanout_soft(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    author_account_id: &str,
    status: &StatusRow,
    payload: &str,
    has_media: bool,
) {
    let tags = status_hashtag_tags(status);
    let event_id = Some(status.id.as_str());

    if visibility_reaches_follower_home_timelines(status.visibility) {
        publish_follower_home_timeline_events_soft(
            env,
            db,
            &config.stream_hub_binding,
            author_account_id,
            "update",
            payload,
            event_id,
        )
        .await;
    }

    if status.visibility == Visibility::Public {
        publish_public_timeline_events_soft(
            env,
            &config.stream_hub_binding,
            has_media,
            "update",
            payload,
            event_id,
        )
        .await;

        if !tags.is_empty() {
            publish_hashtag_timeline_events_soft(
                env,
                &config.stream_hub_binding,
                &tags,
                "update",
                payload,
                event_id,
            )
            .await;
        }

        publish_list_timeline_events_soft(
            env,
            db,
            config,
            &config.stream_hub_binding,
            author_account_id,
            status,
            "update",
            payload,
            event_id,
        )
        .await;
    }

    if status.visibility == Visibility::Direct {
        publish_direct_timeline_events_soft(
            env,
            db,
            config,
            &config.stream_hub_binding,
            author_account_id,
            status,
            "update",
            payload,
            event_id,
        )
        .await;
    }
}

pub(crate) async fn publish_announcement_reaction_user_stream_soft(
    env: &Env,
    binding: &str,
    account_id: &str,
    announcement_id: &str,
    reaction_name: &str,
    count: u64,
) {
    let payload = serde_json::json!({
        "name": reaction_name,
        "count": count,
        "announcement_id": announcement_id,
    })
    .to_string();
    let event_id = format!("{announcement_id}:{reaction_name}");
    publish_user_stream_hub_event_soft(
        env,
        binding,
        account_id,
        "announcement.reaction",
        &payload,
        Some(&event_id),
    )
    .await;
}

pub(crate) async fn publish_announcement_user_stream_soft(
    env: &Env,
    binding: &str,
    account_id: &str,
    announcement_id: &str,
    payload: &str,
) {
    publish_user_stream_hub_event_soft(
        env,
        binding,
        account_id,
        "announcement",
        payload,
        Some(announcement_id),
    )
    .await;
}

pub(crate) async fn publish_local_status_delete_stream_fanout_soft(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    author_account_id: &str,
    status: &StatusRow,
    has_media: bool,
) {
    let tags = status_hashtag_tags(status);
    let payload = status.id.as_str();
    let event_id = Some(status.id.as_str());

    if visibility_reaches_follower_home_timelines(status.visibility) {
        publish_follower_home_timeline_events_soft(
            env,
            db,
            &config.stream_hub_binding,
            author_account_id,
            "delete",
            payload,
            event_id,
        )
        .await;
    }

    if status.visibility == Visibility::Public {
        publish_public_timeline_events_soft(
            env,
            &config.stream_hub_binding,
            has_media,
            "delete",
            payload,
            event_id,
        )
        .await;

        if !tags.is_empty() {
            publish_hashtag_timeline_events_soft(
                env,
                &config.stream_hub_binding,
                &tags,
                "delete",
                payload,
                event_id,
            )
            .await;
        }

        publish_list_timeline_events_soft(
            env,
            db,
            config,
            &config.stream_hub_binding,
            author_account_id,
            status,
            "delete",
            payload,
            event_id,
        )
        .await;
    }

    if status.visibility == Visibility::Direct {
        publish_direct_timeline_events_soft(
            env,
            db,
            config,
            &config.stream_hub_binding,
            author_account_id,
            status,
            "delete",
            payload,
            event_id,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_timeline_streams_include_media_variants_when_needed() {
        assert_eq!(
            public_timeline_streams(false),
            vec!["public", "public:local"]
        );
        assert_eq!(
            public_timeline_streams(true),
            vec![
                "public",
                "public:local",
                "public:media",
                "public:local:media"
            ]
        );
    }

    #[test]
    fn follower_home_visibility_includes_public_unlisted_and_private() {
        assert!(visibility_reaches_follower_home_timelines(
            Visibility::Public
        ));
        assert!(visibility_reaches_follower_home_timelines(
            Visibility::Unlisted
        ));
        assert!(visibility_reaches_follower_home_timelines(
            Visibility::FollowersOnly
        ));
        assert!(!visibility_reaches_follower_home_timelines(
            Visibility::Direct
        ));
    }

    #[test]
    fn list_timeline_visibility_matches_public_timeline_query() {
        use crate::local_status_visible_on_list_timeline;

        assert!(local_status_visible_on_list_timeline(
            Visibility::Public,
            "list",
            None
        ));
        assert!(!local_status_visible_on_list_timeline(
            Visibility::Unlisted,
            "list",
            None
        ));
        assert!(!local_status_visible_on_list_timeline(
            Visibility::FollowersOnly,
            "list",
            None
        ));
        assert!(!local_status_visible_on_list_timeline(
            Visibility::Direct,
            "list",
            None
        ));
        assert!(!local_status_visible_on_list_timeline(
            Visibility::Public,
            "none",
            Some("status-parent")
        ));
        assert!(local_status_visible_on_list_timeline(
            Visibility::Public,
            "none",
            None
        ));
    }
}
