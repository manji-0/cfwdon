use crate::{
    AppConfig, LocalAccount, MastodonAccountResponse, RemoteActorRow, RemoteStatusRow,
    account_search_is_complete_handle, apply_remote_actor_social_counts,
    extract_remote_note_object, fetch_remote_account_profile_by_handle_with_document,
    fetch_remote_activitypub_document, fetch_remote_actor_profile, find_account_by_id,
    find_account_by_username, find_remote_actor_by_actor_uri,
    find_remote_actor_by_profile_url_or_actor_uri, find_remote_status_by_object_uri,
    find_remote_status_by_url_or_object_uri, is_public_activitypub_visibility, load_account_stats,
    load_remote_actor_social_counts_from_document, load_remote_actor_status_summary,
    local_username_from_actor_uri, parse_lookup_handle, parse_remote_http_url,
    remote_actor_uri_from_rest_id, upsert_remote_actor, upsert_remote_status,
    visibility_from_activitypub_object,
};
use worker::{D1Database, Error, Result};

pub(crate) enum AccountReference {
    Local(LocalAccount),
    Remote(RemoteActorRow),
}

pub(crate) async fn resolve_account_reference(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<AccountReference>> {
    if let Some(actor_uri) = remote_actor_uri_from_rest_id(account_id) {
        return Ok(find_remote_actor_by_actor_uri(db, &actor_uri)
            .await?
            .map(AccountReference::Remote));
    }

    if let Some(account) = find_account_by_id(db, account_id).await? {
        return Ok(Some(AccountReference::Local(account)));
    }

    Ok(None)
}

pub(crate) async fn resolve_lookup_account(
    db: &D1Database,
    config: &AppConfig,
    acct: &str,
) -> Result<MastodonAccountResponse> {
    let handle = parse_lookup_handle(acct, config)?;
    if handle.is_local_to(&config.instance_domain) {
        let Some(account) = find_account_by_username(db, &handle.username).await? else {
            return Err(Error::RustError("account not found".to_owned()));
        };
        let stats = load_account_stats(db, account.id()).await?;
        return Ok(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }

    let fetched = fetch_remote_account_profile_by_handle_with_document(&handle).await?;
    let profile = fetched.profile;
    upsert_remote_actor(db, &profile).await?;
    let actor = find_remote_actor_by_actor_uri(db, &profile.actor_uri)
        .await?
        .ok_or_else(|| Error::RustError("remote account could not be cached".to_owned()))?;
    let mut response = MastodonAccountResponse::from_remote_actor(&actor);
    if let Ok(counts) = load_remote_actor_social_counts_from_document(&fetched.document).await {
        apply_remote_actor_social_counts(&mut response, counts);
    }
    if let Ok(summary) = load_remote_actor_status_summary(db, &profile.actor_uri).await {
        if summary.statuses_count > 0 {
            response.statuses_count = summary.statuses_count;
        }
        response.last_status_at = summary.last_status_at;
    }
    Ok(response)
}

pub(crate) async fn resolve_search_account(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }

    if account_search_is_complete_handle(query, config)
        && let Ok(account) = resolve_lookup_account(db, config, query).await
    {
        return Ok(Some(account));
    }

    if parse_remote_http_url(query).is_err() {
        return Ok(None);
    }

    if let Some(username) = local_username_from_actor_uri(config, query)
        && let Some(account) = find_account_by_username(db, &username).await?
    {
        let stats = load_account_stats(db, account.id()).await?;
        return Ok(Some(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        )));
    }

    if let Some(actor) = find_remote_actor_by_profile_url_or_actor_uri(db, query).await? {
        let mut response = MastodonAccountResponse::from_remote_actor(&actor);
        if let Ok(summary) = load_remote_actor_status_summary(db, &actor.actor_uri).await {
            if summary.statuses_count > 0 {
                response.statuses_count = summary.statuses_count;
            }
            response.last_status_at = summary.last_status_at;
        }
        return Ok(Some(response));
    }

    let profile = match fetch_remote_actor_profile(query).await {
        Ok(profile) => profile,
        Err(_) => return Ok(None),
    };
    upsert_remote_actor(db, &profile).await?;
    Ok(find_remote_actor_by_actor_uri(db, &profile.actor_uri)
        .await?
        .map(|actor| MastodonAccountResponse::from_remote_actor(&actor)))
}

pub(crate) async fn resolve_remote_status_by_url(
    db: &D1Database,
    _config: &AppConfig,
    url: &str,
) -> Result<Option<(RemoteStatusRow, RemoteActorRow)>> {
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, url).await? {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
            return Ok(None);
        };
        return Ok(Some((status, actor)));
    }

    let document = match fetch_remote_activitypub_document(url).await {
        Ok(document) => document,
        Err(_) => return Ok(None),
    };
    let Some(object) = extract_remote_note_object(&document) else {
        return Ok(None);
    };
    if !is_public_activitypub_visibility(&visibility_from_activitypub_object(object)) {
        return Ok(None);
    }

    let actor_uri = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| document.get("actor").and_then(serde_json::Value::as_str))
        .ok_or_else(|| {
            Error::RustError("remote status object is missing attributedTo".to_owned())
        })?;
    let actor = fetch_remote_actor_profile(actor_uri).await?;
    upsert_remote_actor(db, &actor).await?;
    upsert_remote_status(db, _config, &actor, object).await?;
    let object_uri = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote status object is missing id".to_owned()))?;
    let Some(status) = find_remote_status_by_object_uri(db, object_uri).await? else {
        return Ok(None);
    };
    let Some(actor_row) = find_remote_actor_by_actor_uri(db, &actor.actor_uri).await? else {
        return Ok(None);
    };
    Ok(Some((status, actor_row)))
}
