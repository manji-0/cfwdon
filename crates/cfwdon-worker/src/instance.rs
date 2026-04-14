use super::{
    Response, Result, RouteContext, build_instance_v1_document, build_instance_v2_document,
    build_nodeinfo_document, build_nodeinfo_links_document, configured_html_document,
    load_active_month_users, load_config, load_instance_summary, load_known_peer_domains,
    load_total_local_accounts, load_total_local_statuses,
};

pub(crate) async fn instance_summary_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let summary = load_instance_summary(&db, config.clone()).await?;
    let active_month = load_active_month_users(&db).await?;
    let user_count = load_total_local_accounts(&db).await?;
    let status_count = load_total_local_statuses(&db).await?;
    let domain_count = load_known_peer_domains(&db, &config).await?.len() as u64;

    Response::from_json(&build_instance_v1_document(
        &summary,
        &config,
        active_month,
        user_count,
        status_count,
        domain_count,
    ))
}

pub(crate) async fn instance_v2_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let summary = load_instance_summary(&db, config.clone()).await?;
    let active_month = load_active_month_users(&db).await?;

    Response::from_json(&build_instance_v2_document(&summary, &config, active_month))
}

pub(crate) async fn instance_peers_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;

    Response::from_json(&load_known_peer_domains(&db, &config).await?)
}

pub(crate) async fn instance_extended_description_response(
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let Some(content) = configured_html_document(
        config.instance_extended_description_html.as_deref(),
        config.instance_extended_description_updated_at.as_deref(),
        "1970-01-01T00:00:00Z",
        false,
    ) else {
        return Response::error("Record not found", 404);
    };

    Response::from_json(&content)
}

pub(crate) async fn instance_privacy_policy_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let Some(content) = configured_html_document(
        config.privacy_policy_html.as_deref(),
        config.privacy_policy_updated_at.as_deref(),
        "1970-01-01T00:00:00Z",
        false,
    ) else {
        return Response::error("Record not found", 404);
    };

    Response::from_json(&content)
}

pub(crate) async fn instance_terms_of_service_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let Some(content) = configured_html_document(
        config.terms_of_service_html.as_deref(),
        config.terms_of_service_effective_date.as_deref(),
        "1970-01-01",
        true,
    ) else {
        return Response::error("Record not found", 404);
    };

    Response::from_json(&content)
}

pub(crate) async fn nodeinfo_links_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    Response::from_json(&build_nodeinfo_links_document(&config))
}

pub(crate) async fn nodeinfo_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let summary = load_instance_summary(&db, config.clone()).await?;
    let active_month = load_active_month_users(&db).await?;
    let user_count = load_total_local_accounts(&db).await?;
    let status_count = load_total_local_statuses(&db).await?;

    Response::from_json(&build_nodeinfo_document(
        &summary,
        &config,
        user_count,
        active_month,
        status_count,
    ))
}
