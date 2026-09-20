use crate::statuses::{
    Request, Result, RouteContext, find_authenticated_local_account,
    find_local_status_by_object_uri, find_remote_status_by_id,
    find_remote_status_by_url_or_object_uri, find_status_by_id, load_config,
    status_id_from_context,
};

pub(crate) enum ResolvedStatus {
    Local(crate::StatusRow),
    Remote(crate::RemoteStatusRow),
}

pub(super) struct StatusDetailBaseContext {
    pub(super) config: cfwdon_core::AppConfig,
    pub(super) session: crate::D1RequestSession,
    pub(super) db: crate::D1Database,
    pub(super) status_id: String,
}

pub(super) struct StatusDetailRequestContext {
    pub(super) base: StatusDetailBaseContext,
    pub(super) viewer: Option<crate::LocalAccount>,
}

pub(crate) async fn resolve_status_reference(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    id: &str,
) -> Result<Option<ResolvedStatus>> {
    let raw_id = id.trim();
    if raw_id.is_empty() {
        return Ok(None);
    }

    if let Some(status) = find_status_by_id(db, raw_id).await? {
        return Ok(Some(ResolvedStatus::Local(status)));
    }
    if let Some(status) = find_remote_status_by_id(db, raw_id).await? {
        return Ok(Some(ResolvedStatus::Remote(status)));
    }

    let decoded_id = urlencoding::decode(raw_id)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| raw_id.to_owned());
    if let Some(status) = find_local_status_by_object_uri(db, config, &decoded_id).await? {
        return Ok(Some(ResolvedStatus::Local(status)));
    }
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, &decoded_id).await? {
        return Ok(Some(ResolvedStatus::Remote(status)));
    }

    Ok(None)
}

pub(super) fn resolve_status_detail_base_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<StatusDetailBaseContext>> {
    let status_id = match status_id_from_context(ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Ok(None),
    };
    let config = load_config(ctx);
    let (session, db) = crate::open_bound_request_session(ctx, &config, req)?;
    Ok(Some(StatusDetailBaseContext {
        config,
        session,
        db,
        status_id,
    }))
}

pub(super) async fn resolve_status_detail_request_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<Option<StatusDetailRequestContext>> {
    let Some(base) = resolve_status_detail_base_context(req, ctx)? else {
        return Ok(None);
    };
    let viewer = find_authenticated_local_account(req, &base.db, &base.config).await?;
    Ok(Some(StatusDetailRequestContext { base, viewer }))
}
