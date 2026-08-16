use super::signatures::{
    signed_post_signing_string, ACTIVITYPUB_ACCEPT, ACTIVITYPUB_CONTENT_TYPE,
};
use crate::auth::load_account_private_key_jwk;
use crate::federation::{parse_http_url_parts, parse_remote_http_url, validate_remote_fetch_url};
use crate::instance::public_key_id;
use crate::{now_http_date_string, sha256_http_digest, sign_http_signature};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use wasm_bindgen::JsValue;
use worker::{Fetch, Headers, Method, Request, RequestInit, RequestRedirect};

use crate::D1Database;

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

struct SignedPostRequest {
    inbox_url: String,
    headers: Headers,
    body: String,
}

async fn build_signed_post_request(
    config: &AppConfig,
    db: &D1Database,
    account: &LocalAccount,
    inbox_url: &str,
    payload_json: &str,
) -> Result<SignedPostRequest, SignedDeliveryFailure> {
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
    let signing_string = signed_post_signing_string(
        &path_and_query,
        &host,
        &date,
        &digest,
        ACTIVITYPUB_CONTENT_TYPE,
    )
    .map_err(|error| {
        SignedDeliveryFailure::retryable(format!("failed to build signing string: {error}"))
    })?;
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
    headers.set("Accept", ACTIVITYPUB_ACCEPT).map_err(|error| {
        SignedDeliveryFailure::retryable(format!("failed to set Accept header: {error}"))
    })?;
    headers
        .set("Content-Type", ACTIVITYPUB_CONTENT_TYPE)
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
                "keyId=\"{}\",algorithm=\"rsa-sha256\",headers=\"(request-target) host date digest content-type\",signature=\"{}\"",
                public_key_id(config, account.username()),
                signature
            ),
        )
        .map_err(|error| {
            SignedDeliveryFailure::retryable(format!("failed to set Signature header: {error}"))
        })?;

    Ok(SignedPostRequest {
        inbox_url: inbox.to_string(),
        headers,
        body: payload_json.to_owned(),
    })
}

async fn dispatch_signed_post(request: SignedPostRequest) -> Result<(), SignedDeliveryFailure> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(request.headers)
        .with_body(Some(JsValue::from_str(&request.body)))
        .with_redirect(RequestRedirect::Manual);

    let request = Request::new_with_init(request.inbox_url.as_str(), &init).map_err(|error| {
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

pub(crate) async fn send_signed_activity(
    config: &AppConfig,
    db: &D1Database,
    account: &LocalAccount,
    inbox_url: &str,
    payload_json: &str,
) -> std::result::Result<(), SignedDeliveryFailure> {
    let request =
        build_signed_post_request(config, db, account, inbox_url, payload_json).await?;
    dispatch_signed_post(request).await
}
