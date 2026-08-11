use crate::{Response, Result, find_authenticated_local_account, load_config, media_object_url};
use serde::Serialize;
use worker::{Request, RouteContext};

#[derive(Debug, Serialize)]
pub(crate) struct WebSessionResponse {
    id: String,
    username: String,
    display_name: String,
    acct: String,
    avatar: String,
    instance_name: String,
}

pub(crate) fn is_web_api_path(path: &str) -> bool {
    path.starts_with("/api/cfwdon/web/")
}

pub(crate) async fn web_session_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Unauthorized", 401),
    };

    let avatar = account
        .avatar_object_key()
        .map(|object_key| media_object_url(&config, object_key))
        .unwrap_or_default();
    let display_name = account.display_name().trim();
    let display_name = if display_name.is_empty() {
        account.username().to_owned()
    } else {
        display_name.to_owned()
    };

    Response::from_json(&WebSessionResponse {
        id: account.id().to_owned(),
        username: account.username().to_owned(),
        display_name,
        acct: format!("{}@{}", account.username(), config.instance_domain),
        avatar,
        instance_name: config.instance_name.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::is_web_api_path;

    #[test]
    fn web_api_paths_are_detected() {
        assert!(is_web_api_path("/api/cfwdon/web/session"));
        assert!(!is_web_api_path("/api/cfwdon/admin/me"));
    }
}
