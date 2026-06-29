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
    }
}
