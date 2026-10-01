use crate::auth::{LocalApiAuthentication, authenticate_local_api_request};
use crate::db_session::bind_request_d1;
use crate::identity::{actor_url, instance_base_url};
use crate::oauth_apps::build_app_verify_credentials_document_from_row;
use crate::oauth_store::{
    app_bearer_token_from_request, find_oauth_app_by_bearer_token, oauth_access_token_has_any_scope,
};
use crate::response::media_object_url;
use crate::response_utils::{CACHE_TTL_OAUTH_DISCOVERY, cache_public_response};
use crate::runtime_config::{load_config, load_config_from_env};
use worker::{Env, Request, Response, Result, RouteContext};

use super::{invalid_access_token_response, outside_authorized_scopes_response};

const OAUTH_SCOPES_SUPPORTED: &[&str] = &[
    "read",
    "profile",
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
    "follow",
    "push",
    "admin:read",
    "admin:read:accounts",
    "admin:read:reports",
    "admin:read:domain_allows",
    "admin:read:domain_blocks",
    "admin:read:ip_blocks",
    "admin:read:email_domain_blocks",
    "admin:read:canonical_email_blocks",
    "admin:write",
    "admin:write:accounts",
    "admin:write:reports",
    "admin:write:domain_allows",
    "admin:write:domain_blocks",
    "admin:write:ip_blocks",
    "admin:write:email_domain_blocks",
    "admin:write:canonical_email_blocks",
];

pub(crate) fn build_oauth_authorization_server_document(
    config: &cfwdon_core::AppConfig,
) -> serde_json::Value {
    let base_url = instance_base_url(config);
    let issuer = format!("{}/", base_url.trim_end_matches('/'));
    serde_json::json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{base_url}/oauth/authorize"),
        "token_endpoint": format!("{base_url}/oauth/token"),
        "userinfo_endpoint": format!("{base_url}/oauth/userinfo"),
        "revocation_endpoint": format!("{base_url}/oauth/revoke"),
        "app_registration_endpoint": format!("{base_url}/api/v1/apps"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query", "fragment", "form_post"],
        "grant_types_supported": ["authorization_code", "client_credentials"],
        "scopes_supported": OAUTH_SCOPES_SUPPORTED,
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "code_challenge_methods_supported": ["S256"],
        "service_documentation": "https://docs.joinmastodon.org/",
    })
}

pub(crate) fn build_oauth_userinfo_document(
    config: &cfwdon_core::AppConfig,
    account: &cfwdon_domain::LocalAccount,
) -> serde_json::Value {
    let base_url = instance_base_url(config);
    let issuer = format!("{}/", base_url.trim_end_matches('/'));
    let actor = actor_url(config, account.username());
    let picture = account
        .avatar_object_key()
        .map(|object_key| serde_json::json!(media_object_url(config, object_key)))
        .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "iss": issuer,
        "sub": actor,
        "preferred_username": account.username(),
        "name": account.display_name(),
        "profile": format!("{base_url}/@{}", account.username()),
        "picture": picture,
    })
}

#[cfg(test)]
pub(crate) fn build_app_verify_credentials_document(
    config: &cfwdon_core::AppConfig,
) -> serde_json::Value {
    crate::oauth_apps::build_app_verify_credentials_document_from_parts(
        "0",
        &config.instance_name,
        None,
        &[String::from("read")],
        &[String::from("urn:ietf:wg:oauth:2.0:oob")],
        "urn:ietf:wg:oauth:2.0:oob",
        config.web_push_vapid_public_key.as_deref().unwrap_or(""),
    )
}

pub(crate) async fn oauth_authorization_server_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    oauth_authorization_server_response_for_config(&config)
}

pub(crate) fn oauth_authorization_server_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    oauth_authorization_server_response_for_config(&config)
}

fn oauth_authorization_server_response_for_config(
    config: &cfwdon_core::AppConfig,
) -> Result<Response> {
    cache_public_response(
        Response::from_json(&build_oauth_authorization_server_document(config))?,
        CACHE_TTL_OAUTH_DISCOVERY,
    )
}

