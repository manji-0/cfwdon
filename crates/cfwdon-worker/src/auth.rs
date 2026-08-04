use crate::D1Database;
#[allow(unused_imports)]
pub(crate) use crate::*;

mod account_store;
mod jwt;
#[allow(unused_imports)]
pub(crate) use account_store::*;
#[allow(unused_imports)]
pub(crate) use jwt::*;

pub(crate) use self::account_store::find_account_by_email;
pub(crate) use self::jwt::{auth0_roles_from_claims, verify_auth0_jwt};
use super::oauth_apps::{
    OAuthAccessTokenRow, app_bearer_token_from_request,
    find_oauth_access_token_with_account_by_bearer_token, find_oauth_app_by_bearer_token,
    oauth_access_token_has_any_scope, parse_bearer_authorization_header,
};
use cfwdon_core::{AppConfig, AuthenticatedUser};
use cfwdon_domain::LocalAccount;
use worker::{Error, Request, Result};

pub(crate) use self::account_store::{
    ensure_account_keys, find_account_by_id, find_account_by_username, resolve_local_account,
};

pub(crate) const AUTH0_SESSION_COOKIE: &str = "cfwdon_auth0_access_token";

#[derive(Clone, Debug)]
pub(crate) struct OAuthAuthenticatedLocalAccount {
    pub(crate) account: LocalAccount,
    pub(crate) token: OAuthAccessTokenRow,
}

#[derive(Clone, Debug)]
pub(crate) enum LocalApiAuthentication {
    Auth0(LocalAccount),
    OAuthToken(OAuthAuthenticatedLocalAccount),
    AppToken,
    InvalidBearer,
    None,
}

pub(crate) async fn extract_authenticated_user(
    req: &Request,
    config: &AppConfig,
) -> Result<Option<AuthenticatedUser>> {
    let Some(token) = auth0_token_from_request(req, config)? else {
        return Ok(None);
    };

    if config.auth0_domain.is_empty() || config.auth0_audience.is_empty() {
        return Err(Error::RustError(
            "missing Auth0 configuration: AUTH0_DOMAIN and AUTH0_AUDIENCE are required".to_owned(),
        ));
    }

    let claims = verify_auth0_jwt(&token, config).await?;
    require_auth0_email_verified(&claims, &config.auth0_email_claim)?;
    let email = claims
        .string_claim(&config.auth0_email_claim)
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::RustError(format!(
                "validated Auth0 JWT did not include a string {} claim",
                config.auth0_email_claim
            ))
        })?;

    let roles = auth0_roles_from_claims(&claims, config);

    Ok(Some(AuthenticatedUser::auth0(email, true, roles)))
}

fn auth0_token_from_request(req: &Request, config: &AppConfig) -> Result<Option<String>> {
    if let Some(value) = req.headers().get(&config.auth0_jwt_header)?
        && !value.trim().is_empty()
    {
        if config
            .auth0_jwt_header
            .eq_ignore_ascii_case("authorization")
        {
            return Ok(parse_bearer_authorization_header(&value));
        }
        return Ok(Some(value.trim().to_owned()));
    }
    request_cookie_value(req, AUTH0_SESSION_COOKIE)
}

fn request_cookie_value(req: &Request, name: &str) -> Result<Option<String>> {
    let Some(cookie_header) = req.headers().get("Cookie")? else {
        return Ok(None);
    };
    Ok(cookie_header.split(';').find_map(|part| {
        let (cookie_name, value) = part.trim().split_once('=')?;
        (cookie_name == name && !value.trim().is_empty()).then(|| value.trim().to_owned())
    }))
}

pub(crate) async fn find_authenticated_local_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<LocalAccount>> {
    Ok(find_authenticated_local_account_with_roles(req, db, config)
        .await?
        .map(|(account, _roles)| account))
}

pub(crate) async fn find_authenticated_local_account_with_roles(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<(LocalAccount, Vec<String>)>> {
    if let Some(token) = app_bearer_token_from_request(req)?
        && let Some(auth) = find_oauth_access_token_with_account_by_bearer_token(db, &token).await?
    {
        if oauth_access_token_allows_request(req, &auth.token) {
            return Ok(auth.account.map(|account| (account, Vec::new())));
        }
        return Ok(None);
    }

    find_auth0_local_account_with_roles(req, db, config).await
}

async fn find_auth0_local_account_with_roles(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<(LocalAccount, Vec<String>)>> {
    let Some(user) = extract_authenticated_user(req, config).await? else {
        return Ok(None);
    };
    let Some(account) = find_account_by_email(db, &user.email).await? else {
        return Ok(None);
    };
    Ok(Some((account, user.roles)))
}

fn oauth_access_token_allows_request(req: &Request, token: &OAuthAccessTokenRow) -> bool {
    let path = req
        .url()
        .ok()
        .map(|url| url.path().to_owned())
        .unwrap_or_default();
    oauth_access_token_allows_method_path(req.method().as_ref(), &path, token)
}

fn oauth_access_token_allows_method_path(
    method: &str,
    path: &str,
    token: &OAuthAccessTokenRow,
) -> bool {
    if method == "GET"
        && matches!(
            path,
            "/api/v1/accounts/verify_credentials" | "/api/v1/profile"
        )
        && oauth_access_token_has_any_scope(token, &["profile", "read:accounts", "read"])
    {
        return true;
    }

    if matches!(method, "PATCH" | "PUT" | "POST")
        && path == "/api/v1/accounts/update_credentials"
        && oauth_access_token_has_any_scope(token, &["write:accounts", "write"])
    {
        return true;
    }

    match method {
        "GET" | "HEAD" | "OPTIONS" => oauth_access_token_has_any_scope(
            token,
            &[
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
            ],
        ),
        _ => oauth_access_token_has_any_scope(
            token,
            &[
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
                "follow",
                "push",
            ],
        ),
    }
}

pub(crate) async fn authenticate_local_api_request(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<LocalApiAuthentication> {
    if let Some(token) = app_bearer_token_from_request(req)? {
        if let Some(auth) = find_oauth_access_token_with_account_by_bearer_token(db, &token).await?
        {
            let Some(account) = auth.account else {
                return Ok(LocalApiAuthentication::InvalidBearer);
            };
            return Ok(LocalApiAuthentication::OAuthToken(
                OAuthAuthenticatedLocalAccount {
                    account,
                    token: auth.token,
                },
            ));
        }
        if find_oauth_app_by_bearer_token(db, &token).await?.is_some() {
            return Ok(LocalApiAuthentication::AppToken);
        }
        return match find_auth0_local_account_with_roles(req, db, config).await {
            Ok(Some((account, _roles))) => Ok(LocalApiAuthentication::Auth0(account)),
            Ok(None) | Err(_) => Ok(LocalApiAuthentication::InvalidBearer),
        };
    }

    Ok(find_authenticated_local_account(req, db, config)
        .await?
        .map(LocalApiAuthentication::Auth0)
        .unwrap_or(LocalApiAuthentication::None))
}
