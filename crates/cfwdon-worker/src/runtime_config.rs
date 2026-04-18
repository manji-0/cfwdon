use super::{AppConfig, RouteContext, parse_csv_list};
use cfwdon_core::BuildMetadata;

#[derive(Debug, serde::Serialize)]
pub(crate) struct RootDocument {
    pub(crate) service: String,
    pub(crate) version: String,
    pub(crate) runtime: String,
    pub(crate) endpoints: Vec<&'static str>,
}

pub(crate) const MAX_IMAGE_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_AV_UPLOAD_BYTES: usize = 40 * 1024 * 1024;

pub(crate) fn root_document() -> RootDocument {
    let build = build_metadata();

    RootDocument {
        service: build.service_name.to_owned(),
        version: build.version.to_owned(),
        runtime: build.runtime.to_owned(),
        endpoints: vec![
            "/",
            "/healthz",
            "/api/v1/instance",
            "/api/v1/timelines/home",
            "/api/v1/timelines/public",
            "/api/v1/timelines/tag/:hashtag",
            "/api/v1/timelines/list/:id",
            "/api/v1/statuses/:id",
            "/api/v1/statuses/:id/favourite",
            "/api/v1/statuses/:id/unfavourite",
            "/api/v1/statuses/:id/reblog",
            "/api/v1/statuses/:id/unreblog",
            "/api/v1/statuses/:id/bookmark",
            "/api/v1/statuses/:id/unbookmark",
            "/api/v1/statuses/:id/context",
            "/api/v1/tags/:name",
            "/.well-known/webfinger",
            "/inbox",
            "/users/:username",
            "/users/:username/followers",
            "/users/:username/following",
            "/users/:username/inbox",
            "/users/:username/outbox",
            "/users/:username/statuses/:id",
            "/media/:id",
            "/api/v1/media/:id",
            "/api/v1/statuses",
            "/api/v2/media",
            "/api/v2/media/:id",
            "/internal/polls/process-expired",
            "/api/v1/accounts/verify_credentials",
            "/api/v1/accounts/update_credentials",
            "/api/v1/accounts/:id",
            "/api/v1/accounts/:id/statuses",
            "/api/v1/accounts/:id/follow",
            "/api/v1/accounts/:id/unfollow",
            "/api/v1/accounts/:id/block",
            "/api/v1/accounts/:id/unblock",
            "/api/v1/accounts/:id/mute",
            "/api/v1/accounts/:id/unmute",
            "/api/v1/accounts/relationships",
            "/api/v1/accounts/lookup",
            "/api/v1/accounts/search",
            "/api/v1/directory",
            "/api/v1/favourites",
            "/api/v1/bookmarks",
            "/api/v1/mutes",
            "/api/v1/notifications",
            "/api/v1/notifications/:id",
            "/api/v1/notifications/:id/dismiss",
            "/api/v1/notifications/clear",
            "/api/v1/notifications/unread_count",
            "/api/v1/polls/:id",
            "/api/v1/polls/:id/votes",
            "/api/v1/reports",
            "/api/v1/instance/peers",
            "/api/v1/instance/extended_description",
            "/api/v1/instance/privacy_policy",
            "/api/v1/instance/terms_of_service",
            "/api/v2/search",
            "/api/v2/instance",
            "/.well-known/nodeinfo",
            "/nodeinfo/2.0",
            "/internal/media/prune-orphans",
            "/internal/outbox/process",
        ],
    }
}

pub(crate) fn load_config(ctx: &RouteContext<()>) -> AppConfig {
    let mut config = AppConfig::new(
        optional_var(ctx, "INSTANCE_DOMAIN").unwrap_or_else(|| "example.com".to_owned()),
        optional_var(ctx, "INSTANCE_NAME").unwrap_or_else(|| "cfwdon".to_owned()),
        optional_var(ctx, "INSTANCE_DESCRIPTION").unwrap_or_else(|| {
            "Cloudflare Workers + D1 + R2 based Mastodon-compatible server".to_owned()
        }),
    );

    if let Some(value) = optional_var(ctx, "SOURCE_URL") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.source_url = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_LANGUAGES") {
        let languages = parse_csv_list(&value);
        if !languages.is_empty() {
            config.instance_languages = languages;
        }
    }

    if let Some(value) = optional_var(ctx, "ADMIN_EMAILS") {
        config.admin_emails = parse_csv_list(&value);
    }

    if let Some(value) = optional_var(ctx, "CONTACT_EMAIL") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.contact_email = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_THUMBNAIL_URL") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.instance_thumbnail_url = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_EXTENDED_DESCRIPTION_HTML") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.instance_extended_description_html = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_EXTENDED_DESCRIPTION_UPDATED_AT") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.instance_extended_description_updated_at = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "PRIVACY_POLICY_HTML") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.privacy_policy_html = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "PRIVACY_POLICY_UPDATED_AT") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.privacy_policy_updated_at = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "TERMS_OF_SERVICE_HTML") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.terms_of_service_html = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "TERMS_OF_SERVICE_EFFECTIVE_DATE") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.terms_of_service_effective_date = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "MEDIA_PUBLIC_BASE_URL") {
        let value = value.trim().trim_end_matches('/').to_owned();
        if !value.is_empty() {
            config.media_public_base_url = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "ACCESS_EMAIL_HEADER") {
        config.access_email_header = value;
    }

    if let Some(value) = optional_var(ctx, "ACCESS_JWT_HEADER") {
        config.access_jwt_header = value;
    }

    if let Some(value) = optional_var(ctx, "ACCESS_TEAM_DOMAIN") {
        config.access_team_domain = value;
    }

    if let Some(value) = optional_var(ctx, "ACCESS_AUD") {
        config.access_audience = value;
    }

    config
}

pub(crate) const fn build_metadata() -> BuildMetadata {
    BuildMetadata::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        "cloudflare-workers",
    )
}

fn optional_var(ctx: &RouteContext<()>, key: &str) -> Option<String> {
    ctx.var(key).ok().map(|value| value.to_string())
}
