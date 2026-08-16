use super::{
    CollectionNotificationPolicyAction, CollectionNotificationRow, CountRow, collection_document,
    collection_item_document, collection_row_by_id, list_collection_items,
};
use crate::notifications::{MastodonNotificationResponse, NotificationEntry};
use crate::{
    AccountReference, MastodonAccountResponse, Result, actor_url, find_account_by_id,
    generate_entity_id, is_blocking_actor, load_notification_policy_row,
    muted_notifications_for_actor, notification_account_matches_filter, notification_type_allowed,
    resolve_account_reference, timestamp_to_mastodon_iso8601,
};
use worker::d1::D1Type;

pub(in crate::collections_alpha) fn merge_collection_notification_policy_action(
    current: CollectionNotificationPolicyAction,
    policy_value: &str,
    condition_matches: bool,
) -> CollectionNotificationPolicyAction {
    if !condition_matches || current == CollectionNotificationPolicyAction::Drop {
        return current;
    }
    match policy_value {
        "drop" => CollectionNotificationPolicyAction::Drop,
        "filter" => CollectionNotificationPolicyAction::Filter,
        _ => current,
    }
}

async fn accepted_follow_exists(
    db: &crate::D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
               AND state = 'accepted'
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.is_some())
}

async fn recent_accepted_follow_exists(
    db: &crate::D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
    threshold: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
        D1Type::Text(threshold),
    ];
    let row = db
        .prepare(
            "SELECT 1 AS found
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
               AND state = 'accepted'
               AND datetime(replace(replace(created_at, 'T', ' '), 'Z', '')) > datetime(CURRENT_TIMESTAMP, ?3)
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.is_some())
}

async fn timestamp_is_after_current_timestamp_modifier(
    db: &crate::D1Database,
    timestamp: &str,
    modifier: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(timestamp), D1Type::Text(modifier)];
    let row = db
        .prepare(
            "SELECT 1 AS found
             WHERE datetime(replace(replace(?1, 'T', ' '), 'Z', '')) > datetime(CURRENT_TIMESTAMP, ?2)",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.is_some())
}

async fn collection_notification_filtered(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    recipient: &cfwdon_domain::LocalAccount,
    sender: &cfwdon_domain::LocalAccount,
    notification_type: &str,
) -> Result<Option<bool>> {
    let sender_actor_uri = actor_url(config, sender.username());
    if is_blocking_actor(db, recipient.id(), &sender_actor_uri).await?
        || muted_notifications_for_actor(db, recipient.id(), &sender_actor_uri).await?
    {
        return Ok(None);
    }
    if notification_type != "added_to_collection" {
        return Ok(Some(false));
    }

    let policy = load_notification_policy_row(db, recipient.id()).await?;
    let recipient_follows_sender =
        accepted_follow_exists(db, recipient.id(), &sender_actor_uri).await?;
    let recipient_actor_uri = actor_url(config, recipient.username());
    let sender_follows_recipient =
        accepted_follow_exists(db, sender.id(), &recipient_actor_uri).await?;
    let sender_is_new_follower =
        recent_accepted_follow_exists(db, sender.id(), &recipient_actor_uri, "-3 days").await?;
    let sender_is_new_account =
        timestamp_is_after_current_timestamp_modifier(db, sender.created_at(), "-30 days").await?;

    let mut action = CollectionNotificationPolicyAction::Deliver;
    action = merge_collection_notification_policy_action(
        action,
        &policy.for_not_following,
        !recipient_follows_sender,
    );
    action = merge_collection_notification_policy_action(
        action,
        &policy.for_not_followers,
        !sender_follows_recipient || sender_is_new_follower,
    );
    action = merge_collection_notification_policy_action(
        action,
        &policy.for_new_accounts,
        sender_is_new_account && !recipient_follows_sender,
    );

    Ok(match action {
        CollectionNotificationPolicyAction::Deliver => Some(false),
        CollectionNotificationPolicyAction::Filter => Some(true),
        CollectionNotificationPolicyAction::Drop => None,
    })
}

