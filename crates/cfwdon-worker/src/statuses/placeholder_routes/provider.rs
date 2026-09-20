use crate::statuses::RouteContext;
use worker::Env;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranslationProviderConfig {
    pub(crate) provider: String,
    pub(crate) endpoint_url: String,
    pub(crate) api_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TranslationProviderKind {
    LibreTranslate,
    DeepL,
}

pub(super) fn normalize_translation_provider(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(super) fn translation_provider_kind(provider: &str) -> Option<TranslationProviderKind> {
    match normalize_translation_provider(provider).as_str() {
        "libretranslate" => Some(TranslationProviderKind::LibreTranslate),
        "deepl" => Some(TranslationProviderKind::DeepL),
        _ => None,
    }
}

pub(super) fn translation_provider_display_name(provider: &str) -> &'static str {
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

pub(super) fn configured_translation_provider_from_vars<F>(
    vars: F,
) -> Option<TranslationProviderConfig>
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
