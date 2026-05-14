use super::{
    build_signature_signing_string, cached_remote_actor_matches_key, extract_activity_actor_uri,
    now_http_date_string, parse_signature_header, sha256_http_digest, sign_http_signature,
    validate_request_date, validate_request_digest, verify_http_signature_bytes,
};
use crate::federation::{RemoteActorProfile, fetch_remote_actor_profile, parse_http_url_parts};
use crate::instance::public_key_id;
use crate::remote::{find_cached_remote_actor_profile_by_actor_uri, upsert_remote_actor};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use wasm_bindgen::JsValue;
use worker::{D1Database, Error, Fetch, Headers, Method, Request, RequestInit, Result};

pub(crate) async fn send_signed_activity(
    config: &AppConfig,
    account: &LocalAccount,
    inbox_url: &str,
    payload_json: &str,
) -> Result<()> {
    let (host, path_and_query) = parse_http_url_parts(inbox_url)?;
    let date = now_http_date_string()?;
    let digest = sha256_http_digest(payload_json.as_bytes()).await?;
    let signing_string = format!(
        "(request-target): post {path_and_query}\nhost: {host}\ndate: {date}\ndigest: {digest}"
    );
    let signature =
        sign_http_signature(&account.private_key_jwk, signing_string.as_bytes()).await?;

    let headers = Headers::new();
    headers.set("Accept", "application/activity+json")?;
    headers.set("Content-Type", "application/activity+json")?;
    headers.set("Date", &date)?;
    headers.set("Digest", &digest)?;
    headers.set(
        "Signature",
        &format!(
            "keyId=\"{}\",algorithm=\"rsa-sha256\",headers=\"(request-target) host date digest\",signature=\"{}\"",
            public_key_id(config, &account.username),
            signature
        ),
    )?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(payload_json)));

    let request = Request::new_with_init(inbox_url, &init)?;
    let response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 == 2 {
        Ok(())
    } else {
        Err(Error::RustError(format!(
            "remote inbox rejected activity with HTTP {}",
            response.status_code()
        )))
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