async fn insert_collection_notification(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    recipient: &cfwdon_domain::LocalAccount,
    sender: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    collection_item_id: Option<&str>,
    notification_type: &str,
) -> Result<()> {
    let Some(filtered) =
        collection_notification_filtered(db, config, recipient, sender, notification_type).await?
    else {
        return Ok(());
    };
    let notification_id = generate_entity_id(16)?;
    let collection_item_key = collection_item_id.unwrap_or("");
    let bindings = [
        D1Type::Text(notification_id.as_str()),
        D1Type::Text(recipient.id()),
        D1Type::Text(sender.id()),
        D1Type::Text(collection_id),
        collection_item_id.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(collection_item_key),
        D1Type::Text(notification_type),
        D1Type::Integer(if filtered { 1 } else { 0 }),
    ];
    db.prepare(
        "INSERT INTO collection_notifications (
            id,
            account_id,
            from_account_id,
            collection_id,
            collection_item_id,
            collection_item_key,
            notification_type,
            filtered
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8
        )
        ON CONFLICT(
            account_id,
            notification_type,
            collection_id,
            collection_item_key
        ) DO UPDATE SET
            filtered = excluded.filtered,
            created_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(in crate::collections_alpha) async fn insert_added_to_collection_notification(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    target: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item_id: &str,
) -> Result<()> {
    if owner.id() == target.id() {
        return Ok(());
    }
    insert_collection_notification(
        db,
        config,
        target,
        owner,
        collection_id,
        Some(item_id),
        "added_to_collection",
    )
    .await
}

pub(in crate::collections_alpha) async fn insert_collection_update_notifications(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
) -> Result<()> {
    for item in list_collection_items(db, collection_id, false).await? {
        let Some(AccountReference::Local(target)) =
            resolve_account_reference(db, &item.target_account_ref).await?
        else {
            continue;
        };
        if target.id() == owner.id() {
            continue;
        }
        insert_collection_notification(
            db,
            config,
            &target,
            owner,
            collection_id,
            None,
            "collection_update",
        )
        .await?;
    }
    Ok(())
}

async fn list_collection_notifications_for_account(
    db: &crate::D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<CollectionNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT id,
                    from_account_id,
                    collection_id,
                    collection_item_id,
                    notification_type,
                    filtered,
                    created_at
             FROM collection_notifications
             WHERE account_id = ?1
             ORDER BY created_at DESC, id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<CollectionNotificationRow>(&result)
}

pub(crate) async fn collect_collection_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    query: &crate::NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    for notification in
        list_collection_notifications_for_account(db, viewer.id(), per_type_limit).await?
    {
        if !notification_type_allowed(query, &notification.notification_type) {
            continue;
        }
        if notification.filtered != 0 && query.account_id.is_none() {
            continue;
        }
        let Some(owner) = find_account_by_id(db, &notification.from_account_id).await? else {
            continue;
        };
        if muted_notifications_for_actor(db, viewer.id(), &actor_url(config, owner.username()))
            .await?
            || !notification_account_matches_filter(query.account_id.as_deref(), owner.id(), None)
        {
            continue;
        }
        let Some(collection) = collection_row_by_id(db, &notification.collection_id).await? else {
            continue;
        };
        let items = list_collection_items(db, &collection.id, false)
            .await?
            .iter()
            .map(collection_item_document)
            .collect::<Vec<_>>();
        let created_at = timestamp_to_mastodon_iso8601(&notification.created_at);
        let mut value = serde_json::to_value(MastodonNotificationResponse {
            id: notification.id.clone(),
            notification_type: notification.notification_type.clone(),
            group_key: format!(
                "{}-{}",
                notification.notification_type, notification.collection_id
            ),
            created_at: created_at.clone(),
            account: MastodonAccountResponse::from_account(&owner, config),
            status: None,
            report: None,
        })?;
        value["collection"] = collection_document(config, &owner, &collection, items);
        if let Some(item_id) = notification.collection_item_id.as_deref() {
            value["collection_item_id"] = serde_json::json!(item_id);
        }
        entries.push(NotificationEntry {
            id: notification.id,
            created_at,
            value,
        });
    }
    Ok(())
}
