use super::languages::{
    TranslationProviderLanguageRow, build_deepl_translation_languages_document,
    build_translation_languages_document,
};
use super::provider::{
    TranslationProviderConfig, TranslationProviderKind, translation_provider_kind,
};
use crate::statuses::{Request, Result};
use serde::Deserialize;
use worker::{Fetch, Headers, Method, RequestInit};

#[derive(Debug, Deserialize)]
pub(super) struct DeepLLanguageRow {
    language: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DeepLLanguageUrls {
    source: String,
    target: String,
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

pub(super) async fn load_libretranslate_languages(
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

pub(super) async fn load_deepl_languages(
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

pub(super) fn deepl_language_urls(endpoint_url: &str) -> DeepLLanguageUrls {
    let base_url = endpoint_url.trim_end_matches('/');
    DeepLLanguageUrls {
        source: format!("{base_url}/v2/languages?type=source"),
        target: format!("{base_url}/v2/languages?type=target"),
    }
}

pub(super) async fn fetch_deepl_language_codes(
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

pub(super) fn deepl_language_request_headers(
    provider_config: &TranslationProviderConfig,
) -> Result<Headers> {
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

pub(super) fn deepl_language_codes(rows: Vec<DeepLLanguageRow>) -> Vec<String> {
    rows.into_iter().filter_map(|row| row.language).collect()
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
}
