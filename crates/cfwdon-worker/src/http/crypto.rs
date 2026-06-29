use crate::crypto_keys::subtle_crypto;
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
    let is_rsa_public_key = public_key_pem
        .lines()
        .any(|line| line.trim() == "-----BEGIN RSA PUBLIC KEY-----");
    let encoded = public_key_pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>();
    let der = STANDARD
        .decode(encoded)
        .map_err(|error| Error::RustError(format!("invalid public key PEM: {error}")))?;
    if is_rsa_public_key {
        Ok(wrap_pkcs1_rsa_public_key_as_spki(&der))
    } else {
        Ok(der)
    }
}

fn der_length_bytes(length: usize) -> Vec<u8> {
    if length < 128 {
        return vec![length as u8];
    }

    let length_bytes = length.to_be_bytes();
    let first_non_zero = length_bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(length_bytes.len() - 1);
    let trimmed = &length_bytes[first_non_zero..];
    let mut encoded = Vec::with_capacity(trimmed.len() + 1);
    encoded.push(0x80 | trimmed.len() as u8);
    encoded.extend_from_slice(trimmed);
    encoded
}

fn der_sequence(content: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(content.len() + 4);
    encoded.push(0x30);
    encoded.extend_from_slice(&der_length_bytes(content.len()));
    encoded.extend_from_slice(content);
    encoded
}

fn der_bit_string(content: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(content.len() + 5);
    encoded.push(0x03);
    encoded.extend_from_slice(&der_length_bytes(content.len() + 1));
    encoded.push(0);
    encoded.extend_from_slice(content);
    encoded
}

fn wrap_pkcs1_rsa_public_key_as_spki(pkcs1_der: &[u8]) -> Vec<u8> {
    let rsa_encryption_algorithm = [
        0x30, 0x0d, // SEQUENCE
        0x06, 0x09, // OBJECT IDENTIFIER
        0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, // rsaEncryption
        0x05, 0x00, // NULL
    ];
    let public_key = der_bit_string(pkcs1_der);
    let mut spki_content = Vec::with_capacity(rsa_encryption_algorithm.len() + public_key.len());
    spki_content.extend_from_slice(&rsa_encryption_algorithm);
    spki_content.extend_from_slice(&public_key);
    der_sequence(&spki_content)
}

async fn import_private_signing_key(
    subtle: &web_sys::SubtleCrypto,
    private_key_jwk: &str,
) -> Result<CryptoKey> {
    let jwk_object = js_sys::JSON::parse(private_key_jwk)
        .map_err(Error::from)?
        .dyn_into::<Object>()
        .map_err(|_| Error::RustError("failed to convert private JWK to object".to_owned()))?;
    let import_params = RsaHashedImportParams::new_with_str("SHA-256");
    let import_algorithm: Object = import_params.into();
    Reflect::set(
        &import_algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )
    .map_err(Error::from)?;

    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("sign"));

    JsFuture::from(subtle.import_key_with_object(
        "jwk",
        &jwk_object,
        &import_algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<CryptoKey>()
    .map_err(|_| Error::RustError("failed to import account private signing key".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{decode_public_key_pem, wrap_pkcs1_rsa_public_key_as_spki};
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    #[test]
    fn decode_public_key_pem_keeps_spki_public_key_der() {
        let der = vec![0x30, 0x03, 0x01, 0x02, 0x03];
        let pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(&der)
        );

        assert_eq!(decode_public_key_pem(&pem).unwrap(), der);
    }

    #[test]
    fn decode_public_key_pem_wraps_pkcs1_rsa_public_key_as_spki() {
        let pkcs1 = vec![0x30, 0x06, 0x02, 0x01, 0x03, 0x02, 0x01, 0x11];
        let pem = format!(
            "-----BEGIN RSA PUBLIC KEY-----\n{}\n-----END RSA PUBLIC KEY-----\n",
            STANDARD.encode(&pkcs1)
        );

        assert_eq!(
            decode_public_key_pem(&pem).unwrap(),
            wrap_pkcs1_rsa_public_key_as_spki(&pkcs1)
        );
    }
}
