use crate::status::Visibility;

pub const ACTIVITYSTREAMS_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";
pub const ACTIVITYSTREAMS_PUBLIC_SHORT: &str = "as:Public";

pub fn is_public_audience_uri(value: &str) -> bool {
    matches!(value, ACTIVITYSTREAMS_PUBLIC | ACTIVITYSTREAMS_PUBLIC_SHORT)
}

/// Mastodon-compatible followers collection URIs end with `/followers`.
pub fn is_followers_collection_uri(value: &str) -> bool {
    value.trim_end_matches('/').ends_with("/followers")
}

pub fn audience_values_contains_public(values: &[String]) -> bool {
    values.iter().any(|value| is_public_audience_uri(value))
}

pub fn audience_values_contain_followers(values: &[String]) -> bool {
    values
        .iter()
        .any(|value| is_followers_collection_uri(value))
}

pub fn visibility_from_activitypub_audiences(
    to_contains_public: bool,
    cc_contains_public: bool,
    addresses_followers: bool,
) -> Visibility {
    if to_contains_public {
        Visibility::Public
    } else if cc_contains_public {
        Visibility::Unlisted
    } else if addresses_followers {
        Visibility::FollowersOnly
    } else {
        Visibility::Direct
    }
}

/// Audience placement flags for locally emitted ActivityPub notes.
///
/// Returns `(to_contains_public, cc_contains_public, addresses_followers)`.
/// Restricted visibilities must not place Public in either audience list.
pub fn activitypub_audience_flags_for_visibility(visibility: Visibility) -> (bool, bool, bool) {
    match visibility {
        Visibility::Public => (true, false, true),
        Visibility::Unlisted => (false, true, true),
        Visibility::FollowersOnly => (false, false, true),
        Visibility::Direct => (false, false, false),
    }
}

pub fn visibility_from_audience_lists(
    to_audiences: &[String],
    cc_audiences: &[String],
) -> Visibility {
    visibility_from_activitypub_audiences(
        audience_values_contains_public(to_audiences),
        audience_values_contains_public(cc_audiences),
        audience_values_contain_followers(to_audiences)
            || audience_values_contain_followers(cc_audiences),
    )
}

pub fn is_public_activitypub_visibility(visibility: Visibility) -> bool {
    matches!(visibility, Visibility::Public | Visibility::Unlisted)
}

pub fn quote_target_uri_from_fields(
    quote_uri: Option<&str>,
    quote_url: Option<&str>,
    misskey_quote: Option<&str>,
) -> Option<String> {
    [quote_uri, quote_url, misskey_quote]
        .into_iter()
        .find_map(|value| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_from_audiences_maps_public_unlisted_private_and_direct() {
        assert_eq!(
            visibility_from_activitypub_audiences(true, false, true),
            Visibility::Public
        );
        assert_eq!(
            visibility_from_activitypub_audiences(false, true, true),
            Visibility::Unlisted
        );
        assert_eq!(
            visibility_from_activitypub_audiences(false, false, true),
            Visibility::FollowersOnly
        );
        assert_eq!(
            visibility_from_activitypub_audiences(false, false, false),
            Visibility::Direct
        );
        assert_eq!(
            visibility_from_activitypub_audiences(true, true, false),
            Visibility::Public
        );
    }

    #[test]
    fn audience_flags_match_worker_activitypub_audiences() {
        assert_eq!(
            activitypub_audience_flags_for_visibility(Visibility::Public),
            (true, false, true)
        );
        assert_eq!(
            activitypub_audience_flags_for_visibility(Visibility::Unlisted),
            (false, true, true)
        );
        assert_eq!(
            activitypub_audience_flags_for_visibility(Visibility::FollowersOnly),
            (false, false, true)
        );
        assert_eq!(
            activitypub_audience_flags_for_visibility(Visibility::Direct),
            (false, false, false)
        );
    }

    #[test]
    fn visibility_from_audience_lists_honors_as_public_shortcut() {
        assert_eq!(
            visibility_from_audience_lists(&["as:Public".to_owned()], &[]),
            Visibility::Public
        );
    }

    #[test]
    fn visibility_from_audience_lists_detects_direct_without_followers() {
        assert_eq!(
            visibility_from_audience_lists(&["https://social.example/users/bob".to_owned()], &[]),
            Visibility::Direct
        );
        assert_eq!(
            visibility_from_audience_lists(
                &["https://social.example/users/alice/followers".to_owned()],
                &[]
            ),
            Visibility::FollowersOnly
        );
    }
}
