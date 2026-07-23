use crate::{
    AppConfig, LocalAccount, MastodonAccountResponse, RemoteActorRow, RemoteCollectionFetchContext,
    RemoteStatusRow, account_search_is_complete_handle, enrich_remote_account_response,
    ensure_remote_actor_username_matches_handle, extract_remote_note_object,
    fetch_activitypub_document_with_context, fetch_remote_actor_profile_with_context,
    find_account_by_id, find_account_by_username, find_remote_actor_by_actor_uri,
    find_remote_actor_by_profile_url_or_actor_uri, find_remote_actor_by_username_domain,
    find_remote_status_by_object_uri, find_remote_status_by_url_or_object_uri,
    is_public_activitypub_visibility, load_account_stats, local_username_from_actor_uri,
    log_json_event, parse_lookup_handle, parse_remote_http_url,
    reconcile_remote_account_status_summary, remote_actor_uri_from_rest_id,
    resolve_webfinger_actor_uri, upsert_remote_actor, upsert_remote_status,
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
    resolve_account_reference_with_fetch(db, account_id, None).await
}

pub(crate) async fn resolve_account_reference_with_fetch(
    db: &D1Database,
    account_id: &str,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<Option<AccountReference>> {
    if let Some(actor_uri) = remote_actor_uri_from_rest_id(account_id) {
        if let Some(actor) = find_remote_actor_by_actor_uri(db, &actor_uri).await? {
            return Ok(Some(AccountReference::Remote(actor)));
        }
        return materialize_remote_account_reference(db, &actor_uri, fetch_context).await;
    }

    if let Some(account) = find_account_by_id(db, account_id).await? {
        return Ok(Some(AccountReference::Local(account)));
    }

    Ok(None)
}

async fn materialize_remote_account_reference(
    db: &D1Database,
    actor_uri: &str,
    fetch_context: Option<&RemoteCollectionFetchContext<'_>>,
) -> Result<Option<AccountReference>> {
    let fetched = match fetch_remote_actor_profile_with_context(actor_uri, fetch_context).await {
        Ok(fetched) => fetched,
        Err(error) => {
            log_json_event(serde_json::json!({
                "event": "remote_account_reference_fetch_failed",
                "actor_uri": actor_uri,
                "error": error.to_string(),
            }));
            return Ok(None);
        }
    };
    upsert_remote_actor(db, &fetched.profile).await?;
    let Some(actor) = find_remote_actor_by_actor_uri(db, &fetched.profile.actor_uri).await? else {
        return Ok(None);
    };
    let mut response = MastodonAccountResponse::from_remote_actor(&actor);
    enrich_remote_account_response(
        db,
        &actor.actor_uri,
        actor.social_counts_updated_at.as_deref(),
        &mut response,
        &fetched.document,
        fetch_context,
    )
    .await?;
    Ok(find_remote_actor_by_actor_uri(db, &actor.actor_uri)
        .await?
        .map(AccountReference::Remote))
}

#[allow(dead_code)]
pub(crate) async fn resolve_lookup_account(
    db: &D1Database,
    config: &AppConfig,
    acct: &str,
) -> Result<MastodonAccountResponse> {
    resolve_lookup_account_with_viewer(db, config, acct, None).await
}

pub(crate) async fn resolve_lookup_account_with_viewer(
    db: &D1Database,
    config: &AppConfig,
    acct: &str,
    viewer: Option<&LocalAccount>,
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

    let domain = handle
        .domain
        .as_deref()
        .ok_or_else(|| Error::RustError("remote handle is missing domain".to_owned()))?;
    let fetch_context = RemoteCollectionFetchContext {
        config,
        db,
        signer: viewer,
    };
    match resolve_remote_lookup_via_fetch(db, &handle, &fetch_context).await {
        Ok(response) => Ok(response),
        Err(error) => {
            if let Some(actor) =
                find_remote_actor_by_username_domain(db, &handle.username, domain).await?
            {
                log_json_event(serde_json::json!({
                    "event": "account_lookup_cache_fallback",
                    "acct": acct,
                    "actor_uri": actor.actor_uri,
                    "error": error.to_string(),
                }));
                let mut response = MastodonAccountResponse::from_remote_actor(&actor);
                if let Err(enrich_error) =
                    reconcile_remote_account_status_summary(db, &actor.actor_uri, &mut response)
                        .await
                {
                    log_json_event(serde_json::json!({
                        "event": "remote_account_enrichment_failed",
                        "actor_uri": actor.actor_uri,
                        "stage": "status_summary",
                        "error": enrich_error.to_string(),
                    }));
                }
                return Ok(response);
            }
            Err(error)
        }
    }
}

async fn resolve_remote_lookup_via_fetch(
    db: &D1Database,
    handle: &cfwdon_domain::AccountHandle,
    fetch_context: &RemoteCollectionFetchContext<'_>,
) -> Result<MastodonAccountResponse> {
    let actor_uri = resolve_webfinger_actor_uri(handle).await?;
    let fetched = fetch_remote_actor_profile_with_context(&actor_uri, Some(fetch_context)).await?;
    let profile = fetched.profile;
    ensure_remote_actor_username_matches_handle(&profile, &handle.username)?;
    upsert_remote_actor(db, &profile).await?;
    let actor = find_remote_actor_by_actor_uri(db, &profile.actor_uri)
        .await?
        .ok_or_else(|| Error::RustError("remote account could not be cached".to_owned()))?;
    let mut response = MastodonAccountResponse::from_remote_actor(&actor);
    enrich_remote_account_response(
        db,
        &profile.actor_uri,
        actor.social_counts_updated_at.as_deref(),
        &mut response,
        &fetched.document,
        Some(fetch_context),
    )
    .await?;
    Ok(response)
}

pub(crate) async fn resolve_search_account(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
) -> Result<Option<MastodonAccountResponse>> {
    resolve_search_account_with_viewer(db, config, query, None).await
}

pub(crate) async fn resolve_search_account_with_viewer(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
    viewer: Option<&LocalAccount>,
) -> Result<Option<MastodonAccountResponse>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }

    if account_search_is_complete_handle(query, config)
        && let Ok(account) = resolve_lookup_account_with_viewer(db, config, query, viewer).await
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
        if let Err(error) =
            reconcile_remote_account_status_summary(db, &actor.actor_uri, &mut response).await
        {
            log_json_event(serde_json::json!({
                "event": "remote_account_enrichment_failed",
                "actor_uri": actor.actor_uri,
                "stage": "status_summary",
                "error": error.to_string(),
            }));
        }
        return Ok(Some(response));
    }

    let fetch_context = RemoteCollectionFetchContext {
        config,
        db,
        signer: viewer,
    };
    let fetched = match fetch_remote_actor_profile_with_context(query, Some(&fetch_context)).await {
        Ok(fetched) => fetched,
        Err(_) => return Ok(None),
    };
    upsert_remote_actor(db, &fetched.profile).await?;
    let Some(actor) = find_remote_actor_by_actor_uri(db, &fetched.profile.actor_uri).await? else {
        return Ok(None);
    };
    let mut response = MastodonAccountResponse::from_remote_actor(&actor);
    enrich_remote_account_response(
        db,
        &actor.actor_uri,
        actor.social_counts_updated_at.as_deref(),
        &mut response,
        &fetched.document,
        Some(&fetch_context),
    )
    .await?;
    Ok(Some(response))
}

