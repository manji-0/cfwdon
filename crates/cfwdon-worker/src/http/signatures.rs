use super::load_instance_actor_signing_key;
use super::signer_cache::{
    recently_verified_signer, remember_signer_refetch, remember_verified_signer,
    signer_refetched_at,
};
use super::{
    build_signature_signing_string, build_signature_signing_string_from_parts,
    cached_remote_actor_matches_key, extract_activity_actor_uri, now_http_date_string,
    parse_signature_header, sha256_http_digest, sign_http_signature,
    validate_activitypub_signature_headers, validate_request_date, validate_request_digest,
    validate_signature_time, validate_signed_host_header, verify_http_signature_bytes,
};
use crate::activitypub::activitypub_primary_type;
use crate::federation::{
    RemoteActorProfile, SecureModeFetcher, fetch_remote_actor_profile,
    fetch_unsigned_activitypub_document, install_secure_mode_fetcher, parse_http_url_parts,
    parse_remote_http_json_response, parse_remote_http_url, resolve_remote_redirect_location,
    suspended_instance_domains, url_host_within_blocked_domains, validate_remote_fetch_url,
};
use crate::identity::instance_actor_key_id;
use crate::observability::log_json_event;
use crate::store::remote::{find_cached_remote_actor_profile_and_refresh, upsert_remote_actor};
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{
    SignedFetchVerdict, activitypub_fetch_signature_lists_required_headers,
    activitypub_key_id_matches_actor, remote_key_refetch_allowed,
};
use std::rc::Rc;
use worker::{Env, Error, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, Result};
const MAX_SIGNED_REMOTE_FETCH_REDIRECTS: usize = 5;
pub(super) const ACTIVITYPUB_ACCEPT: &str = "application/activity+json, application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"";
pub(super) const ACTIVITYPUB_CONTENT_TYPE: &str = "application/activity+json";
const SIGNED_POST_HEADERS: &[&str] =
    &["(request-target)", "host", "date", "digest", "content-type"];

pub(crate) fn signed_get_signing_string(path_and_query: &str, host: &str, date: &str) -> String {
    format!("(request-target): get {path_and_query}\nhost: {host}\ndate: {date}")
}

pub(crate) fn signed_post_signing_string(
    path_and_query: &str,
    host: &str,
    date: &str,
    digest: &str,
    content_type: &str,
) -> Result<String> {
    build_signature_signing_string_from_parts(
        "POST",
        path_and_query,
        SIGNED_POST_HEADERS,
        &[
            ("host", host),
            ("date", date),
            ("digest", digest),
            ("content-type", content_type),
        ],
    )
}

