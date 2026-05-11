use worker::{Response, Result, RouteContext};

pub(crate) async fn cached_account_api_response(
    _ctx: &RouteContext<()>,
    _account_id: &str,
) -> Result<Option<Response>> {
    Ok(None)
}

pub(crate) async fn cache_account_api_response(
    _ctx: &RouteContext<()>,
    _account_id: &str,
    _value: &crate::MastodonAccountResponse,
) -> Result<()> {
    Ok(())
}

pub(crate) async fn cached_actor_json_response(
    _ctx: &RouteContext<()>,
    _username: &str,
) -> Result<Option<Response>> {
    Ok(None)
}

pub(crate) async fn cache_actor_json_response(
    _ctx: &RouteContext<()>,
    _username: &str,
    _value: &impl serde::Serialize,
) -> Result<()> {
    Ok(())
}

pub(crate) async fn cached_actor_profile_html_response(
    _ctx: &RouteContext<()>,
    _username: &str,
) -> Result<Option<Response>> {
    Ok(None)
}

pub(crate) async fn cache_actor_profile_html_response(
    _ctx: &RouteContext<()>,
    _username: &str,
    _html: String,
) -> Result<()> {
    Ok(())
}

pub(crate) async fn cached_status_api_response(
    _ctx: &RouteContext<()>,
    _status_id: &str,
) -> Result<Option<Response>> {
    Ok(None)
}

pub(crate) async fn cache_status_api_response(
    _ctx: &RouteContext<()>,
    _status_id: &str,
    _value: &crate::MastodonStatusResponse,
) -> Result<()> {
    Ok(())
}

pub(crate) async fn invalidate_status_api_cache(_ctx: &RouteContext<()>, _status_id: &str) {}

pub(crate) async fn invalidate_account_dynamic_public_cache(
    _ctx: &RouteContext<()>,
    _account_id: &str,
    _username: &str,
) {
}

pub(crate) async fn invalidate_account_public_cache(
    _ctx: &RouteContext<()>,
    _account_id: &str,
    _username: &str,
) {
}
