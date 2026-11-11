use crate::federation::url::remote_http_url_scheme_allowed;

pub const ACTIVITYPUB_REQUIRED_SIGNED_HEADERS: &[&str] = &["(request-target)", "date", "digest"];

/// How old a signed request's `Date` may be: Mastodon's
/// `SignatureVerification::EXPIRATION_WINDOW_LIMIT` (12 hours).
pub const ACTIVITYPUB_MAX_DATE_AGE_MS: f64 = 12.0 * 60.0 * 60.0 * 1000.0;

/// How far ahead of this server's clock a signed request's `Date` may be:
/// Mastodon's `CLOCK_SKEW_MARGIN` (1 hour).
pub const ACTIVITYPUB_MAX_DATE_AHEAD_MS: f64 = 60.0 * 60.0 * 1000.0;

/// How long an hs2019 signature without an `expires` parameter stays valid
/// after its `created`: Mastodon's default in `matches_time_window?`.
pub const ACTIVITYPUB_SIGNATURE_DEFAULT_LIFETIME_MS: f64 = 5.0 * 60.0 * 1000.0;

pub fn activitypub_signature_lists_required_headers(signed_headers: &[String]) -> bool {
    ACTIVITYPUB_REQUIRED_SIGNED_HEADERS
        .iter()
        .all(|required| signed_headers.iter().any(|header| header == required))
}

/// What a signed GET must cover, as Mastodon's `verify_signature_strength!`
/// asks: the `(request-target)`, the `host` it was sent to (so a GET signed
/// for another server cannot be replayed here), and when it was made, as
/// `date` or the hs2019 `(created)`. A GET has no body, so no `digest`.
pub fn activitypub_fetch_signature_lists_required_headers(signed_headers: &[String]) -> bool {
    let signs = |name: &str| signed_headers.iter().any(|header| header == name);
    signs("(request-target)") && signs("host") && (signs("date") || signs("(created)"))
}

pub fn activitypub_date_within_skew(parsed_ms: f64, now_ms: f64) -> bool {
    parsed_ms >= now_ms - ACTIVITYPUB_MAX_DATE_AGE_MS
        && parsed_ms <= now_ms + ACTIVITYPUB_MAX_DATE_AHEAD_MS
}

/// Whether an hs2019 signature that signs `(created)` is still valid, as
/// Mastodon's `matches_time_window?` decides: `created` at most 1 hour
/// ahead, and `now` at most 1 hour past `expires`. Without `expires` the
/// signature expires 5 minutes after `created`, and never later than 12 hours
/// after it.
pub fn activitypub_signature_created_within_window(
    created_ms: f64,
    expires_ms: Option<f64>,
    now_ms: f64,
) -> bool {
    let expires_ms = expires_ms
        .unwrap_or(created_ms + ACTIVITYPUB_SIGNATURE_DEFAULT_LIFETIME_MS)
        .min(created_ms + ACTIVITYPUB_MAX_DATE_AGE_MS);
    created_ms <= now_ms + ACTIVITYPUB_MAX_DATE_AHEAD_MS
        && now_ms <= expires_ms + ACTIVITYPUB_MAX_DATE_AHEAD_MS
}

pub fn activitypub_key_id_matches_actor(
    key_id: &str,
    raw_actor_uri: &str,
    canonical_actor_uri: &str,
) -> bool {
    if !remote_http_url_scheme_allowed(key_id) {
        return false;
    }
    let Some(key_actor) = key_id.trim().split('#').next() else {
        return false;
    };
    let key_actor = key_actor.trim();
    key_actor == raw_actor_uri.trim() || key_actor == canonical_actor_uri.trim()
}

pub fn cached_remote_actor_key_matches(
    key_id_matches_actor: bool,
    cached_public_key_id: &str,
    key_id: &str,
) -> bool {
    key_id_matches_actor && (cached_public_key_id.is_empty() || key_id == cached_public_key_id)
}

