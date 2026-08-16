use super::{
    build_signature_signing_string, build_signature_signing_string_from_parts,
    cached_remote_actor_matches_key, extract_activity_actor_uri, now_http_date_string,
    parse_signature_header, sha256_http_digest, sign_http_signature,
    validate_activitypub_signature_headers, validate_request_date, validate_request_digest,
    validate_signed_host_header, verify_http_signature_bytes,
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
use worker::{Error, Fetch, Headers, Method, Request, RequestInit, RequestRedirect, Result};

use crate::D1Database;
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

        return response.json().await;
    }

    Err(Error::RustError(format!(
        "signed remote fetch exceeded redirect limit for {url}"
    )))
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

async fn resolve_remote_actor_for_signature(
    db: &D1Database,
    key_id: &str,
    signing_string: &[u8],
    signature: &[u8],
) -> Result<RemoteActorProfile> {
    let actor_uri = actor_uri_from_key_id(key_id)?;
    if let Some(remote_actor) =
        find_cached_remote_actor_profile_by_actor_uri(db, &actor_uri).await?
        && cached_remote_actor_matches_key(&remote_actor, key_id, &actor_uri)
        && verify_http_signature_bytes(&remote_actor.public_key_pem, signing_string, signature)
            .await
            .is_ok()
    {
        return Ok(remote_actor);
    }

    let remote_actor = fetch_remote_actor_profile(&actor_uri).await?;
    if !cached_remote_actor_matches_key(&remote_actor, key_id, &actor_uri) {
        return Err(Error::RustError(
            "Signature keyId did not match delivery actor".to_owned(),
        ));
    }
    verify_http_signature_bytes(&remote_actor.public_key_pem, signing_string, signature).await?;
    upsert_remote_actor(db, &remote_actor).await?;
    Ok(remote_actor)
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
    let activity_type = crate::activitypub_primary_type(activity).unwrap_or("Unknown");
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
}
