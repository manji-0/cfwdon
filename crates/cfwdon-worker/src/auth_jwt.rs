use super::crypto_keys::subtle_crypto;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cfwdon_core::AppConfig;
use js_sys::{Array, Object, Reflect, Uint8Array};
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Algorithm, RsaHashedImportParams};
use worker::{Error, Fetch, Method, Request, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct AccessJwtHeader {
    pub(crate) alg: String,
    pub(crate) kid: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum AccessAudClaim {
    Single(String),
    Multiple(Vec<String>),
}

impl AccessAudClaim {
    pub(crate) fn contains(&self, expected: &str) -> bool {
        match self {
            Self::Single(value) => value == expected,
            Self::Multiple(values) => values.iter().any(|value| value == expected),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccessJwtClaims {
    pub(crate) iss: String,
    pub(crate) aud: AccessAudClaim,
    pub(crate) email: Option<String>,
    pub(crate) exp: Option<u64>,
    pub(crate) nbf: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct AccessJwk {
    pub(crate) kid: String,
    pub(crate) kty: String,
    pub(crate) alg: String,
    #[serde(rename = "use")]
    pub(crate) use_: String,
    pub(crate) e: String,
    pub(crate) n: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccessCertsResponse {
    pub(crate) keys: Vec<AccessJwk>,
}

pub(crate) async fn verify_access_jwt(token: &str, config: &AppConfig) -> Result<AccessJwtClaims> {
    let (header_segment, payload_segment, signature_segment) = split_jwt(token)
        .ok_or_else(|| Error::RustError("malformed Cloudflare Access JWT".to_owned()))?;

    let header: AccessJwtHeader = decode_jwt_segment(header_segment)?;
    if header.alg != "RS256" {
        return Err(Error::RustError(format!(
            "unsupported Cloudflare Access JWT algorithm: {}",
            header.alg
        )));
    }

    let claims: AccessJwtClaims = decode_jwt_segment(payload_segment)?;
    let expected_issuer = normalized_access_team_origin(&config.access_team_domain);
    if claims.iss != expected_issuer {
        return Err(Error::RustError(
            "Cloudflare Access JWT issuer mismatch".to_owned(),
        ));
    }
    if !claims.aud.contains(&config.access_audience) {
        return Err(Error::RustError(
            "Cloudflare Access JWT audience mismatch".to_owned(),
        ));
    }

    let now = current_unix_timestamp();
    if let Some(exp) = claims.exp
        && exp < now
    {
        return Err(Error::RustError(
            "Cloudflare Access JWT has expired".to_owned(),
        ));
    }
    if let Some(nbf) = claims.nbf
        && nbf > now
    {
        return Err(Error::RustError(
            "Cloudflare Access JWT is not yet valid".to_owned(),
        ));
    }

    let jwk = fetch_access_jwk(config, &header.kid).await?;
    verify_rs256_signature(
        &jwk,
        format!("{header_segment}.{payload_segment}").as_bytes(),
        &decode_base64url(signature_segment)?,
    )
    .await?;

    Ok(claims)
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
    serde_json::from_slice(&bytes).map_err(|error| {
        Error::RustError(format!("invalid Cloudflare Access JWT payload: {error}"))
    })
}

fn decode_base64url(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|error| Error::RustError(format!("invalid base64url data: {error}")))
}

async fn fetch_access_jwk(config: &AppConfig, expected_kid: &str) -> Result<AccessJwk> {
    let certs_url = format!(
        "{}/cdn-cgi/access/certs",
        normalized_access_team_origin(&config.access_team_domain)
    );
    let request = Request::new(&certs_url, Method::Get)?;
    let mut response = Fetch::Request(request).send().await?;
    let certs: AccessCertsResponse = response.json().await?;

    certs
        .keys
        .into_iter()
        .find(|jwk| jwk.kid == expected_kid)
        .ok_or_else(|| {
            Error::RustError("matching Cloudflare Access signing key was not found".to_owned())
        })
}

fn normalized_access_team_origin(team_domain: &str) -> String {
    let trimmed = team_domain.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    }
}

async fn verify_rs256_signature(jwk: &AccessJwk, data: &[u8], signature: &[u8]) -> Result<()> {
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
    .map_err(|_| Error::RustError("failed to import Cloudflare Access public key".to_owned()))?;

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
            "Cloudflare Access JWT signature verification failed".to_owned(),
        ))
    }
}

fn current_unix_timestamp() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

#[cfg(test)]
mod tests {
    use super::normalized_access_team_origin;

    #[test]
    fn normalized_access_team_origin_adds_https_for_bare_team_domain() {
        assert_eq!(
            normalized_access_team_origin("team.example.cloudflareaccess.com/"),
            "https://team.example.cloudflareaccess.com"
        );
        assert_eq!(
            normalized_access_team_origin("https://team.example.cloudflareaccess.com/"),
            "https://team.example.cloudflareaccess.com"
        );
    }
}
