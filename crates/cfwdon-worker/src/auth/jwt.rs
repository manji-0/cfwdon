use crate::crypto_keys::subtle_crypto;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cfwdon_core::AppConfig;
use js_sys::{Array, Object, Reflect, Uint8Array};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Algorithm, RsaHashedImportParams};
use worker::{Cache, Error, Fetch, Method, Request, Response, ResponseBody, Result};

const JWT_CLOCK_SKEW_LEEWAY_SECS: u64 = 60;
const AUTH0_JWKS_CACHE_TTL_SECS: u32 = 600;
const AUTH0_JWKS_L1_TTL_MS: f64 = 60_000.0;

#[derive(Debug, Deserialize)]
pub(crate) struct Auth0JwtHeader {
    pub(crate) alg: String,
    pub(crate) kid: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Auth0AudClaim {
    Single(String),
    Multiple(Vec<String>),
}

impl Auth0AudClaim {
    pub(crate) fn contains(&self, expected: &str) -> bool {
        match self {
            Self::Single(value) => value == expected,
            Self::Multiple(values) => values.iter().any(|value| value == expected),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Auth0JwtClaims {
    pub(crate) iss: String,
    pub(crate) aud: Auth0AudClaim,
    #[serde(flatten)]
    pub(crate) extra: serde_json::Map<String, serde_json::Value>,
    pub(crate) exp: Option<u64>,
    pub(crate) nbf: Option<u64>,
    pub(crate) iat: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Auth0Jwk {
    pub(crate) kid: String,
    pub(crate) kty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) alg: Option<String>,
    #[serde(rename = "use", default, skip_serializing_if = "Option::is_none")]
    pub(crate) use_: Option<String>,
    pub(crate) e: String,
    pub(crate) n: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Auth0JwksResponse {
    pub(crate) keys: Vec<Auth0Jwk>,
}

thread_local! {
    // Isolate-local memo (same pattern as remote DNS L1). Cloudflare Cache API
    // below is the cross-request layer; L1 avoids repeat Cache/Fetch work while
    // an isolate is reused. Per-request isolate reuse is not guaranteed.
    static AUTH0_JWKS_L1: RefCell<Option<Auth0JwksL1Entry>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Auth0JwksL1Entry {
    jwks_url: String,
    jwks: Auth0JwksResponse,
    expires_at_ms: f64,
}

impl Auth0JwtClaims {
    pub(crate) fn string_claim(&self, name: &str) -> Option<&str> {
        self.extra.get(name)?.as_str()
    }

    pub(crate) fn claim_value(&self, name: &str) -> Option<&serde_json::Value> {
        self.extra.get(name)
    }
}

pub(crate) async fn verify_auth0_jwt(token: &str, config: &AppConfig) -> Result<Auth0JwtClaims> {
    let (header_segment, payload_segment, signature_segment) =
        split_jwt(token).ok_or_else(|| Error::RustError("malformed Auth0 JWT".to_owned()))?;

    let header: Auth0JwtHeader = decode_jwt_segment(header_segment)?;
    require_rs256_alg(&header.alg)?;

    let jwk = fetch_auth0_jwk(config, &header.kid).await?;
    verify_rs256_signature(
        &jwk,
        format!("{header_segment}.{payload_segment}").as_bytes(),
        &decode_base64url(signature_segment)?,
    )
    .await?;

    let claims: Auth0JwtClaims = decode_jwt_segment(payload_segment)?;
    let expected_issuer = normalized_auth0_issuer(&config.auth0_domain);
    if claims.iss != expected_issuer {
        return Err(Error::RustError("Auth0 JWT issuer mismatch".to_owned()));
    }
    if !claims.aud.contains(&config.auth0_audience) {
        return Err(Error::RustError("Auth0 JWT audience mismatch".to_owned()));
    }

    validate_auth0_time_claims(claims.exp, claims.nbf, claims.iat, current_unix_timestamp())?;
    require_auth0_email_verified(&claims, &config.auth0_email_claim)?;

    Ok(claims)
}

pub(crate) fn require_auth0_email_verified(
    claims: &Auth0JwtClaims,
    email_claim: &str,
) -> Result<()> {
    if auth0_email_verified(claims, email_claim) {
        Ok(())
    } else {
        Err(Error::RustError(
            "Auth0 JWT email is not verified".to_owned(),
        ))
    }
}

pub(crate) fn auth0_email_verified(claims: &Auth0JwtClaims, email_claim: &str) -> bool {
    email_verified_claim_names(email_claim)
        .into_iter()
        .any(|name| claim_value_is_true(claims.claim_value(&name)))
}

fn email_verified_claim_names(email_claim: &str) -> Vec<String> {
    let mut names = Vec::with_capacity(2);
    if let Some(derived) = derived_email_verified_claim_name(email_claim) {
        names.push(derived);
    }
    if !names.iter().any(|name| name == "email_verified") {
        names.push("email_verified".to_owned());
    }
    names
}

fn derived_email_verified_claim_name(email_claim: &str) -> Option<String> {
    let prefix = email_claim.strip_suffix("email")?;
    if prefix.is_empty() || prefix.ends_with('/') || prefix.ends_with('.') {
        Some(format!("{prefix}email_verified"))
    } else {
        None
    }
}

fn claim_value_is_true(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::Bool(true)) => true,
        Some(serde_json::Value::String(text)) if text.eq_ignore_ascii_case("true") => true,
        _ => false,
    }
}

pub(crate) fn validate_auth0_time_claims(
    exp: Option<u64>,
    nbf: Option<u64>,
    iat: Option<u64>,
    now: u64,
) -> Result<()> {
    let Some(exp) = exp else {
        return Err(Error::RustError(
            "Auth0 JWT is missing required exp claim".to_owned(),
        ));
    };
    let leeway = JWT_CLOCK_SKEW_LEEWAY_SECS;
    if exp < now.saturating_sub(leeway) {
        return Err(Error::RustError("Auth0 JWT has expired".to_owned()));
    }
    if let Some(nbf) = nbf
        && nbf > now.saturating_add(leeway)
    {
        return Err(Error::RustError("Auth0 JWT is not yet valid".to_owned()));
    }
    if let Some(iat) = iat
        && iat > now.saturating_add(leeway)
    {
        return Err(Error::RustError(
            "Auth0 JWT iat is unreasonably far in the future".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) fn require_rs256_alg(alg: &str) -> Result<()> {
    if alg == "RS256" {
        Ok(())
    } else {
        Err(Error::RustError(format!(
            "unsupported Auth0 JWT algorithm: {alg}"
        )))
    }
}

fn split_jwt(token: &str) -> Option<(&str, &str, &str)> {
    let mut parts = token.split('.');
    let header = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;

    if parts.next().is_some() {
        return None;
    }

    Some((header, payload, signature))
}

fn decode_jwt_segment<T>(segment: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = decode_base64url(segment)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| Error::RustError(format!("invalid Auth0 JWT payload: {error}")))
}

fn decode_base64url(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|error| Error::RustError(format!("invalid base64url data: {error}")))
}

async fn fetch_auth0_jwk(config: &AppConfig, expected_kid: &str) -> Result<Auth0Jwk> {
    let jwks = fetch_auth0_jwks(config).await?;
    select_auth0_signing_jwk(&jwks.keys, expected_kid)
        .ok_or_else(|| Error::RustError("matching Auth0 signing key was not found".to_owned()))
}

pub(crate) fn select_auth0_signing_jwk(keys: &[Auth0Jwk], expected_kid: &str) -> Option<Auth0Jwk> {
    keys.iter()
        .find(|jwk| jwk.kid == expected_kid && jwk_is_usable_rs256_signing_key(jwk))
        .cloned()
}

pub(crate) fn jwk_is_usable_rs256_signing_key(jwk: &Auth0Jwk) -> bool {
    jwk.kty == "RSA"
        && jwk.alg.as_deref() == Some("RS256")
        && matches!(jwk.use_.as_deref(), None | Some("sig"))
}

async fn fetch_auth0_jwks(config: &AppConfig) -> Result<Auth0JwksResponse> {
    let jwks_url = format!(
        "{}/.well-known/jwks.json",
        normalized_auth0_issuer(&config.auth0_domain).trim_end_matches('/')
    );

    if let Some(jwks) = auth0_jwks_l1_get(&jwks_url) {
        return Ok(jwks);
    }

    if let Some(jwks) = auth0_jwks_cache_get(&jwks_url).await? {
        auth0_jwks_l1_put(&jwks_url, &jwks);
        return Ok(jwks);
    }

    let jwks = fetch_auth0_jwks_uncached(&jwks_url).await?;
    let _ = auth0_jwks_cache_put(&jwks_url, &jwks).await;
    auth0_jwks_l1_put(&jwks_url, &jwks);
    Ok(jwks)
}

async fn fetch_auth0_jwks_uncached(jwks_url: &str) -> Result<Auth0JwksResponse> {
    let request = Request::new(jwks_url, Method::Get)?;
    let mut response = Fetch::Request(request).send().await?;
    response.json().await
}

fn auth0_jwks_l1_get(jwks_url: &str) -> Option<Auth0JwksResponse> {
    let now_ms = js_sys::Date::now();
    AUTH0_JWKS_L1.with(|slot| {
        let mut slot = slot.borrow_mut();
        match slot.as_ref() {
            Some(entry) if entry.jwks_url == jwks_url && entry.expires_at_ms > now_ms => {
                Some(entry.jwks.clone())
            }
            Some(_) => {
                *slot = None;
                None
            }
            None => None,
        }
    })
}

fn auth0_jwks_l1_put(jwks_url: &str, jwks: &Auth0JwksResponse) {
    let expires_at_ms = js_sys::Date::now() + AUTH0_JWKS_L1_TTL_MS;
    AUTH0_JWKS_L1.with(|slot| {
        *slot.borrow_mut() = Some(Auth0JwksL1Entry {
            jwks_url: jwks_url.to_owned(),
            jwks: jwks.clone(),
            expires_at_ms,
        });
    });
}

async fn auth0_jwks_cache_get(jwks_url: &str) -> Result<Option<Auth0JwksResponse>> {
    let Some(mut cached) = Cache::default()
        .get(jwks_url, true)
        .await
        .unwrap_or_default()
    else {
        return Ok(None);
    };
    let body = cached.bytes().await?;
    match serde_json::from_slice::<Auth0JwksResponse>(&body) {
        Ok(jwks) => Ok(Some(jwks)),
        Err(_) => Ok(None),
    }
}

async fn auth0_jwks_cache_put(jwks_url: &str, jwks: &Auth0JwksResponse) -> Result<()> {
    let body = serde_json::to_vec(jwks).map_err(|error| {
        Error::RustError(format!("failed to encode Auth0 JWKS cache body: {error}"))
    })?;
    let mut response = Response::from_body(ResponseBody::Body(body))?;
    response
        .headers_mut()
        .set("Content-Type", "application/json")?;
    response.headers_mut().set(
        "Cache-Control",
        &format!("public, max-age={AUTH0_JWKS_CACHE_TTL_SECS}"),
    )?;
    let _ = Cache::default().put(jwks_url, response).await;
    Ok(())
}

fn normalized_auth0_issuer(domain: &str) -> String {
    let trimmed = domain.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        format!("{trimmed}/")
    } else {
        format!("https://{trimmed}/")
    }
}

async fn verify_rs256_signature(jwk: &Auth0Jwk, data: &[u8], signature: &[u8]) -> Result<()> {
    let subtle = subtle_crypto()?;

    let jwk_value = worker::d1::serde_wasm_bindgen::to_value(jwk)
        .map_err(|error| Error::RustError(format!("failed to serialize JWK: {error}")))?;
    let jwk_object = jwk_value
        .dyn_into::<Object>()
        .map_err(|_| Error::RustError("failed to convert JWK to object".to_owned()))?;

    let import_params = RsaHashedImportParams::new_with_str("SHA-256");
    let import_algorithm: Object = import_params.into();
    Reflect::set(
        &import_algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )
    .map_err(Error::from)?;

    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("verify"));

    let crypto_key = JsFuture::from(subtle.import_key_with_object(
        "jwk",
        &jwk_object,
        &import_algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<web_sys::CryptoKey>()
    .map_err(|_| Error::RustError("failed to import Auth0 public key".to_owned()))?;

    let verify_algorithm = Algorithm::new("RSASSA-PKCS1-v1_5");
    let verify_algorithm: Object = verify_algorithm.into();
    let signature = Uint8Array::from(signature);
    let data = Uint8Array::from(data);

    let verified = JsFuture::from(
        subtle.verify_with_object_and_buffer_source_and_buffer_source(
            &verify_algorithm,
            &crypto_key,
            signature.as_ref(),
            data.as_ref(),
        )?,
    )
    .await?
    .as_bool()
    .unwrap_or(false);

    if verified {
        Ok(())
    } else {
        Err(Error::RustError(
            "Auth0 JWT signature verification failed".to_owned(),
        ))
    }
}

fn current_unix_timestamp() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

#[cfg(test)]
mod tests {
    use super::{
        Auth0AudClaim, Auth0Jwk, Auth0JwtClaims, JWT_CLOCK_SKEW_LEEWAY_SECS, auth0_email_verified,
        jwk_is_usable_rs256_signing_key, normalized_auth0_issuer, require_auth0_email_verified,
        require_rs256_alg, select_auth0_signing_jwk, validate_auth0_time_claims,
    };
    use serde_json::json;

    fn sample_jwk(kid: &str, kty: &str, alg: Option<&str>, use_: Option<&str>) -> Auth0Jwk {
        Auth0Jwk {
            kid: kid.to_owned(),
            kty: kty.to_owned(),
            alg: alg.map(str::to_owned),
            use_: use_.map(str::to_owned),
            e: "AQAB".to_owned(),
            n: "sXch".to_owned(),
        }
    }

    fn claims_with_extra(extra: serde_json::Map<String, serde_json::Value>) -> Auth0JwtClaims {
        Auth0JwtClaims {
            iss: "https://tenant.auth0.com/".to_owned(),
            aud: Auth0AudClaim::Single("https://example.com/api".to_owned()),
            extra,
            exp: Some(1),
            nbf: None,
            iat: None,
        }
    }

    #[test]
    fn normalized_auth0_issuer_adds_https_and_trailing_slash() {
        assert_eq!(
            normalized_auth0_issuer("tenant.auth0.com/"),
            "https://tenant.auth0.com/"
        );
        assert_eq!(
            normalized_auth0_issuer("https://tenant.auth0.com/"),
            "https://tenant.auth0.com/"
        );
    }

    #[test]
    fn missing_exp_is_rejected() {
        let err =
            validate_auth0_time_claims(None, None, None, 1_000).expect_err("missing exp must fail");
        assert!(err.to_string().contains("missing required exp"));
    }

    #[test]
    fn expired_exp_is_rejected() {
        let now: u64 = 1_000;
        let err = validate_auth0_time_claims(
            Some(now.saturating_sub(JWT_CLOCK_SKEW_LEEWAY_SECS + 1)),
            None,
            None,
            now,
        )
        .expect_err("expired exp must fail");
        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn exp_within_leeway_is_accepted() {
        let now: u64 = 1_000;
        validate_auth0_time_claims(
            Some(now.saturating_sub(JWT_CLOCK_SKEW_LEEWAY_SECS)),
            None,
            None,
            now,
        )
        .expect("exp within leeway must pass");
    }

    #[test]
    fn future_nbf_beyond_leeway_is_rejected() {
        let now: u64 = 1_000;
        let err = validate_auth0_time_claims(
            Some(now + 3_600),
            Some(now + JWT_CLOCK_SKEW_LEEWAY_SECS + 1),
            None,
            now,
        )
        .expect_err("future nbf must fail");
        assert!(err.to_string().contains("not yet valid"));
    }

    #[test]
    fn nbf_within_leeway_is_accepted() {
        let now: u64 = 1_000;
        validate_auth0_time_claims(
            Some(now + 3_600),
            Some(now + JWT_CLOCK_SKEW_LEEWAY_SECS),
            None,
            now,
        )
        .expect("nbf within leeway must pass");
    }

    #[test]
    fn future_iat_beyond_leeway_is_rejected() {
        let now: u64 = 1_000;
        let err = validate_auth0_time_claims(
            Some(now + 3_600),
            None,
            Some(now + JWT_CLOCK_SKEW_LEEWAY_SECS + 1),
            now,
        )
        .expect_err("future iat must fail");
        assert!(err.to_string().contains("iat"));
    }

    #[test]
    fn non_rs256_header_alg_is_rejected() {
        let err = require_rs256_alg("HS256").expect_err("non-RS256 must fail");
        assert!(err.to_string().contains("unsupported Auth0 JWT algorithm"));
        assert!(require_rs256_alg("RS256").is_ok());
    }

    #[test]
    fn jwk_filtered_out_when_kty_use_or_alg_mismatch() {
        assert!(!jwk_is_usable_rs256_signing_key(&sample_jwk(
            "kid-1",
            "EC",
            Some("RS256"),
            Some("sig"),
        )));
        assert!(!jwk_is_usable_rs256_signing_key(&sample_jwk(
            "kid-1",
            "RSA",
            Some("RS384"),
            Some("sig"),
        )));
        assert!(!jwk_is_usable_rs256_signing_key(&sample_jwk(
            "kid-1",
            "RSA",
            Some("RS256"),
            Some("enc"),
        )));
        assert!(!jwk_is_usable_rs256_signing_key(&sample_jwk(
            "kid-1",
            "RSA",
            None,
            Some("sig"),
        )));
        assert!(jwk_is_usable_rs256_signing_key(&sample_jwk(
            "kid-1",
            "RSA",
            Some("RS256"),
            None,
        )));
        assert!(jwk_is_usable_rs256_signing_key(&sample_jwk(
            "kid-1",
            "RSA",
            Some("RS256"),
            Some("sig"),
        )));
    }

    #[test]
    fn select_signing_jwk_skips_non_signing_key_with_same_kid() {
        let keys = vec![
            sample_jwk("shared", "RSA", Some("RS256"), Some("enc")),
            sample_jwk("shared", "RSA", Some("RS256"), Some("sig")),
        ];
        let selected = select_auth0_signing_jwk(&keys, "shared").expect("sig key");
        assert_eq!(selected.use_.as_deref(), Some("sig"));
    }

    #[test]
    fn email_verified_false_or_absent_is_rejected() {
        let absent = claims_with_extra(serde_json::Map::new());
        assert!(!auth0_email_verified(&absent, "email"));
        assert!(require_auth0_email_verified(&absent, "email").is_err());

        let mut false_extra = serde_json::Map::new();
        false_extra.insert("email_verified".to_owned(), json!(false));
        let false_claims = claims_with_extra(false_extra);
        assert!(!auth0_email_verified(&false_claims, "email"));
        assert!(require_auth0_email_verified(&false_claims, "email").is_err());
    }

    #[test]
    fn email_verified_true_bool_or_string_is_accepted() {
        let mut bool_extra = serde_json::Map::new();
        bool_extra.insert("email_verified".to_owned(), json!(true));
        assert!(auth0_email_verified(
            &claims_with_extra(bool_extra),
            "email"
        ));

        let mut string_extra = serde_json::Map::new();
        string_extra.insert("email_verified".to_owned(), json!("true"));
        assert!(auth0_email_verified(
            &claims_with_extra(string_extra),
            "email"
        ));
        assert!(
            require_auth0_email_verified(
                &claims_with_extra({
                    let mut extra = serde_json::Map::new();
                    extra.insert("email_verified".to_owned(), json!(true));
                    extra
                }),
                "email"
            )
            .is_ok()
        );
    }

    #[test]
    fn namespaced_email_verified_claim_is_accepted() {
        let email_claim = "https://example.com/claims/email";
        let mut extra = serde_json::Map::new();
        extra.insert(
            "https://example.com/claims/email_verified".to_owned(),
            json!(true),
        );
        assert!(auth0_email_verified(&claims_with_extra(extra), email_claim));
    }
}
