use crate::oauth_store::{
    AUTH0_REFRESH_COOKIE, AUTH0_SESSION_COOKIE, CredentialAccount, OAuthAccessTokenRow,
    access_token_cookie_max_age, app_bearer_token_from_request, exchange_auth0_refresh_token,
    find_legacy_oauth_access_token_with_account_by_plaintext,
    find_oauth_access_token_with_account_by_bearer_token,
    find_oauth_access_token_with_account_by_token_hash, find_oauth_app_by_bearer_token,
    migrate_legacy_oauth_access_token_hash, oauth_access_token_has_any_scope,
    oauth_bearer_token_hash, parse_bearer_authorization_header, set_auth0_session_cookies,
};
use crate::observability::log_federation_event;
use crate::tracked_d1::D1Database;

mod account_store;
mod approvals;
mod jwt;
mod request_state;
mod scopes;
pub(crate) use account_store::*;
pub(crate) use approvals::*;
pub(crate) use jwt::*;
pub(crate) use request_state::{
    CredentialRefusal, credential_refusal, record_credential_refusal, scope_request_auth_state,
};
use request_state::{
    RefreshedAuth0Session, record_refreshed_auth0_session, refreshed_auth0_session,
};
use scopes::required_oauth_scopes;

pub(crate) use self::jwt::{auth0_roles_from_claims, verify_auth0_jwt};
use cfwdon_core::{AppConfig, AuthenticatedUser};
use cfwdon_domain::{AccountApproval, LocalAccount};
use worker::{Error, Request, Response, Result};

#[derive(Clone, Debug)]
pub(crate) struct OAuthAuthenticatedLocalAccount {
    pub(crate) account: LocalAccount,
    pub(crate) token: OAuthAccessTokenRow,
}

#[derive(Clone, Debug)]
pub(crate) enum LocalApiAuthentication {
    Auth0(LocalAccount),
    OAuthToken(OAuthAuthenticatedLocalAccount),
    /// A valid credential whose account waits for approval. Mastodon's
    /// `require_user!` refuses it with `403`; only the endpoints that skip it
    /// (e-mail confirmation) act on it. Resolving it records
    /// [`CredentialRefusal::PendingApproval`], so a handler's `401` becomes
    /// that `403`.
    PendingApproval(PendingCredential),
    AppToken,
    InvalidBearer,
    None,
}

#[derive(Clone, Debug)]
pub(crate) struct PendingCredential {
    pub(crate) account: LocalAccount,
    /// The OAuth token presented; `None` for an Auth0 identity.
    pub(crate) token: Option<OAuthAccessTokenRow>,
}

pub(crate) fn pending_approval_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "Your login is currently pending approval",
    }))?
    .with_status(403))
}

/// The account a credential names when its owner may use it. A pending one
/// yields `None` and records why, so the router answers Mastodon's `403`.
pub(crate) fn approved_credential_account(
    credential: Option<CredentialAccount>,
) -> Option<LocalAccount> {
    let credential = credential?;
    match credential.approval {
        AccountApproval::Approved => Some(credential.account),
        AccountApproval::Pending => {
            record_credential_refusal(CredentialRefusal::PendingApproval);
            None
        }
    }
}

pub(crate) async fn extract_authenticated_user(
    req: &Request,
    config: &AppConfig,
) -> Result<Option<AuthenticatedUser>> {
    if let Some(session) = refreshed_auth0_session() {
        return Ok(Some(session.user));
    }

    if config.auth0_domain.is_empty() || config.auth0_audience.is_empty() {
        if auth0_token_from_request(req, config)?.is_some() {
            return Err(Error::RustError(
                "missing Auth0 configuration: AUTH0_DOMAIN and AUTH0_AUDIENCE are required"
                    .to_owned(),
            ));
        }
        return Ok(None);
    }

    if auth0_authorization_header_present(req, config)? {
        return match user_from_access_token(
            &auth0_token_from_request(req, config)?.unwrap_or_default(),
            config,
        )
        .await
        {
            Ok(user) => Ok(user),
            Err(error) if auth0_jwt_is_client_error(&error) => {
                log_auth0_jwt_client_error(&error);
                Ok(None)
            }
            Err(error) => Err(error),
        };
    }

    if let Some(token) = request_cookie_value(req, AUTH0_SESSION_COOKIE)? {
        match user_from_access_token(&token, config).await {
            Ok(user) => return Ok(user),
            Err(error) if auth0_jwt_is_client_error(&error) => {
                log_auth0_jwt_client_error(&error);
            }
            Err(error) => return Err(error),
        }
    }

    refresh_auth0_cookie_session(req, config).await
}

