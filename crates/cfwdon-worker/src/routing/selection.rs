#[derive(Clone, Copy)]
pub(crate) enum FastRouterKind {
    Account,
    Discovery,
    Instance,
    Media,
    OAuth,
    Status,
    Timeline,
}

pub(crate) fn fast_router_kind(method: &str, path: &str) -> Option<FastRouterKind> {
    if matches!(
        path,
        "/.well-known/oauth-authorization-server"
            | "/.well-known/webfinger"
            | "/.well-known/host-meta"
            | "/.well-known/host-meta.json"
            | "/.well-known/nodeinfo"
            | "/nodeinfo/2.0"
            | "/api/oembed"
            | "/authorize_interaction"
            | "/share"
    ) {
        return Some(FastRouterKind::Discovery);
    }
    if path.starts_with("/oauth/") {
        return Some(FastRouterKind::OAuth);
    }
    if method == "GET"
        && (path.starts_with("/api/v1/instance")
            || matches!(
                path,
                "/api/v2/instance"
                    | "/api/v1/custom_emojis"
                    | "/api/v1/trends"
                    | "/api/v1/trends/statuses"
                    | "/api/v1/trends/tags"
                    | "/api/v1/trends/links"
                    | "/api/v1/announcements"
                    | "/api/v1/donation_campaigns"
            ))
    {
        return Some(FastRouterKind::Instance);
    }
    if path.starts_with("/api/v1/timelines/") {
        return Some(FastRouterKind::Timeline);
    }
    if path == "/api/v1/accounts"
        || path.starts_with("/api/v1/accounts/")
        || matches!(
            path,
            "/api/v1/blocks"
                | "/api/v1/directory"
                | "/api/v1/favourites"
                | "/api/v1/endorsements"
                | "/api/v1/bookmarks"
                | "/api/v1/followed_tags"
                | "/api/v1/mutes"
                | "/api/v1/follow_requests"
        )
        || path.starts_with("/api/v1/follow_requests/")
    {
        return Some(FastRouterKind::Account);
    }
    if path == "/api/v1/statuses" || path.starts_with("/api/v1/statuses/") {
        return Some(FastRouterKind::Status);
    }
    if path == "/api/v1/media"
        || path == "/api/v2/media"
        || path.starts_with("/api/v1/media/")
        || path.starts_with("/api/v2/media/")
        || path.starts_with("/media/")
    {
        return Some(FastRouterKind::Media);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{FastRouterKind, fast_router_kind};

    #[test]
    fn fast_router_kind_covers_hot_exact_and_prefix_routes() {
        assert!(matches!(
            fast_router_kind("GET", "/api/v1/instance"),
            Some(FastRouterKind::Instance)
        ));
        assert!(matches!(
            fast_router_kind("GET", "/api/v1/timelines/public"),
            Some(FastRouterKind::Timeline)
        ));
        assert!(matches!(
            fast_router_kind("POST", "/api/v1/statuses/1/favourite"),
            Some(FastRouterKind::Status)
        ));
        assert!(matches!(
            fast_router_kind("GET", "/api/v1/accounts/relationships"),
            Some(FastRouterKind::Account)
        ));
        assert!(matches!(
            fast_router_kind("POST", "/oauth/token"),
            Some(FastRouterKind::OAuth)
        ));
    }

    #[test]
    fn fast_router_kind_leaves_unclassified_routes_for_fallback_router() {
        assert!(fast_router_kind("GET", "/api/v1/lists").is_none());
        assert!(fast_router_kind("GET", "/users/alice").is_none());
    }
}
