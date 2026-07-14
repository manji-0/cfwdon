use cfwdon_domain::{
    ACTIVITYPUB_REQUIRED_SIGNED_HEADERS, activitypub_date_within_skew,
    activitypub_key_id_matches_actor, activitypub_signature_lists_required_headers,
    cached_remote_actor_key_matches, remote_url_policy_from_parts,
};
use stateright::Model;

use crate::federation_request_policy::{
    FederationRequestPolicyAction, FederationRequestPolicyModel, FederationRequestPolicyModelState,
};
use crate::refinement::verify::assert_model_matches_domain;

const ACTOR_URI: &str = "https://remote.example/users/bob";
const CANONICAL_ACTOR_URI: &str = "https://remote.example/@bob";

fn model_domain_step(
    state: FederationRequestPolicyModelState,
    action: FederationRequestPolicyAction,
) -> Option<FederationRequestPolicyModelState> {
    FederationRequestPolicyModel.next_state(&state, action)
}

/// Mirrors `validate_activitypub_signature_headers` in the worker.
fn worker_validate_signature_headers(signed_headers: &[String]) -> bool {
    if activitypub_signature_lists_required_headers(signed_headers) {
        return true;
    }
    ACTIVITYPUB_REQUIRED_SIGNED_HEADERS
        .iter()
        .all(|required| signed_headers.iter().any(|header| header == required))
}

/// Mirrors `cached_remote_actor_matches_key` in the worker.
fn worker_cached_key_matches(
    key_id_matches_actor: bool,
    cached_public_key_id: &str,
    key_id: &str,
) -> bool {
    cached_remote_actor_key_matches(key_id_matches_actor, cached_public_key_id, key_id)
}

/// Mirrors scheme/host checks applied before `remote_url_policy_from_parts` in `url_guard`.
fn worker_remote_url_allowed(scheme: &str, host: &str, has_userinfo: bool) -> bool {
    remote_url_policy_from_parts(scheme, host, has_userinfo).is_ok()
}

/// Mirrors `validate_request_date` skew gate (ignoring parse failures).
fn worker_date_within_skew(parsed_ms: f64, now_ms: f64) -> bool {
    activitypub_date_within_skew(parsed_ms, now_ms)
}

pub(crate) fn check_federation_request_policy_refinement() {
    assert_model_matches_domain(&FederationRequestPolicyModel, model_domain_step);

    let signature_cases = [
        (vec!["date".to_owned(), "digest".to_owned()], false),
        (
            vec!["(request-target)".to_owned(), "date".to_owned()],
            false,
        ),
        (
            vec![
                "(request-target)".to_owned(),
                "host".to_owned(),
                "date".to_owned(),
                "digest".to_owned(),
            ],
            true,
        ),
    ];
    for (headers, expected) in signature_cases {
        assert_eq!(
            worker_validate_signature_headers(&headers),
            expected,
            "worker signature validation for {headers:?}"
        );
        assert_eq!(
            activitypub_signature_lists_required_headers(&headers),
            expected,
            "domain signature headers for {headers:?}"
        );
    }

    let key_id_cases = [
        ("https://remote.example/users/bob#main-key", true),
        ("https://remote.example/@bob", true),
        ("https://remote.example/users/eve#main-key", false),
        ("ftp://remote.example/users/bob#main-key", false),
    ];
    for (key_id, expected) in key_id_cases {
        assert_eq!(
            activitypub_key_id_matches_actor(key_id, ACTOR_URI, CANONICAL_ACTOR_URI),
            expected,
            "key id match for {key_id}"
        );
    }

    assert!(worker_cached_key_matches(
        true,
        "https://remote.example/users/bob#main-key",
        "https://remote.example/users/bob#main-key"
    ));
    assert!(!worker_cached_key_matches(
        false,
        "https://remote.example/users/bob#main-key",
        "https://remote.example/users/bob#main-key"
    ));
    assert!(worker_cached_key_matches(
        true,
        "",
        "https://remote.example/users/bob#main-key"
    ));

    let url_cases = [
        ("https", "remote.example", false, true),
        ("https", "localhost", false, false),
        ("https", "127.0.0.1", false, false),
        ("https", "remote.example", true, false),
        ("file", "remote.example", false, false),
    ];
    for (scheme, host, has_userinfo, expected) in url_cases {
        assert_eq!(
            worker_remote_url_allowed(scheme, host, has_userinfo),
            expected,
            "remote url policy for {scheme}://{host}"
        );
    }

    let now_ms = 1_000_000.0;
    assert!(worker_date_within_skew(now_ms, now_ms));
    assert!(!worker_date_within_skew(
        now_ms + cfwdon_domain::ACTIVITYPUB_MAX_DATE_SKEW_MS + 1.0,
        now_ms
    ));
}

#[cfg(test)]
mod tests {
    use super::check_federation_request_policy_refinement;

    #[test]
    fn federation_request_policy_refinement_holds() {
        check_federation_request_policy_refinement();
    }
}
