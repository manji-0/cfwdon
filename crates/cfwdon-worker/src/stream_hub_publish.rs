use crate::{
    AppConfig, D1Database, Env, RemoteActorProfile, RemoteActorRow, RemoteStatusRow,
    STREAM_HUB_FOLLOWER_FANOUT_LIMIT, STREAM_HUB_LIST_FANOUT_LIMIT, StatusRow,
    build_remote_status_response, conversation_document, extract_hashtags_from_html,
    extract_hashtags_from_text, extract_mentions_from_text, find_account_by_id,
    find_account_by_username, find_conversation_for_account, find_conversation_id_by_status_id,
    find_remote_actor_by_actor_uri, is_muted_actor, list_local_account_list_stream_fanout,
    list_local_follower_account_ids_for_remote_actor_stream_fanout,
    list_local_follower_account_ids_for_stream_fanout, list_membership_variants_for_local_account,
    list_membership_variants_for_remote_actor, load_remote_status_hashtag_names,
    load_remote_status_updated_at, local_status_visible_on_list_timeline,
    publish_stream_hub_event_soft, publish_user_stream_hub_event_soft, remote_status_has_media,
    stream_hub_id_name,
};
use cfwdon_domain::Visibility;
use std::collections::HashSet;
use worker::console_error;

const HASHTAG_STREAMS: [&str; 2] = ["hashtag", "hashtag:local"];
const REMOTE_HASHTAG_STREAMS: [&str; 1] = ["hashtag"];

fn status_hashtag_tags(status: &StatusRow) -> Vec<String> {
    let mut tags = extract_hashtags_from_text(&status.text);
    for tag in extract_hashtags_from_html(&status.content_html) {
        if !tags.iter().any(|existing| existing == &tag) {
            tags.push(tag);
        }
    }
    tags
}

