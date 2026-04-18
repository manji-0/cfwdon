use super::{
    Response, Result, RouteContext, build_default_privacy_policy_document,
    build_instance_activity_document, build_instance_v1_document, build_instance_v2_document,
    build_nodeinfo_document, build_nodeinfo_links_document, configured_html_document,
    count_accounts_created_between, count_local_statuses_between, load_active_month_users,
    load_config, load_instance_summary, load_known_peer_domains, load_total_local_accounts,
    load_total_local_statuses,
};
use time::{Duration, OffsetDateTime, Time, format_description::well_known::Rfc3339};

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

pub(crate) async fn instance_activity_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let now = OffsetDateTime::now_utc();
    let midnight = now.replace_time(Time::MIDNIGHT);
    let week_floor = midnight - Duration::days(midnight.weekday().number_days_from_monday().into());

    let mut weekly_totals = Vec::with_capacity(12);
    for offset in (0..12).rev() {
        let week_start = week_floor - Duration::weeks(offset);
        let week_end = week_start + Duration::weeks(1);
        let start = week_start.format(&Rfc3339).map_err(|error| {
            worker::Error::RustError(format!("failed to format week start: {error}"))
        })?;
        let end = week_end.format(&Rfc3339).map_err(|error| {
            worker::Error::RustError(format!("failed to format week end: {error}"))
        })?;
        weekly_totals.push((
            count_local_statuses_between(&db, &start, &end).await?,
            0,
            count_accounts_created_between(&db, &start, &end).await?,
        ));
    }

    Response::from_json(&build_instance_activity_document(
        week_floor,
        &weekly_totals,
    ))
}

pub(crate) async fn instance_rules_response(_ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&serde_json::json!([]))
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
    let content = configured_html_document(
        config.privacy_policy_html.as_deref(),
        config.privacy_policy_updated_at.as_deref(),
        "1970-01-01T00:00:00Z",
        false,
    )
    .unwrap_or_else(|| build_default_privacy_policy_document(&config.instance_description));

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

pub(crate) async fn instance_translation_languages_response(
    _ctx: RouteContext<()>,
) -> Result<Response> {
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn announcements_response(_ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&serde_json::json!([]))
}

pub(crate) async fn trending_statuses_response(_ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&serde_json::json!([]))
}

pub(crate) async fn trending_links_response(_ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&serde_json::json!([]))
}

pub(crate) async fn trending_tags_response(_ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&serde_json::json!([]))
}

pub(crate) async fn custom_emojis_response(_ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&serde_json::json!([]))
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