pub(crate) async fn resolve_remote_status_by_url(
    db: &D1Database,
    config: &AppConfig,
    url: &str,
    viewer: Option<&LocalAccount>,
) -> Result<Option<(RemoteStatusRow, RemoteActorRow)>> {
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, url).await? {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
            return Ok(None);
        };
        return Ok(Some((status, actor)));
    }

    let fetch_url = parse_remote_http_url(url)?;
    let fetch_context = RemoteCollectionFetchContext {
        config,
        db,
        signer: viewer,
    };
    let document =
        match fetch_activitypub_document_with_context(fetch_url.as_str(), Some(&fetch_context))
            .await
        {
            Ok(document) => document,
            Err(_) => return Ok(None),
        };
    let Some(object) = extract_remote_note_object(&document) else {
        return Ok(None);
    };
    if !is_public_activitypub_visibility(&visibility_from_activitypub_object(object)) {
        return Ok(None);
    }

    let object_id = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote status object is missing id".to_owned()))?;
    let actor_uri = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| document.get("actor").and_then(serde_json::Value::as_str))
        .ok_or_else(|| {
            Error::RustError("remote status object is missing attributedTo".to_owned())
        })?;
    if cfwdon_domain::remote_status_object_authority_allowed(
        fetch_url.as_str(),
        object_id,
        actor_uri,
    )
    .is_err()
    {
        return Ok(None);
    }

    let actor = fetch_remote_actor_profile_with_context(actor_uri, Some(&fetch_context)).await?;
    upsert_remote_actor(db, &actor.profile).await?;
    upsert_remote_status(db, config, &actor.profile, object).await?;
    let Some(status) = find_remote_status_by_object_uri(db, object_id).await? else {
        return Ok(None);
    };
    let Some(actor_row) = find_remote_actor_by_actor_uri(db, &actor.profile.actor_uri).await?
    else {
        return Ok(None);
    };
    Ok(Some((status, actor_row)))
}
