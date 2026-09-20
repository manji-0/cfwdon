#[allow(unused_imports)]
pub(crate) use crate::*;

mod announcements;
mod documents;
mod identity;
mod nodeinfo_documents;
mod nodeinfo_routes;
mod policy_documents;
mod store;
mod trending_links;
pub(crate) use announcements::*;
pub(crate) use documents::*;
pub(crate) use identity::*;
pub(crate) use nodeinfo_documents::*;
pub(crate) use nodeinfo_routes::*;
pub(crate) use policy_documents::*;
pub(crate) use store::*;
pub(crate) use trending_links::*;

use crate::statuses::{
    configured_translation_provider, configured_translation_provider_from_env,
    load_translation_provider_languages,
};
use time::{Duration, OffsetDateTime, Time, format_description::well_known::Rfc3339};
use worker::Env;

#[derive(Debug, Default, serde::Deserialize)]
struct TrendsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(crate) async fn instance_summary_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    instance_summary_response_for_config(&db, config).await
}

pub(crate) async fn instance_summary_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = crate::D1Database::new(env.d1(&config.database_binding)?);
    instance_summary_response_for_config(&db, config).await
}

async fn instance_summary_response_for_config(
    db: &crate::D1Database,
    config: super::AppConfig,
) -> Result<Response> {
    let summary = load_instance_summary(db, config.clone()).await?;
    let active_month = load_active_month_users(db).await?;
    let user_count = load_total_local_accounts(db).await?;
    let status_count = load_total_local_statuses(db).await?;
    let domain_count = load_known_peer_domains(db, &config).await?.len() as u64;

    cache_public_response(
        Response::from_json(&build_instance_v1_document(
            &summary,
            &config,
            active_month,
            user_count,
            status_count,
            domain_count,
        ))?,
        60,
    )
}

pub(crate) async fn instance_v2_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    instance_v2_response_for_config(&db, config, configured_translation_provider(&ctx).is_some())
        .await
}

pub(crate) async fn instance_v2_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = crate::D1Database::new(env.d1(&config.database_binding)?);
    instance_v2_response_for_config(
        &db,
        config,
        configured_translation_provider_from_env(env).is_some(),
    )
    .await
}

async fn instance_v2_response_for_config(
    db: &crate::D1Database,
    config: super::AppConfig,
    translation_enabled: bool,
) -> Result<Response> {
    let (summary, active_month) = futures_util::try_join!(
        load_instance_summary(db, config.clone()),
        load_active_month_users(db),
    )?;
    let mut document = build_instance_v2_document(&summary, &config, active_month);
    set_instance_translation_enabled(&mut document, translation_enabled);

    cache_public_response(Response::from_json(&document)?, 60)
}

pub(crate) async fn instance_peers_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;

    cache_public_response(
        Response::from_json(&load_known_peer_domains(&db, &config).await?)?,
        300,
    )
}

#[derive(Debug, Default, serde::Deserialize)]
struct PeerSearchQuery {
    q: Option<String>,
}

pub(crate) async fn instance_peers_search_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: PeerSearchQuery = req.query().unwrap_or_default();
    let db = crate::bind_request_d1(&ctx, &config)?;
    let mut domains = load_known_peer_domains(&db, &config).await?;

    if let Some(q) = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let q = q.to_ascii_lowercase();
        domains.retain(|domain| domain.to_ascii_lowercase().contains(&q));
    }

    cache_public_response(Response::from_json(&domains)?, CACHE_TTL_INSTANCE_SUMMARY)
}

pub(crate) async fn instance_activity_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let Some(response) = crate::d1_pressure_load_shed_response()? {
        return Ok(response);
    }

    let config = load_config(&ctx);
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;

    let document = if let Some(cached) =
        crate::load_public_endpoint_cache(&db, crate::PUBLIC_CACHE_INSTANCE_ACTIVITY).await?
    {
        cached
    } else {
        let live = compute_instance_activity_document(&db).await?;
        let _ =
            crate::store_public_endpoint_cache(&db, crate::PUBLIC_CACHE_INSTANCE_ACTIVITY, &live)
                .await;
        live
    };

    with_d1_bookmark(
        cache_public_response(Response::from_json(&document)?, 300)?,
        &session,
    )
}

pub(crate) async fn compute_instance_activity_document(
    db: &crate::D1Database,
) -> Result<serde_json::Value> {
    // Prefer js_sys::Date via now_unix_timestamp — std SystemTime panics on wasm32.
    let now = OffsetDateTime::from_unix_timestamp(now_unix_timestamp()).map_err(|error| {
        worker::Error::RustError(format!("invalid current unix timestamp: {error}"))
    })?;
    let midnight = now.replace_time(Time::MIDNIGHT);
    let week_floor = midnight - Duration::days(midnight.weekday().number_days_from_monday().into());
    let week_floor_rfc3339 = week_floor.format(&Rfc3339).map_err(|error| {
        worker::Error::RustError(format!("failed to format week floor: {error}"))
    })?;
    let range_start = (week_floor - Duration::weeks(11))
        .format(&Rfc3339)
        .map_err(|error| {
            worker::Error::RustError(format!("failed to format activity range start: {error}"))
        })?;
    let range_end = (week_floor + Duration::weeks(1))
        .format(&Rfc3339)
        .map_err(|error| {
            worker::Error::RustError(format!("failed to format activity range end: {error}"))
        })?;

    let (status_counts, account_counts) = futures_util::try_join!(
        count_local_statuses_by_week_offset(db, &week_floor_rfc3339, &range_start, &range_end),
        count_accounts_created_by_week_offset(db, &week_floor_rfc3339, &range_start, &range_end),
    )?;

    let weekly_totals = (0..12)
        .rev()
        .map(|offset| {
            (
                status_counts.get(&offset).copied().unwrap_or(0),
                0,
                account_counts.get(&offset).copied().unwrap_or(0),
            )
        })
        .collect::<Vec<_>>();

    Ok(build_instance_activity_document(week_floor, &weekly_totals))
}

