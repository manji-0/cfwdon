use crate::{
    AppConfig, InstanceSummary, MAX_AV_UPLOAD_BYTES, MAX_IMAGE_UPLOAD_BYTES,
    configured_instance_languages, extended_description_url, instance_base_url,
    instance_supported_mime_types, privacy_policy_url, render_status_html, terms_of_service_url,
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

const INSTANCE_API_VERSION: u64 = 6;
const MAX_FEATURED_TAGS: u64 = 10;
const MAX_PINNED_STATUSES: u64 = 5;
const MAX_DISPLAY_NAME_LENGTH: u64 = 30;
const MAX_NOTE_LENGTH: u64 = 500;
const MAX_PROFILE_FIELDS: u64 = 4;
const PROFILE_FIELD_NAME_LIMIT: u64 = 255;
const PROFILE_FIELD_VALUE_LIMIT: u64 = 255;
const IMAGE_MATRIX_LIMIT: u64 = 16_777_216;
const VIDEO_FRAME_RATE_LIMIT: u64 = 60;
const VIDEO_MATRIX_LIMIT: u64 = 2_304_000;
const POLL_MAX_OPTIONS: u64 = 4;
const POLL_MAX_CHARACTERS_PER_OPTION: u64 = 50;
const POLL_MIN_EXPIRATION: u64 = 300;
const POLL_MAX_EXPIRATION: u64 = 2_629_746;

fn streaming_api_url(config: &AppConfig) -> String {
    instance_base_url(config)
        .replace("https://", "wss://")
        .replace("http://", "ws://")
}

fn instance_accounts_configuration(include_pinned_statuses: bool) -> serde_json::Value {
    let mut accounts = serde_json::Map::new();
    accounts.insert(
        "max_display_name_length".to_owned(),
        serde_json::json!(MAX_DISPLAY_NAME_LENGTH),
    );
    accounts.insert(
        "max_note_length".to_owned(),
        serde_json::json!(MAX_NOTE_LENGTH),
    );
    accounts.insert(
        "max_featured_tags".to_owned(),
        serde_json::json!(MAX_FEATURED_TAGS),
    );
    if include_pinned_statuses {
        accounts.insert(
            "max_pinned_statuses".to_owned(),
            serde_json::json!(MAX_PINNED_STATUSES),
        );
    }
    accounts.insert(
        "max_profile_fields".to_owned(),
        serde_json::json!(MAX_PROFILE_FIELDS),
    );
    accounts.insert(
        "profile_field_name_limit".to_owned(),
        serde_json::json!(PROFILE_FIELD_NAME_LIMIT),
    );
    accounts.insert(
        "profile_field_value_limit".to_owned(),
        serde_json::json!(PROFILE_FIELD_VALUE_LIMIT),
    );
    serde_json::Value::Object(accounts)
}

fn instance_media_attachments_configuration() -> serde_json::Value {
    serde_json::json!({
        "supported_mime_types": instance_supported_mime_types(),
        "description_limit": 1500,
        "image_size_limit": MAX_IMAGE_UPLOAD_BYTES,
        "image_matrix_limit": IMAGE_MATRIX_LIMIT,
        "video_size_limit": MAX_AV_UPLOAD_BYTES,
        "video_frame_rate_limit": VIDEO_FRAME_RATE_LIMIT,
        "video_matrix_limit": VIDEO_MATRIX_LIMIT,
    })
}

fn instance_polls_configuration() -> serde_json::Value {
    serde_json::json!({
        "max_options": POLL_MAX_OPTIONS,
        "max_characters_per_option": POLL_MAX_CHARACTERS_PER_OPTION,
        "min_expiration": POLL_MIN_EXPIRATION,
        "max_expiration": POLL_MAX_EXPIRATION,
    })
}

fn instance_thumbnail_document(config: &AppConfig) -> Option<serde_json::Value> {
    let thumbnail_url = config.instance_thumbnail_url.as_deref()?;
    Some(serde_json::json!({
        "url": thumbnail_url,
        "versions": {
            "@1x": thumbnail_url,
            "@2x": thumbnail_url,
        },
    }))
}

fn instance_icon_document(config: &AppConfig) -> serde_json::Value {
    match config.instance_thumbnail_url.as_deref() {
        Some(thumbnail_url) => serde_json::json!([
            {
                "src": thumbnail_url,
                "size": "512x512",
            }
        ]),
        None => serde_json::json!(Vec::<serde_json::Value>::new()),
    }
}

pub(crate) fn build_instance_v1_document(
    summary: &InstanceSummary,
    config: &AppConfig,
    _active_month: u64,
    user_count: u64,
    status_count: u64,
    domain_count: u64,
) -> serde_json::Value {
    serde_json::json!({
        "uri": summary.domain,
        "title": summary.title,
        "short_description": summary.description,
        "description": render_status_html(&summary.description),
        "email": config.contact_email.clone().unwrap_or_default(),
        "version": summary.software.version,
        "urls": {
            "streaming_api": streaming_api_url(config),
        },
        "stats": {
            "user_count": user_count,
            "status_count": status_count,
            "domain_count": domain_count,
        },
        "thumbnail": config.instance_thumbnail_url,
        "languages": configured_instance_languages(config),
        "registrations": false,
        "approval_required": false,
        "invites_enabled": false,
        "configuration": {
            "accounts": instance_accounts_configuration(false),
            "statuses": {
                "max_characters": 500,
                "max_media_attachments": 4,
                "characters_reserved_per_url": 23,
            },
            "media_attachments": instance_media_attachments_configuration(),
            "polls": instance_polls_configuration(),
        },
        "contact_account": serde_json::Value::Null,
        "rules": Vec::<serde_json::Value>::new(),
    })
}

pub(crate) fn build_instance_v2_document(
    summary: &InstanceSummary,
    config: &AppConfig,
    active_month: u64,
) -> serde_json::Value {
    let about_url = config
        .instance_extended_description_html
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|_| extended_description_url(config));
    let privacy_policy_url = config
        .privacy_policy_html
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|_| privacy_policy_url(config));
    let terms_of_service_url = config
        .terms_of_service_html
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|_| terms_of_service_url(config));
    let mut response = serde_json::Map::new();
    response.insert("domain".to_owned(), serde_json::json!(summary.domain));
    response.insert("title".to_owned(), serde_json::json!(summary.title));
    response.insert(
        "version".to_owned(),
        serde_json::json!(summary.software.version),
    );
    response.insert(
        "description".to_owned(),
        serde_json::json!(summary.description),
    );
    response.insert(
        "usage".to_owned(),
        serde_json::json!({
            "users": {
                "active_month": active_month,
            },
        }),
    );
    response.insert("icon".to_owned(), instance_icon_document(config));
    response.insert(
        "languages".to_owned(),
        serde_json::json!(configured_instance_languages(config)),
    );
    response.insert(
        "configuration".to_owned(),
        serde_json::json!({
            "urls": {
                "streaming": streaming_api_url(config),
                "status": serde_json::Value::Null,
                "about": about_url,
                "privacy_policy": privacy_policy_url,
                "terms_of_service": terms_of_service_url,
            },
            "vapid": {
                "public_key": config.web_push_vapid_public_key.as_deref().unwrap_or(""),
            },
            "accounts": instance_accounts_configuration(true),
            "statuses": {
                "max_characters": 500,
                "max_media_attachments": 4,
                "characters_reserved_per_url": 23,
            },
            "media_attachments": instance_media_attachments_configuration(),
            "polls": instance_polls_configuration(),
            "translation": {
                "enabled": false,
            },
                "timelines_access": {
                    "live_feeds": {
                        "local": config.timeline_live_feeds_local.as_str(),
                        "remote": config.timeline_live_feeds_remote.as_str(),
                    },
                    "hashtag_feeds": {
                        "local": config.timeline_hashtag_feeds_local.as_str(),
                        "remote": config.timeline_hashtag_feeds_remote.as_str(),
                    },
                    "trending_link_feeds": {
                        "local": config.timeline_trending_link_feeds_local.as_str(),
                        "remote": config.timeline_trending_link_feeds_remote.as_str(),
                    },
                },
            "limited_federation": false,
        }),
    );
    response.insert(
        "registrations".to_owned(),
        serde_json::json!({
            "enabled": false,
            "approval_required": false,
            "reason_required": false,
            "message": "Registration is handled by Cloudflare Access.",
            "min_age": serde_json::Value::Null,
            "url": serde_json::Value::Null,
        }),
    );
    response.insert(
        "api_versions".to_owned(),
        serde_json::json!({ "mastodon": INSTANCE_API_VERSION }),
    );
    response.insert(
        "rules".to_owned(),
        serde_json::json!(Vec::<serde_json::Value>::new()),
    );

    if let Some(source_url) = config.source_url.as_deref() {
        response.insert("source_url".to_owned(), serde_json::json!(source_url));
    }

    if let Some(thumbnail) = instance_thumbnail_document(config) {
        response.insert("thumbnail".to_owned(), thumbnail);
    }

    response.insert(
        "contact".to_owned(),
        serde_json::json!({
            "email": config.contact_email.clone().unwrap_or_default(),
            "account": serde_json::Value::Null,
        }),
    );

    serde_json::Value::Object(response)
}

pub(crate) fn set_instance_translation_enabled(document: &mut serde_json::Value, enabled: bool) {
    if let Some(translation) = document.pointer_mut("/configuration/translation/enabled") {
        *translation = serde_json::json!(enabled);
    }
}

pub(crate) fn build_instance_activity_document(
    week_floor: OffsetDateTime,
    weekly_totals: &[(u64, u64, u64)],
) -> serde_json::Value {
    serde_json::Value::Array(
        weekly_totals
            .iter()
            .enumerate()
            .map(|(index, (statuses, logins, registrations))| {
                let week_start = week_floor - Duration::weeks((11 - index) as i64);
                serde_json::json!({
                    "week": week_start.unix_timestamp().to_string(),
                    "statuses": statuses.to_string(),
                    "logins": logins.to_string(),
                    "registrations": registrations.to_string(),
                })
            })
            .collect(),
    )
}

pub(crate) fn build_default_privacy_policy_document(content: &str) -> serde_json::Value {
    serde_json::json!({
        "updated_at": OffsetDateTime::UNIX_EPOCH
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
        "content": content,
    })
}
