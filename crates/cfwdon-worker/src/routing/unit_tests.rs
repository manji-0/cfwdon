use crate::routing::is_cors_enabled_path;

#[test]
fn cors_enabled_paths_cover_browser_client_oauth_surfaces() {
    assert!(is_cors_enabled_path("/api/v1/apps"));
    assert!(is_cors_enabled_path("/oauth/token"));
    assert!(is_cors_enabled_path("/media/attachment-1"));
    assert!(is_cors_enabled_path("/profiles/account-1/avatar/avatar-1"));
    assert!(is_cors_enabled_path("/users/alice"));
    assert!(is_cors_enabled_path("/users/alice/statuses/status-1"));
    assert!(is_cors_enabled_path(
        "/.well-known/oauth-authorization-server"
    ));
    assert!(is_cors_enabled_path("/.well-known/webfinger"));
    assert!(is_cors_enabled_path("/.well-known/host-meta"));
    assert!(is_cors_enabled_path("/.well-known/host-meta.json"));
    assert!(is_cors_enabled_path("/.well-known/nodeinfo"));
    assert!(is_cors_enabled_path("/nodeinfo/2.0"));
    assert!(is_cors_enabled_path("/nodeinfo/2.1"));
    assert!(!is_cors_enabled_path("/admin"));
}