pub(crate) async fn oauth_userinfo_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let account = match authenticate_local_api_request(&req, &db, &config).await? {
        LocalApiAuthentication::OAuthToken(auth) => {
            if !oauth_access_token_has_any_scope(&auth.token, &["profile"]) {
                return outside_authorized_scopes_response();
            }
            auth.account
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            return invalid_access_token_response();
        }
        LocalApiAuthentication::Auth0(account) => account,
        LocalApiAuthentication::None => return invalid_access_token_response(),
    };
    Response::from_json(&build_oauth_userinfo_document(&config, &account))
}

pub(crate) async fn app_verify_credentials_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let Some(token) = app_bearer_token_from_request(&req)? else {
        return Response::error("The access token is invalid", 401);
    };
    let Some(app) = find_oauth_app_by_bearer_token(&db, &token).await? else {
        return Response::error("The access token is invalid", 401);
    };
    Response::from_json(&build_app_verify_credentials_document_from_row(
        &app, &config,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::actor_fixture_account;
    use cfwdon_core::AppConfig;

    #[test]
    fn oauth_authorization_server_document_matches_mastodon_discovery_shape() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let document = build_oauth_authorization_server_document(&config);

        assert_eq!(
            document.pointer("/issuer"),
            Some(&serde_json::json!("https://social.example/"))
        );
        assert_eq!(
            document.pointer("/app_registration_endpoint"),
            Some(&serde_json::json!("https://social.example/api/v1/apps"))
        );
        assert_eq!(
            document.pointer("/response_modes_supported/2"),
            Some(&serde_json::json!("form_post"))
        );
        assert_eq!(
            document.pointer("/code_challenge_methods_supported/0"),
            Some(&serde_json::json!("S256"))
        );
        assert_eq!(
            document.pointer("/service_documentation"),
            Some(&serde_json::json!("https://docs.joinmastodon.org/"))
        );
    }

    #[test]
    fn oauth_userinfo_document_exposes_standard_claims() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.media_public_base_url = Some("https://media.example.com".to_owned());
        let account = actor_fixture_account();
        let document = build_oauth_userinfo_document(&config, &account);

        assert_eq!(
            document.pointer("/iss"),
            Some(&serde_json::json!("https://social.example/"))
        );
        assert_eq!(
            document.pointer("/sub"),
            Some(&serde_json::json!("https://social.example/users/alice"))
        );
        assert_eq!(
            document.pointer("/preferred_username"),
            Some(&serde_json::json!("alice"))
        );
        assert_eq!(
            document.pointer("/profile"),
            Some(&serde_json::json!("https://social.example/@alice"))
        );
        assert_eq!(
            document.pointer("/picture"),
            Some(&serde_json::json!(
                "https://media.example.com/media/account/avatar/alice"
            ))
        );
    }

    #[test]
    fn app_verify_credentials_document_matches_mastodon_shape_without_secrets() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let document = build_app_verify_credentials_document(&config);

        assert_eq!(document.pointer("/id"), Some(&serde_json::json!("0")));
        assert_eq!(
            document.pointer("/name"),
            Some(&serde_json::json!("cfwdon"))
        );
        assert_eq!(
            document.pointer("/scopes/0"),
            Some(&serde_json::json!("read"))
        );
        assert_eq!(
            document.pointer("/redirect_uris/0"),
            Some(&serde_json::json!("urn:ietf:wg:oauth:2.0:oob"))
        );
        assert_eq!(document.pointer("/client_id"), None);
        assert_eq!(document.pointer("/client_secret"), None);
    }

    #[test]
    fn app_verify_credentials_document_uses_configured_vapid_key() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.web_push_vapid_public_key = Some("BExamplePublicKey".to_owned());

        let document = build_app_verify_credentials_document(&config);

        assert_eq!(
            document.pointer("/vapid_key"),
            Some(&serde_json::json!("BExamplePublicKey"))
        );
    }
}
