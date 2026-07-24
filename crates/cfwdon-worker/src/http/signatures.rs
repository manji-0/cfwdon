use super::{
    build_signature_signing_string, cached_remote_actor_matches_key, extract_activity_actor_uri,
    now_http_date_string, parse_signature_header, sha256_http_digest, sign_http_signature,
    validate_activitypub_signature_headers, validate_request_date, validate_request_digest,
    verify_http_signature_bytes,
};
use crate::auth::load_account_private_key_jwk;
use crate::federation::{
    RemoteActorProfile, fetch_remote_actor_profile, parse_http_url_parts, parse_remote_http_url,
    resolve_remote_redirect_location, validate_remote_fetch_url,
};
use crate::instance::public_key_id;
use crate::remote::{find_cached_remote_actor_profile_by_actor_uri, upsert_remote_actor};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use wasm_bindgen::JsValue;
use worker::{
    D1Database, Error, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, Result,
};

const MAX_SIGNED_REMOTE_FETCH_REDIRECTS: usize = 5;
const ACTIVITYPUB_ACCEPT: &str = "application/activity+json, application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"";

pub(crate) fn signed_get_signing_string(path_and_query: &str, host: &str, date: &str) -> String {
    format!("(request-target): get {path_and_query}\nhost: {host}\ndate: {date}")
}

pub(crate) async fn fetch_signed_activitypub_document(
    config: &AppConfig,
    db: &D1Database,
    account: &LocalAccount,
    url: &str,
) -> Result<serde_json::Value> {
    let private_key_jwk = load_account_private_key_jwk(db, config, account.id())
        .await?
        .ok_or_else(|| Error::RustError("account private signing key is missing".to_owned()))?;
    let key_id = public_key_id(config, account.username());
    let mut current_url = parse_remote_http_url(url)?;
    validate_remote_fetch_url(&current_url).await?;

    for redirect_count in 0..=MAX_SIGNED_REMOTE_FETCH_REDIRECTS {
        let (host, path_and_query) = parse_http_url_parts(current_url.as_str())?;
        let date = now_http_date_string()?;
        let signing_string = signed_get_signing_string(&path_and_query, &host, &date);
        let signature = sign_http_signature(&private_key_jwk, signing_string.as_bytes()).await?;

        let headers = Headers::new();
        headers.set("Accept", ACTIVITYPUB_ACCEPT)?;
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

        return response.json().await;
    }

    Err(Error::RustError(format!(
        "signed remote fetch exceeded redirect limit for {url}"
    )))
}

#[derive(Debug)]
pub(crate) struct SignedDeliveryFailure {
    pub(crate) outcome: cfwdon_domain::DeliveryAttemptOutcome,
    pub(crate) detail: String,
}

impl SignedDeliveryFailure {
    pub(crate) fn retryable(detail: impl Into<String>) -> Self {
        Self {
            outcome: cfwdon_domain::DeliveryAttemptOutcome::Failure,
            detail: detail.into(),
        }
    }

    pub(crate) fn permanent(detail: impl Into<String>) -> Self {
        Self {
            outcome: cfwdon_domain::DeliveryAttemptOutcome::PermanentFailure,
            detail: detail.into(),
        }
    }

    pub(crate) fn is_permanent(&self) -> bool {
        matches!(
            self.outcome,
            cfwdon_domain::DeliveryAttemptOutcome::PermanentFailure
        )
    }
}

