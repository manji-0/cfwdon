use super::{
    MentionNotificationRow, NotificationCandidate, NotificationsQuery,
    RemoteMentionNotificationRow, list_local_mention_notifications_for_account,
    list_remote_mention_notifications_for_account, notification_time_window,
    notification_type_allowed,
};
use crate::activitypub::is_public_activitypub_visibility;
use crate::remote::remote_status_from_record;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    LocalAccount, LocalStatus, QuoteState, RemoteStatus, RemoteStatusRecord, Visibility,
};
use worker::Result;
fn local_mention_status_row(mention: MentionNotificationRow) -> Option<LocalStatus> {
    let visibility = Visibility::parse(&mention.visibility).ok()?;
    let quote_state = QuoteState::parse(&mention.quote_state).ok()?;
    Some(LocalStatus {
        id: mention.id,
        account_id: mention.account_id.clone(),
        ap_id: mention.ap_id,
        in_reply_to_id: mention.in_reply_to_id,
        in_reply_to_account_id: mention.in_reply_to_account_id,
        boost_of_uri: None,
        quote_of_uri: mention.quote_of_uri,
        content_html: mention.content_html,
        text: mention.text_content,
        spoiler_text: mention.spoiler_text,
        visibility,
        sensitive: mention.sensitive != 0,
        language: mention.language,
        quote_approval_policy: None,
        quote_state,
        application_id: None,
        card_json: None,
        created_at: mention.created_at.clone(),
        updated_at: None,
    })
}

fn remote_mention_status_row(mention: RemoteMentionNotificationRow) -> Option<RemoteStatus> {
    remote_status_from_record(RemoteStatusRecord {
        id: mention.id,
        actor_uri: mention.actor_uri.clone(),
        object_uri: mention.object_uri,
        url: mention.url,
        in_reply_to_uri: mention.in_reply_to_uri,
        boost_of_uri: mention.boost_of_uri,
        quote_of_uri: mention.quote_of_uri,
        content_html: mention.content_html,
        text_content: mention.text_content,
        spoiler_text: mention.spoiler_text,
        visibility: mention.visibility,
        sensitive: mention.sensitive,
        language: mention.language,
        quote_state: mention.quote_state,
        published_at: mention.published_at.clone(),
        edited_at: mention.edited_at,
        card_json: mention.card_json,
        federated_emojis_json: mention.federated_emojis_json,
        in_reply_to_id: mention.in_reply_to_id,
        favourites_count: None,
        reblogs_count: None,
        replies_count: None,
    })
    .ok()
}

pub(crate) async fn collect_mention_notification_candidates(
    candidates: &mut Vec<NotificationCandidate>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "mention") {
        return Ok(());
    }
    let window = notification_time_window(query);
    let (local_mentions, remote_mentions) = futures_util::try_join!(
        list_local_mention_notifications_for_account(db, viewer, config, per_type_limit, &window),
        list_remote_mention_notifications_for_account(db, viewer, per_type_limit, &window),
    )?;
    candidates.extend(
        local_mentions
            .into_iter()
            .filter_map(local_mention_status_row)
            .map(|status| NotificationCandidate::authored_local_status("mention", status)),
    );
    candidates.extend(
        remote_mentions
            .into_iter()
            .filter_map(remote_mention_status_row)
            .filter(|status| {
                is_public_activitypub_visibility(status.visibility.as_str())
                    || status.visibility.as_str() == "direct"
                    || status.visibility.as_str() == "private"
            })
            .map(|status| NotificationCandidate::authored_remote_status("mention", status)),
    );
    Ok(())
}
