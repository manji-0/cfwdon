use super::{
    CollectionItemRow, CollectionRequest, CollectionRow, CountRow, InCollectionPageEntry,
    MAX_REMOTE_APPROVAL_REVALIDATIONS, RemoteCollectionDraft, RemoteCollectionItemRevalidationRow,
    RemoteCollectionItemRow, RemoteCollectionRow, activitypub_value_id,
};
use crate::{AccountReference, Result, generate_entity_id, remote_account_rest_id};
use worker::d1::D1Type;

pub(in crate::collections_alpha) async fn collection_row_by_id(
    db: &crate::D1Database,
    collection_id: &str,
) -> Result<Option<CollectionRow>> {
    let collection_id = D1Type::Text(collection_id);
    db.prepare(
        "SELECT c.id,
                c.account_id,
                c.name,
                c.description,
                c.language,
                c.sensitive,
                c.discoverable,
                c.tag_name,
                c.created_at,
                c.updated_at
         FROM account_collections c
         WHERE c.id = ?1
         LIMIT 1",
    )
    .bind_refs(&collection_id)?
    .first::<CollectionRow>(None)
    .await
}

pub(in crate::collections_alpha) async fn list_collection_rows_for_account(
    db: &crate::D1Database,
    account_id: &str,
    include_private: bool,
    offset: u32,
    limit: u32,
) -> Result<Vec<CollectionRow>> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Integer(if include_private { 1 } else { 0 }),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
        D1Type::Integer(i32::try_from(offset).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT c.id,
                    c.account_id,
                    c.name,
                    c.description,
                    c.language,
                    c.sensitive,
                    c.discoverable,
                    c.tag_name,
                    c.created_at,
                    c.updated_at
             FROM account_collections c
             WHERE c.account_id = ?1
               AND (?2 = 1 OR c.discoverable = 1)
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT ?3 OFFSET ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<CollectionRow>(&result)
}

pub(in crate::collections_alpha) async fn count_collection_rows_for_account(
    db: &crate::D1Database,
    account_id: &str,
    include_private: bool,
) -> Result<u64> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Integer(if include_private { 1 } else { 0 }),
    ];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM account_collections c
             WHERE c.account_id = ?1
               AND (?2 = 1 OR c.discoverable = 1)",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

