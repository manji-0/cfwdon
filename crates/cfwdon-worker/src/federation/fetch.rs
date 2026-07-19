use cfwdon_domain::{
    AccountHandle, RemoteActorAuthorityIssue, remote_actor_id_authority_allowed,
    remote_actor_public_key_owner_allowed, remote_actor_related_uri_authority_allowed,
    remote_http_authority, webfinger_link_is_activitypub_type,
};
use std::collections::HashSet;
use std::net::IpAddr;
use url::Url;
use worker::{Error, Result};

use super::{fetch_remote_http_json, validate_remote_actor_profile_url};

#[derive(Debug)]
pub(crate) struct RemoteActorProfile {
    pub(crate) actor_uri: String,
    pub(crate) username: String,
    pub(crate) domain: String,
    pub(crate) locked: bool,
    pub(crate) bot: bool,
    pub(crate) discoverable: bool,
    pub(crate) indexable: bool,
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

#[derive(Debug)]
pub(crate) struct FetchedRemoteActorProfile {
    pub(crate) document: serde_json::Value,
    pub(crate) profile: RemoteActorProfile,
}

#[allow(dead_code)]
pub(crate) async fn fetch_remote_account_profile_by_handle_with_document(
    handle: &AccountHandle,
) -> Result<FetchedRemoteActorProfile> {
    let actor_uri = resolve_webfinger_actor_uri(handle).await?;
    fetch_remote_actor_profile_with_document(&actor_uri).await
}

pub(crate) async fn resolve_webfinger_actor_uri(handle: &AccountHandle) -> Result<String> {
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
    let webfinger: serde_json::Value = fetch_remote_http_json(
        webfinger_url.as_str(),
        "application/jrd+json, application/json",
    )
    .await?;
    let actor_uri = webfinger
        .get("links")
        .and_then(serde_json::Value::as_array)
        .and_then(|links| select_webfinger_self_link(links))
        .ok_or_else(|| {
            Error::RustError("webfinger response did not include a self link".to_owned())
        })?;
    let actor_authority = remote_http_authority(&actor_uri).ok_or_else(|| {
        Error::RustError("webfinger self link is not a valid http(s) URL".to_owned())
    })?;
    let expected_domain = domain.trim_end_matches('.').to_ascii_lowercase();
    if actor_authority != expected_domain
        && !actor_authority.starts_with(&format!("{expected_domain}:"))
    {
        return Err(Error::RustError(
            "webfinger self link authority did not match account domain".to_owned(),
        ));
    }
    Ok(actor_uri)
}

fn select_webfinger_self_link(links: &[serde_json::Value]) -> Option<String> {
    let typed = links.iter().find_map(|link| {
        let rel = link.get("rel").and_then(serde_json::Value::as_str)?;
        let href = link.get("href").and_then(serde_json::Value::as_str)?;
        let link_type = link.get("type").and_then(serde_json::Value::as_str);
        (rel == "self" && webfinger_link_is_activitypub_type(link_type)).then(|| href.to_owned())
    });
    typed.or_else(|| {
        links.iter().find_map(|link| {
            let rel = link.get("rel").and_then(serde_json::Value::as_str)?;
            let href = link.get("href").and_then(serde_json::Value::as_str)?;
            (rel == "self").then(|| href.to_owned())
        })
    })
}

pub(crate) async fn fetch_remote_actor_profile(actor_uri: &str) -> Result<RemoteActorProfile> {
    Ok(fetch_remote_actor_profile_with_document(actor_uri)
        .await?
        .profile)
}

pub(crate) async fn fetch_remote_actor_profile_with_document(
    actor_uri: &str,
) -> Result<FetchedRemoteActorProfile> {
    let actor_url = parse_remote_http_url(actor_uri)?;
    let document = fetch_remote_activitypub_document(actor_url.as_str()).await?;
    let profile = parse_remote_actor_profile_document(&document, actor_uri)?;
    validate_remote_actor_profile_urls(&profile).await?;
    Ok(FetchedRemoteActorProfile { document, profile })
}

pub(crate) fn parse_remote_actor_profile_document(
    actor: &serde_json::Value,
    fetched_actor_uri: &str,
) -> Result<RemoteActorProfile> {
    let canonical_actor_uri = remote_actor_canonical_uri(actor, fetched_actor_uri);
    remote_actor_id_authority_allowed(fetched_actor_uri, &canonical_actor_uri)
        .map_err(remote_actor_authority_error)?;
    let actor_url = parse_remote_http_url(&canonical_actor_uri)?;
    let (public_key_id, public_key_pem) = remote_actor_public_key(actor)?;
    let public_key_owner = actor
        .get("publicKey")
        .and_then(|value| value.get("owner"))
        .and_then(serde_json::Value::as_str);
    remote_actor_public_key_owner_allowed(&canonical_actor_uri, public_key_owner)
        .map_err(remote_actor_authority_error)?;
    let flags = remote_actor_profile_flags(actor);
    let media = remote_actor_profile_media(actor);
    let inbox_uri =
        required_remote_actor_string(actor, "inbox", "remote actor document is missing inbox")?;
    let shared_inbox_uri = remote_actor_shared_inbox_uri(actor);
    ensure_remote_actor_endpoint_authorities(
        &canonical_actor_uri,
        &inbox_uri,
        shared_inbox_uri.as_deref(),
        &public_key_id,
    )?;

    Ok(RemoteActorProfile {
        actor_uri: canonical_actor_uri,
        username: remote_actor_username(actor, &actor_url),
        domain: remote_actor_domain(&actor_url),
        locked: flags.locked,
        bot: flags.bot,
        discoverable: flags.discoverable,
        indexable: flags.indexable,
        inbox_uri,
        shared_inbox_uri,
        public_key_id,
        public_key_pem,
        display_name: remote_actor_optional_string(actor, "name"),
        summary_html: remote_actor_optional_string(actor, "summary"),
        profile_url: media.profile_url,
        avatar_url: media.avatar_url,
        header_url: media.header_url,
    })
}

fn ensure_remote_actor_endpoint_authorities(
    actor_uri: &str,
    inbox_uri: &str,
    shared_inbox_uri: Option<&str>,
    public_key_id: &str,
) -> Result<()> {
    remote_actor_related_uri_authority_allowed(
        actor_uri,
        inbox_uri,
        RemoteActorAuthorityIssue::CrossAuthorityInbox,
    )
    .map_err(remote_actor_authority_error)?;
    if let Some(shared_inbox_uri) = shared_inbox_uri {
        remote_actor_related_uri_authority_allowed(
            actor_uri,
            shared_inbox_uri,
            RemoteActorAuthorityIssue::CrossAuthoritySharedInbox,
        )
        .map_err(remote_actor_authority_error)?;
    }
    remote_actor_related_uri_authority_allowed(
        actor_uri,
        public_key_id,
        RemoteActorAuthorityIssue::CrossAuthorityPublicKey,
    )
    .map_err(remote_actor_authority_error)?;
    Ok(())
}

fn remote_actor_authority_error(issue: RemoteActorAuthorityIssue) -> Error {
    Error::RustError(match issue {
        RemoteActorAuthorityIssue::InvalidActorUri => {
            "remote actor URI is missing a valid http(s) authority".to_owned()
        }
        RemoteActorAuthorityIssue::InvalidRelatedUri => {
            "remote actor related URI is missing a valid http(s) authority".to_owned()
        }
        RemoteActorAuthorityIssue::CrossAuthorityId => {
            "remote actor document id authority did not match fetched URI".to_owned()
        }
        RemoteActorAuthorityIssue::CrossAuthorityInbox => {
            "remote actor inbox authority did not match actor URI".to_owned()
        }
        RemoteActorAuthorityIssue::CrossAuthoritySharedInbox => {
            "remote actor sharedInbox authority did not match actor URI".to_owned()
        }
        RemoteActorAuthorityIssue::CrossAuthorityPublicKey => {
            "remote actor publicKey.id authority did not match actor URI".to_owned()
        }
        RemoteActorAuthorityIssue::PublicKeyOwnerMismatch => {
            "remote actor publicKey.owner did not match actor URI".to_owned()
        }
    })
}

/// Prefer webfinger username when present; reject document preferredUsername mismatch.
pub(crate) fn ensure_remote_actor_username_matches_handle(
    profile: &RemoteActorProfile,
    expected_username: &str,
) -> Result<()> {
    let expected = expected_username.trim().to_ascii_lowercase();
    if expected.is_empty() {
        return Ok(());
    }
    if profile.username != expected {
        return Err(Error::RustError(
            "remote actor preferredUsername did not match looked-up handle".to_owned(),
        ));
    }
    Ok(())
}

fn remote_actor_canonical_uri(actor: &serde_json::Value, fallback_actor_uri: &str) -> String {
    actor
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback_actor_uri)
        .to_owned()
}

