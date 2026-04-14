use cfwdon_domain::AccountHandle;
use url::Url;
use worker::{Error, Fetch, Headers, Method, Request, RequestInit, Result};

use super::federation_url_guard::validate_remote_fetch_url;

#[derive(Debug)]
pub(crate) struct RemoteActorProfile {
    pub(crate) actor_uri: String,
    pub(crate) username: String,
    pub(crate) domain: String,
    pub(crate) inbox_uri: String,
    pub(crate) shared_inbox_uri: Option<String>,
    pub(crate) public_key_id: String,
    pub(crate) public_key_pem: String,
    pub(crate) display_name: String,
    pub(crate) summary_html: String,
    pub(crate) profile_url: Option<String>,
    pub(crate) avatar_url: Option<String>,
    pub(crate) header_url: Option<String>,
}

pub(crate) async fn fetch_remote_account_profile_by_handle(
    handle: &AccountHandle,
) -> Result<RemoteActorProfile> {
    let domain = handle
        .domain
        .as_deref()
        .ok_or_else(|| Error::RustError("remote handle is missing domain".to_owned()))?;
    let resource = format!("acct:{}@{}", handle.username, domain);
    let encoded_resource =
        url::form_urlencoded::byte_serialize(resource.as_bytes()).collect::<String>();
    let webfinger_url = format!(
        "https://{}/.well-known/webfinger?resource={}",
        domain, encoded_resource
    );
    let webfinger_url = parse_remote_http_url(&webfinger_url)?;
    validate_remote_fetch_url(&webfinger_url).await?;

    let request = Request::new(webfinger_url.as_str(), Method::Get)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(Error::RustError(format!(
            "failed to resolve remote account {}@{}: HTTP {}",
            handle.username,
            domain,
            response.status_code()
        )));
    }

    let webfinger: serde_json::Value = response.json().await?;
    let actor_uri = webfinger
        .get("links")
        .and_then(serde_json::Value::as_array)
        .and_then(|links| {
            links.iter().find_map(|link| {
                let rel = link.get("rel").and_then(serde_json::Value::as_str)?;
                let href = link.get("href").and_then(serde_json::Value::as_str)?;
                (rel == "self").then_some(href)
            })
        })
        .ok_or_else(|| {
            Error::RustError("webfinger response did not include a self link".to_owned())
        })?;

    fetch_remote_actor_profile(actor_uri).await
}

pub(crate) async fn fetch_remote_actor_profile(actor_uri: &str) -> Result<RemoteActorProfile> {
    let actor_url = parse_remote_http_url(actor_uri)?;
    let actor = fetch_remote_activitypub_document(actor_url.as_str()).await?;
    let profile = parse_remote_actor_profile_document(&actor, actor_uri)?;
    validate_remote_actor_profile_urls(&profile).await?;
    Ok(profile)
}

pub(crate) fn parse_remote_actor_profile_document(
    actor: &serde_json::Value,
    fallback_actor_uri: &str,
) -> Result<RemoteActorProfile> {
    let canonical_actor_uri = actor
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback_actor_uri)
        .to_owned();
    let actor_url = parse_remote_http_url(&canonical_actor_uri)?;
    let inbox_uri = actor
        .get("inbox")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote actor document is missing inbox".to_owned()))?
        .to_owned();
    let shared_inbox_uri = actor
        .get("endpoints")
        .and_then(|value| value.get("sharedInbox"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let username = actor
        .get("preferredUsername")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| {
            actor_url
                .path_segments()
                .and_then(|segments| segments.last())
                .unwrap_or("remote")
        })
        .to_ascii_lowercase();
    let domain = actor_url
        .host_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let public_key_id = actor
        .get("publicKey")
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::RustError("remote actor document is missing publicKey.id".to_owned())
        })?
        .to_owned();
    let public_key_pem = actor
        .get("publicKey")
        .and_then(|value| value.get("publicKeyPem"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::RustError("remote actor document is missing publicKey.publicKeyPem".to_owned())
        })?
        .to_owned();

    Ok(RemoteActorProfile {
        actor_uri: canonical_actor_uri,
        username,
        domain,
        inbox_uri,
        shared_inbox_uri,
        public_key_id,
        public_key_pem,
        display_name: actor
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        summary_html: actor
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        profile_url: actor.get("url").and_then(extract_remote_profile_url),
        avatar_url: extract_remote_profile_media_url(actor.get("icon")),
        header_url: extract_remote_profile_media_url(actor.get("image")),
    })
}

pub(crate) async fn validate_remote_actor_profile_urls(profile: &RemoteActorProfile) -> Result<()> {
    validate_remote_fetch_url(&parse_remote_http_url(&profile.actor_uri)?).await?;
    validate_remote_fetch_url(&parse_remote_http_url(&profile.inbox_uri)?).await?;
    if let Some(shared_inbox_uri) = profile.shared_inbox_uri.as_deref() {
        validate_remote_fetch_url(&parse_remote_http_url(shared_inbox_uri)?).await?;
    }
    validate_remote_fetch_url(&parse_remote_http_url(&profile.public_key_id)?).await?;
    Ok(())
}

pub(crate) async fn fetch_remote_activitypub_document(url: &str) -> Result<serde_json::Value> {
    let parsed = parse_remote_http_url(url)?;
    validate_remote_fetch_url(&parsed).await?;

    let headers = Headers::new();
    headers.set(
        "Accept",
        "application/activity+json, application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"",
    )?;

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let request = Request::new_with_init(parsed.as_str(), &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(Error::RustError(format!(
            "failed to fetch remote activitypub document {}: HTTP {}",
            url,
            response.status_code()
        )));
    }

    response.json().await
}

fn extract_remote_profile_url(value: &serde_json::Value) -> Option<String> {
    extract_remote_profile_media_url(Some(value))
}

pub(crate) fn extract_remote_profile_media_url(
    value: Option<&serde_json::Value>,
) -> Option<String> {
    match value? {
        serde_json::Value::String(url) => normalize_remote_media_url(url),
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|entry| extract_remote_profile_media_url(Some(entry))),
        serde_json::Value::Object(map) => map
            .get("url")
            .and_then(|entry| extract_remote_profile_media_url(Some(entry)))
            .or_else(|| {
                map.get("href")
                    .and_then(|entry| extract_remote_profile_media_url(Some(entry)))
            }),
        _ => None,
    }
}

fn normalize_remote_media_url(url: &str) -> Option<String> {
    parse_remote_http_url(url).ok().map(Into::into)
}

pub(crate) fn parse_remote_http_url(url: &str) -> Result<Url> {
    let parsed = Url::parse(url.trim())
        .map_err(|error| Error::RustError(format!("invalid remote URL {url}: {error}")))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        scheme => Err(Error::RustError(format!(
            "unsupported remote URL scheme {scheme}"
        ))),
    }
}

pub(crate) fn parse_http_url_parts(url: &str) -> Result<(String, String)> {
    let url = parse_remote_http_url(url)?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::RustError("URL host missing".to_owned()))?
        .to_owned();
    let path = match url.query() {
        Some(query) if url.path().is_empty() || url.path() == "/" => format!("/?{query}"),
        Some(query) => format!("{}?{query}", url.path()),
        None if url.path().is_empty() => "/".to_owned(),
        None => url.path().to_owned(),
    };
    Ok((host, path))
}
