use super::{AppConfig, Env, RouteContext, parse_csv_list};
use cfwdon_core::{BuildMetadata, TimelineAccessLevel};

#[derive(Debug, serde::Serialize)]
pub(crate) struct RootDocument {
    pub(crate) service: String,
    pub(crate) version: String,
    pub(crate) runtime: String,
    pub(crate) endpoints: Vec<&'static str>,
}

pub(crate) const MAX_IMAGE_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_AV_UPLOAD_BYTES: usize = 40 * 1024 * 1024;

const DEFAULT_INSTANCE_DOMAIN: &str = "example.com";
const DEFAULT_INSTANCE_NAME: &str = "cfwdon";
const DEFAULT_INSTANCE_DESCRIPTION: &str =
    "Cloudflare Workers + D1 + R2 based Mastodon-compatible server";
const TIMELINE_ACCESS_PUBLIC: &str = "public";
const TIMELINE_ACCESS_AUTHENTICATED: &str = "authenticated";
const TIMELINE_ACCESS_DISABLED: &str = "disabled";

const ROOT_ENDPOINTS: &[&str] = &[
    "/",
    "/healthz",
    "/.well-known/oauth-authorization-server",
    "/authorize_interaction",
    "/api/v1_alpha/async_refreshes/:id",
    "/api/v1/instance",
    "/api/v1/timelines/home",
    "/api/v1/timelines/direct",
    "/api/v1/timelines/public",
    "/api/v1/timelines/link",
    "/api/v1/timelines/tag/:hashtag",
    "/api/v1/timelines/list/:id",
    "/api/v1/statuses",
    "/api/v1/statuses/:id",
    "/api/v1/statuses/:id/favourite",
    "/api/v1/statuses/:id/unfavourite",
    "/api/v1/statuses/:id/reblog",
    "/api/v1/statuses/:id/unreblog",
    "/api/v1/statuses/:id/bookmark",
    "/api/v1/statuses/:id/unbookmark",
    "/api/v1/statuses/:id/context",
    "/api/v1/statuses/:id/quotes",
    "/api/v1/statuses/:id/quotes/:quote_id/revoke",
    "/api/v1/statuses/:id/interaction_policy",
    "/api/v1/statuses/:id/translate",
    "/api/v1/tags/:name",
    "/api/v1/tags/:id/follow",
    "/api/v1/tags/:id/unfollow",
    "/api/v1/tags/:id/feature",
    "/api/v1/tags/:id/unfeature",
    "/api/v1/suggestions",
    "/.well-known/webfinger",
    "/inbox",
    "/users/:username",
    "/users/:username/followers",
    "/users/:username/following",
    "/users/:username/inbox",
    "/users/:username/outbox",
    "/users/:username/statuses/:id",
    "/media/:id",
    "/api/v1/media",
    "/api/v1/media/:id",
    "/api/v2/media",
    "/api/v2/media/:id",
    "/internal/polls/process-expired",
    "/api/v1/accounts/verify_credentials",
    "/api/v1/accounts/update_credentials",
    "/api/v1/profile/header",
    "/api/v1/profile/avatar",
    "/api/v1/profile",
    "/api/v1/suggestions/:id",
    "/api/v1/accounts/:id",
    "/api/v1/accounts/:id/statuses",
    "/api/v1/accounts/:id/followers",
    "/api/v1/accounts/:id/following",
    "/api/v1/accounts/:id/featured_tags",
    "/api/v1/accounts/:id/endorsements",
    "/api/v1/accounts/:id/lists",
    "/api/v1/accounts/:id/identity_proofs",
    "/api/v1/accounts/:id/follow",
    "/api/v1/accounts/:id/unfollow",
    "/api/v1/accounts/:id/block",
    "/api/v1/accounts/:id/unblock",
    "/api/v1/accounts/:id/mute",
    "/api/v1/accounts/:id/unmute",
    "/api/v1/accounts/:id/pin",
    "/api/v1/accounts/:id/unpin",
    "/api/v1/accounts/:id/endorse",
    "/api/v1/accounts/:id/unendorse",
    "/api/v1/accounts/:id/note",
    "/api/v1/accounts/:id/email_subscriptions",
    "/api/v1/accounts/:id/remove_from_followers",
    "/api/v1/accounts/relationships",
    "/api/v1/accounts/familiar_followers",
    "/api/v1/accounts/lookup",
    "/api/v1/accounts/search",
    "/api/v1/accounts",
    "/api/v1/directory",
    "/api/v1/endorsements",
    "/api/v1/favourites",
    "/api/v1/bookmarks",
    "/api/v1/followed_tags",
    "/api/v1/filters",
    "/api/v1/filters/:id",
    "/api/v2/filters",
    "/api/v2/filters/:id",
    "/api/v2/filters/:id/keywords",
    "/api/v2/filters/keywords/:id",
    "/api/v2/filters/:id/statuses",
    "/api/v2/filters/statuses/:id",
    "/api/v1/blocks",
    "/api/v1/mutes",
    "/api/v1/follow_requests",
    "/api/v1/follow_requests/:id",
    "/api/v1/follow_requests/:id/authorize",
    "/api/v1/follow_requests/:id/reject",
    "/api/v1/notifications",
    "/api/v1/notifications/requests",
    "/api/v1/notifications/requests/:id",
    "/api/v1/notifications/requests/accept",
    "/api/v1/notifications/requests/dismiss",
    "/api/v1/notifications/requests/merged",
    "/api/v1/notifications/requests/:id/accept",
    "/api/v1/notifications/requests/:id/dismiss",
    "/api/v1/notifications/:id",
    "/api/v2/notifications/:group_key",
    "/api/v1/notifications/policy",
    "/api/v2/notifications/policy",
    "/api/v1/notifications/:id/dismiss",
    "/api/v1/notifications/clear",
    "/api/v2/notifications/clear",
    "/api/v1/notifications/unread_count",
    "/api/v2/notifications/unread_count",
    "/api/v2/notifications/:group_key/dismiss",
    "/api/v2/notifications/:group_key/accounts",
    "/api/v1/conversations/:id/unread",
    "/api/v1/polls/:id",
    "/api/v1/scheduled_statuses",
    "/api/v1/scheduled_statuses/:id",
    "/api/v1/polls/:id/votes",
    "/api/v1/reports",
    "/api/v1/lists/:id/accounts",
    "/api/v1/push/subscription",
    "/api/v1/peers/search",
    "/api/v1/domain_blocks/preview",
    "/api/v1/domain_blocks",
    "/api/v1/donation_campaigns",
    "/api/v1/annual_reports",
    "/api/v1/annual_reports/:id",
    "/api/v1/annual_reports/:id/read",
    "/api/v1/annual_reports/:id/generate",
    "/api/v1/annual_reports/:id/state",
    "/api/v1/apps/verify_credentials",
    "/api/v1/apps",
    "/api/v1/emails/confirmations",
    "/api/v1/emails/check_confirmation",
    "/auth/confirmation",
    "/api/v1/instance/peers",
    "/api/v1/instance/domain_blocks",
    "/api/v1/instance/extended_description",
    "/api/v1/instance/privacy_policy",
    "/api/v1/instance/terms_of_service",
    "/api/v1/instance/terms_of_service/:date",
    "/api/v1/instance/languages",
    "/api/v1/announcements/:id/reactions/:id",
    "/api/v1/announcements/:id/dismiss",
    "/api/v1/search",
    "/api/v2/search",
    "/api/v2/suggestions",
    "/api/v2/instance",
    "/api/v1/streaming",
    "/api/v1/streaming/*any",
    "/oauth/userinfo",
    "/api/oembed",
    "/.well-known/nodeinfo",
    "/nodeinfo/2.0",
    "/internal/media/prune-orphans",
    "/internal/outbox/process",
];