pub(crate) async fn send_signed_activity(
    config: &AppConfig,
    db: &D1Database,
    account: &LocalAccount,
    inbox_url: &str,
    payload_json: &str,
) -> std::result::Result<(), SignedDeliveryFailure> {
    let inbox = parse_remote_http_url(inbox_url).map_err(|error| {
        SignedDeliveryFailure::permanent(format!("invalid remote inbox URL: {error}"))
    })?;
    validate_remote_fetch_url(&inbox).await.map_err(|error| {
        SignedDeliveryFailure::permanent(format!("remote inbox URL rejected: {error}"))
    })?;
    let (host, path_and_query) = parse_http_url_parts(inbox.as_str()).map_err(|error| {
        SignedDeliveryFailure::permanent(format!("failed to parse inbox URL parts: {error}"))
    })?;
    let date = now_http_date_string().map_err(|error| {
        SignedDeliveryFailure::retryable(format!("failed to build Date header: {error}"))
    })?;
    let digest = sha256_http_digest(payload_json.as_bytes())
        .await
        .map_err(|error| {
            SignedDeliveryFailure::retryable(format!("failed to build Digest header: {error}"))
        })?;
    let signing_string = format!(
        "(request-target): post {path_and_query}\nhost: {host}\ndate: {date}\ndigest: {digest}"
    );
    let private_key_jwk = load_account_private_key_jwk(db, config, account.id())
        .await
        .map_err(|error| {
            SignedDeliveryFailure::retryable(format!("failed to load signing key: {error}"))
        })?
        .ok_or_else(|| {
            SignedDeliveryFailure::permanent("account private signing key is missing")
        })?;
    let signature = sign_http_signature(&private_key_jwk, signing_string.as_bytes())
        .await
        .map_err(|error| {
            SignedDeliveryFailure::retryable(format!("failed to sign delivery request: {error}"))
        })?;

    let headers = Headers::new();
    headers
        .set("Accept", "application/activity+json")
        .map_err(|error| {
            SignedDeliveryFailure::retryable(format!("failed to set Accept header: {error}"))
        })?;
    headers
        .set("Content-Type", "application/activity+json")
        .map_err(|error| {
            SignedDeliveryFailure::retryable(format!("failed to set Content-Type header: {error}"))
        })?;
    headers.set("Date", &date).map_err(|error| {
        SignedDeliveryFailure::retryable(format!("failed to set Date header: {error}"))
    })?;
    headers.set("Digest", &digest).map_err(|error| {
        SignedDeliveryFailure::retryable(format!("failed to set Digest header: {error}"))
    })?;
    headers
        .set(
            "Signature",
            &format!(
                "keyId=\"{}\",algorithm=\"rsa-sha256\",headers=\"(request-target) host date digest\",signature=\"{}\"",
                public_key_id(config, account.username()),
                signature
            ),
        )
        .map_err(|error| {
            SignedDeliveryFailure::retryable(format!("failed to set Signature header: {error}"))
        })?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(payload_json)))
        .with_redirect(RequestRedirect::Manual);

    let request = Request::new_with_init(inbox.as_str(), &init).map_err(|error| {
        SignedDeliveryFailure::retryable(format!("failed to build delivery request: {error}"))
    })?;
    let response = Fetch::Request(request).send().await.map_err(|error| {
        SignedDeliveryFailure::retryable(format!("remote inbox delivery fetch failed: {error}"))
    })?;
    let status = response.status_code();
    match cfwdon_domain::delivery_disposition_for_http_status(status) {
        cfwdon_domain::DeliveryAttemptOutcome::Success => Ok(()),
        cfwdon_domain::DeliveryAttemptOutcome::PermanentFailure => {
            Err(SignedDeliveryFailure::permanent(format!(
                "remote inbox rejected activity with HTTP {status}"
            )))
        }
        cfwdon_domain::DeliveryAttemptOutcome::Failure => {
            let detail = if (300..400).contains(&status) {
                format!("remote inbox redirected signed delivery with HTTP {status}")
            } else {
                format!("remote inbox rejected activity with HTTP {status}")
            };
            Err(SignedDeliveryFailure::retryable(detail))
        }
    }
}

pub(crate) async fn verify_incoming_activitypub_request(
    req: &Request,
    db: &D1Database,
    body: &[u8],
    activity: &serde_json::Value,
) -> Result<RemoteActorProfile> {
    let actor_uri = extract_activity_actor_uri(activity)?;
    let signature_header = req
        .headers()
        .get("Signature")?
        .ok_or_else(|| Error::RustError("missing Signature header".to_owned()))?;
    let parsed_signature = parse_signature_header(&signature_header)?;
    validate_activitypub_signature_headers(&parsed_signature)?;
    let signing_string = build_signature_signing_string(req, req.headers(), &parsed_signature)?;

    validate_request_date(req.headers())?;
    validate_request_digest(req.headers(), body).await?;

    if let Some(remote_actor) =
        find_cached_remote_actor_profile_by_actor_uri(db, &actor_uri).await?
        && cached_remote_actor_matches_key(&remote_actor, &parsed_signature.key_id, &actor_uri)
        && verify_http_signature_bytes(
            &remote_actor.public_key_pem,
            signing_string.as_bytes(),
            &parsed_signature.signature,
        )
        .await
        .is_ok()
    {
        return Ok(remote_actor);
    }

    let remote_actor = fetch_remote_actor_profile(&actor_uri).await?;
    if !cached_remote_actor_matches_key(&remote_actor, &parsed_signature.key_id, &actor_uri) {
        return Err(Error::RustError(
            "Signature keyId did not match activity actor".to_owned(),
        ));
    }
    verify_http_signature_bytes(
        &remote_actor.public_key_pem,
        signing_string.as_bytes(),
        &parsed_signature.signature,
    )
    .await?;
    upsert_remote_actor(db, &remote_actor).await?;

    Ok(remote_actor)
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
    let activity_type = activity
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Unknown");
    let digest = sha256_http_digest(body).await?;
    Ok(format!(
        "derived:{remote_actor_uri}:{activity_type}:{digest}"
    ))
}
