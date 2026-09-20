use super::cache::{
    find_cached_translation_document, store_cached_translation_document,
    translation_cache_source_fingerprint,
};
use super::languages::translation_provider_supported_target_language;
use super::provider::{
    TranslationProviderConfig, TranslationProviderKind, translation_provider_display_name,
    translation_provider_kind,
};
use super::provider_client::{translate_text_with_deepl, translate_text_with_libretranslate};
use super::provider_languages::load_translation_provider_languages;
use crate::D1Database;
use crate::statuses::{Result, now_iso_string};

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

pub(super) fn set_json_pointer_value(
    document: &mut serde_json::Value,
    pointer: &str,
    value: String,
) {
    if let Some(target) = document.pointer_mut(pointer) {
        *target = serde_json::json!(value);
    }
}

pub(super) fn status_translation_source_language(status: &serde_json::Value) -> &str {
    status
        .get("language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("und")
}

pub(super) struct TranslationDocumentBuilder<'a> {
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

pub(super) async fn build_translation_document_with_provider(
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

pub(super) async fn cached_or_fresh_provider_translation_document(
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

pub(super) fn translation_status_id<'a>(
    status: &'a serde_json::Value,
    route_status_id: &'a str,
) -> &'a str {
    status
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(route_status_id)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
