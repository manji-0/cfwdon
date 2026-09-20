use super::{
    CollectionItemRow, CollectionRow, RemoteActorRow, RemoteCollectionItemRow, RemoteCollectionRow,
    list_collection_items, list_remote_collection_items,
    revalidate_remote_collection_item_approvals,
};
use crate::{
    AccountReference, MastodonAccountResponse, Result, actor_url, find_remote_actor_by_actor_uri,
    instance_base_url, is_blocking_actor, load_account_stats, local_username_from_actor_uri,
    remote_account_rest_id, resolve_account_reference, timestamp_to_mastodon_iso8601,
    timestamp_to_mastodon_iso8601_opt,
};
use std::collections::HashSet;

fn tag_document(config: &cfwdon_core::AppConfig, tag_name: Option<&str>) -> serde_json::Value {
    let Some(tag_name) = tag_name else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "name": tag_name,
        "url": format!("{}/tags/{}", instance_base_url(config), tag_name),
    })
}

pub(in crate::collections_alpha) fn collection_uri(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
) -> String {
    format!(
        "{}/collections/{collection_id}",
        actor_url(config, owner.username())
    )
}

pub(in crate::collections_alpha) fn collection_item_uri(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    collection_id: &str,
    item_id: &str,
) -> String {
    format!(
        "{}/items/{item_id}",
        collection_uri(config, owner, collection_id)
    )
}

pub(crate) fn local_collection_id_from_uri(
    config: &cfwdon_core::AppConfig,
    uri: &str,
) -> Option<String> {
    let base = format!("{}/users/", instance_base_url(config));
    let rest = uri.strip_prefix(&base)?;
    let (_, collection_id) = rest.split_once("/collections/")?;
    collection_id
        .split('/')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(in crate::collections_alpha) fn collection_document(
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
    items: Vec<serde_json::Value>,
) -> serde_json::Value {
    let uri = collection_uri(config, owner, &row.id);
    serde_json::json!({
        "id": row.id,
        "uri": uri,
        "name": row.name,
        "description": row.description,
        "language": row.language,
        "account_id": row.account_id,
        "local": true,
        "sensitive": row.sensitive,
        "discoverable": row.discoverable != 0,
        "url": uri,
        "item_count": items.len(),
        "created_at": timestamp_to_mastodon_iso8601(&row.created_at),
        "updated_at": timestamp_to_mastodon_iso8601(&row.updated_at),
        "tag": tag_document(config, row.tag_name.as_deref()),
        "items": items,
    })
}

pub(in crate::collections_alpha) fn remote_collection_document(
    config: &cfwdon_core::AppConfig,
    owner: &RemoteActorRow,
    row: &RemoteCollectionRow,
    items: Vec<serde_json::Value>,
) -> serde_json::Value {
    let uri = row.url.as_deref().unwrap_or(&row.uri);
    let created_at = row.published_at.as_deref().unwrap_or(&row.created_at);
    let updated_at = row.remote_updated_at.as_deref().unwrap_or(&row.updated_at);
    serde_json::json!({
        "id": row.id,
        "uri": row.uri,
        "name": row.name,
        "description": row.description,
        "language": row.language,
        "account_id": remote_account_rest_id(&owner.actor_uri),
        "local": false,
        "sensitive": row.sensitive,
        "discoverable": row.discoverable != 0,
        "url": uri,
        "item_count": items.len(),
        "created_at": timestamp_to_mastodon_iso8601(created_at),
        "updated_at": timestamp_to_mastodon_iso8601(updated_at),
        "tag": tag_document(config, row.tag_name.as_deref()),
        "items": items,
    })
}

pub(in crate::collections_alpha) fn collection_list_document(
    collections: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({ "collections": collections })
}

pub(in crate::collections_alpha) fn collection_response_document(
    collection: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({ "collection": collection })
}

pub(in crate::collections_alpha) fn collection_item_document(
    row: &CollectionItemRow,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "id": row.id,
        "state": row.state,
        "created_at": timestamp_to_mastodon_iso8601(&row.created_at),
    });
    if row.state == "accepted" || row.state == "pending" {
        value["account_id"] = serde_json::json!(row.target_account_ref);
    }
    if let Some(activity_uri) = row.activity_uri.as_deref() {
        value["activity_uri"] = serde_json::json!(activity_uri);
    }
    if let Some(feature_authorization) = row.feature_authorization.as_deref() {
        value["feature_authorization"] = serde_json::json!(feature_authorization);
    }
    value
}

async fn account_id_for_actor_uri(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    actor_uri: &str,
) -> Result<String> {
    if let Some(username) = local_username_from_actor_uri(config, actor_uri)
        && let Some(account) = crate::find_account_by_username(db, &username).await?
    {
        return Ok(account.id().to_owned());
    }
    Ok(remote_account_rest_id(actor_uri))
}

