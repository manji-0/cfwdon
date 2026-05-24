use crate::crypto_keys::subtle_crypto;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use js_sys::{Array, Object, Reflect, Uint8Array};
use sha2::{Digest, Sha256};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{CryptoKey, SubtleCrypto};
use worker::{Error, Result};

const ENCRYPTED_SECRET_PREFIX: &str = "v1";
const AES_GCM_NONCE_BYTES: usize = 12;

pub(crate) fn is_encrypted_secret(value: &str) -> bool {
    value.starts_with("v1:")
}

pub(crate) async fn encrypt_secret(plaintext: &str, encryption_key: &str) -> Result<String> {
    let subtle = subtle_crypto()?;
    let key = import_aes_gcm_key(&subtle, encryption_key).await?;
    let nonce = random_nonce()?;
    let algorithm = aes_gcm_algorithm(&nonce)?;
    let ciphertext = JsFuture::from(subtle.encrypt_with_object_and_u8_array(
        &algorithm,
        &key,
        plaintext.as_bytes(),
    )?)
    .await?;
    Ok(format!(
        "{ENCRYPTED_SECRET_PREFIX}:{}:{}",
        STANDARD.encode(nonce),
        STANDARD.encode(Uint8Array::new(&ciphertext).to_vec())
    ))
}

pub(crate) async fn decrypt_secret(encrypted: &str, encryption_key: &str) -> Result<String> {
    let parts = encrypted.split(':').collect::<Vec<_>>();
    if parts.len() != 3 || parts[0] != ENCRYPTED_SECRET_PREFIX {
        return Err(Error::RustError(
            "encrypted secret has unsupported format".to_owned(),
        ));
    }
    let nonce = STANDARD
        .decode(parts[1])
        .map_err(|error| Error::RustError(format!("invalid encrypted secret nonce: {error}")))?;
    let ciphertext = STANDARD.decode(parts[2]).map_err(|error| {
        Error::RustError(format!("invalid encrypted secret ciphertext: {error}"))
    })?;
    let subtle = subtle_crypto()?;
    let key = import_aes_gcm_key(&subtle, encryption_key).await?;
    let algorithm = aes_gcm_algorithm(&nonce)?;
    let plaintext = JsFuture::from(subtle.decrypt_with_object_and_u8_array(
        &algorithm,
        &key,
        ciphertext.as_slice(),
    )?)
    .await?;
    String::from_utf8(Uint8Array::new(&plaintext).to_vec())
        .map_err(|error| Error::RustError(format!("encrypted secret is not UTF-8: {error}")))
}

fn aes_gcm_algorithm(nonce: &[u8]) -> Result<Object> {
    let algorithm = Object::new();
    Reflect::set(
        &algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("AES-GCM"),
    )?;
    Reflect::set(
        &algorithm,
        &JsValue::from_str("iv"),
        Uint8Array::from(nonce).as_ref(),
    )?;
    Ok(algorithm)
}

async fn import_aes_gcm_key(subtle: &SubtleCrypto, encryption_key: &str) -> Result<CryptoKey> {
    let digest = Sha256::digest(encryption_key.as_bytes());
    let algorithm = Object::new();
    Reflect::set(
        &algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("AES-GCM"),
    )?;
    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("encrypt"));
    key_usages.push(&JsValue::from_str("decrypt"));
    JsFuture::from(subtle.import_key_with_object(
        "raw",
        Uint8Array::from(digest.as_slice()).as_ref(),
        &algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<CryptoKey>()
    .map_err(|_| Error::RustError("failed to import secret encryption key".to_owned()))
}

fn random_nonce() -> Result<Vec<u8>> {
    let crypto = js_sys::Reflect::get(&js_sys::global(), &JsValue::from_str("crypto"))?
        .dyn_into::<web_sys::Crypto>()
        .map_err(|_| Error::RustError("failed to access global crypto".to_owned()))?;
    let mut nonce = vec![0u8; AES_GCM_NONCE_BYTES];
    crypto
        .get_random_values_with_u8_array(&mut nonce)
        .map_err(Error::from)?;
    Ok(nonce)
}
