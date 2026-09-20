use super::provider::{TranslationProviderKind, translation_provider_kind};
use serde::Deserialize;

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

pub(super) fn translation_provider_request_source_language(
    provider: &str,
    language: &str,
) -> Option<String> {
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

pub(super) fn translation_provider_request_target_language(
    provider: &str,
    language: &str,
) -> String {
    match translation_provider_kind(provider) {
        Some(TranslationProviderKind::DeepL) => language.trim().replace('_', "-"),
        _ => translation_provider_language_code(language),
    }
}

pub(super) fn normalize_translation_language_code(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    Some(value)
}

pub(super) fn translation_language_code_variants(value: &str) -> Vec<String> {
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

#[derive(Debug, Deserialize)]
pub(crate) struct TranslationProviderLanguageRow {
    pub(crate) code: Option<String>,
    pub(crate) targets: Option<Vec<String>>,
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