pub(crate) async fn refresh_instance_activity_cache(db: &crate::D1Database) -> Result<()> {
    let document = compute_instance_activity_document(db).await?;
    crate::store_public_endpoint_cache(db, crate::PUBLIC_CACHE_INSTANCE_ACTIVITY, &document).await
}

pub(crate) async fn instance_rules_response(_ctx: RouteContext<()>) -> Result<Response> {
    instance_rules_response_direct()
}

pub(crate) fn instance_rules_response_direct() -> Result<Response> {
    cache_public_response(Response::from_json(&serde_json::json!([]))?, 300)
}

pub(crate) async fn instance_extended_description_response(
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let content = configured_html_document(
        config.instance_extended_description_html.as_deref(),
        config.instance_extended_description_updated_at.as_deref(),
        "1970-01-01T00:00:00Z",
        false,
    )
    .unwrap_or_else(|| build_default_extended_description_document(&config.instance_description));

    cache_public_response(Response::from_json(&content)?, 300)
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

    cache_public_response(Response::from_json(&content)?, 300)
}

pub(crate) async fn instance_terms_of_service_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let content = configured_html_document(
        config.terms_of_service_html.as_deref(),
        config.terms_of_service_effective_date.as_deref(),
        "1970-01-01",
        true,
    )
    .unwrap_or_else(|| build_default_terms_of_service_document(&config.instance_description));

    cache_public_response(Response::from_json(&content)?, 300)
}

pub(crate) async fn instance_terms_of_service_version_response(
    ctx: RouteContext<()>,
) -> Result<Response> {
    instance_terms_of_service_response(ctx).await
}

pub(crate) async fn instance_domain_blocks_response(_ctx: RouteContext<()>) -> Result<Response> {
    instance_domain_blocks_response_direct()
}

pub(crate) fn instance_domain_blocks_response_direct() -> Result<Response> {
    cache_public_response(Response::from_json(&Vec::<serde_json::Value>::new())?, 300)
}

pub(crate) async fn instance_languages_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    instance_languages_response_for_config(&config)
}

pub(crate) fn instance_languages_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    instance_languages_response_for_config(&config)
}

fn instance_languages_response_for_config(config: &super::AppConfig) -> Result<Response> {
    cache_public_response(
        Response::from_json(&configured_instance_languages(config))?,
        300,
    )
}

pub(crate) async fn instance_translation_languages_response(
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let Some(provider_config) = configured_translation_provider(&ctx) {
        return Response::from_json(&load_translation_provider_languages(&provider_config).await?);
    }

    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn trending_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let Some(response) = crate::d1_pressure_load_shed_response()? {
        return Ok(response);
    }

    let config = load_config(&ctx);
    let query: TrendsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(10).clamp(1, 20);
    let offset = query.offset.unwrap_or(0);
    let (session, db) = open_bound_request_session(&ctx, &config, &req)?;

    let statuses = if let Some(cached) = crate::load_trending_statuses_cache().await {
        crate::slice_trending_cache(cached, offset, limit)
    } else {
        let live = trending_status_documents(
            &db,
            &config,
            crate::TRENDING_STATUSES_CACHE_SIZE,
            0,
            crate::TRENDING_STATUSES_CACHE_SIZE,
        )
        .await?;
        crate::slice_trending_cache(live, offset, limit)
    };

    with_d1_bookmark(
        cache_public_response(Response::from_json(&statuses)?, CACHE_TTL_TRENDS)?,
        &session,
    )
}

pub(crate) async fn refresh_trending_statuses_cache(
    db: &crate::D1Database,
    config: &AppConfig,
) -> Result<()> {
    let statuses = trending_status_documents(
        db,
        config,
        crate::TRENDING_STATUSES_CACHE_SIZE,
        0,
        crate::TRENDING_STATUSES_CACHE_SIZE,
    )
    .await?;
    crate::store_trending_statuses_cache(&statuses).await
}

pub(crate) async fn trending_tags_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    if let Some(response) = crate::d1_pressure_load_shed_response()? {
        return Ok(response);
    }

    let config = load_config(&ctx);
    let query: TrendsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(10).clamp(1, 20);
    let offset = query.offset.unwrap_or(0);
    let db = crate::bind_request_d1(&ctx, &config)?;

    let documents = if let Some(cached) = crate::load_trending_tags_cache().await {
        crate::slice_trending_cache(cached, offset, limit)
    } else {
        let live = trending_tags_documents(&db, &config, offset, limit).await?;
        live.into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?
    };

    cache_public_response(Response::from_json(&documents)?, CACHE_TTL_TRENDS)
}

pub(crate) async fn custom_emojis_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let config = config_with_resolved_custom_emojis(&db, &config).await?;
    custom_emojis_response_direct(&config)
}

pub(crate) fn custom_emojis_response_direct(config: &AppConfig) -> Result<Response> {
    cache_public_response(Response::from_json(&list_custom_emojis(config))?, 300)
}

pub(crate) async fn custom_emojis_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = crate::D1Database::new(env.d1(&config.database_binding)?);
    let config = config_with_resolved_custom_emojis(&db, &config).await?;
    custom_emojis_response_direct(&config)
}