fn parse_timeline_access_level(value: Option<String>) -> Option<TimelineAccessLevel> {
    match value.as_deref().map(str::trim) {
        Some(TIMELINE_ACCESS_PUBLIC) => Some(TimelineAccessLevel::Public),
        Some(TIMELINE_ACCESS_AUTHENTICATED) => Some(TimelineAccessLevel::Authenticated),
        Some(TIMELINE_ACCESS_DISABLED) => Some(TimelineAccessLevel::Disabled),
        _ => None,
    }
}

pub(crate) fn root_document() -> RootDocument {
    let build = build_metadata();

    RootDocument {
        service: build.service_name.to_owned(),
        version: build.version.to_owned(),
        runtime: build.runtime.to_owned(),
        endpoints: root_endpoint_list(),
    }
}

fn root_endpoint_list() -> Vec<&'static str> {
    ROOT_ENDPOINTS.to_vec()
}

pub(crate) fn load_config<D>(ctx: &RouteContext<D>) -> AppConfig {
    config_from_vars(|key| optional_var(ctx, key))
}

pub(crate) fn load_config_from_env(env: &Env) -> AppConfig {
    config_from_vars(|key| env.var(key).ok().map(|value| value.to_string()))
}

fn config_from_vars<F>(vars: F) -> AppConfig
where
    F: Fn(&str) -> Option<String>,
{
    let mut config = AppConfig::new(
        config_string_or_default(&vars, "INSTANCE_DOMAIN", DEFAULT_INSTANCE_DOMAIN),
        config_string_or_default(&vars, "INSTANCE_NAME", DEFAULT_INSTANCE_NAME),
        config_string_or_default(&vars, "INSTANCE_DESCRIPTION", DEFAULT_INSTANCE_DESCRIPTION),
    );

    // Keep these grouped by product area so new environment variables land near
    // related defaults and tests.
    set_instance_metadata_config(&vars, &mut config);
    set_web_push_config(&vars, &mut config);
    set_instance_document_config(&vars, &mut config);

    set_timeline_access_config(&vars, &mut config);

    set_content_config(&vars, &mut config);
    set_access_config(&vars, &mut config);

    config
}