pub(in crate::collections_alpha) async fn remote_collection_item_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    row: &RemoteCollectionItemRow,
) -> Result<serde_json::Value> {
    let mut value = serde_json::json!({
        "id": row.id,
        "uri": row.uri,
        "state": row.state,
        "created_at": timestamp_to_mastodon_iso8601(
            row.published_at.as_deref().unwrap_or(&row.created_at),
        ),
        "feature_authorization": row.feature_authorization,
        "approval_last_verified_at": timestamp_to_mastodon_iso8601_opt(
            row.approval_last_verified_at.as_deref(),
        ),
    });
    if row.state == "accepted" || row.state == "pending" {
        value["account_id"] =
            serde_json::json!(account_id_for_actor_uri(db, config, &row.target_actor_uri).await?);
    }
    Ok(value)
}

pub(in crate::collections_alpha) fn collection_item_response_document(
    collection_item: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({ "collection_item": collection_item })
}

pub(in crate::collections_alpha) async fn account_response_for_reference(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    account_ref: &str,
) -> Result<Option<MastodonAccountResponse>> {
    match resolve_account_reference(db, account_ref).await? {
        Some(AccountReference::Local(account)) => {
            let stats = load_account_stats(db, account.id()).await?;
            Ok(Some(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            )))
        }
        Some(AccountReference::Remote(actor)) => {
            Ok(Some(MastodonAccountResponse::from_remote_actor(&actor)))
        }
        None => Ok(None),
    }
}

pub(in crate::collections_alpha) async fn remote_account_response_for_actor_uri(
    db: &crate::D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    Ok(find_remote_actor_by_actor_uri(db, actor_uri)
        .await?
        .map(|actor| MastodonAccountResponse::from_remote_actor(&actor)))
}

async fn collection_item_visible_to_viewer(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    item: &CollectionItemRow,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<bool> {
    let Some(viewer) = viewer else {
        return Ok(true);
    };
    let target_actor_uri = match resolve_account_reference(db, &item.target_account_ref).await? {
        Some(AccountReference::Local(account)) => actor_url(config, account.username()),
        Some(AccountReference::Remote(actor)) => actor.actor_uri,
        None => return Ok(true),
    };
    Ok(!is_blocking_actor(db, viewer.id(), &target_actor_uri).await?)
}

pub(in crate::collections_alpha) async fn collection_with_accounts_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &cfwdon_domain::LocalAccount,
    row: &CollectionRow,
    include_pending: bool,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<serde_json::Value> {
    let item_rows = list_collection_items(db, &row.id, include_pending).await?;
    let mut visible_item_rows = Vec::new();
    for item in item_rows {
        if collection_item_visible_to_viewer(db, config, &item, viewer).await? {
            visible_item_rows.push(item);
        }
    }

    let items = visible_item_rows
        .iter()
        .map(collection_item_document)
        .collect::<Vec<_>>();
    let collection = collection_document(config, owner, row, items);

    let mut accounts = Vec::new();
    let stats = load_account_stats(db, owner.id()).await?;
    accounts.push(MastodonAccountResponse::from_account_with_stats(
        owner, config, &stats,
    ));

    let mut seen = HashSet::from([owner.id().to_owned()]);
    for item in visible_item_rows {
        if !seen.insert(item.target_account_ref.clone()) {
            continue;
        }
        if let Some(account) =
            account_response_for_reference(db, config, &item.target_account_ref).await?
        {
            accounts.push(account);
        }
    }

    Ok(serde_json::json!({
        "collection": collection,
        "accounts": accounts,
    }))
}

async fn remote_collection_item_visible_to_viewer(
    db: &crate::D1Database,
    item: &RemoteCollectionItemRow,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<bool> {
    let Some(viewer) = viewer else {
        return Ok(true);
    };
    Ok(!is_blocking_actor(db, viewer.id(), &item.target_actor_uri).await?)
}

pub(in crate::collections_alpha) async fn remote_collection_with_accounts_document(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    owner: &RemoteActorRow,
    row: &RemoteCollectionRow,
    include_pending: bool,
    viewer: Option<&cfwdon_domain::LocalAccount>,
) -> Result<serde_json::Value> {
    revalidate_remote_collection_item_approvals(db, config, row).await?;
    let item_rows = list_remote_collection_items(db, &row.id, include_pending).await?;
    let mut visible_item_rows = Vec::new();
    for item in item_rows {
        if remote_collection_item_visible_to_viewer(db, &item, viewer).await? {
            visible_item_rows.push(item);
        }
    }

    let mut items = Vec::new();
    for item in &visible_item_rows {
        items.push(remote_collection_item_document(db, config, item).await?);
    }
    let collection = remote_collection_document(config, owner, row, items);

    let mut accounts = Vec::new();
    accounts.push(MastodonAccountResponse::from_remote_actor(owner));

    let mut seen = HashSet::from([owner.actor_uri.clone()]);
    for item in visible_item_rows {
        if !seen.insert(item.target_actor_uri.clone()) {
            continue;
        }
        if let Some(username) = local_username_from_actor_uri(config, &item.target_actor_uri)
            && let Some(account) = crate::find_account_by_username(db, &username).await?
        {
            let stats = load_account_stats(db, account.id()).await?;
            accounts.push(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            ));
        } else if let Some(account) =
            remote_account_response_for_actor_uri(db, &item.target_actor_uri).await?
        {
            accounts.push(account);
        }
    }

    Ok(serde_json::json!({
        "collection": collection,
        "accounts": accounts,
    }))
}