fn log_auth0_jwt_client_error(error: &Error) {
    if auth0_jwt_is_expired_error(error) {
        return;
    }
    log_federation_event(
        "auth0_jwt_rejected",
        "skipped",
        format!("Auth0 JWT rejected as unauthenticated: {error}"),
        serde_json::json!({ "error": error.to_string() }),
    );
}

async fn user_from_access_token(
    token: &str,
    config: &AppConfig,
) -> Result<Option<AuthenticatedUser>> {
    if token.trim().is_empty() {
        return Ok(None);
    }
    let claims = verify_auth0_jwt(token, config).await?;
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

async fn refresh_auth0_cookie_session(
    req: &Request,
    config: &AppConfig,
) -> Result<Option<AuthenticatedUser>> {
    let Some(refresh_token) = request_cookie_value(req, AUTH0_REFRESH_COOKIE)? else {
        return Ok(None);
    };
    let token = match exchange_auth0_refresh_token(config, &refresh_token).await {
        Ok(token) => token,
        Err(_) => return Ok(None),
    };
    let user = match user_from_access_token(&token.access_token, config).await {
        Ok(user) => user,
        Err(error) if auth0_jwt_is_client_error(&error) => {
            log_auth0_jwt_client_error(&error);
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let Some(user) = user else {
        return Ok(None);
    };
    let next_refresh = token
        .refresh_token
        .filter(|value| !value.is_empty())
        .or(Some(refresh_token));
    record_refreshed_auth0_session(RefreshedAuth0Session {
        user: user.clone(),
        access_token: token.access_token,
        refresh_token: next_refresh,
        access_max_age: access_token_cookie_max_age(token.expires_in),
    });
    Ok(Some(user))
}

pub(crate) fn oauth_scope_denied_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

pub(crate) fn apply_auth0_web_session_cookies(response: &mut Response) -> Result<()> {
    let Some(session) = refreshed_auth0_session() else {
        return Ok(());
    };
    set_auth0_session_cookies(
        response,
        &session.access_token,
        session.refresh_token.as_deref(),
        session.access_max_age,
    )
}

fn auth0_authorization_header_present(req: &Request, config: &AppConfig) -> Result<bool> {
    Ok(req
        .headers()
        .get(&config.auth0_jwt_header)?
        .is_some_and(|value| !value.trim().is_empty()))
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

pub(crate) fn request_cookie_value(req: &Request, name: &str) -> Result<Option<String>> {
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
        if !oauth_access_token_allows_request(req, &auth.token) {
            record_credential_refusal(CredentialRefusal::ScopeDenied);
            return Ok(None);
        }
        return Ok(approved_credential_account(auth.account).map(|account| (account, Vec::new())));
    }

    let Some((credential, roles)) = find_auth0_credential_account(req, db, config).await? else {
        return Ok(None);
    };
    Ok(approved_credential_account(Some(credential)).map(|account| (account, roles)))
}

/// The account the request's Auth0 identity signs in as, by its e-mail, with
/// the identity's roles. An identity never creates an account: sign-ups go
/// through `POST /api/v1/accounts`, which applies `REGISTRATIONS_MODE`.
pub(crate) async fn find_auth0_credential_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<(CredentialAccount, Vec<String>)>> {
    let Some(user) = extract_authenticated_user(req, config).await? else {
        return Ok(None);
    };
    Ok(find_credential_account_by_email(db, &user.email)
        .await?
        .map(|credential| (credential, user.roles)))
}

fn api_authentication_from_credential(
    credential: CredentialAccount,
    token: Option<OAuthAccessTokenRow>,
) -> LocalApiAuthentication {
    match (credential.approval, token) {
        (AccountApproval::Approved, Some(token)) => {
            LocalApiAuthentication::OAuthToken(OAuthAuthenticatedLocalAccount {
                account: credential.account,
                token,
            })
        }
        (AccountApproval::Approved, None) => LocalApiAuthentication::Auth0(credential.account),
        (AccountApproval::Pending, token) => {
            record_credential_refusal(CredentialRefusal::PendingApproval);
            LocalApiAuthentication::PendingApproval(PendingCredential {
                account: credential.account,
                token,
            })
        }
    }
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
    oauth_access_token_has_any_scope(token, required_oauth_scopes(method, path))
}

pub(crate) async fn authenticate_local_api_request(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<LocalApiAuthentication> {
    if let Some(token) = app_bearer_token_from_request(req)? {
        let token_hash = oauth_bearer_token_hash(&token);
        let mut auth = find_oauth_access_token_with_account_by_token_hash(db, &token_hash).await?;
        let mut app_token = false;
        if auth.is_none() {
            // Miss on the hashed user token: the legacy user-token and app-token
            // lookups are independent, so run them together.
            let (legacy, app) = futures_util::try_join!(
                find_legacy_oauth_access_token_with_account_by_plaintext(db, &token),
                find_oauth_app_by_bearer_token(db, &token),
            )?;
            if legacy.is_some() {
                migrate_legacy_oauth_access_token_hash(db, &token, &token_hash).await?;
            }
            auth = legacy;
            app_token = app.is_some();
        }
        if let Some(auth) = auth {
            let Some(credential) = auth.account else {
                return Ok(LocalApiAuthentication::InvalidBearer);
            };
            return Ok(api_authentication_from_credential(
                credential,
                Some(auth.token),
            ));
        }
        if app_token {
            return Ok(LocalApiAuthentication::AppToken);
        }
        return match find_auth0_credential_account(req, db, config).await {
            Ok(Some((credential, _roles))) => {
                Ok(api_authentication_from_credential(credential, None))
            }
            Ok(None) | Err(_) => Ok(LocalApiAuthentication::InvalidBearer),
        };
    }

    Ok(find_auth0_credential_account(req, db, config)
        .await?
        .map_or(LocalApiAuthentication::None, |(credential, _roles)| {
            api_authentication_from_credential(credential, None)
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::LocalAccountRecord;
    use futures_util::FutureExt;

    fn credential(approval: AccountApproval) -> CredentialAccount {
        CredentialAccount {
            account: LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice")),
            approval,
        }
    }

    fn token() -> OAuthAccessTokenRow {
        OAuthAccessTokenRow {
            access_token: "sha256:abc".to_owned(),
            oauth_app_id: 1,
            scopes_json: "[\"read\"]".to_owned(),
        }
    }

    #[test]
    fn a_pending_credential_acts_as_no_account_and_records_why() {
        let (approved, refusal) = scope_request_auth_state(async {
            (
                approved_credential_account(Some(credential(AccountApproval::Approved))),
                credential_refusal(),
            )
        })
        .now_or_never()
        .unwrap();
        assert_eq!(
            approved.map(|account| account.id().to_owned()).as_deref(),
            Some("acct-1")
        );
        assert_eq!(refusal, None);

        let (pending, refusal) = scope_request_auth_state(async {
            (
                approved_credential_account(Some(credential(AccountApproval::Pending))),
                credential_refusal(),
            )
        })
        .now_or_never()
        .unwrap();
        assert!(pending.is_none());
        assert_eq!(refusal, Some(CredentialRefusal::PendingApproval));
    }

    #[test]
    fn api_authentication_keeps_a_pending_account_apart() {
        let outcome = |approval, token: Option<OAuthAccessTokenRow>| {
            scope_request_auth_state(async move {
                (
                    api_authentication_from_credential(credential(approval), token),
                    credential_refusal(),
                )
            })
            .now_or_never()
            .unwrap()
        };
        assert!(matches!(
            outcome(AccountApproval::Approved, Some(token())),
            (LocalApiAuthentication::OAuthToken(_), None)
        ));
        assert!(matches!(
            outcome(AccountApproval::Approved, None),
            (LocalApiAuthentication::Auth0(_), None)
        ));
        for presented in [Some(token()), None] {
            let has_token = presented.is_some();
            let (authentication, refusal) = outcome(AccountApproval::Pending, presented);
            let LocalApiAuthentication::PendingApproval(pending) = authentication else {
                panic!("a pending account must not authenticate as itself");
            };
            assert_eq!(pending.token.is_some(), has_token);
            assert_eq!(refusal, Some(CredentialRefusal::PendingApproval));
        }
    }
}
