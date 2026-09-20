use super::{
    Env, Response, Result, RouteContext, build_nodeinfo_21_document,
    build_nodeinfo_document_with_halfyear, build_nodeinfo_links_document, cache_public_response,
    load_active_halfyear_users, load_active_month_users, load_config, load_config_from_env,
    load_instance_summary, load_total_local_accounts, load_total_local_statuses,
};

pub(crate) async fn nodeinfo_links_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    nodeinfo_links_response_for_config(&config)
}

pub(crate) fn nodeinfo_links_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    nodeinfo_links_response_for_config(&config)
}

fn nodeinfo_links_response_for_config(config: &super::AppConfig) -> Result<Response> {
    cache_public_response(
        Response::from_json(&build_nodeinfo_links_document(config))?,
        300,
    )
}

pub(crate) async fn nodeinfo_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    nodeinfo_response_for_config(&db, config).await
}

pub(crate) async fn nodeinfo_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = crate::D1Database::new(env.d1(&config.database_binding)?);
    nodeinfo_response_for_config(&db, config).await
}

async fn nodeinfo_response_for_config(
    db: &crate::D1Database,
    config: super::AppConfig,
) -> Result<Response> {
    let summary = load_instance_summary(db, config.clone()).await?;
    let active_month = load_active_month_users(db).await?;
    let active_halfyear = load_active_halfyear_users(db).await?;
    let user_count = load_total_local_accounts(db).await?;
    let status_count = load_total_local_statuses(db).await?;

    cache_public_response(
        Response::from_json(&build_nodeinfo_document_with_halfyear(
            &summary,
            &config,
            user_count,
            active_month,
            active_halfyear,
            status_count,
        ))?,
        300,
    )
}

pub(crate) async fn nodeinfo_21_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    nodeinfo_21_response_for_config(&db, config).await
}

pub(crate) async fn nodeinfo_21_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = crate::D1Database::new(env.d1(&config.database_binding)?);
    nodeinfo_21_response_for_config(&db, config).await
}

async fn nodeinfo_21_response_for_config(
    db: &crate::D1Database,
    config: super::AppConfig,
) -> Result<Response> {
    let summary = load_instance_summary(db, config.clone()).await?;
    let active_month = load_active_month_users(db).await?;
    let active_halfyear = load_active_halfyear_users(db).await?;
    let user_count = load_total_local_accounts(db).await?;
    let status_count = load_total_local_statuses(db).await?;

    cache_public_response(
        Response::from_json(&build_nodeinfo_21_document(
            &summary,
            &config,
            user_count,
            active_month,
            active_halfyear,
            status_count,
        ))?,
        300,
    )
}
