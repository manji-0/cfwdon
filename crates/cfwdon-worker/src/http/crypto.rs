use crate::crypto_keys::{rsa_signing_algorithm, subtle_crypto};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use js_sys::{Array, Date, Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Algorithm, CryptoKey, RsaHashedImportParams};
use worker::{Error, Result};

pub(crate) async fn verify_http_signature_bytes(
    public_key_pem: &str,
    data: &[u8],
    signature: &[u8],
) -> Result<()> {
    let subtle = subtle_crypto()?;
    let public_key = import_public_verification_key(&subtle, public_key_pem).await?;
    let verify_algorithm = Algorithm::new("RSASSA-PKCS1-v1_5");
    let verify_algorithm: Object = verify_algorithm.into();
    let signature = Uint8Array::from(signature);
    let data = Uint8Array::from(data);

    let verified = JsFuture::from(
        subtle.verify_with_object_and_buffer_source_and_buffer_source(
            &verify_algorithm,
            &public_key,
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
            "ActivityPub signature verification failed".to_owned(),
        ))
    }
}

pub(crate) fn now_http_date_string() -> Result<String> {
    Date::new_0()
        .to_utc_string()
        .as_string()
        .ok_or_else(|| Error::RustError("failed to format HTTP date".to_owned()))
}

pub(crate) async fn sha256_http_digest(payload: &[u8]) -> Result<String> {
    let subtle = subtle_crypto()?;
    let hash = JsFuture::from(subtle.digest_with_str_and_u8_array("SHA-256", payload)?).await?;
    let bytes = Uint8Array::new(&hash).to_vec();
    Ok(format!("SHA-256={}", STANDARD.encode(bytes)))
}

pub(crate) async fn sign_http_signature(private_key_jwk: &str, payload: &[u8]) -> Result<String> {
    let subtle = subtle_crypto()?;
    let key = import_private_signing_key(&subtle, private_key_jwk).await?;
    let signature = JsFuture::from(subtle.sign_with_object_and_u8_array(
        &Algorithm::new("RSASSA-PKCS1-v1_5").into(),
        &key,
        payload,
    )?)
    .await?;
    Ok(STANDARD.encode(Uint8Array::new(&signature).to_vec()))
}

async fn import_public_verification_key(
    subtle: &web_sys::SubtleCrypto,
    public_key_pem: &str,
) -> Result<CryptoKey> {
    let public_key_der = decode_public_key_pem(public_key_pem)?;
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

    JsFuture::from(subtle.import_key_with_object(
        "spki",
        Uint8Array::from(public_key_der.as_slice()).as_ref(),
        &import_algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<CryptoKey>()
    .map_err(|_| Error::RustError("failed to import actor public key".to_owned()))
}

fn decode_public_key_pem(public_key_pem: &str) -> Result<Vec<u8>> {
    let encoded = public_key_pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>();
    STANDARD
        .decode(encoded)
        .map_err(|error| Error::RustError(format!("invalid public key PEM: {error}")))
}

async fn import_private_signing_key(
    subtle: &web_sys::SubtleCrypto,
    private_key_jwk: &str,
) -> Result<CryptoKey> {
    let jwk: serde_json::Value = serde_json::from_str(private_key_jwk).map_err(|error| {
        Error::RustError(format!("failed to parse account private key JWK: {error}"))
    })?;
    let jwk_value = worker::d1::serde_wasm_bindgen::to_value(&jwk)
        .map_err(|error| Error::RustError(format!("failed to serialize private JWK: {error}")))?;
    let jwk_object = jwk_value
        .dyn_into::<Object>()
        .map_err(|_| Error::RustError("failed to convert private JWK to object".to_owned()))?;
    let algorithm = rsa_signing_algorithm(2048)?;

    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("sign"));

    JsFuture::from(subtle.import_key_with_object(
        "jwk",
        &jwk_object,
        &algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<CryptoKey>()
    .map_err(|_| Error::RustError("failed to import account private signing key".to_owned()))
}