async fn remote_status_hashtag_tags(db: &D1Database, status: &RemoteStatusRow) -> Vec<String> {
    let mut tags = extract_hashtags_from_html(&status.content_html);
    if let Ok(stored) = load_remote_status_hashtag_names(db, &status.id).await {
        for tag in stored {
            if !tags.iter().any(|existing| existing == &tag) {
                tags.push(tag);
            }
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

fn remote_public_timeline_streams(has_media: bool) -> Vec<&'static str> {
    let mut streams = vec!["public", "public:remote"];
    if has_media {
        streams.push("public:media");
        streams.push("public:remote:media");
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

async fn publish_remote_public_timeline_events_soft(
    env: &Env,
    binding: &str,
    has_media: bool,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    for stream in remote_public_timeline_streams(has_media) {
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

async fn publish_remote_hashtag_timeline_events_soft(
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
            &REMOTE_HASHTAG_STREAMS,
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

async fn publish_remote_list_timeline_events_soft(
    env: &Env,
    db: &D1Database,
    binding: &str,
    actor_row: &RemoteActorRow,
    remote_status: &RemoteStatusRow,
    event: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    if remote_status.visibility != Visibility::Public {
        return;
    }

    let membership_refs = list_membership_variants_for_remote_actor(actor_row);
    let fanout = match list_local_account_list_stream_fanout(db, &membership_refs).await {
        Ok(fanout) => fanout,
        Err(error) => {
            console_error!(
                "failed to list memberships for remote list stream fan-out ({}): {error}",
                actor_row.actor_uri
            );
            return;
        }
    };

    if fanout.truncated {
        console_error!(
            "stream hub list fan-out truncated to {} for remote actor {}",
            STREAM_HUB_LIST_FANOUT_LIMIT,
            actor_row.actor_uri
        );
    }

    for list in fanout.lists {
        if !local_status_visible_on_list_timeline(
            remote_status.visibility,
            &list.replies_policy,
            remote_status.in_reply_to_uri.as_deref(),
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

async fn publish_remote_follower_home_create_events_soft(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    binding: &str,
    remote_actor: &RemoteActorProfile,
    remote_status: &RemoteStatusRow,
    actor_row: &RemoteActorRow,
) {
    let fanout = match list_local_follower_account_ids_for_remote_actor_stream_fanout(
        db,
        &remote_actor.actor_uri,
    )
    .await
    {
        Ok(fanout) => fanout,
        Err(error) => {
            console_error!(
                "failed to list local followers for remote status stream fan-out ({}): {error}",
                remote_actor.actor_uri
            );
            return;
        }
    };

    if fanout.truncated {
        console_error!(
            "stream hub follower fan-out truncated to {} for remote actor {}",
            STREAM_HUB_FOLLOWER_FANOUT_LIMIT,
            remote_actor.actor_uri
        );
    }

    let event_id = Some(remote_status.id.as_str());
    for follower_id in fanout.account_ids {
        if is_muted_actor(db, &follower_id, &remote_actor.actor_uri)
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let recipient = match find_account_by_id(db, &follower_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => continue,
            Err(error) => {
                console_error!(
                    "failed to load follower {follower_id} for remote status stream fan-out: {error}"
                );
                continue;
            }
        };
        let response = match build_remote_status_response(
            db,
            config,
            Some(&recipient),
            remote_status,
            actor_row,
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                console_error!(
                    "failed to build remote status stream payload for follower {follower_id}: {error}"
                );
                continue;
            }
        };
        let payload = match serde_json::to_string(&response) {
            Ok(payload) => payload,
            Err(error) => {
                console_error!(
                    "failed to serialize remote status stream payload for follower {follower_id}: {error}"
                );
                continue;
            }
        };
        publish_user_stream_hub_event_soft(env, binding, &follower_id, "update", &payload, event_id)
            .await;
    }
}

async fn publish_remote_follower_home_delete_events_soft(
    env: &Env,
    db: &D1Database,
    binding: &str,
    remote_actor_uri: &str,
    payload: &str,
    event_id: Option<&str>,
) {
    let fanout = match list_local_follower_account_ids_for_remote_actor_stream_fanout(
        db,
        remote_actor_uri,
    )
    .await
    {
        Ok(fanout) => fanout,
        Err(error) => {
            console_error!(
                "failed to list local followers for remote status delete stream fan-out ({remote_actor_uri}): {error}"
            );
            return;
        }
    };

    if fanout.truncated {
        console_error!(
            "stream hub follower fan-out truncated to {} for remote actor {}",
            STREAM_HUB_FOLLOWER_FANOUT_LIMIT,
            remote_actor_uri
        );
    }

    for follower_id in fanout.account_ids {
        publish_user_stream_hub_event_soft(env, binding, &follower_id, "delete", payload, event_id)
            .await;
    }
}

async fn build_remote_status_public_stream_payload_soft(
    db: &D1Database,
    config: &AppConfig,
    remote_status: &RemoteStatusRow,
    actor_row: &RemoteActorRow,
) -> Option<String> {
    let response = match build_remote_status_response(db, config, None, remote_status, actor_row)
        .await
    {
        Ok(response) => response,
        Err(error) => {
            console_error!(
                "failed to build remote status public stream payload (status {}): {error}",
                remote_status.id
            );
            return None;
        }
    };
    match serde_json::to_string(&response) {
        Ok(payload) => Some(payload),
        Err(error) => {
            console_error!(
                "failed to serialize remote status public stream payload (status {}): {error}",
                remote_status.id
            );
            None
        }
    }
}

async fn direct_status_recipient_account_ids(
    db: &D1Database,
    config: &AppConfig,
    author_account_id: &str,
    status: &StatusRow,
) -> HashSet<String> {
    let mut recipient_ids = HashSet::new();
    recipient_ids.insert(author_account_id.to_owned());

    for handle in extract_mentions_from_text(&status.text, config) {
        if let Ok(Some(account)) = find_account_by_username(db, &handle.username).await {
            recipient_ids.insert(account.id().to_owned());
        }
    }

    recipient_ids
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
    for recipient_id in
        direct_status_recipient_account_ids(db, config, author_account_id, status).await
    {
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

async fn publish_direct_conversation_stream_events_soft(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    binding: &str,
    author_account_id: &str,
    status: &StatusRow,
) {
    let conversation_id = match find_conversation_id_by_status_id(db, &status.id).await {
        Ok(Some(conversation_id)) => conversation_id,
        Ok(None) => return,
        Err(error) => {
            console_error!(
                "failed to resolve conversation for direct status {} stream fan-out: {error}",
                status.id
            );
            return;
        }
    };

    for recipient_id in
        direct_status_recipient_account_ids(db, config, author_account_id, status).await
    {
        let recipient = match find_account_by_id(db, &recipient_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => continue,
            Err(error) => {
                console_error!(
                    "failed to load direct conversation stream recipient {recipient_id}: {error}"
                );
                continue;
            }
        };
        let conversation = match find_conversation_for_account(db, &recipient_id, &conversation_id)
            .await
        {
            Ok(Some(conversation)) => conversation,
            Ok(None) => continue,
            Err(error) => {
                console_error!(
                    "failed to load conversation {conversation_id} for recipient {recipient_id}: {error}"
                );
                continue;
            }
        };
        let document = match conversation_document(db, config, &recipient, &conversation).await {
            Ok(document) => document,
            Err(error) => {
                console_error!(
                    "failed to build conversation document for recipient {recipient_id}: {error}"
                );
                continue;
            }
        };
        let payload = match serde_json::to_string(&document) {
            Ok(payload) => payload,
            Err(error) => {
                console_error!(
                    "failed to serialize conversation document for recipient {recipient_id}: {error}"
                );
                continue;
            }
        };
        let hub_name = stream_hub_id_name("direct", Some(&recipient_id), None, None);
        publish_stream_hub_event_soft(
            env,
            binding,
            &hub_name,
            "direct",
            Some(&recipient_id),
            "update",
            &payload,
            Some(&conversation.id),
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
        publish_direct_conversation_stream_events_soft(
            env,
            db,
            config,
            &config.stream_hub_binding,
            author_account_id,
            status,
        )
        .await;
    }
}

pub(crate) async fn publish_remote_status_update_user_stream_fanout_soft(
    env: &Env,
    db: &D1Database,
    config: &AppConfig,
    remote_actor: &RemoteActorProfile,
    remote_status: &RemoteStatusRow,
) {
    if !visibility_reaches_follower_home_timelines(remote_status.visibility) {
        return;
    }

    let updated_at = match load_remote_status_updated_at(db, &remote_status.id).await {
        Ok(Some(updated_at)) => updated_at,
        Ok(None) => return,
        Err(error) => {
            console_error!(
                "failed to load remote status updated_at for stream fan-out (status {}): {error}",
                remote_status.id
            );
            return;
        }
    };
    if updated_at == remote_status.published_at {
        return;
    }

    let actor_row = match find_remote_actor_by_actor_uri(db, &remote_actor.actor_uri).await {
        Ok(Some(actor_row)) => actor_row,
        Ok(None) => return,
        Err(error) => {
            console_error!(
                "failed to load remote actor for stream fan-out ({}): {error}",
                remote_actor.actor_uri
            );
            return;
        }
    };

    let fanout = match list_local_follower_account_ids_for_remote_actor_stream_fanout(
        db,
        &remote_actor.actor_uri,
    )
    .await
    {
        Ok(fanout) => fanout,
        Err(error) => {
            console_error!(
                "failed to list local followers for remote status stream fan-out ({}): {error}",
                remote_actor.actor_uri
            );
            return;
        }
    };

    if fanout.truncated {
        console_error!(
            "stream hub follower fan-out truncated to {} for remote actor {}",
            STREAM_HUB_FOLLOWER_FANOUT_LIMIT,
            remote_actor.actor_uri
        );
    }

    for follower_id in fanout.account_ids {
        if is_muted_actor(db, &follower_id, &remote_actor.actor_uri)
            .await
            .unwrap_or(false)
        {
            continue;
        }
        let recipient = match find_account_by_id(db, &follower_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => continue,
            Err(error) => {
                console_error!(
                    "failed to load follower {follower_id} for remote status stream fan-out: {error}"
                );
                continue;
            }
        };
        let response = match build_remote_status_response(
            db,
            config,
            Some(&recipient),
            remote_status,
            &actor_row,
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                console_error!(
                    "failed to build remote status stream payload for follower {follower_id}: {error}"
                );
                continue;
            }
        };
        let payload = match serde_json::to_string(&response) {
            Ok(payload) => payload,
            Err(error) => {
                console_error!(
                    "failed to serialize remote status stream payload for follower {follower_id}: {error}"
                );
                continue;
            }
        };
        publish_user_stream_hub_event_soft(
            env,
            &config.stream_hub_binding,
            &follower_id,
            "status.update",
            &payload,
            Some(&remote_status.id),
        )
        .await;
    }
}

pub(crate) async fn publish_remote_status_create_stream_fanout_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    remote_actor: &RemoteActorProfile,
    remote_status: &RemoteStatusRow,
) {
    let Some(env) = env else {
        return;
    };

    let actor_row = match find_remote_actor_by_actor_uri(db, &remote_actor.actor_uri).await {
        Ok(Some(actor_row)) => actor_row,
        Ok(None) => return,
        Err(error) => {
            console_error!(
                "failed to load remote actor for stream fan-out ({}): {error}",
                remote_actor.actor_uri
            );
            return;
        }
    };

    let has_media = remote_status_has_media(db, &remote_status.id)
        .await
        .unwrap_or(false);
    let tags = remote_status_hashtag_tags(db, remote_status).await;
    let event_id = Some(remote_status.id.as_str());
    let binding = &config.stream_hub_binding;

    if visibility_reaches_follower_home_timelines(remote_status.visibility) {
        publish_remote_follower_home_create_events_soft(
            env,
            db,
            config,
            binding,
            remote_actor,
            remote_status,
            &actor_row,
        )
        .await;
    }

    if remote_status.visibility != Visibility::Public {
        return;
    }

    let payload = match build_remote_status_public_stream_payload_soft(
        db,
        config,
        remote_status,
        &actor_row,
    )
    .await
    {
        Some(payload) => payload,
        None => return,
    };

    publish_remote_public_timeline_events_soft(
        env,
        binding,
        has_media,
        "update",
        &payload,
        event_id,
    )
    .await;

    if !tags.is_empty() {
        publish_remote_hashtag_timeline_events_soft(
            env,
            binding,
            &tags,
            "update",
            &payload,
            event_id,
        )
        .await;
    }

    publish_remote_list_timeline_events_soft(
        env,
        db,
        binding,
        &actor_row,
        remote_status,
        "update",
        &payload,
        event_id,
    )
    .await;
}

pub(crate) async fn publish_remote_status_delete_stream_fanout_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    remote_actor: &RemoteActorProfile,
    remote_status: &RemoteStatusRow,
    hashtag_names: &[String],
    has_media: bool,
) {
    let Some(env) = env else {
        return;
    };

    let payload = remote_status.id.as_str();
    let event_id = Some(remote_status.id.as_str());
    let binding = &config.stream_hub_binding;

    if visibility_reaches_follower_home_timelines(remote_status.visibility) {
        publish_remote_follower_home_delete_events_soft(
            env,
            db,
            binding,
            &remote_actor.actor_uri,
            payload,
            event_id,
        )
        .await;
    }

    if remote_status.visibility != Visibility::Public {
        return;
    }

    publish_remote_public_timeline_events_soft(
        env,
        binding,
        has_media,
        "delete",
        payload,
        event_id,
    )
    .await;

    if !hashtag_names.is_empty() {
        publish_remote_hashtag_timeline_events_soft(
            env,
            binding,
            hashtag_names,
            "delete",
            payload,
            event_id,
        )
        .await;
    }

    let actor_row = match find_remote_actor_by_actor_uri(db, &remote_actor.actor_uri).await {
        Ok(Some(actor_row)) => actor_row,
        Ok(None) => return,
        Err(error) => {
            console_error!(
                "failed to load remote actor for delete stream fan-out ({}): {error}",
                remote_actor.actor_uri
            );
            return;
        }
    };

    publish_remote_list_timeline_events_soft(
        env,
        db,
        binding,
        &actor_row,
        remote_status,
        "delete",
        payload,
        event_id,
    )
    .await;
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
    fn remote_public_timeline_streams_exclude_local_only_hubs() {
        assert_eq!(
            remote_public_timeline_streams(false),
            vec!["public", "public:remote"]
        );
        assert_eq!(
            remote_public_timeline_streams(true),
            vec!["public", "public:remote", "public:media", "public:remote:media"]
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