/// Signed GET as the instance actor (`/actor#main-key`), as Mastodon signs
/// the fetches it makes on the server's behalf: servers in secure mode answer
/// it, and no local user's identity is attached to the request.
pub(crate) async fn fetch_signed_activitypub_document(
    config: &AppConfig,
    db: &D1Database,
    url: &str,
) -> Result<serde_json::Value> {
    let signing_key = load_instance_actor_signing_key(db, config).await?;
    let key_id = instance_actor_key_id(config);
    let mut current_url = parse_remote_http_url(url)?;
    validate_remote_fetch_url(&current_url).await?;

    for redirect_count in 0..=MAX_SIGNED_REMOTE_FETCH_REDIRECTS {
        let (host, path_and_query) = parse_http_url_parts(current_url.as_str())?;
        let date = now_http_date_string()?;
        let signing_string = signed_get_signing_string(&path_and_query, &host, &date);
        let signature = sign_http_signature(&signing_key, signing_string.as_bytes()).await?;

        let headers = Headers::new();
        headers.set("Accept", ACTIVITYPUB_ACCEPT)?;
        headers.set(
            "User-Agent",
            &format!(
                "cfwdon (+https://{}/)",
                config
                    .instance_domain
                    .trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .trim_end_matches('/')
            ),
        )?;
        headers.set("Date", &date)?;
        headers.set(
            "Signature",
            &format!(
                "keyId=\"{key_id}\",algorithm=\"rsa-sha256\",headers=\"(request-target) host date\",signature=\"{signature}\""
            ),
        )?;

        let mut init = RequestInit::new();
        init.with_method(Method::Get)
            .with_headers(headers)
            .with_redirect(RequestRedirect::Manual);
        let request = Request::new_with_init(current_url.as_str(), &init)?;
        let mut response = Fetch::Request(request).send().await?;
        let status = response.status_code();

        if (300..400).contains(&status) {
            if redirect_count == MAX_SIGNED_REMOTE_FETCH_REDIRECTS {
                return Err(Error::RustError(format!(
                    "signed remote fetch exceeded redirect limit for {url}"
                )));
            }
            let location = response.headers().get("Location")?.ok_or_else(|| {
                Error::RustError(format!(
                    "signed remote fetch redirect missing Location header for {}",
                    current_url
                ))
            })?;
            current_url = resolve_remote_redirect_location(&current_url, &location)?;
            validate_remote_fetch_url(&current_url).await?;
            continue;
        }

        if status / 100 != 2 {
            return Err(Error::RustError(format!(
                "failed to fetch signed remote document {}: HTTP {}",
                current_url, status
            )));
        }

        return parse_remote_http_json_response(&mut response, current_url.as_str(), status).await;
    }

    Err(Error::RustError(format!(
        "signed remote fetch exceeded redirect limit for {url}"
    )))
}

/// Whether a failed signed GET may be retried unsigned. Never in secure mode,
/// as Mastodon never retries unsigned: the retry would double the traffic of
/// every failed fetch, key fetches included, and a remote that refused the
/// signed GET has no reason to answer an unsigned one. Outside secure mode,
/// auth rejection (401/403) is the common case, but some remotes (and
/// Cloudflare WAF in front of them) return other status codes or non-JSON
/// challenge bodies that still leave the unsigned public document reachable.
fn signed_fetch_allows_unsigned_fallback(secure_mode: bool, error: &Error) -> bool {
    if secure_mode {
        return false;
    }
    let message = error.to_string();
    message.contains("HTTP ")
        || message.contains("error decoding response body")
        || message.contains("expected value")
        || message.contains("invalid type")
        || message.contains("EOF while parsing")
}

/// A GET signed as the instance actor, retried unsigned when
/// `signed_fetch_allows_unsigned_fallback` allows it.
pub(crate) async fn fetch_activitypub_document_as_instance(
    config: &AppConfig,
    db: &D1Database,
    url: &str,
) -> Result<serde_json::Value> {
    match fetch_signed_activitypub_document(config, db, url).await {
        Ok(document) => Ok(document),
        Err(error) if signed_fetch_allows_unsigned_fallback(config.authorized_fetch, &error) => {
            log_json_event(serde_json::json!({
                "event": "signed_remote_fetch_fallback",
                "url": url,
                "error": error.to_string(),
            }));
            fetch_unsigned_activitypub_document(url).await
        }
        Err(error) => Err(error),
    }
}

/// In secure mode (`AUTHORIZED_FETCH`), sign every ActivityPub GET this
/// isolate makes as the instance actor, key fetches for signature checks
/// included. A remote in secure mode verifies such a fetch with this server's
/// `/actor`, which stays public, so a key fetch never waits on one of its own.
pub(crate) fn install_secure_mode_fetch_signing(env: &Env, config: &AppConfig) {
    if !config.authorized_fetch {
        install_secure_mode_fetcher(None);
        return;
    }
    let binding = match env.d1(&config.database_binding) {
        Ok(binding) => binding,
        Err(error) => {
            log_json_event(serde_json::json!({
                "event": "secure_mode_fetch_signing_unavailable",
                "level": "warn",
                "binding": config.database_binding,
                "error": error.to_string(),
                "message": "AUTHORIZED_FETCH is on but the instance actor key cannot be read; remote ActivityPub GETs go out unsigned",
            }));
            install_secure_mode_fetcher(None);
            return;
        }
    };
    let db = D1Database::new(binding);
    let config = config.clone();
    install_secure_mode_fetcher(Some(Rc::new(move |url: String| {
        let db = db.detached();
        let config = config.clone();
        Box::pin(async move { fetch_activitypub_document_as_instance(&config, &db, &url).await })
            as futures_util::future::LocalBoxFuture<'static, _>
    }) as SecureModeFetcher));
}

