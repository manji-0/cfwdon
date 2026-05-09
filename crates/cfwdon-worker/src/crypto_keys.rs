use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::CryptoKey;
use worker::{Error, Result};

#[derive(Debug)]
pub(crate) struct AccountKeyMaterial {
    pub(crate) private_key_jwk: String,
    pub(crate) public_key_pem: String,
}

pub(crate) async fn generate_account_key_material() -> Result<AccountKeyMaterial> {
    let subtle = subtle_crypto()?;
    let algorithm = rsa_signing_algorithm(2048)?;
    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("sign"));
    key_usages.push(&JsValue::from_str("verify"));

    let key_pair =
        JsFuture::from(subtle.generate_key_with_object(&algorithm, true, &key_usages.into())?)
            .await?;
    let private_key = Reflect::get(&key_pair, &JsValue::from_str("privateKey"))?
        .dyn_into::<CryptoKey>()
        .map_err(|_| Error::RustError("failed to read generated private key".to_owned()))?;
    let public_key = Reflect::get(&key_pair, &JsValue::from_str("publicKey"))?
        .dyn_into::<CryptoKey>()
        .map_err(|_| Error::RustError("failed to read generated public key".to_owned()))?;

    Ok(AccountKeyMaterial {
        private_key_jwk: export_private_key_jwk(&subtle, &private_key).await?,
        public_key_pem: export_public_key_pem(&subtle, &public_key).await?,
    })
}

pub(crate) fn subtle_crypto() -> Result<web_sys::SubtleCrypto> {
    let crypto = Reflect::get(&js_sys::global(), &JsValue::from_str("crypto"))?
        .dyn_into::<web_sys::Crypto>()
        .map_err(|_| Error::RustError("failed to access global crypto".to_owned()))?;
    Ok(crypto.subtle())
}

pub(crate) fn rsa_signing_algorithm(modulus_length: u32) -> Result<Object> {
    let algorithm = Object::new();
    Reflect::set(
        &algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )
    .map_err(Error::from)?;
    Reflect::set(
        &algorithm,
        &JsValue::from_str("modulusLength"),
        &JsValue::from_f64(modulus_length as f64),
    )
    .map_err(Error::from)?;

    let public_exponent = Uint8Array::from([1u8, 0, 1].as_slice());
    Reflect::set(
        &algorithm,
        &JsValue::from_str("publicExponent"),
        public_exponent.as_ref(),
    )
    .map_err(Error::from)?;

    let hash = Object::new();
    Reflect::set(
        &hash,
        &JsValue::from_str("name"),
        &JsValue::from_str("SHA-256"),
    )
    .map_err(Error::from)?;
    Reflect::set(&algorithm, &JsValue::from_str("hash"), &hash).map_err(Error::from)?;

    Ok(algorithm)
}

async fn export_private_key_jwk(subtle: &web_sys::SubtleCrypto, key: &CryptoKey) -> Result<String> {
    let exported = JsFuture::from(subtle.export_key("jwk", key)?).await?;
    js_sys::JSON::stringify(&exported)
        .map_err(Error::from)?
        .as_string()
        .ok_or_else(|| Error::RustError("failed to stringify private JWK".to_owned()))
}

async fn export_public_key_pem(subtle: &web_sys::SubtleCrypto, key: &CryptoKey) -> Result<String> {
    let exported = JsFuture::from(subtle.export_key("spki", key)?).await?;
    let bytes = Uint8Array::new(&exported).to_vec();
    Ok(spki_to_pem(&bytes))
}

fn spki_to_pem(bytes: &[u8]) -> String {
    let encoded = STANDARD.encode(bytes);
    let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");

    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        pem.push('\n');
    }

    pem.push_str("-----END PUBLIC KEY-----\n");
    pem
}
