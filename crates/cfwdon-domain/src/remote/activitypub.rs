use crate::status::Visibility;

pub const ACTIVITYSTREAMS_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";
pub const ACTIVITYSTREAMS_PUBLIC_SHORT: &str = "as:Public";

pub fn is_public_audience_uri(value: &str) -> bool {
    matches!(value, ACTIVITYSTREAMS_PUBLIC | ACTIVITYSTREAMS_PUBLIC_SHORT)
}

pub fn audience_values_contains_public(values: &[String]) -> bool {
    values.iter().any(|value| is_public_audience_uri(value))
}

pub fn visibility_from_activitypub_audiences(
    to_contains_public: bool,
    cc_contains_public: bool,
) -> Visibility {
    if to_contains_public {
        Visibility::Public
    } else if cc_contains_public {
        Visibility::Unlisted
    } else {
        Visibility::FollowersOnly
    }
}

/// Audience placement flags for locally emitted ActivityPub notes.
///
/// Matches worker `activitypub_audiences`: unlisted posts place Public in `cc`,
/// while public, followers-only, and direct posts place Public in `to`.
pub fn activitypub_audience_flags_for_visibility(visibility: Visibility) -> (bool, bool) {
    match visibility {
        Visibility::Unlisted => (false, true),
        Visibility::Public | Visibility::FollowersOnly | Visibility::Direct => (true, false),
    }
}

pub fn visibility_from_audience_lists(
    to_audiences: &[String],
    cc_audiences: &[String],
) -> Visibility {
    visibility_from_activitypub_audiences(
        audience_values_contains_public(to_audiences),
        audience_values_contains_public(cc_audiences),
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
    fn visibility_from_audiences_maps_public_and_unlisted() {
        assert_eq!(
            visibility_from_activitypub_audiences(true, false),
            Visibility::Public
        );
        assert_eq!(
            visibility_from_activitypub_audiences(false, true),
            Visibility::Unlisted
        );
        assert_eq!(
            visibility_from_activitypub_audiences(false, false),
            Visibility::FollowersOnly
        );
        assert_eq!(
            visibility_from_activitypub_audiences(true, true),
            Visibility::Public
        );
    }

    #[test]
    fn audience_flags_match_worker_activitypub_audiences() {
        assert_eq!(
            activitypub_audience_flags_for_visibility(Visibility::Public),
            (true, false)
        );
        assert_eq!(
            activitypub_audience_flags_for_visibility(Visibility::Unlisted),
            (false, true)
        );
        assert_eq!(
            activitypub_audience_flags_for_visibility(Visibility::FollowersOnly),
            (true, false)
        );
    }

    #[test]
    fn visibility_from_audience_lists_honors_as_public_shortcut() {
        assert_eq!(
            visibility_from_audience_lists(&["as:Public".to_owned()], &[]),
            Visibility::Public
        );
    }
}
