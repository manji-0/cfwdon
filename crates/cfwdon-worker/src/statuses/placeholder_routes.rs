use super::{
    Request, Response, Result, RouteContext, app_bearer_token_from_request,
    build_loaded_local_status_response, configured_instance_languages, find_account_by_id,
    find_authenticated_local_account, find_oauth_app_by_bearer_token, find_status_by_id,
    load_config, now_iso_string, oauth_app_has_any_scope, status_api_response,
    update_local_status_quote_approval_policy,
};
use serde::Deserialize;
use worker::{Env, Fetch, Headers, Method, RequestInit, d1::D1Type};

use crate::D1Database;
#[derive(Debug, Default, Deserialize)]
struct InteractionPolicyUpdateRequest {
    quote_approval_policy: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TranslateStatusRequest {
    lang: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationProviderConfig {
    pub(crate) provider: String,
    pub(crate) endpoint_url: String,
    pub(crate) api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TranslationProviderKind {
    LibreTranslate,
    DeepL,
}

#[derive(Debug, Deserialize)]
struct TranslationCacheRow {
    source_fingerprint: String,
    translation_json: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TranslationProviderLanguageRow {
    pub(crate) code: Option<String>,
    pub(crate) targets: Option<Vec<String>>,
}

pub(crate) fn normalize_quote_approval_policy(
    value: Option<String>,
) -> std::result::Result<Option<cfwdon_domain::QuoteApprovalPolicy>, String> {
    let value = value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    match value.as_deref() {
        None => Ok(None),
        Some(value) => cfwdon_domain::QuoteApprovalPolicy::parse(value)
            .map(Some)
            .map_err(|error| error.to_string()),
    }
}

async fn parse_interaction_policy_update_request(
    req: &mut Request,
) -> std::result::Result<Option<String>, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let policy = if content_type.contains("application/json") {
        req.json::<InteractionPolicyUpdateRequest>()
            .await
            .map_err(|error| format!("invalid JSON interaction policy payload: {error}"))?
            .quote_approval_policy
    } else {
        req.form_data()
            .await
            .map_err(|error| format!("invalid form interaction policy payload: {error}"))?
            .get_field("quote_approval_policy")
    };

    normalize_quote_approval_policy(policy)
        .map(|policy| policy.map(|policy| policy.as_str().to_owned()))
}

pub(crate) fn build_translation_document_for_language(
    status: &serde_json::Value,
    target_language: &str,
    provider: &str,
) -> serde_json::Value {
    let source_language = status
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und");
    let media_attachments = status
        .get("media_attachments")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "id": item.get("id").cloned().unwrap_or_else(|| serde_json::json!("")),
                        "description": item
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let poll = status
        .get("poll")
        .and_then(serde_json::Value::as_object)
        .map(|poll| {
            serde_json::json!({
                "id": poll.get("id").cloned().unwrap_or_else(|| serde_json::json!("")),
                "options": poll
                    .get("options")
                    .and_then(serde_json::Value::as_array)
                    .map(|options| {
                        options
                            .iter()
                            .map(|option| {
                                serde_json::json!({
                                    "title": option
                                        .get("title")
                                        .and_then(serde_json::Value::as_str)
                                        .unwrap_or("")
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            })
        })
        .unwrap_or(serde_json::Value::Null);

    serde_json::json!({
        "content": status.get("content").cloned().unwrap_or_else(|| serde_json::json!("")),
        "spoiler_text": status.get("spoiler_text").cloned().unwrap_or_else(|| serde_json::json!("")),
        "language": target_language,
        "poll": poll,
        "media_attachments": media_attachments,
        "detected_source_language": source_language,
        "provider": provider,
    })
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_translation_document(status: &serde_json::Value) -> serde_json::Value {
    let source_language = status
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und");
    build_translation_document_for_language(status, source_language, "cfwdon-placeholder")
}

async fn parse_translate_status_request(
    req: &mut Request,
) -> std::result::Result<TranslateStatusRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<TranslateStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON translation payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form translation payload: {error}"))?;
        TranslateStatusRequest {
            lang: form.get_field("lang"),
        }
    };

    if let Some(lang) = request.lang.as_mut() {
        *lang = lang.trim().to_ascii_lowercase();
        if lang.is_empty() {
            request.lang = None;
        }
    }

    Ok(request)
}

pub(crate) fn translation_target_language(
    requested_language: Option<&str>,
    viewer_default_language: Option<&str>,
    instance_languages: &[String],
    source_language: &str,
) -> String {
    requested_language
        .filter(|value| !value.is_empty())
        .or(viewer_default_language.filter(|value| !value.is_empty()))
        .or_else(|| {
            instance_languages
                .first()
                .map(String::as_str)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or(source_language)
        .to_owned()
}

pub(crate) fn translation_provider_language_code(language: &str) -> String {
    let language = language.trim().to_ascii_lowercase();
    if language.is_empty() || language == "und" {
        return "auto".to_owned();
    }
    language
        .split(['-', '_'])
        .next()
        .unwrap_or(language.as_str())
        .to_owned()
}

fn translation_provider_request_source_language(provider: &str, language: &str) -> Option<String> {
    match translation_provider_kind(provider) {
        Some(TranslationProviderKind::DeepL) => {
            let normalized = language.trim().replace('_', "-");
            if normalized.is_empty() || normalized.eq_ignore_ascii_case("und") {
                None
            } else {
                Some(normalized.to_ascii_uppercase())
            }
        }
        _ => Some(translation_provider_language_code(language)),
    }
}

fn translation_provider_request_target_language(provider: &str, language: &str) -> String {
    match translation_provider_kind(provider) {
        Some(TranslationProviderKind::DeepL) => language.trim().replace('_', "-"),
        _ => translation_provider_language_code(language),
    }
}

fn normalize_translation_provider(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn translation_provider_kind(provider: &str) -> Option<TranslationProviderKind> {
    match normalize_translation_provider(provider).as_str() {
        "libretranslate" => Some(TranslationProviderKind::LibreTranslate),
        "deepl" => Some(TranslationProviderKind::DeepL),
        _ => None,
    }
}

fn translation_provider_display_name(provider: &str) -> &'static str {
    match translation_provider_kind(provider) {
        Some(TranslationProviderKind::LibreTranslate) => "LibreTranslate",
        Some(TranslationProviderKind::DeepL) => "DeepL.com",
        None => "cfwdon-placeholder",
    }
}

pub(crate) fn configured_translation_provider(
    ctx: &RouteContext<()>,
) -> Option<TranslationProviderConfig> {
    configured_translation_provider_from_vars(|key| {
        ctx.var(key).ok().map(|value| value.to_string())
    })
}

pub(crate) fn configured_translation_provider_from_env(
    env: &Env,
) -> Option<TranslationProviderConfig> {
    configured_translation_provider_from_vars(|key| {
        env.var(key).ok().map(|value| value.to_string())
    })
}

fn configured_translation_provider_from_vars<F>(vars: F) -> Option<TranslationProviderConfig>
where
    F: Fn(&str) -> Option<String>,
{
    let provider = vars("TRANSLATION_PROVIDER")
        .map(|value| normalize_translation_provider(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "libretranslate".to_owned());
    translation_provider_kind(&provider)?;

    let endpoint_url = vars("TRANSLATION_API_URL")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())?;
    let api_key = vars("TRANSLATION_API_KEY")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if provider == "deepl" && api_key.is_none() {
        return None;
    }

    Some(TranslationProviderConfig {
        provider,
        endpoint_url,
        api_key,
    })
}

fn normalize_translation_language_code(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    Some(value)
}

fn translation_language_code_variants(value: &str) -> Vec<String> {
    let Some(value) = normalize_translation_language_code(value) else {
        return Vec::new();
    };
    let mut variants = vec![value.clone()];
    if let Some(primary) = value.split(['-', '_']).next()
        && primary != value
    {
        variants.push(primary.to_owned());
    }
    variants
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn translation_provider_language_matches(
    supported_languages: &serde_json::Value,
    source_language: &str,
    target_language: &str,
) -> bool {
    let source_keys = translation_language_code_variants(source_language);
    let target_keys = translation_language_code_variants(target_language);
    if source_keys.is_empty() || target_keys.is_empty() {
        return false;
    }

    for source_key in &source_keys {
        let Some(targets) = supported_languages
            .get(source_key.as_str())
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        for target_key in &target_keys {
            if targets
                .iter()
                .any(|value| value.as_str() == Some(target_key.as_str()))
            {
                return true;
            }
        }
    }

    false
}

pub(crate) fn translation_provider_supported_target_language(
    supported_languages: &serde_json::Value,
    source_language: &str,
    target_language: &str,
) -> Option<String> {
    let source_keys = translation_language_code_variants(source_language);
    let target_keys = translation_language_code_variants(target_language);
    if source_keys.is_empty() || target_keys.is_empty() {
        return None;
    }

    for source_key in &source_keys {
        let Some(targets) = supported_languages
            .get(source_key.as_str())
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        for target_key in &target_keys {
            if targets
                .iter()
                .any(|value| value.as_str() == Some(target_key.as_str()))
            {
                return Some(target_key.clone());
            }
        }
    }

    None
}

pub(crate) fn build_translation_languages_document(
    languages: &[TranslationProviderLanguageRow],
) -> serde_json::Value {
    let mut supported = serde_json::Map::new();
    let mut und_targets = Vec::<String>::new();
    let mut seen_und_targets = std::collections::HashSet::<String>::new();

    for language in languages {
        let Some(code) = language
            .code
            .as_deref()
            .and_then(normalize_translation_language_code)
        else {
            continue;
        };
        let targets = language
            .targets
            .as_ref()
            .map(|targets| {
                targets
                    .iter()
                    .filter_map(|target| normalize_translation_language_code(target))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for target in &targets {
            if target != &code && seen_und_targets.insert(target.clone()) {
                und_targets.push(target.clone());
            }
        }

        supported.insert(code, serde_json::json!(targets));
    }

    if !und_targets.is_empty() {
        supported.insert("und".to_owned(), serde_json::json!(und_targets));
    }

    serde_json::Value::Object(supported)
}

#[derive(Debug, Deserialize)]
struct DeepLLanguageRow {
    language: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct DeepLLanguageUrls {
    source: String,
    target: String,
}

pub(crate) fn build_deepl_translation_languages_document(
    source_languages: &[String],
    target_languages: &[String],
) -> serde_json::Value {
    fn push_unique(
        codes: &mut Vec<String>,
        seen: &mut std::collections::HashSet<String>,
        value: &str,
    ) {
        let Some(code) = normalize_translation_language_code(value) else {
            return;
        };
        if seen.insert(code.clone()) {
            codes.push(code);
        }
    }

    let mut source_codes = Vec::new();
    let mut seen_source_codes = std::collections::HashSet::<String>::new();
    for language in source_languages {
        push_unique(&mut source_codes, &mut seen_source_codes, language);
    }

    let mut target_codes = Vec::new();
    let mut seen_target_codes = std::collections::HashSet::<String>::new();
    for language in ["en", "pt"] {
        push_unique(&mut target_codes, &mut seen_target_codes, language);
    }
    for language in target_languages {
        push_unique(&mut target_codes, &mut seen_target_codes, language);
    }

    let mut supported = serde_json::Map::new();
    for source in &source_codes {
        let targets = target_codes
            .iter()
            .filter(|target| *target != source)
            .cloned()
            .collect::<Vec<_>>();
        supported.insert(source.clone(), serde_json::json!(targets));
    }
    if !target_codes.is_empty() {
        supported.insert("und".to_owned(), serde_json::json!(target_codes));
    }
    serde_json::Value::Object(supported)
}

pub(crate) async fn load_translation_provider_languages(
    provider_config: &TranslationProviderConfig,
) -> Result<serde_json::Value> {
    match translation_provider_kind(&provider_config.provider) {
        Some(TranslationProviderKind::LibreTranslate) => {
            load_libretranslate_languages(provider_config).await
        }
        Some(TranslationProviderKind::DeepL) => load_deepl_languages(provider_config).await,
        None => Err(worker::Error::RustError(
            "translation provider is not configured".to_owned(),
        )),
    }
}

async fn load_libretranslate_languages(
    provider_config: &TranslationProviderConfig,
) -> Result<serde_json::Value> {
    let languages_url = format!(
        "{}/languages",
        provider_config.endpoint_url.trim_end_matches('/')
    );
    let headers = Headers::new();
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let request = Request::new_with_init(&languages_url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(worker::Error::RustError(format!(
            "translation provider rejected languages request with HTTP {}",
            response.status_code()
        )));
    }
    let languages = response
        .json::<Vec<TranslationProviderLanguageRow>>()
        .await?;
    Ok(build_translation_languages_document(&languages))
}

async fn load_deepl_languages(
    provider_config: &TranslationProviderConfig,
) -> Result<serde_json::Value> {
    let urls = deepl_language_urls(&provider_config.endpoint_url);
    let headers = deepl_language_request_headers(provider_config)?;
    let source_languages =
        fetch_deepl_language_codes(&urls.source, headers.clone(), "source").await?;
    let target_languages = fetch_deepl_language_codes(&urls.target, headers, "target").await?;
    Ok(build_deepl_translation_languages_document(
        &source_languages,
        &target_languages,
    ))
}

fn deepl_language_urls(endpoint_url: &str) -> DeepLLanguageUrls {
    let base_url = endpoint_url.trim_end_matches('/');
    DeepLLanguageUrls {
        source: format!("{base_url}/v2/languages?type=source"),
        target: format!("{base_url}/v2/languages?type=target"),
    }
}

async fn fetch_deepl_language_codes(
    url: &str,
    headers: Headers,
    language_kind: &str,
) -> Result<Vec<String>> {
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let request = Request::new_with_init(url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(worker::Error::RustError(format!(
            "translation provider rejected {language_kind} languages request with HTTP {}",
            response.status_code()
        )));
    }
    Ok(deepl_language_codes(
        response.json::<Vec<DeepLLanguageRow>>().await?,
    ))
}

fn deepl_language_request_headers(provider_config: &TranslationProviderConfig) -> Result<Headers> {
    let headers = Headers::new();
    if let Some(api_key) = provider_config
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        headers.set("Authorization", &format!("DeepL-Auth-Key {api_key}"))?;
    }
    Ok(headers)
}

fn deepl_language_codes(rows: Vec<DeepLLanguageRow>) -> Vec<String> {
    rows.into_iter().filter_map(|row| row.language).collect()
}

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

pub(crate) fn translation_cache_source_fingerprint(
    status: &serde_json::Value,
) -> std::result::Result<String, serde_json::Error> {
    let media_attachments = status
        .get("media_attachments")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    serde_json::json!({
                        "id": item.get("id").cloned().unwrap_or_else(|| serde_json::json!("")),
                        "description": item
                            .get("description")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let poll_options = status
        .pointer("/poll/options")
        .and_then(serde_json::Value::as_array)
        .map(|options| {
            options
                .iter()
                .map(|option| {
                    serde_json::json!({
                        "title": option
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("")
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    serde_json::to_string(&serde_json::json!({
        "content": status
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        "spoiler_text": status
            .get("spoiler_text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        "language": status
            .get("language")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("und"),
        "media_attachments": media_attachments,
        "poll_options": poll_options,
    }))
}

async fn find_cached_translation_document(
    db: &D1Database,
    status_id: &str,
    target_language: &str,
    provider: &str,
    source_fingerprint: &str,
) -> Result<Option<serde_json::Value>> {
    let bindings = [
        D1Type::Text(status_id),
        D1Type::Text(target_language),
        D1Type::Text(provider),
    ];
    let Some(row) = db
        .prepare(
            "SELECT source_fingerprint, translation_json
             FROM status_translation_cache
             WHERE status_id = ?1
               AND target_language = ?2
               AND provider = ?3",
        )
        .bind_refs(bindings.iter())?
        .first::<TranslationCacheRow>(None)
        .await?
    else {
        return Ok(None);
    };
    if row.source_fingerprint != source_fingerprint {
        return Ok(None);
    }
    serde_json::from_str::<serde_json::Value>(&row.translation_json)
        .map(Some)
        .map_err(|error| {
            worker::Error::RustError(format!("failed to decode cached translation: {error}"))
        })
}

async fn store_cached_translation_document(
    db: &D1Database,
    status_id: &str,
    target_language: &str,
    provider: &str,
    source_fingerprint: &str,
    document: &serde_json::Value,
    timestamp: &str,
) -> Result<()> {
    let translation_json = serde_json::to_string(document).map_err(|error| {
        worker::Error::RustError(format!("failed to encode cached translation: {error}"))
    })?;
    let bindings = [
        D1Type::Text(status_id),
        D1Type::Text(target_language),
        D1Type::Text(provider),
        D1Type::Text(source_fingerprint),
        D1Type::Text(translation_json.as_str()),
        D1Type::Text(timestamp),
        D1Type::Text(timestamp),
    ];
    db.prepare(
        "INSERT INTO status_translation_cache (
             status_id, target_language, provider, source_fingerprint,
             translation_json, created_at, updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(status_id, target_language, provider) DO UPDATE SET
             source_fingerprint = excluded.source_fingerprint,
             translation_json = excluded.translation_json,
             updated_at = excluded.updated_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn translate_text_with_libretranslate(
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

async fn translate_text_with_deepl(
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

fn set_json_pointer_value(document: &mut serde_json::Value, pointer: &str, value: String) {
    if let Some(target) = document.pointer_mut(pointer) {
        *target = serde_json::json!(value);
    }
}

fn status_translation_source_language(status: &serde_json::Value) -> &str {
    status
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und")
}

struct TranslationDocumentBuilder<'a> {
    provider_config: &'a TranslationProviderConfig,
    source_language: &'a str,
    target_language: &'a str,
}

impl TranslationDocumentBuilder<'_> {
    async fn translate_text(&self, text: &str, libretranslate_format: &str) -> Result<String> {
        match translation_provider_kind(&self.provider_config.provider) {
            Some(TranslationProviderKind::DeepL) => {
                translate_text_with_deepl(
                    self.provider_config,
                    text,
                    self.source_language,
                    self.target_language,
                )
                .await
            }
            _ => {
                translate_text_with_libretranslate(
                    self.provider_config,
                    text,
                    self.source_language,
                    self.target_language,
                    libretranslate_format,
                )
                .await
            }
        }
    }

    async fn translate_status_string_field(
        &self,
        document: &mut serde_json::Value,
        status: &serde_json::Value,
        field: &str,
        document_pointer: &str,
        libretranslate_format: &str,
        skip_blank: bool,
    ) -> Result<()> {
        let Some(text) = status.get(field).and_then(serde_json::Value::as_str) else {
            return Ok(());
        };
        if skip_blank && text.trim().is_empty() {
            return Ok(());
        }

        let translated = self.translate_text(text, libretranslate_format).await?;
        set_json_pointer_value(document, document_pointer, translated);
        Ok(())
    }

    async fn translate_indexed_string_values(
        &self,
        document: &mut serde_json::Value,
        items: &[serde_json::Value],
        item_field: &str,
        document_pointer_prefix: &str,
    ) -> Result<()> {
        for (index, item) in items.iter().enumerate() {
            let Some(text) = item
                .get(item_field)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
            else {
                continue;
            };
            let translated = self.translate_text(text, "text").await?;
            set_json_pointer_value(
                document,
                &format!("{document_pointer_prefix}/{index}/{item_field}"),
                translated,
            );
        }
        Ok(())
    }
}

async fn build_translation_document_with_provider(
    status: &serde_json::Value,
    target_language: &str,
    provider_config: &TranslationProviderConfig,
) -> Result<serde_json::Value> {
    let source_language = status_translation_source_language(status);
    let builder = TranslationDocumentBuilder {
        provider_config,
        source_language,
        target_language,
    };
    let mut document = build_translation_document_for_language(
        status,
        target_language,
        translation_provider_display_name(&provider_config.provider),
    );

    builder
        .translate_status_string_field(&mut document, status, "content", "/content", "html", false)
        .await?;
    builder
        .translate_status_string_field(
            &mut document,
            status,
            "spoiler_text",
            "/spoiler_text",
            "text",
            true,
        )
        .await?;
    if let Some(media) = status
        .get("media_attachments")
        .and_then(serde_json::Value::as_array)
    {
        builder
            .translate_indexed_string_values(
                &mut document,
                media,
                "description",
                "/media_attachments",
            )
            .await?;
    }
    if let Some(options) = status
        .pointer("/poll/options")
        .and_then(serde_json::Value::as_array)
    {
        builder
            .translate_indexed_string_values(&mut document, options, "title", "/poll/options")
            .await?;
    }

    Ok(document)
}

async fn cached_or_fresh_provider_translation_document(
    db: &D1Database,
    status: &serde_json::Value,
    source_language: &str,
    target_language: &str,
    route_status_id: &str,
    provider_config: &TranslationProviderConfig,
) -> Result<Option<serde_json::Value>> {
    let supported_languages = load_translation_provider_languages(provider_config).await?;
    let Some(normalized_target_language) = translation_provider_supported_target_language(
        &supported_languages,
        source_language,
        target_language,
    ) else {
        return Ok(None);
    };
    let status_id = translation_status_id(status, route_status_id);
    let source_fingerprint = translation_cache_source_fingerprint(status).map_err(|error| {
        worker::Error::RustError(format!(
            "failed to encode translation source fingerprint: {error}"
        ))
    })?;
    if let Some(document) = find_cached_translation_document(
        db,
        status_id,
        &normalized_target_language,
        &provider_config.provider,
        &source_fingerprint,
    )
    .await?
    {
        return Ok(Some(document));
    }
    let document = build_translation_document_with_provider(
        status,
        &normalized_target_language,
        provider_config,
    )
    .await?;
    let timestamp = now_iso_string()?;
    store_cached_translation_document(
        db,
        status_id,
        &normalized_target_language,
        &provider_config.provider,
        &source_fingerprint,
        &document,
        &timestamp,
    )
    .await?;
    Ok(Some(document))
}

fn translation_status_id<'a>(status: &'a serde_json::Value, route_status_id: &'a str) -> &'a str {
    status
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(route_status_id)
}

fn translation_status_visibility_allows_translation(visibility: Option<&str>) -> bool {
    !matches!(visibility.unwrap_or("public"), "private" | "direct")
}

fn translation_language_pair_allows_translation(
    source_language: &str,
    target_language: &str,
) -> bool {
    target_language != source_language
}

pub(crate) async fn status_interaction_policy_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    if app_bearer_token_from_request(&req)?.is_some() {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "The access token is invalid",
        }))?
        .with_status(401));
    }
    let db = crate::bind_request_d1(&ctx, &config)?;
    let Some(viewer) = find_authenticated_local_account(&req, &db, &config).await? else {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "The access token is invalid",
        }))?
        .with_status(401));
    };
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let requested_policy = match parse_interaction_policy_update_request(&mut req).await {
        Ok(policy) => policy,
        Err(message) => return Response::error(message, 422),
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != viewer.id() {
        return Response::error("status not found", 404);
    }
    let effective_policy = match requested_policy.as_deref() {
        Some(_) if matches!(status.visibility.as_str(), "private" | "direct") => "nobody",
        Some(policy) => policy,
        None => crate::effective_local_quote_approval_policy(&status),
    };
    let updated_at = now_iso_string()?;
    let updated =
        update_local_status_quote_approval_policy(&db, &status, effective_policy, &updated_at)
            .await?;
    let Some(account) = find_account_by_id(&db, &updated.account_id).await? else {
        return Response::error("status not found", 404);
    };
    Response::from_json(
        &build_loaded_local_status_response(&db, &config, Some(&viewer), &updated, &account)
            .await?,
    )
}

pub(crate) async fn translate_status_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let bearer_token = app_bearer_token_from_request(&req)?;
    let app = match bearer_token.as_deref() {
        Some(token) => match find_oauth_app_by_bearer_token(&db, token).await? {
            Some(app) => {
                if !oauth_app_has_any_scope(&app, &["read:statuses", "read"]) {
                    return Ok(Response::from_json(&serde_json::json!({
                        "error": "This action is outside the authorized scopes",
                    }))?
                    .with_status(403));
                }
                Some(app)
            }
            None => {
                return Ok(Response::from_json(&serde_json::json!({
                    "error": "The access token is invalid",
                }))?
                .with_status(401));
            }
        },
        None => None,
    };
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    if viewer.is_none() && app.is_none() {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "The access token is invalid",
        }))?
        .with_status(401));
    };
    let request = match parse_translate_status_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let route_status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let provider_config = configured_translation_provider(&ctx);

    let mut response = status_api_response(req, ctx).await?;
    if response.status_code() != 200 {
        return Response::error("Record not found", 404);
    }
    let value = response.json::<serde_json::Value>().await?;
    let visibility = value.get("visibility").and_then(serde_json::Value::as_str);
    if !translation_status_visibility_allows_translation(visibility) {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This action is not allowed",
        }))?
        .with_status(403));
    }

    let source_language = value
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und");
    let viewer_default_language = viewer
        .as_ref()
        .and_then(|viewer| viewer.default_language().map(str::to_owned));
    let target_language = translation_target_language(
        request.lang.as_deref(),
        viewer_default_language.as_deref(),
        &configured_instance_languages(&config),
        source_language,
    );
    if !translation_language_pair_allows_translation(source_language, &target_language) {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This action is not allowed",
        }))?
        .with_status(403));
    }

    if let Some(provider_config) = provider_config {
        let Some(document) = cached_or_fresh_provider_translation_document(
            &db,
            &value,
            source_language,
            &target_language,
            &route_status_id,
            &provider_config,
        )
        .await?
        else {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "This action is not allowed",
            }))?
            .with_status(403));
        };
        return Response::from_json(&document);
    }

    Ok(Response::from_json(&serde_json::json!({
        "error": "Translation provider is not configured",
    }))?
    .with_status(503))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepl_language_codes_ignores_rows_without_language() {
        let rows = vec![
            DeepLLanguageRow {
                language: Some("EN".to_owned()),
            },
            DeepLLanguageRow { language: None },
            DeepLLanguageRow {
                language: Some("JA".to_owned()),
            },
        ];

        assert_eq!(deepl_language_codes(rows), vec!["EN", "JA"]);
    }

    #[test]
    fn deepl_language_urls_trim_endpoint_trailing_slash() {
        assert_eq!(
            deepl_language_urls("https://api-free.deepl.com/"),
            DeepLLanguageUrls {
                source: "https://api-free.deepl.com/v2/languages?type=source".to_owned(),
                target: "https://api-free.deepl.com/v2/languages?type=target".to_owned(),
            }
        );
    }

    #[test]
    fn translation_status_id_prefers_document_id_over_route_fallback() {
        assert_eq!(
            translation_status_id(
                &serde_json::json!({"id": "status-from-document"}),
                "route-1"
            ),
            "status-from-document"
        );
        assert_eq!(
            translation_status_id(&serde_json::json!({}), "route-1"),
            "route-1"
        );
    }

    #[test]
    fn translation_status_visibility_blocks_private_and_direct_statuses() {
        assert!(translation_status_visibility_allows_translation(None));
        assert!(translation_status_visibility_allows_translation(Some(
            "public"
        )));
        assert!(translation_status_visibility_allows_translation(Some(
            "unlisted"
        )));
        assert!(!translation_status_visibility_allows_translation(Some(
            "private"
        )));
        assert!(!translation_status_visibility_allows_translation(Some(
            "direct"
        )));
    }

    #[test]
    fn translation_language_pair_blocks_noop_translations() {
        assert!(translation_language_pair_allows_translation("en", "ja"));
        assert!(!translation_language_pair_allows_translation("en", "en"));
    }
}