/// Who signed a GET of a local ActivityPub resource (an "authorized fetch").
pub(crate) enum SignedFetch {
    Unsigned,
    Invalid(Error),
    Refused,
    Signed(RemoteActorProfile),
}

impl SignedFetch {
    pub(crate) fn verdict(&self) -> SignedFetchVerdict {
        match self {
            Self::Unsigned => SignedFetchVerdict::Unsigned,
            Self::Invalid(_) => SignedFetchVerdict::Invalid,
            Self::Refused => SignedFetchVerdict::Refused,
            Self::Signed(_) => SignedFetchVerdict::Signed,
        }
    }
}

/// Verify the `Signature` of a GET as the inbox verifies a delivery, without
/// the `Digest` a GET has no body for. A key or actor on a suspended domain is
/// refused, the key before anything is fetched from its server.
pub(crate) async fn verify_signed_fetch(req: &Request, db: &D1Database) -> Result<SignedFetch> {
    let Some(header) = req.headers().get("Signature")? else {
        return Ok(SignedFetch::Unsigned);
    };
    let Ok(parsed_signature) = parse_signature_header(&header) else {
        return Ok(SignedFetch::Unsigned);
    };
    let suspended_domains = suspended_instance_domains().await?;
    if url_host_within_blocked_domains(&parsed_signature.key_id, &suspended_domains) {
        return Ok(SignedFetch::Refused);
    }
    match verify_fetch_signature(req, db, &parsed_signature).await {
        Err(error) => Ok(SignedFetch::Invalid(error)),
        Ok(actor) if url_host_within_blocked_domains(&actor.actor_uri, &suspended_domains) => {
            Ok(SignedFetch::Refused)
        }
        Ok(actor) => Ok(SignedFetch::Signed(actor)),
    }
}

async fn verify_fetch_signature(
    req: &Request,
    db: &D1Database,
    parsed_signature: &super::ParsedSignatureHeader,
) -> Result<RemoteActorProfile> {
    if !activitypub_fetch_signature_lists_required_headers(&parsed_signature.headers) {
        return Err(Error::RustError(
            "Signature must cover (request-target), host, and date or (created)".to_owned(),
        ));
    }
    validate_signed_host_header(req, req.headers(), parsed_signature)?;
    let signing_string = build_signature_signing_string(req, req.headers(), parsed_signature)?;
    validate_signature_time(req.headers(), parsed_signature)?;
    resolve_remote_actor_for_signature(
        db,
        &parsed_signature.key_id,
        signing_string.as_bytes(),
        &parsed_signature.signature,
    )
    .await
}

pub(crate) async fn verify_incoming_activitypub_request(
    req: &Request,
    db: &D1Database,
    body: &[u8],
    activity: &serde_json::Value,
) -> Result<RemoteActorProfile> {
    let delivery = verify_incoming_activitypub_delivery(req, db, body, activity).await?;
    if delivery.relayed {
        return Err(Error::RustError(
            "activitypub unauthorized: relayed delivery must use shared inbox handler".to_owned(),
        ));
    }
    Ok(delivery.delivery_actor)
}

#[derive(Debug, Clone)]
pub(crate) struct VerifiedActivityPubDelivery {
    pub(crate) delivery_actor: RemoteActorProfile,
    pub(crate) content_actor_uri: String,
    pub(crate) relayed: bool,
}