/// Compare a signed `Host` header value to the request URL hostname.
///
/// The header may include a port (`example.com:443`); only the hostname is
/// compared. IPv6 bracket forms are compared as a whole against `url_host`.
pub fn activitypub_host_header_matches_url_host(host_header: &str, url_host: &str) -> bool {
    let host_header = host_header.trim();
    let url_host = url_host.trim();
    if host_header.eq_ignore_ascii_case(url_host) {
        return true;
    }
    if let Some((name, port)) = host_header.rsplit_once(':')
        && !name.is_empty()
        && !name.contains(']')
        && port.chars().all(|c| c.is_ascii_digit())
    {
        return name.eq_ignore_ascii_case(url_host);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_may_be_twelve_hours_old_but_only_one_hour_ahead() {
        let now_ms = 1_000_000_000.0;
        let hour_ms = 60.0 * 60.0 * 1000.0;
        assert!(activitypub_date_within_skew(now_ms, now_ms));
        assert!(activitypub_date_within_skew(
            now_ms - 12.0 * hour_ms,
            now_ms
        ));
        assert!(!activitypub_date_within_skew(
            now_ms - 12.0 * hour_ms - 1.0,
            now_ms
        ));
        assert!(activitypub_date_within_skew(now_ms + hour_ms, now_ms));
        assert!(!activitypub_date_within_skew(
            now_ms + hour_ms + 1.0,
            now_ms
        ));
        assert!(!activitypub_date_within_skew(
            now_ms + 2.0 * hour_ms,
            now_ms
        ));
    }

    #[test]
    fn signature_headers_require_body_integrity_fields() {
        assert!(!activitypub_signature_lists_required_headers(&[
            "date".to_owned(),
            "digest".to_owned(),
        ]));
        assert!(activitypub_signature_lists_required_headers(&[
            "(request-target)".to_owned(),
            "host".to_owned(),
            "date".to_owned(),
            "digest".to_owned(),
        ]));
    }

    #[test]
    fn signed_get_must_cover_host_and_a_creation_time() {
        let headers = |names: &[&str]| {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        assert!(activitypub_fetch_signature_lists_required_headers(
            &headers(&["(request-target)", "host", "date",])
        ));
        assert!(activitypub_fetch_signature_lists_required_headers(
            &headers(&["(request-target)", "host", "(created)",])
        ));
        assert!(!activitypub_fetch_signature_lists_required_headers(
            &headers(&["(request-target)", "date",])
        ));
        assert!(!activitypub_fetch_signature_lists_required_headers(
            &headers(&["(request-target)", "host",])
        ));
        assert!(!activitypub_fetch_signature_lists_required_headers(
            &headers(&["host", "date",])
        ));
    }

    #[test]
    fn created_signature_expires_after_five_minutes_unless_it_says_otherwise() {
        let minute_ms = 60.0 * 1000.0;
        let hour_ms = 60.0 * minute_ms;
        let created_ms = 1_000_000_000.0;
        let within = |expires_ms, now_ms| {
            activitypub_signature_created_within_window(created_ms, expires_ms, now_ms)
        };
        assert!(within(None, created_ms));
        assert!(within(None, created_ms + 5.0 * minute_ms + hour_ms));
        assert!(!within(None, created_ms + 5.0 * minute_ms + hour_ms + 1.0));
        assert!(within(
            Some(created_ms + 2.0 * hour_ms),
            created_ms + 3.0 * hour_ms
        ));
        assert!(!within(
            Some(created_ms + 2.0 * hour_ms),
            created_ms + 3.0 * hour_ms + 1.0
        ));
        assert!(!within(
            Some(created_ms + 24.0 * hour_ms),
            created_ms + 14.0 * hour_ms
        ));
        assert!(within(None, created_ms - hour_ms));
        assert!(!within(None, created_ms - hour_ms - 1.0));
    }

    #[test]
    fn key_id_matches_actor_with_or_without_fragment() {
        assert!(activitypub_key_id_matches_actor(
            "https://remote.example/users/bob#main-key",
            "https://remote.example/users/bob",
            "https://remote.example/@bob",
        ));
        assert!(activitypub_key_id_matches_actor(
            "https://remote.example/@bob",
            "https://remote.example/users/bob",
            "https://remote.example/@bob",
        ));
        assert!(!activitypub_key_id_matches_actor(
            "https://remote.example/users/eve#main-key",
            "https://remote.example/users/bob",
            "https://remote.example/@bob",
        ));
    }

    #[test]
    fn host_header_matches_url_host_with_optional_port() {
        assert!(activitypub_host_header_matches_url_host(
            "social.example",
            "social.example"
        ));
        assert!(activitypub_host_header_matches_url_host(
            "Social.Example:443",
            "social.example"
        ));
        assert!(!activitypub_host_header_matches_url_host(
            "other.example",
            "social.example"
        ));
    }
}