pub(in crate::collections_alpha) async fn count_in_collection_rows(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<u64> {
    let bindings = [D1Type::Text(account_id)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM account_collections c
             JOIN account_collection_items target_item
               ON target_item.collection_id = c.id
              AND target_item.target_account_ref = ?1
              AND target_item.state IN ('accepted', 'pending')",
        )
        .bind_refs(bindings.iter())?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

pub(in crate::collections_alpha) async fn remote_collection_row_by_id(
    db: &crate::D1Database,
    collection_id: &str,
) -> Result<Option<RemoteCollectionRow>> {
    let collection_id = D1Type::Text(collection_id);
    db.prepare(
        "SELECT id,
                actor_uri,
                uri,
                name,
                description,
                language,
                sensitive,
                discoverable,
                tag_name,
                url,
                published_at,
                remote_updated_at,
                created_at,
                updated_at
         FROM remote_account_collections
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&collection_id)?
    .first::<RemoteCollectionRow>(None)
    .await
}

pub(in crate::collections_alpha) async fn remote_collection_row_by_uri(
    db: &crate::D1Database,
    collection_uri: &str,
) -> Result<Option<RemoteCollectionRow>> {
    let collection_uri = D1Type::Text(collection_uri);
    db.prepare(
        "SELECT id,
                actor_uri,
                uri,
                name,
                description,
                language,
                sensitive,
                discoverable,
                tag_name,
                url,
                published_at,
                remote_updated_at,
                created_at,
                updated_at
         FROM remote_account_collections
         WHERE uri = ?1
         LIMIT 1",
    )
    .bind_refs(&collection_uri)?
    .first::<RemoteCollectionRow>(None)
    .await
}

pub(in crate::collections_alpha) async fn list_remote_collection_rows_for_actor(
    db: &crate::D1Database,
    actor_uri: &str,
    offset: u32,
    limit: u32,
) -> Result<Vec<RemoteCollectionRow>> {
    let bindings = [
        D1Type::Text(actor_uri),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
        D1Type::Integer(i32::try_from(offset).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    actor_uri,
                    uri,
                    name,
                    description,
                    language,
                    sensitive,
                    discoverable,
                    tag_name,
                    url,
                    published_at,
                    remote_updated_at,
                    created_at,
                    updated_at
             FROM remote_account_collections
             WHERE actor_uri = ?1
               AND discoverable = 1
             ORDER BY created_at DESC, id DESC
             LIMIT ?2 OFFSET ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<RemoteCollectionRow>(&result)
}

pub(in crate::collections_alpha) async fn count_remote_collection_rows_for_actor(
    db: &crate::D1Database,
    actor_uri: &str,
) -> Result<u64> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM remote_account_collections
             WHERE actor_uri = ?1
               AND discoverable = 1",
        )
        .bind_refs(&actor_uri)?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

pub(in crate::collections_alpha) async fn count_remote_in_collection_rows(
    db: &crate::D1Database,
    target_actor_uri: &str,
) -> Result<u64> {
    let target_actor_uri = D1Type::Text(target_actor_uri);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM remote_account_collections c
             JOIN remote_account_collection_items target_item
               ON target_item.collection_id = c.id
              AND target_item.target_actor_uri = ?1
              AND target_item.state IN ('accepted', 'pending')",
        )
        .bind_refs(&target_actor_uri)?
        .first::<CountRow>(None)
        .await?;
    Ok(row.map(|row| row.count).unwrap_or(0))
}

pub(in crate::collections_alpha) async fn list_collection_items(
    db: &crate::D1Database,
    collection_id: &str,
    include_pending: bool,
) -> Result<Vec<CollectionItemRow>> {
    let bindings = [
        D1Type::Text(collection_id),
        D1Type::Integer(if include_pending { 1 } else { 0 }),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    target_account_ref,
                    state,
                    activity_uri,
                    feature_authorization,
                    created_at
             FROM account_collection_items
             WHERE collection_id = ?1
               AND state IN ('accepted', CASE WHEN ?2 = 1 THEN 'pending' ELSE 'accepted' END)
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<CollectionItemRow>(&result)
}

pub(in crate::collections_alpha) async fn list_remote_collection_items(
    db: &crate::D1Database,
    collection_id: &str,
    include_pending: bool,
) -> Result<Vec<RemoteCollectionItemRow>> {
    let bindings = [
        D1Type::Text(collection_id),
        D1Type::Integer(if include_pending { 1 } else { 0 }),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    uri,
                    target_actor_uri,
                    state,
                    feature_authorization,
                    approval_last_verified_at,
                    published_at,
                    created_at
             FROM remote_account_collection_items
             WHERE collection_id = ?1
               AND state IN ('accepted', CASE WHEN ?2 = 1 THEN 'pending' ELSE 'accepted' END)
             ORDER BY created_at ASC, id ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<RemoteCollectionItemRow>(&result)
}

pub(in crate::collections_alpha) async fn remote_collection_item_by_id(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<Option<RemoteCollectionItemRow>> {
    db.prepare(
        "SELECT id,
                uri,
                target_actor_uri,
                state,
                feature_authorization,
                approval_last_verified_at,
                published_at,
                created_at
         FROM remote_account_collection_items
         WHERE collection_id = ?1
           AND id = ?2
         LIMIT 1",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .first::<RemoteCollectionItemRow>(None)
    .await
}

pub(in crate::collections_alpha) async fn list_remote_collection_items_due_for_approval_revalidation(
    db: &crate::D1Database,
    collection_id: &str,
) -> Result<Vec<RemoteCollectionItemRow>> {
    let bindings = [
        D1Type::Text(collection_id),
        D1Type::Integer(MAX_REMOTE_APPROVAL_REVALIDATIONS),
    ];
    let result = db
        .prepare(
            "SELECT id,
                    uri,
                    target_actor_uri,
                    state,
                    feature_authorization,
                    approval_last_verified_at,
                    published_at,
                    created_at
             FROM remote_account_collection_items
             WHERE collection_id = ?1
               AND state = 'accepted'
               AND feature_authorization IS NOT NULL
               AND (
                    approval_last_verified_at IS NULL
                    OR approval_last_verified_at <= datetime('now', '-1 day')
               )
             ORDER BY COALESCE(approval_last_verified_at, created_at) ASC, id ASC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<RemoteCollectionItemRow>(&result)
}

pub(in crate::collections_alpha) async fn list_stale_remote_collection_items_for_approval_revalidation(
    db: &crate::D1Database,
    limit: i32,
) -> Result<Vec<RemoteCollectionItemRevalidationRow>> {
    let result = db
        .prepare(
            "SELECT item.collection_id AS collection_id,
                    collection.uri AS collection_uri,
                    item.target_actor_uri AS target_actor_uri,
                    item.feature_authorization AS feature_authorization
             FROM remote_account_collection_items item
             JOIN remote_account_collections collection
               ON collection.id = item.collection_id
             WHERE item.state = 'accepted'
               AND item.feature_authorization IS NOT NULL
               AND (
                    item.approval_last_verified_at IS NULL
                    OR item.approval_last_verified_at <= datetime('now', '-1 day')
               )
             ORDER BY COALESCE(item.approval_last_verified_at, item.created_at) ASC, item.id ASC
             LIMIT ?1",
        )
        .bind_refs(&[D1Type::Integer(limit)])?
        .all()
        .await?;
    crate::d1_results::<RemoteCollectionItemRevalidationRow>(&result)
}

pub(in crate::collections_alpha) async fn list_remote_in_collection_rows(
    db: &crate::D1Database,
    target_actor_uri: &str,
    limit: u32,
) -> Result<Vec<RemoteCollectionRow>> {
    let bindings = [
        D1Type::Text(target_actor_uri),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT c.id,
                    c.actor_uri,
                    c.uri,
                    c.name,
                    c.description,
                    c.language,
                    c.sensitive,
                    c.discoverable,
                    c.tag_name,
                    c.url,
                    c.published_at,
                    c.remote_updated_at,
                    c.created_at,
                    c.updated_at
             FROM remote_account_collections c
             JOIN remote_account_collection_items target_item
              ON target_item.collection_id = c.id
             AND target_item.target_actor_uri = ?1
             AND target_item.state IN ('accepted', 'pending')
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<RemoteCollectionRow>(&result)
}

pub(in crate::collections_alpha) async fn list_local_in_collection_rows(
    db: &crate::D1Database,
    target_account_id: &str,
    limit: u32,
) -> Result<Vec<CollectionRow>> {
    let bindings = [
        D1Type::Text(target_account_id),
        D1Type::Integer(i32::try_from(limit).unwrap_or(i32::MAX)),
    ];
    let result = db
        .prepare(
            "SELECT c.id,
                    c.account_id,
                    c.name,
                    c.description,
                    c.language,
                    c.sensitive,
                    c.discoverable,
                    c.tag_name,
                    c.created_at,
                    c.updated_at
             FROM account_collections c
             JOIN account_collection_items target_item
               ON target_item.collection_id = c.id
              AND target_item.target_account_ref = ?1
              AND target_item.state IN ('accepted', 'pending')
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    crate::d1_results::<CollectionRow>(&result)
}

fn in_collection_entry_sort_key(entry: &InCollectionPageEntry) -> (&str, &str) {
    match entry {
        InCollectionPageEntry::Local(row) => (&row.created_at, &row.id),
        InCollectionPageEntry::Remote(row) => (&row.created_at, &row.id),
    }
}

pub(in crate::collections_alpha) fn sort_in_collection_page_entries(
    entries: &mut [InCollectionPageEntry],
) {
    entries.sort_by(|left, right| {
        let (left_created_at, left_id) = in_collection_entry_sort_key(left);
        let (right_created_at, right_id) = in_collection_entry_sort_key(right);
        right_created_at
            .cmp(left_created_at)
            .then_with(|| right_id.cmp(left_id))
    });
}
pub(in crate::collections_alpha) async fn upsert_remote_collection_draft(
    db: &crate::D1Database,
    draft: &RemoteCollectionDraft,
) -> Result<()> {
    let bindings = [
        D1Type::Text(draft.id.as_str()),
        D1Type::Text(draft.actor_uri.as_str()),
        D1Type::Text(draft.uri.as_str()),
        D1Type::Text(draft.name.as_str()),
        D1Type::Text(draft.description.as_str()),
        draft.language.as_deref().map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(i32::from(draft.sensitive)),
        D1Type::Integer(i32::from(draft.discoverable)),
        draft.tag_name.as_deref().map_or(D1Type::Null, D1Type::Text),
        draft.url.as_deref().map_or(D1Type::Null, D1Type::Text),
        draft
            .published_at
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        draft
            .remote_updated_at
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    ];
    db.prepare(
        "INSERT INTO remote_account_collections (
            id,
            actor_uri,
            uri,
            name,
            description,
            language,
            sensitive,
            discoverable,
            tag_name,
            url,
            published_at,
            remote_updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
        )
        ON CONFLICT(uri) DO UPDATE SET
            actor_uri = excluded.actor_uri,
            name = excluded.name,
            description = excluded.description,
            language = excluded.language,
            sensitive = excluded.sensitive,
            discoverable = excluded.discoverable,
            tag_name = excluded.tag_name,
            url = excluded.url,
            published_at = excluded.published_at,
            remote_updated_at = excluded.remote_updated_at,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await
    .map(|_| ())
}
pub(in crate::collections_alpha) async fn update_remote_collection_item_approval_verification(
    db: &crate::D1Database,
    collection_id: &str,
    target_actor_uri: &str,
    state: &str,
    approval_verified: bool,
) -> Result<()> {
    let bindings = [
        D1Type::Text(state),
        D1Type::Integer(if approval_verified { 1 } else { 0 }),
        D1Type::Text(collection_id),
        D1Type::Text(target_actor_uri),
    ];
    db.prepare(
        "UPDATE remote_account_collection_items
         SET state = ?1,
             approval_last_verified_at = CASE WHEN ?2 = 1 THEN CURRENT_TIMESTAMP ELSE NULL END,
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?3
           AND target_actor_uri = ?4",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}
pub(in crate::collections_alpha) async fn delete_remote_collection_by_uri(
    db: &crate::D1Database,
    actor_uri: &str,
    collection_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Text(collection_uri)];
    db.prepare(
        "DELETE FROM remote_account_collections
         WHERE actor_uri = ?1
           AND uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(in crate::collections_alpha) async fn delete_remote_collection_item_by_object(
    db: &crate::D1Database,
    collection_id: &str,
    object: &serde_json::Value,
) -> Result<()> {
    let item_uri = activitypub_value_id(Some(object));
    let target_actor_uri = object
        .get("featuredObject")
        .and_then(|value| activitypub_value_id(Some(value)));
    let bindings = [
        D1Type::Text(collection_id),
        item_uri.map_or(D1Type::Null, D1Type::Text),
        target_actor_uri.map_or(D1Type::Null, D1Type::Text),
    ];
    db.prepare(
        "DELETE FROM remote_account_collection_items
         WHERE collection_id = ?1
           AND (
             (?2 IS NOT NULL AND uri = ?2)
             OR (?3 IS NOT NULL AND target_actor_uri = ?3)
           )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(in crate::collections_alpha) async fn revoke_remote_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<bool> {
    if remote_collection_item_by_id(db, collection_id, item_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    db.prepare(
        "UPDATE remote_account_collection_items
         SET state = 'revoked',
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .run()
    .await?;
    Ok(true)
}

pub(in crate::collections_alpha) async fn insert_collection(
    db: &crate::D1Database,
    account_id: &str,
    request: &CollectionRequest,
) -> Result<CollectionRow> {
    let collection_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(collection_id.as_str()),
        D1Type::Text(account_id),
        D1Type::Text(request.name.as_deref().unwrap_or_default()),
        D1Type::Text(request.description.as_deref().unwrap_or_default()),
        request
            .language
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
        D1Type::Integer(if request.sensitive.unwrap_or(false) {
            1
        } else {
            0
        }),
        D1Type::Integer(if request.discoverable.unwrap_or(true) {
            1
        } else {
            0
        }),
        request
            .tag_name
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    ];
    db.prepare(
        "INSERT INTO account_collections (
            id,
            account_id,
            name,
            description,
            language,
            sensitive,
            discoverable,
            tag_name
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    collection_row_by_id(db, &collection_id)
        .await?
        .ok_or_else(|| worker::Error::RustError("failed to reload created collection".to_owned()))
}

pub(in crate::collections_alpha) async fn update_collection(
    db: &crate::D1Database,
    collection_id: &str,
    request: &CollectionRequest,
) -> Result<Option<CollectionRow>> {
    let existing = collection_row_by_id(db, collection_id).await?;
    let Some(existing) = existing else {
        return Ok(None);
    };
    let bindings = [
        D1Type::Text(request.name.as_deref().unwrap_or(&existing.name)),
        D1Type::Text(
            request
                .description
                .as_deref()
                .unwrap_or(&existing.description),
        ),
        request.language.as_deref().map_or_else(
            || {
                existing
                    .language
                    .as_deref()
                    .map_or(D1Type::Null, D1Type::Text)
            },
            D1Type::Text,
        ),
        D1Type::Integer(if request.sensitive.unwrap_or(existing.sensitive != 0) {
            1
        } else {
            0
        }),
        D1Type::Integer(
            if request.discoverable.unwrap_or(existing.discoverable != 0) {
                1
            } else {
                0
            },
        ),
        request.tag_name.as_deref().map_or_else(
            || {
                existing
                    .tag_name
                    .as_deref()
                    .map_or(D1Type::Null, D1Type::Text)
            },
            D1Type::Text,
        ),
        D1Type::Text(collection_id),
    ];
    db.prepare(
        "UPDATE account_collections
         SET name = ?1,
             description = ?2,
             language = ?3,
             sensitive = ?4,
             discoverable = ?5,
             tag_name = ?6,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?7",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    collection_row_by_id(db, collection_id).await
}

pub(in crate::collections_alpha) async fn delete_collection(
    db: &crate::D1Database,
    collection_id: &str,
) -> Result<bool> {
    if collection_row_by_id(db, collection_id).await?.is_none() {
        return Ok(false);
    }
    let collection_id = D1Type::Text(collection_id);
    db.prepare("DELETE FROM account_collections WHERE id = ?1")
        .bind_refs(&collection_id)?
        .run()
        .await?;
    Ok(true)
}

pub(in crate::collections_alpha) async fn insert_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    target: &AccountReference,
) -> Result<CollectionItemRow> {
    let item_id = generate_entity_id(16)?;
    let (target_ref, state) = match target {
        AccountReference::Local(account) => (account.id().to_owned(), "accepted"),
        AccountReference::Remote(actor) => (remote_account_rest_id(&actor.actor_uri), "pending"),
    };
    let bindings = [
        D1Type::Text(item_id.as_str()),
        D1Type::Text(collection_id),
        D1Type::Text(target_ref.as_str()),
        D1Type::Text(state),
    ];
    db.prepare(
        "INSERT INTO account_collection_items (
            id,
            collection_id,
            target_account_ref,
            state
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4
        )
        ON CONFLICT(collection_id, target_account_ref) DO UPDATE SET
            state = excluded.state,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "SELECT id, target_account_ref, state, activity_uri, feature_authorization, created_at
         FROM account_collection_items
         WHERE collection_id = ?1
           AND target_account_ref = ?2",
    )
    .bind_refs(&[
        D1Type::Text(collection_id),
        D1Type::Text(target_ref.as_str()),
    ])?
    .first::<CollectionItemRow>(None)
    .await?
    .ok_or_else(|| worker::Error::RustError("failed to reload collection item".to_owned()))
}

pub(in crate::collections_alpha) async fn collection_item_by_id(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<Option<CollectionItemRow>> {
    db.prepare(
        "SELECT id, target_account_ref, state, activity_uri, feature_authorization, created_at
         FROM account_collection_items
         WHERE collection_id = ?1
           AND id = ?2
         LIMIT 1",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .first::<CollectionItemRow>(None)
    .await
}

pub(in crate::collections_alpha) async fn collection_item_by_feature_request_uri(
    db: &crate::D1Database,
    activity_uri: &str,
) -> Result<Option<(CollectionRow, CollectionItemRow)>> {
    let activity_uri_binding = D1Type::Text(activity_uri);
    let row = db
        .prepare(
            "SELECT c.id AS collection_id,
                    i.id AS item_id
             FROM account_collection_items i
             JOIN account_collections c
               ON c.id = i.collection_id
             WHERE i.activity_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&activity_uri_binding)?
        .first::<serde_json::Value>(None)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let Some(collection_id) = row.get("collection_id").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(item_id) = row.get("item_id").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(collection) = collection_row_by_id(db, collection_id).await? else {
        return Ok(None);
    };
    let Some(item) = collection_item_by_id(db, collection_id, item_id).await? else {
        return Ok(None);
    };
    Ok(Some((collection, item)))
}

pub(in crate::collections_alpha) async fn update_collection_item_feature_request_uri(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
    activity_uri: &str,
) -> Result<()> {
    db.prepare(
        "UPDATE account_collection_items
         SET activity_uri = ?3,
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[
        D1Type::Text(collection_id),
        D1Type::Text(item_id),
        D1Type::Text(activity_uri),
    ])?
    .run()
    .await?;
    Ok(())
}

pub(in crate::collections_alpha) async fn update_collection_item_feature_state(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
    state: &str,
    feature_authorization: Option<&str>,
) -> Result<Option<CollectionItemRow>> {
    let bindings = [
        D1Type::Text(state),
        feature_authorization.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(collection_id),
        D1Type::Text(item_id),
    ];
    db.prepare(
        "UPDATE account_collection_items
         SET state = ?1,
             feature_authorization = COALESCE(?2, feature_authorization),
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?3
           AND id = ?4",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    collection_item_by_id(db, collection_id, item_id).await
}

pub(in crate::collections_alpha) async fn delete_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<bool> {
    if collection_item_by_id(db, collection_id, item_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    db.prepare(
        "DELETE FROM account_collection_items
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .run()
    .await?;
    Ok(true)
}

pub(in crate::collections_alpha) async fn revoke_collection_item(
    db: &crate::D1Database,
    collection_id: &str,
    item_id: &str,
) -> Result<bool> {
    if collection_item_by_id(db, collection_id, item_id)
        .await?
        .is_none()
    {
        return Ok(false);
    }
    db.prepare(
        "UPDATE account_collection_items
         SET state = 'revoked',
             updated_at = CURRENT_TIMESTAMP
         WHERE collection_id = ?1
           AND id = ?2",
    )
    .bind_refs(&[D1Type::Text(collection_id), D1Type::Text(item_id)])?
    .run()
    .await?;
    Ok(true)
}
