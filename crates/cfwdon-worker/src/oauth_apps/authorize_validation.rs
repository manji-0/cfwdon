use super::{
    OAuthAuthorizeFailure, OAuthAuthorizeRequest, find_oauth_app_by_client_id,
    oauth_app_redirect_uris, oauth_app_scopes, redirect_uri_matches_registered,
    requested_oauth_token_scopes,
};
use crate::D1Database;

pub(super) fn code_challenge_method_is_supported(method: Option<&str>) -> bool {
    matches!(method, Some("S256"))
}

pub(super) async fn validate_authorize_request(
    db: &D1Database,
    request: OAuthAuthorizeRequest,
) -> std::result::Result<
    (OAuthAuthorizeRequest, super::OAuthAppRow, Vec<String>),
    OAuthAuthorizeFailure,
> {
    let client_id = request
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| OAuthAuthorizeFailure::Html {
            message: "client_id is required".to_owned(),
        })?;
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| OAuthAuthorizeFailure::Html {
            message: "redirect_uri is required".to_owned(),
        })?;
    let state = request
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let Some(app) = find_oauth_app_by_client_id(db, &client_id)
        .await
        .map_err(|error| OAuthAuthorizeFailure::Html {
            message: format!("failed to load OAuth app: {error}"),
        })?
    else {
        return Err(OAuthAuthorizeFailure::Html {
            message: "Unknown OAuth client".to_owned(),
        });
    };
    if !oauth_app_redirect_uris(&app)
        .iter()
        .any(|value| redirect_uri_matches_registered(value, &redirect_uri))
    {
        return Err(OAuthAuthorizeFailure::Html {
            message: "Redirect URI is not registered for this OAuth client".to_owned(),
        });
    }

    let response_type = request
        .response_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("code");
    if response_type != "code" {
        return Err(OAuthAuthorizeFailure::Redirect {
            redirect_uri: redirect_uri.clone(),
            state: state.clone(),
            error: "unsupported_response_type",
            description: "Only response_type=code is supported".to_owned(),
        });
    }

    let code_challenge_method = request
        .code_challenge_method
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let code_challenge = request
        .code_challenge
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if code_challenge.is_some()
        && !code_challenge_method_is_supported(code_challenge_method.as_deref())
    {
        return Err(OAuthAuthorizeFailure::Redirect {
            redirect_uri: redirect_uri.clone(),
            state: state.clone(),
            error: "invalid_request",
            description: "unsupported code_challenge_method".to_owned(),
        });
    }

    let requested_scopes = requested_oauth_token_scopes(request.scope.clone());
    let registered_scopes = oauth_app_scopes(&app);
    if requested_scopes
        .iter()
        .any(|scope| !registered_scopes.contains(scope))
    {
        return Err(OAuthAuthorizeFailure::Redirect {
            redirect_uri: redirect_uri.clone(),
            state: state.clone(),
            error: "invalid_scope",
            description: "Requested scope is outside the registered app scopes".to_owned(),
        });
    }

    Ok((
        OAuthAuthorizeRequest {
            response_type: Some(response_type.to_owned()),
            client_id: Some(client_id),
            redirect_uri: Some(redirect_uri),
            scope: request.scope.map(|value| value.trim().to_owned()),
            state,
            code_challenge,
            code_challenge_method,
        },
        app,
        requested_scopes,
    ))
}