pub(crate) async fn verify_incoming_activitypub_delivery(
    req: &Request,
    db: &D1Database,
    body: &[u8],
    activity: &serde_json::Value,
) -> Result<VerifiedActivityPubDelivery> {
    let content_actor_uri = extract_activity_actor_uri(activity)?;
    let signature_header = req
        .headers()
        .get("Signature")?
        .ok_or_else(|| Error::RustError("missing Signature header".to_owned()))?;
    let parsed_signature = parse_signature_header(&signature_header)?;
    validate_activitypub_signature_headers(&parsed_signature)?;
    validate_signed_host_header(req, req.headers(), &parsed_signature)?;
    let signing_string = build_signature_signing_string(req, req.headers(), &parsed_signature)?;

    validate_request_date(req.headers())?;
    validate_request_digest(req.headers(), body).await?;

    let delivery_actor = resolve_remote_actor_for_signature(
        db,
        &parsed_signature.key_id,
        signing_string.as_bytes(),
        &parsed_signature.signature,
    )
    .await?;
    let relayed = delivery_actor.actor_uri != content_actor_uri;
    Ok(VerifiedActivityPubDelivery {
        delivery_actor,
        content_actor_uri,
        relayed,
    })
}

/// The actor whose key verifies a signature: this isolate's recent copy, the
/// D1 cache, or, when neither verifies, the actor fetched again. The refetch
/// waits out `REMOTE_KEY_REFETCH_COOLDOWN_MS` since the last fetch of that
/// actor, by any isolate (D1's `updated_at`) or by this one (a fetch that
/// failed stamps no row), so bogus signatures naming a foreign `keyId` cannot
/// make this server fetch that actor on every request. The fetched profile
/// is stored before its key is checked, which stamps the refetch for every
/// isolate whether or not the signature verifies.
async fn resolve_remote_actor_for_signature(
    db: &D1Database,
    key_id: &str,
    signing_string: &[u8],
    signature: &[u8],
) -> Result<RemoteActorProfile> {
    let actor_uri = actor_uri_from_key_id(key_id)?;
    let now_ms = js_sys::Date::now();
    if let Some(actor) = recently_verified_signer(&actor_uri, now_ms)
        && cached_actor_verifies(&actor, key_id, &actor_uri, signing_string, signature).await
    {
        return Ok(actor);
    }

    let cached = find_cached_remote_actor_profile_and_refresh(db, &actor_uri).await?;
    if let Some((actor, _)) = cached.as_ref()
        && cached_actor_verifies(actor, key_id, &actor_uri, signing_string, signature).await
    {
        remember_verified_signer(&actor_uri, actor, now_ms);
        return Ok(actor.clone());
    }

    let last_refreshed_ms = cached
        .and_then(|(_, refreshed_at_ms)| refreshed_at_ms)
        .into_iter()
        .chain(signer_refetched_at(&actor_uri))
        .reduce(f64::max);
    if !remote_key_refetch_allowed(last_refreshed_ms, now_ms) {
        return Err(Error::RustError(
            "Signature did not verify, and the signer's key was refetched within the cooldown"
                .to_owned(),
        ));
    }
    remember_signer_refetch(&actor_uri, now_ms);

    let remote_actor = fetch_remote_actor_profile(&actor_uri).await?;
    if !cached_remote_actor_matches_key(&remote_actor, key_id, &actor_uri) {
        return Err(Error::RustError(
            "Signature keyId did not match delivery actor".to_owned(),
        ));
    }
    upsert_remote_actor(db, &remote_actor).await?;
    verify_http_signature_bytes(&remote_actor.public_key_pem, signing_string, signature).await?;
    remember_verified_signer(&actor_uri, &remote_actor, now_ms);
    Ok(remote_actor)
}

async fn cached_actor_verifies(
    actor: &RemoteActorProfile,
    key_id: &str,
    actor_uri: &str,
    signing_string: &[u8],
    signature: &[u8],
) -> bool {
    cached_actor_can_verify_key(actor, key_id, actor_uri)
        && verify_http_signature_bytes(&actor.public_key_pem, signing_string, signature)
            .await
            .is_ok()
}

