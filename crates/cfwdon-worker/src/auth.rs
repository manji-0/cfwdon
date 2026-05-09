use super::auth_account_store::find_account_by_email;
use super::auth_jwt::verify_access_jwt;
use super::oauth_apps::{
    OAuthAccessTokenRow, app_bearer_token_from_request, find_oauth_access_token_by_bearer_token,
    find_oauth_app_by_bearer_token, oauth_access_token_has_any_scope,
};
use cfwdon_core::{AppConfig, AuthenticatedUser};
use cfwdon_domain::LocalAccount;
use worker::{D1Database, Error, Request, Result};

pub(crate) use super::auth_account_store::{
    ensure_account_keys, find_account_by_id, find_account_by_username, resolve_local_account,
};

#[derive(Clone, Debug)]
pub(crate) struct OAuthAuthenticatedLocalAccount {
    pub(crate) account: LocalAccount,
    pub(crate) token: OAuthAccessTokenRow,
}

#[derive(Clone, Debug)]
pub(crate) enum LocalApiAuthentication {
    Access(LocalAccount),
    OAuthToken(OAuthAuthenticatedLocalAccount),
    AppToken,
    InvalidBearer,
    None,
}

pub(crate) async fn extract_authenticated_user(
    req: &Request,
    config: &AppConfig,
) -> Result<Option<AuthenticatedUser>> {
    let token = match req.headers().get(&config.access_jwt_header)? {
        Some(value) if !value.trim().is_empty() => value.trim().to_owned(),
        _ => return Ok(None),
    };

    if config.access_team_domain.is_empty() || config.access_audience.is_empty() {
        return Err(Error::RustError(
            "missing Cloudflare Access configuration: ACCESS_TEAM_DOMAIN and ACCESS_AUD are required"
                .to_owned(),
        ));
    }

    let claims = verify_access_jwt(&token, config).await?;
    let header_email = req
        .headers()
        .get(&config.access_email_header)?
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    let email = claims
        .email
        .map(|value| value.trim().to_ascii_lowercase())
        .or(header_email.clone())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::RustError("validated Access JWT did not include an email".to_owned())
        })?;

    if let Some(header_email) = header_email
        && header_email != email
    {
        return Err(Error::RustError(
            "Cloudflare Access email header did not match JWT email claim".to_owned(),
        ));
    }

    Ok(Some(AuthenticatedUser::cloudflare_access(email, true)))
}

pub(crate) async fn find_authenticated_local_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<LocalAccount>> {
    if let Some(token) = app_bearer_token_from_request(req)? {
        let Some(access_token) = find_oauth_access_token_by_bearer_token(db, &token).await? else {
            return Ok(None);
        };
        if !oauth_access_token_allows_request(req, &access_token) {
            return Ok(None);
        }
        return find_account_by_id(db, &access_token.account_id).await;
    }

    let Some(user) = extract_authenticated_user(req, config).await? else {
        return Ok(None);
    };

    find_account_by_email(db, &user.email).await
}

fn oauth_access_token_allows_request(req: &Request, token: &OAuthAccessTokenRow) -> bool {
    match req.method().as_ref() {
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
        if let Some(access_token) = find_oauth_access_token_by_bearer_token(db, &token).await? {
            let Some(account) = find_account_by_id(db, &access_token.account_id).await? else {
                return Ok(LocalApiAuthentication::InvalidBearer);
            };
            return Ok(LocalApiAuthentication::OAuthToken(
                OAuthAuthenticatedLocalAccount {
                    account,
                    token: access_token,
                },
            ));
        }
        if find_oauth_app_by_bearer_token(db, &token).await?.is_some() {
            return Ok(LocalApiAuthentication::AppToken);
        }
        return Ok(LocalApiAuthentication::InvalidBearer);
    }

    Ok(find_authenticated_local_account(req, db, config)
        .await?
        .map(LocalApiAuthentication::Access)
        .unwrap_or(LocalApiAuthentication::None))
}
