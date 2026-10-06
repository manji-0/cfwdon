//! Per-endpoint OAuth scope requirements, mirroring the `doorkeeper_authorize`
//! declarations on upstream Mastodon API controllers. A token passes when it
//! holds any scope in the returned set; each set already lists the parent
//! scope (`read` / `write`) and, where Mastodon still honours it, the legacy
//! `follow` scope.

const READ_ANY: &[&str] = &[
    "read",
    "read:accounts",
    "read:blocks",
    "read:bookmarks",
    "read:collections",
    "read:favourites",
    "read:filters",
    "read:follows",
    "read:lists",
    "read:mutes",
    "read:notifications",
    "read:search",
    "read:statuses",
];
const WRITE_ANY: &[&str] = &[
    "write",
    "write:accounts",
    "write:blocks",
    "write:bookmarks",
    "write:collections",
    "write:conversations",
    "write:favourites",
    "write:filters",
    "write:follows",
    "write:lists",
    "write:media",
    "write:mutes",
    "write:notifications",
    "write:reports",
    "write:statuses",
];

const PROFILE: &[&str] = &["profile", "read", "read:accounts"];
const READ_ACCOUNTS: &[&str] = &["read", "read:accounts"];
const READ_BLOCKS: &[&str] = &["follow", "read", "read:blocks"];
const READ_BOOKMARKS: &[&str] = &["read", "read:bookmarks"];
const READ_FAVOURITES: &[&str] = &["read", "read:favourites"];
const READ_FILTERS: &[&str] = &["read", "read:filters"];
const READ_FOLLOWS: &[&str] = &["follow", "read", "read:follows"];
const READ_LISTS: &[&str] = &["read", "read:lists"];
const READ_MUTES: &[&str] = &["follow", "read", "read:mutes"];
const READ_NOTIFICATIONS: &[&str] = &["read", "read:notifications"];
const READ_SEARCH: &[&str] = &["read", "read:search"];
const READ_STATUSES: &[&str] = &["read", "read:statuses"];
const WRITE_ACCOUNTS: &[&str] = &["write", "write:accounts"];
const WRITE_BLOCKS: &[&str] = &["follow", "write", "write:blocks"];
const WRITE_BOOKMARKS: &[&str] = &["write", "write:bookmarks"];
const WRITE_CONVERSATIONS: &[&str] = &["write", "write:conversations"];
const WRITE_FAVOURITES: &[&str] = &["write", "write:favourites"];
const WRITE_FILTERS: &[&str] = &["write", "write:filters"];
const WRITE_FOLLOWS: &[&str] = &["follow", "write", "write:follows"];
const WRITE_LISTS: &[&str] = &["write", "write:lists"];
const WRITE_MEDIA: &[&str] = &["write", "write:media"];
const WRITE_MUTES: &[&str] = &["follow", "write", "write:mutes"];
const WRITE_NOTIFICATIONS: &[&str] = &["write", "write:notifications"];
const WRITE_REPORTS: &[&str] = &["write", "write:reports"];
const WRITE_STATUSES: &[&str] = &["write", "write:statuses"];
const PUSH: &[&str] = &["push"];

