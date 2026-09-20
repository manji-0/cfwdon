use crate::{
    LocalApiAuthentication, Request, Result, authenticate_local_api_request,
    find_oauth_access_token_with_account_by_bearer_token, oauth_access_token_has_any_scope,
};

pub(super) enum StreamingAuthOutcome {
    Viewer(Option<cfwdon_domain::LocalAccount>),
    InvalidToken,
}

pub(super) async fn resolve_streaming_auth(
    req: &Request,
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    query_access_token: Option<&str>,
    websocket_protocol_token: Option<&str>,
) -> Result<StreamingAuthOutcome> {
    match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::Auth0(viewer) => Ok(StreamingAuthOutcome::Viewer(Some(viewer))),
        LocalApiAuthentication::OAuthToken(auth) => {
            Ok(StreamingAuthOutcome::Viewer(Some(auth.account)))
        }
        LocalApiAuthentication::AppToken | LocalApiAuthentication::InvalidBearer => {
            Ok(StreamingAuthOutcome::InvalidToken)
        }
        LocalApiAuthentication::None => {
            let token = query_access_token
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| websocket_protocol_token.map(ToOwned::to_owned));
            match token {
                Some(token) => {
                    let Some(auth) =
                        find_oauth_access_token_with_account_by_bearer_token(db, &token).await?
                    else {
                        return Ok(StreamingAuthOutcome::InvalidToken);
                    };
                    if !oauth_access_token_has_any_scope(
                        &auth.token,
                        &["read", "read:statuses", "read:notifications"],
                    ) {
                        return Ok(StreamingAuthOutcome::InvalidToken);
                    }
                    Ok(StreamingAuthOutcome::Viewer(auth.account))
                }
                None => Ok(StreamingAuthOutcome::Viewer(None)),
            }
        }
    }
}
