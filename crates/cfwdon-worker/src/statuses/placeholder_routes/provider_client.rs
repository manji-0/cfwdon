use super::languages::{
    translation_provider_language_code, translation_provider_request_source_language,
    translation_provider_request_target_language,
};
use super::provider::TranslationProviderConfig;
use crate::statuses::{Request, Result};
use worker::{Fetch, Headers, Method, RequestInit};

pub(crate) fn build_libretranslate_request_payload(
    text: &str,
    source_language: &str,
    target_language: &str,
    format: &str,
    api_key: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "q": text,
        "source": translation_provider_language_code(source_language),
        "target": translation_provider_language_code(target_language),
        "format": format,
    });
    if let Some(api_key) = api_key.map(str::trim).filter(|value| !value.is_empty()) {
        payload["api_key"] = serde_json::json!(api_key);
    }
    payload
}

pub(crate) fn build_deepl_request_body(
    text: &str,
    source_language: &str,
    target_language: &str,
) -> String {
    let mut parts = vec![
        format!("text={}", urlencoding::encode(text)),
        format!(
            "target_lang={}",
            urlencoding::encode(&translation_provider_request_target_language(
                "deepl",
                target_language
            ))
        ),
        "tag_handling=html".to_owned(),
    ];
    parts.extend(
        translation_provider_request_source_language("deepl", source_language)
            .into_iter()
            .map(|source_language| {
                format!("source_lang={}", urlencoding::encode(&source_language))
            }),
    );
    parts.join("&")
}

pub(crate) fn parse_libretranslate_translated_text(response: &serde_json::Value) -> Option<String> {
    response
        .get("translatedText")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(crate) fn parse_deepl_translated_text(response: &serde_json::Value) -> Option<String> {
    response
        .get("translations")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first())
        .and_then(|value| value.get("text"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub(super) async fn translate_text_with_libretranslate(
    config: &TranslationProviderConfig,
    text: &str,
    source_language: &str,
    target_language: &str,
    format: &str,
) -> Result<String> {
    if text.trim().is_empty() {
        return Ok(text.to_owned());
    }

    let payload = build_libretranslate_request_payload(
        text,
        source_language,
        target_language,
        format,
        config.api_key.as_deref(),
    );
    let payload_json = serde_json::to_string(&payload).map_err(|error| {
        worker::Error::RustError(format!("failed to encode translation payload: {error}"))
    })?;

    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&payload_json)));
    let request = Request::new_with_init(&config.endpoint_url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(worker::Error::RustError(format!(
            "translation provider rejected request with HTTP {}",
            response.status_code()
        )));
    }

    let value = response.json::<serde_json::Value>().await?;
    parse_libretranslate_translated_text(&value).ok_or_else(|| {
        worker::Error::RustError("translation provider response missing translatedText".to_owned())
    })
}

pub(super) async fn translate_text_with_deepl(
    config: &TranslationProviderConfig,
    text: &str,
    source_language: &str,
    target_language: &str,
) -> Result<String> {
    if text.trim().is_empty() {
        return Ok(text.to_owned());
    }

    let body = build_deepl_request_body(text, source_language, target_language);
    let headers = Headers::new();
    headers.set(
        "Authorization",
        &format!("DeepL-Auth-Key {}", config.api_key.as_deref().unwrap_or("")),
    )?;
    headers.set("Content-Type", "application/x-www-form-urlencoded")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(wasm_bindgen::JsValue::from_str(&body)));
    let request = Request::new_with_init(&config.endpoint_url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(worker::Error::RustError(format!(
            "translation provider rejected request with HTTP {}",
            response.status_code()
        )));
    }

    let value = response.json::<serde_json::Value>().await?;
    parse_deepl_translated_text(&value).ok_or_else(|| {
        worker::Error::RustError("translation provider response missing translations".to_owned())
    })
}