fn set_access_config(vars: &impl Fn(&str) -> Option<String>, config: &mut AppConfig) {
    set_raw_string(vars, "ACCESS_EMAIL_HEADER", &mut config.access_email_header);
    set_raw_string(vars, "ACCESS_JWT_HEADER", &mut config.access_jwt_header);
    set_raw_string(vars, "ACCESS_TEAM_DOMAIN", &mut config.access_team_domain);
    set_raw_string(vars, "ACCESS_AUD", &mut config.access_audience);
}

fn set_content_config(vars: &impl Fn(&str) -> Option<String>, config: &mut AppConfig) {
    set_trimmed_optional(vars, "ANNOUNCEMENTS_JSON", &mut config.announcements_json);
    set_trimmed_optional(
        vars,
        "DONATION_CAMPAIGN_JSON",
        &mut config.donation_campaign_json,
    );
    set_trimmed_base_url(
        vars,
        "MEDIA_PUBLIC_BASE_URL",
        &mut config.media_public_base_url,
    );
}

fn set_web_push_config(vars: &impl Fn(&str) -> Option<String>, config: &mut AppConfig) {
    set_trimmed_optional(
        vars,
        "WEB_PUSH_VAPID_PUBLIC_KEY",
        &mut config.web_push_vapid_public_key,
    );
    set_trimmed_optional(
        vars,
        "WEB_PUSH_VAPID_PRIVATE_KEY",
        &mut config.web_push_vapid_private_key,
    );
    set_trimmed_optional(
        vars,
        "WEB_PUSH_VAPID_SUBJECT",
        &mut config.web_push_vapid_subject,
    );
}

fn set_instance_metadata_config(vars: &impl Fn(&str) -> Option<String>, config: &mut AppConfig) {
    set_trimmed_optional(vars, "SOURCE_URL", &mut config.source_url);
    set_non_empty_csv_list(vars, "INSTANCE_LANGUAGES", &mut config.instance_languages);
    set_csv_list(vars, "ADMIN_EMAILS", &mut config.admin_emails);
    set_trimmed_optional(vars, "CONTACT_EMAIL", &mut config.contact_email);
    set_trimmed_optional(
        vars,
        "INSTANCE_THUMBNAIL_URL",
        &mut config.instance_thumbnail_url,
    );
}