fn cached_actor_can_verify_key(
    remote_actor: &RemoteActorProfile,
    key_id: &str,
    actor_uri: &str,
) -> bool {
    cached_remote_actor_matches_key(remote_actor, key_id, actor_uri)
        || activitypub_key_id_matches_actor(key_id, actor_uri, &remote_actor.actor_uri)
}

fn actor_uri_from_key_id(key_id: &str) -> Result<String> {
    let parsed = parse_remote_http_url(key_id)?;
    let mut actor_url = parsed.clone();
    actor_url.set_fragment(None);
    Ok(actor_url.to_string())
}

pub(crate) fn inbox_activity_id(activity: &serde_json::Value) -> Option<String> {
    activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) async fn inbox_activity_dedupe_id(
    activity: &serde_json::Value,
    remote_actor_uri: &str,
    body: &[u8],
) -> Result<String> {
    if let Some(id) = inbox_activity_id(activity) {
        return Ok(id);
    }
    let activity_type = activitypub_primary_type(activity).unwrap_or("Unknown");
    let digest = sha256_http_digest(body).await?;
    Ok(format!(
        "derived:{remote_actor_uri}:{activity_type}:{digest}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_post_signing_string_covers_content_type_after_digest() {
        let signing_string = signed_post_signing_string(
            "/users/alice/inbox",
            "social.example",
            "Sat, 25 Jul 2026 00:00:00 GMT",
            "SHA-256=abc",
            ACTIVITYPUB_CONTENT_TYPE,
        )
        .unwrap();
        assert_eq!(
            signing_string,
            "(request-target): post /users/alice/inbox\nhost: social.example\ndate: Sat, 25 Jul 2026 00:00:00 GMT\ndigest: SHA-256=abc\ncontent-type: application/activity+json"
        );
        assert_eq!(
            SIGNED_POST_HEADERS.join(" "),
            "(request-target) host date digest content-type"
        );
    }

    fn sample_remote_actor(public_key_id: &str) -> RemoteActorProfile {
        RemoteActorProfile {
            actor_uri: "https://remote.example/users/bob".to_owned(),
            username: "bob".to_owned(),
            domain: "remote.example".to_owned(),
            locked: false,
            bot: false,
            discoverable: true,
            indexable: true,
            inbox_uri: "https://remote.example/users/bob/inbox".to_owned(),
            shared_inbox_uri: None,
            public_key_id: public_key_id.to_owned(),
            public_key_pem: "pem".to_owned(),
            display_name: "Bob".to_owned(),
            summary_html: String::new(),
            profile_url: None,
            avatar_url: None,
            header_url: None,
        }
    }

    #[test]
    fn cached_actor_can_verify_key_when_key_id_fragment_differs() {
        let actor = sample_remote_actor("https://remote.example/users/bob#main-key");
        assert!(cached_actor_can_verify_key(
            &actor,
            "https://remote.example/users/bob",
            "https://remote.example/users/bob",
        ));
        assert!(!cached_remote_actor_matches_key(
            &actor,
            "https://remote.example/users/bob",
            "https://remote.example/users/bob",
        ));
    }

    #[test]
    fn signed_fetch_falls_back_unsigned_only_outside_secure_mode() {
        let failures = [
            "failed to fetch signed remote document https://x: HTTP 401",
            "failed to fetch signed remote document https://x: HTTP 404",
            "failed to fetch signed remote document https://x: HTTP 523",
            "error decoding response body: expected value at line 1 column 1",
        ];
        for failure in failures {
            let error = Error::RustError(failure.to_owned());
            assert!(
                signed_fetch_allows_unsigned_fallback(false, &error),
                "{failure}"
            );
            assert!(
                !signed_fetch_allows_unsigned_fallback(true, &error),
                "{failure}"
            );
        }
        assert!(!signed_fetch_allows_unsigned_fallback(
            false,
            &Error::RustError("remote fetch blocked for private address".to_owned())
        ));
    }
}