fn remote_actor_username(actor: &serde_json::Value, actor_url: &Url) -> String {
    actor
        .get("preferredUsername")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| {
            actor_url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .unwrap_or("remote")
        })
        .to_ascii_lowercase()
}

fn remote_actor_domain(actor_url: &Url) -> String {
    actor_url
        .host_str()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn remote_actor_shared_inbox_uri(actor: &serde_json::Value) -> Option<String> {
    actor
        .get("endpoints")
        .and_then(|value| value.get("sharedInbox"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn remote_actor_optional_string(actor: &serde_json::Value, field: &str) -> String {
    actor
        .get(field)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn required_remote_actor_string(
    actor: &serde_json::Value,
    field: &str,
    missing_message: &str,
) -> Result<String> {
    actor
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::RustError(missing_message.to_owned()))
}

fn required_remote_actor_nested_string(
    actor: &serde_json::Value,
    parent: &str,
    field: &str,
    missing_message: &str,
) -> Result<String> {
    actor
        .get(parent)
        .and_then(|value| value.get(field))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::RustError(missing_message.to_owned()))
}

fn remote_actor_public_key(actor: &serde_json::Value) -> Result<(String, String)> {
    Ok((
        required_remote_actor_nested_string(
            actor,
            "publicKey",
            "id",
            "remote actor document is missing publicKey.id",
        )?,
        required_remote_actor_nested_string(
            actor,
            "publicKey",
            "publicKeyPem",
            "remote actor document is missing publicKey.publicKeyPem",
        )?,
    ))
}

struct RemoteActorProfileFlags {
    locked: bool,
    bot: bool,
    discoverable: bool,
    indexable: bool,
}

fn remote_actor_profile_flags(actor: &serde_json::Value) -> RemoteActorProfileFlags {
    RemoteActorProfileFlags {
        locked: actor
            .get("manuallyApprovesFollowers")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        bot: actor
            .get("bot")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_else(|| remote_actor_type_is_bot(actor.get("type"))),
        discoverable: actor
            .get("discoverable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        indexable: actor
            .get("indexable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
    }
}

struct RemoteActorProfileMedia {
    profile_url: Option<String>,
    avatar_url: Option<String>,
    header_url: Option<String>,
}

fn remote_actor_profile_media(actor: &serde_json::Value) -> RemoteActorProfileMedia {
    RemoteActorProfileMedia {
        profile_url: actor.get("url").and_then(extract_remote_profile_url),
        avatar_url: extract_remote_profile_media_url(actor.get("icon")),
        header_url: extract_remote_profile_media_url(actor.get("image")),
    }
}

pub(crate) async fn validate_remote_actor_profile_urls(profile: &RemoteActorProfile) -> Result<()> {
    let mut validated_ip_hosts = HashSet::<IpAddr>::new();
    validate_remote_actor_profile_url(&profile.actor_uri, &mut validated_ip_hosts).await?;
    validate_remote_actor_profile_url(&profile.inbox_uri, &mut validated_ip_hosts).await?;
    if let Some(shared_inbox_uri) = profile.shared_inbox_uri.as_deref() {
        validate_remote_actor_profile_url(shared_inbox_uri, &mut validated_ip_hosts).await?;
    }
    validate_remote_actor_profile_url(&profile.public_key_id, &mut validated_ip_hosts).await?;
    Ok(())
}

pub(crate) async fn fetch_remote_activitypub_document(url: &str) -> Result<serde_json::Value> {
    fetch_remote_http_json(
        url,
        "application/activity+json, application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"",
    )
    .await
}

fn remote_actor_type_is_bot(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::String(actor_type)) => {
            matches!(actor_type.as_str(), "Application" | "Service")
        }
        Some(serde_json::Value::Array(values)) => values.iter().any(|entry| {
            entry
                .as_str()
                .is_some_and(|actor_type| matches!(actor_type, "Application" | "Service"))
        }),
        _ => false,
    }
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