fn set_instance_document_config(vars: &impl Fn(&str) -> Option<String>, config: &mut AppConfig) {
    set_trimmed_optional(
        vars,
        "INSTANCE_EXTENDED_DESCRIPTION_HTML",
        &mut config.instance_extended_description_html,
    );
    set_trimmed_optional(
        vars,
        "INSTANCE_EXTENDED_DESCRIPTION_UPDATED_AT",
        &mut config.instance_extended_description_updated_at,
    );
    set_trimmed_optional(vars, "PRIVACY_POLICY_HTML", &mut config.privacy_policy_html);
    set_trimmed_optional(
        vars,
        "PRIVACY_POLICY_UPDATED_AT",
        &mut config.privacy_policy_updated_at,
    );
    set_trimmed_optional(
        vars,
        "TERMS_OF_SERVICE_HTML",
        &mut config.terms_of_service_html,
    );
    set_trimmed_optional(
        vars,
        "TERMS_OF_SERVICE_EFFECTIVE_DATE",
        &mut config.terms_of_service_effective_date,
    );
}

fn set_trimmed_optional(
    vars: &impl Fn(&str) -> Option<String>,
    key: &str,
    target: &mut Option<String>,
) {
    if let Some(value) = trimmed_non_empty(vars(key).as_deref()) {
        *target = Some(value);
    }
}

fn config_string_or_default(
    vars: &impl Fn(&str) -> Option<String>,
    key: &str,
    default: &str,
) -> String {
    vars(key).unwrap_or_else(|| default.to_owned())
}

fn set_csv_list(vars: &impl Fn(&str) -> Option<String>, key: &str, target: &mut Vec<String>) {
    if let Some(value) = vars(key) {
        *target = parse_csv_list(&value);
    }
}

fn set_non_empty_csv_list(
    vars: &impl Fn(&str) -> Option<String>,
    key: &str,
    target: &mut Vec<String>,
) {
    if let Some(value) = vars(key) {
        let values = parse_csv_list(&value);
        if !values.is_empty() {
            *target = values;
        }
    }
}

fn set_trimmed_base_url(
    vars: &impl Fn(&str) -> Option<String>,
    key: &str,
    target: &mut Option<String>,
) {
    if let Some(value) = normalized_optional_base_url(vars(key).as_deref()) {
        *target = Some(value);
    }
}

fn normalized_optional_base_url(value: Option<&str>) -> Option<String> {
    value
        .map(normalize_optional_base_url)
        .and_then(|value| trimmed_non_empty(Some(&value)))
}

fn trimmed_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_optional_base_url(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

fn set_timeline_access_level(
    vars: &impl Fn(&str) -> Option<String>,
    key: &str,
    target: &mut TimelineAccessLevel,
) {
    if let Some(value) = parse_timeline_access_level(vars(key)) {
        *target = value;
    }
}

fn set_timeline_access_config(vars: &impl Fn(&str) -> Option<String>, config: &mut AppConfig) {
    set_timeline_access_level(
        vars,
        "TIMELINES_ACCESS_LIVE_FEEDS_LOCAL",
        &mut config.timeline_live_feeds_local,
    );
    set_timeline_access_level(
        vars,
        "TIMELINES_ACCESS_LIVE_FEEDS_REMOTE",
        &mut config.timeline_live_feeds_remote,
    );
    set_timeline_access_level(
        vars,
        "TIMELINES_ACCESS_HASHTAG_FEEDS_LOCAL",
        &mut config.timeline_hashtag_feeds_local,
    );
    set_timeline_access_level(
        vars,
        "TIMELINES_ACCESS_HASHTAG_FEEDS_REMOTE",
        &mut config.timeline_hashtag_feeds_remote,
    );
    set_timeline_access_level(
        vars,
        "TIMELINES_ACCESS_TRENDING_LINK_FEEDS_LOCAL",
        &mut config.timeline_trending_link_feeds_local,
    );
    set_timeline_access_level(
        vars,
        "TIMELINES_ACCESS_TRENDING_LINK_FEEDS_REMOTE",
        &mut config.timeline_trending_link_feeds_remote,
    );
}

fn set_raw_string(vars: &impl Fn(&str) -> Option<String>, key: &str, target: &mut String) {
    if let Some(value) = vars(key) {
        *target = value;
    }
}

pub(crate) const fn build_metadata() -> BuildMetadata {
    BuildMetadata::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        "cloudflare-workers",
    )
}

