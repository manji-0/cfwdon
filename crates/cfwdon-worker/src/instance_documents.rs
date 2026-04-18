use crate::{
    AppConfig, InstanceSummary, MAX_AV_UPLOAD_BYTES, MAX_IMAGE_UPLOAD_BYTES,
    configured_instance_languages, extended_description_url, instance_supported_mime_types,
    privacy_policy_url, render_status_html, terms_of_service_url,
};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

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
            "streaming_api": serde_json::Value::Null,
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
            "statuses": {
                "max_characters": 500,
                "max_media_attachments": 4,
                "characters_reserved_per_url": 23,
            },
            "media_attachments": {
                "supported_mime_types": instance_supported_mime_types(),
                "description_limit": 1500,
                "image_size_limit": MAX_IMAGE_UPLOAD_BYTES,
                "video_size_limit": MAX_AV_UPLOAD_BYTES,
            },
            "polls": {
                "max_options": 0,
                "max_characters_per_option": 0,
                "min_expiration": 0,
                "max_expiration": 0,
            },
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
    response.insert(
        "icon".to_owned(),
        serde_json::json!(Vec::<serde_json::Value>::new()),
    );
    response.insert(
        "languages".to_owned(),
        serde_json::json!(configured_instance_languages(config)),
    );
    response.insert(
        "configuration".to_owned(),
        serde_json::json!({
            "urls": {
                "streaming": serde_json::Value::Null,
                "status": serde_json::Value::Null,
                "about": about_url,
                "privacy_policy": privacy_policy_url,
                "terms_of_service": terms_of_service_url,
            },
            "accounts": {
                "max_featured_tags": 10,
                "max_pinned_statuses": 0,
            },
            "statuses": {
                "max_characters": 500,
                "max_media_attachments": 4,
                "characters_reserved_per_url": 23,
            },
            "media_attachments": {
                "supported_mime_types": instance_supported_mime_types(),
                "description_limit": 1500,
                "image_size_limit": MAX_IMAGE_UPLOAD_BYTES,
                "video_size_limit": MAX_AV_UPLOAD_BYTES,
            },
            "polls": {
                "max_options": 0,
                "max_characters_per_option": 0,
                "min_expiration": 0,
                "max_expiration": 0,
            },
            "translation": {
                "enabled": false,
            },
            "timelines_access": {
                "live_feeds": {
                    "local": "public",
                    "remote": "public",
                },
                "hashtag_feeds": {
                    "local": "public",
                    "remote": "public",
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
        serde_json::json!({ "mastodon": 1 }),
    );
    response.insert(
        "rules".to_owned(),
        serde_json::json!(Vec::<serde_json::Value>::new()),
    );

    if let Some(source_url) = config.source_url.as_deref() {
        response.insert("source_url".to_owned(), serde_json::json!(source_url));
    }

    if let Some(thumbnail_url) = config.instance_thumbnail_url.as_deref() {
        response.insert(
            "thumbnail".to_owned(),
            serde_json::json!({
                "url": thumbnail_url,
            }),
        );
    }

    if let Some(contact_email) = config.contact_email.as_deref() {
        response.insert(
            "contact".to_owned(),
            serde_json::json!({
                "email": contact_email,
                "account": serde_json::Value::Null,
            }),
        );
    }

    serde_json::Value::Object(response)
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
