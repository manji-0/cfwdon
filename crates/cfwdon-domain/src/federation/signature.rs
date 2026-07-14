use crate::federation::url::remote_http_url_scheme_allowed;

pub const ACTIVITYPUB_REQUIRED_SIGNED_HEADERS: &[&str] = &["(request-target)", "date", "digest"];

pub const ACTIVITYPUB_MAX_DATE_SKEW_MS: f64 = 12.0 * 60.0 * 60.0 * 1000.0;

pub fn activitypub_signature_lists_required_headers(signed_headers: &[String]) -> bool {
    ACTIVITYPUB_REQUIRED_SIGNED_HEADERS
        .iter()
        .all(|required| signed_headers.iter().any(|header| header == required))
}

pub fn activitypub_date_within_skew(parsed_ms: f64, now_ms: f64) -> bool {
    (now_ms - parsed_ms).abs() <= ACTIVITYPUB_MAX_DATE_SKEW_MS
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