fn optional_var<D>(ctx: &RouteContext<D>, key: &str) -> Option<String> {
    ctx.var(key).ok().map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_timeline_access_level_accepts_supported_values() {
        assert_eq!(
            parse_timeline_access_level(Some(format!(" {TIMELINE_ACCESS_PUBLIC} "))),
            Some(TimelineAccessLevel::Public)
        );
        assert_eq!(
            parse_timeline_access_level(Some(TIMELINE_ACCESS_AUTHENTICATED.to_owned())),
            Some(TimelineAccessLevel::Authenticated)
        );
        assert_eq!(
            parse_timeline_access_level(Some(TIMELINE_ACCESS_DISABLED.to_owned())),
            Some(TimelineAccessLevel::Disabled)
        );
    }

    #[test]
    fn parse_timeline_access_level_ignores_unknown_values() {
        assert_eq!(parse_timeline_access_level(None), None);
        assert_eq!(parse_timeline_access_level(Some("".to_owned())), None);
        assert_eq!(
            parse_timeline_access_level(Some("private".to_owned())),
            None
        );
    }

    #[test]
    fn normalize_optional_base_url_trims_space_and_trailing_slashes() {
        assert_eq!(
            normalize_optional_base_url(" https://media.example.com/// "),
            "https://media.example.com"
        );
        assert_eq!(normalize_optional_base_url("   "), "");
    }

    #[test]
    fn normalized_optional_base_url_discards_blank_urls() {
        assert_eq!(
            normalized_optional_base_url(Some(" https://media.example.com/// ")).as_deref(),
            Some("https://media.example.com")
        );
        assert_eq!(normalized_optional_base_url(Some("///")), None);
        assert_eq!(normalized_optional_base_url(None), None);
    }

    #[test]
    fn trimmed_non_empty_discards_blank_values() {
        assert_eq!(
            trimmed_non_empty(Some(" cfwdon ")).as_deref(),
            Some("cfwdon")
        );
        assert_eq!(trimmed_non_empty(Some("   ")), None);
        assert_eq!(trimmed_non_empty(None), None);
    }

    #[test]
    fn root_endpoint_list_has_no_duplicates() {
        let endpoints = root_endpoint_list();
        let unique = endpoints.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), endpoints.len());
    }

    #[test]
    fn root_endpoint_list_is_not_empty() {
        assert!(!root_endpoint_list().is_empty());
    }

    #[test]
    fn root_endpoints_use_absolute_paths() {
        assert!(
            root_endpoint_list()
                .iter()
                .all(|endpoint| endpoint.starts_with('/'))
        );
    }

    #[test]
    fn root_endpoints_include_service_entrypoints() {
        let endpoints = root_endpoint_list();

        assert!(endpoints.contains(&"/"));
        assert!(endpoints.contains(&"/healthz"));
    }

    #[test]
    fn root_endpoints_include_discovery_entrypoints() {
        let endpoints = root_endpoint_list();

        assert!(endpoints.contains(&"/.well-known/webfinger"));
        assert!(endpoints.contains(&"/.well-known/oauth-authorization-server"));
    }

    #[test]
    fn root_endpoints_include_core_mastodon_apis() {
        let endpoints = root_endpoint_list();

        assert!(endpoints.contains(&"/api/v1/instance"));
        assert!(endpoints.contains(&"/api/v1/statuses"));
        assert!(endpoints.contains(&"/api/v1/accounts/verify_credentials"));
    }

    #[test]
    fn root_document_reflects_build_metadata() {
        let build = build_metadata();
        let document = root_document();

        assert_eq!(document.service, build.service_name);
        assert_eq!(document.version, build.version);
        assert_eq!(document.runtime, build.runtime);
        assert_eq!(document.endpoints, ROOT_ENDPOINTS);
    }

    #[test]
    fn default_instance_metadata_is_non_empty() {
        assert!(!DEFAULT_INSTANCE_DOMAIN.is_empty());
        assert!(!DEFAULT_INSTANCE_NAME.is_empty());
        assert!(!DEFAULT_INSTANCE_DESCRIPTION.is_empty());
    }

    #[test]
    fn upload_limits_keep_video_above_image_limit() {
        let av_limit = MAX_AV_UPLOAD_BYTES;
        let image_limit = MAX_IMAGE_UPLOAD_BYTES;
        assert!(av_limit > image_limit);
    }
}
