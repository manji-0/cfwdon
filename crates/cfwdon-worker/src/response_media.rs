use super::{
    AppConfig, MediaAttachmentRow, MediaKind, RemoteStatusAttachmentRow, classify_media_kind,
    instance_base_url, media_kind_label,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct MastodonMediaAttachmentResponse {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) media_type: &'static str,
    pub(crate) url: String,
    pub(crate) preview_url: String,
    pub(crate) remote_url: Option<String>,
    pub(crate) text_url: Option<String>,
    pub(crate) meta: MastodonMediaMeta,
    pub(crate) description: Option<String>,
    pub(crate) blurhash: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonMediaMeta {
    pub(crate) original: Option<MastodonMediaMetaDetails>,
    pub(crate) small: Option<MastodonMediaMetaDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) focus: Option<MastodonMediaFocus>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonMediaMetaDetails {
    pub(crate) width: Option<u32>,
    pub(crate) height: Option<u32>,
    pub(crate) size: Option<String>,
    pub(crate) aspect: Option<f64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct MastodonMediaFocus {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

pub(crate) fn media_object_url(config: &AppConfig, object_key: &str) -> String {
    let base = config
        .media_public_base_url
        .clone()
        .unwrap_or_else(|| instance_base_url(config));
    let base = base.trim_end_matches('/');
    let path = object_key.trim_start_matches('/');
    format!("{base}/{path}")
}

pub(crate) fn media_fallback_url(config: &AppConfig, media_id: &str) -> String {
    format!("{}/media/{}", instance_base_url(config), media_id)
}

pub(crate) fn media_attachment_url(config: &AppConfig, media_id: &str, object_key: &str) -> String {
    if config.media_public_base_url.is_some() {
        media_object_url(config, object_key)
    } else {
        media_fallback_url(config, media_id)
    }
}

impl MastodonMediaAttachmentResponse {
    pub(crate) fn from_row(row: &MediaAttachmentRow, config: &AppConfig) -> Self {
        let url = media_attachment_url(config, &row.id, &row.object_key);
        let fallback_url = media_fallback_url(config, &row.id);
        let focus = row
            .focus_x
            .zip(row.focus_y)
            .map(|(x, y)| MastodonMediaFocus { x, y });

        Self {
            id: row.id.clone(),
            media_type: media_kind_label(
                classify_media_kind(&row.content_type).unwrap_or(MediaKind::Image),
            ),
            url: url.clone(),
            preview_url: url,
            remote_url: None,
            text_url: Some(fallback_url),
            meta: MastodonMediaMeta {
                original: Some(MastodonMediaMetaDetails {
                    width: None,
                    height: None,
                    size: None,
                    aspect: None,
                }),
                small: Some(MastodonMediaMetaDetails {
                    width: None,
                    height: None,
                    size: None,
                    aspect: None,
                }),
                focus,
            },
            description: if row.description.is_empty() {
                None
            } else {
                Some(row.description.clone())
            },
            blurhash: None,
        }
    }

    pub(crate) fn from_remote_row(row: &RemoteStatusAttachmentRow) -> Self {
        let url = row.remote_url.clone();
        let preview_url = row.preview_url.clone().unwrap_or_else(|| url.clone());
        let aspect = row
            .width
            .zip(row.height)
            .and_then(|(width, height)| (height != 0).then_some(width as f64 / height as f64));

        Self {
            id: row.id.clone(),
            media_type: media_kind_label(
                classify_media_kind(&row.content_type).unwrap_or(MediaKind::Image),
            ),
            url: url.clone(),
            preview_url,
            remote_url: Some(url),
            text_url: None,
            meta: MastodonMediaMeta {
                original: Some(MastodonMediaMetaDetails {
                    width: row.width,
                    height: row.height,
                    size: row
                        .width
                        .zip(row.height)
                        .map(|(width, height)| format!("{width}x{height}")),
                    aspect,
                }),
                small: None,
                focus: None,
            },
            description: row.description.clone(),
            blurhash: row.blurhash.clone(),
        }
    }
}