/// Scopes that authorize `method path` for a user OAuth token.
///
/// Endpoints without a specific entry fall back to "any read scope" for safe
/// methods and "any write scope" otherwise; neither fallback accepts the
/// `follow` or `push` scopes.
pub(crate) fn required_oauth_scopes(method: &str, path: &str) -> &'static [&'static str] {
    let read = matches!(method, "GET" | "HEAD" | "OPTIONS");
    let rest = path
        .strip_prefix("/api/v1/")
        .or_else(|| path.strip_prefix("/api/v2/"))
        .unwrap_or_default();
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let pick = |r: &'static [&'static str], w: &'static [&'static str]| if read { r } else { w };

    match segments.as_slice() {
        ["profile"] if read => PROFILE,
        ["profile", ..] => WRITE_ACCOUNTS,
        ["accounts", "verify_credentials"] => PROFILE,
        ["accounts", "update_credentials"] => WRITE_ACCOUNTS,
        ["accounts", "relationships" | "familiar_followers"] => READ_FOLLOWS,
        ["accounts", _, "statuses"] => READ_STATUSES,
        ["accounts", _, "lists"] => READ_LISTS,
        [
            "accounts",
            _,
            "follow" | "unfollow" | "remove_from_followers",
        ] => WRITE_FOLLOWS,
        ["accounts", _, "block" | "unblock"] => WRITE_BLOCKS,
        ["accounts", _, "mute" | "unmute"] => WRITE_MUTES,
        [
            "accounts",
            _,
            "pin" | "unpin" | "endorse" | "unendorse" | "note",
        ] => WRITE_ACCOUNTS,
        ["accounts", ..] => pick(READ_ACCOUNTS, WRITE_ACCOUNTS),
        ["blocks", ..] => READ_BLOCKS,
        ["mutes", ..] => READ_MUTES,
        ["domain_blocks", ..] => pick(READ_BLOCKS, WRITE_BLOCKS),
        ["follow_requests", ..] => pick(READ_FOLLOWS, WRITE_FOLLOWS),
        ["followed_tags", ..] => READ_FOLLOWS,
        ["tags", _, "follow" | "unfollow"] => WRITE_FOLLOWS,
        ["tags", _, "feature" | "unfeature"] => WRITE_ACCOUNTS,
        ["endorsements" | "preferences", ..] => READ_ACCOUNTS,
        ["featured_tags", ..] | ["suggestions", ..] => pick(READ_ACCOUNTS, WRITE_ACCOUNTS),
        ["favourites", ..] => READ_FAVOURITES,
        ["bookmarks", ..] => READ_BOOKMARKS,
        ["statuses", _, "favourite" | "unfavourite"] => WRITE_FAVOURITES,
        ["statuses", _, "bookmark" | "unbookmark"] => WRITE_BOOKMARKS,
        ["statuses", _, "mute" | "unmute"] => WRITE_MUTES,
        ["statuses", _, "pin" | "unpin"] => WRITE_ACCOUNTS,
        ["statuses", _, "reblogged_by" | "favourited_by"] => READ_ACCOUNTS,
        ["statuses", _, "translate"] => READ_STATUSES,
        ["statuses", ..] | ["scheduled_statuses", ..] | ["polls", ..] | ["markers", ..] => {
            pick(READ_STATUSES, WRITE_STATUSES)
        }
        ["timelines", "list", ..] => READ_LISTS,
        ["timelines", ..] => READ_STATUSES,
        ["conversations", ..] => pick(READ_STATUSES, WRITE_CONVERSATIONS),
        ["notifications", ..] => pick(READ_NOTIFICATIONS, WRITE_NOTIFICATIONS),
        ["lists", ..] => pick(READ_LISTS, WRITE_LISTS),
        ["filters", ..] => pick(READ_FILTERS, WRITE_FILTERS),
        ["media", ..] => WRITE_MEDIA,
        ["reports", ..] => WRITE_REPORTS,
        ["search", ..] => READ_SEARCH,
        ["push", ..] => PUSH,
        _ => pick(READ_ANY, WRITE_ANY),
    }
}

#[cfg(test)]
mod tests {
    use super::required_oauth_scopes;

    fn allows(method: &str, path: &str, scope: &str) -> bool {
        required_oauth_scopes(method, path).contains(&scope)
    }

    #[test]
    fn push_scope_only_reaches_push_subscription() {
        assert!(allows("POST", "/api/v1/push/subscription", "push"));
        assert!(!allows("POST", "/api/v1/statuses", "push"));
        assert!(!allows("GET", "/api/v1/timelines/home", "push"));
        assert!(!allows("POST", "/api/v1/unknown_endpoint", "push"));
    }

    #[test]
    fn granular_scopes_do_not_cross_resources() {
        assert!(allows("POST", "/api/v1/statuses", "write:statuses"));
        assert!(!allows("POST", "/api/v1/statuses", "write:media"));
        assert!(allows("POST", "/api/v2/media", "write:media"));
        assert!(!allows("GET", "/api/v1/notifications", "read:statuses"));
        assert!(allows("GET", "/api/v2/notifications", "read:notifications"));
        assert!(allows("GET", "/api/v1/timelines/list/1", "read:lists"));
        assert!(!allows("GET", "/api/v1/timelines/list/1", "read:statuses"));
        assert!(allows(
            "POST",
            "/api/v1/statuses/1/favourite",
            "write:favourites"
        ));
        assert!(!allows(
            "POST",
            "/api/v1/statuses/1/favourite",
            "write:statuses"
        ));
    }

    #[test]
    fn parent_scopes_cover_children() {
        assert!(allows("GET", "/api/v1/bookmarks", "read"));
        assert!(allows("DELETE", "/api/v1/lists/1", "write"));
        assert!(!allows("DELETE", "/api/v1/lists/1", "read"));
    }

    #[test]
    fn legacy_follow_scope_covers_relationship_endpoints_only() {
        assert!(allows("POST", "/api/v1/accounts/1/follow", "follow"));
        assert!(allows("GET", "/api/v1/blocks", "follow"));
        assert!(allows(
            "POST",
            "/api/v1/follow_requests/1/authorize",
            "follow"
        ));
        assert!(!allows("POST", "/api/v1/statuses", "follow"));
        assert!(!allows(
            "PATCH",
            "/api/v1/accounts/update_credentials",
            "follow"
        ));
    }

    #[test]
    fn profile_scope_reads_own_credentials() {
        assert!(allows(
            "GET",
            "/api/v1/accounts/verify_credentials",
            "profile"
        ));
        assert!(allows("GET", "/api/v1/profile", "profile"));
        assert!(!allows(
            "PATCH",
            "/api/v1/accounts/update_credentials",
            "profile"
        ));
    }
}
