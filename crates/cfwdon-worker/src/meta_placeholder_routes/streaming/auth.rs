use crate::auth::{LocalApiAuthentication, authenticate_local_api_request};
use crate::oauth_store::{
    OAuthAccessTokenRow, find_oauth_access_token_with_account_by_bearer_token,
    oauth_access_token_has_any_scope,
};
use crate::tracked_d1::D1Database;
use worker::{Request, Result};

pub(super) enum StreamingAuthOutcome {
    Viewer(Option<cfwdon_domain::LocalAccount>),
    InvalidToken,
}

/// Mastodon authorizes a streaming token with `read`, or the channel's granular
/// read scope: the notification channel also accepts `read:notifications`, every
/// other channel also accepts `read:statuses`. Blank sockets (no `stream` yet)
/// use the non-notification set, matching upstream `wsVerifyClient`.
fn streaming_token_covers_channel(token: &OAuthAccessTokenRow, stream: Option<&str>) -> bool {
    let granular = if stream == Some("user:notification") {
        "read:notifications"
    } else {
        "read:statuses"
    };
    oauth_access_token_has_any_scope(token, &["read", granular])
}

pub(super) async fn resolve_streaming_auth(
    req: &Request,
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    stream: Option<&str>,
    query_access_token: Option<&str>,
    websocket_protocol_token: Option<&str>,
) -> Result<StreamingAuthOutcome> {
    match authenticate_local_api_request(req, db, config).await? {
        LocalApiAuthentication::Auth0(viewer) => Ok(StreamingAuthOutcome::Viewer(Some(viewer))),
        LocalApiAuthentication::OAuthToken(auth) => {
            if !streaming_token_covers_channel(&auth.token, stream) {
                return Ok(StreamingAuthOutcome::InvalidToken);
            }
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
                    if !streaming_token_covers_channel(&auth.token, stream) {
                        return Ok(StreamingAuthOutcome::InvalidToken);
                    }
                    Ok(StreamingAuthOutcome::Viewer(auth.account))
                }
                None => Ok(StreamingAuthOutcome::Viewer(None)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(scopes: &[&str]) -> OAuthAccessTokenRow {
        OAuthAccessTokenRow {
            access_token: "token".to_owned(),
            oauth_app_id: 1,
            scopes_json: serde_json::to_string(scopes).unwrap(),
        }
    }

    #[test]
    fn streaming_token_scope_accepts_read_or_channel_read_scope() {
        assert!(streaming_token_covers_channel(
            &token(&["read"]),
            Some("user")
        ));
        assert!(streaming_token_covers_channel(
            &token(&["read:statuses"]),
            Some("public")
        ));
        assert!(streaming_token_covers_channel(
            &token(&["read:notifications"]),
            Some("user:notification")
        ));
        // Blank sockets fall back to the non-notification scope set.
        assert!(streaming_token_covers_channel(
            &token(&["read:statuses"]),
            None
        ));
    }

    #[test]
    fn streaming_token_scope_rejects_cross_channel_and_write_only_tokens() {
        assert!(!streaming_token_covers_channel(
            &token(&["read:notifications"]),
            Some("public")
        ));
        assert!(!streaming_token_covers_channel(
            &token(&["read:statuses"]),
            Some("user:notification")
        ));
        assert!(!streaming_token_covers_channel(
            &token(&["write:statuses"]),
            Some("user")
        ));
        assert!(!streaming_token_covers_channel(&token(&[]), Some("user")));
    }
}
