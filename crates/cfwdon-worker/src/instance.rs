use super::{
    Request, Response, Result, RouteContext, build_default_privacy_policy_document,
    build_instance_activity_document, build_instance_v1_document, build_instance_v2_document,
    build_nodeinfo_document, build_nodeinfo_links_document, configured_html_document,
    configured_instance_languages, count_accounts_created_between, count_local_statuses_between,
    extract_authenticated_user, load_active_month_users, load_config, load_instance_summary,
    load_known_peer_domains, load_total_local_accounts, load_total_local_statuses,
    resolve_local_account,
};
use std::collections::{HashMap, HashSet};
use time::{Duration, OffsetDateTime, Time, format_description::well_known::Rfc3339};
use worker::d1::D1Type;

#[derive(Debug, serde::Deserialize)]
struct AnnouncementDismissalRow {
    announcement_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct AnnouncementReactionCountRow {
    announcement_id: String,
    reaction_name: String,
    count: u64,
    me: u64,
}

pub(crate) fn build_announcements_document(
    config: &cfwdon_core::AppConfig,
    read_ids: &HashSet<String>,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> Vec<serde_json::Value> {
    let Some(raw) = config.announcements_json.as_deref() else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };

    items
        .into_iter()
        .filter_map(|item| {
            let mut announcement = item.as_object()?.clone();
            let id = announcement.get("id")?.as_str()?.to_owned();
            announcement.insert(
                "read".to_owned(),
                serde_json::Value::Bool(read_ids.contains(&id)),
            );

            let reactions = announcement
                .get("reactions")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|reaction| {
                    let mut reaction = reaction.as_object()?.clone();
                    let name = reaction.get("name")?.as_str()?.to_owned();
                    let (count, me) = reaction_state
                        .get(&(id.clone(), name))
                        .copied()
                        .unwrap_or_else(|| {
                            let count = reaction
                                .get("count")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0);
                            let me = reaction
                                .get("me")
                                .and_then(serde_json::Value::as_bool)
                                .unwrap_or(false);
                            (count, me)
                        });
                    reaction.insert("count".to_owned(), serde_json::json!(count));
                    reaction.insert("me".to_owned(), serde_json::Value::Bool(me));
                    Some(serde_json::Value::Object(reaction))
                })
                .collect::<Vec<_>>();
            announcement.insert("reactions".to_owned(), serde_json::Value::Array(reactions));

            for key in ["mentions", "statuses", "tags", "emojis"] {
                if !announcement.contains_key(key) {
                    announcement.insert(key.to_owned(), serde_json::json!([]));
                }
            }

            Some(serde_json::Value::Object(announcement))
        })
        .collect()
}

fn configured_announcement_exists(config: &cfwdon_core::AppConfig, announcement_id: &str) -> bool {
    let Some(raw) = config.announcements_json.as_deref() else {
        return false;
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    items.iter().any(|item| {
        item.get("id")
            .and_then(serde_json::Value::as_str)
            .map(|id| id == announcement_id)
            .unwrap_or(false)
    })
}

async fn list_announcement_read_ids(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<HashSet<String>> {
    let rows = db
        .prepare(
            "SELECT announcement_id
             FROM account_announcement_dismissals
             WHERE account_id = ?1",
        )
        .bind_refs(&[D1Type::Text(account_id)])?
        .all()
        .await?
        .results::<AnnouncementDismissalRow>()?;
    Ok(rows.into_iter().map(|row| row.announcement_id).collect())
}

async fn load_announcement_reaction_state(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<HashMap<(String, String), (u64, bool)>> {
    let rows = db
        .prepare(
            "SELECT
                announcement_id,
                reaction_name,
                COUNT(*) AS count,
                MAX(CASE WHEN account_id = ?1 THEN 1 ELSE 0 END) AS me
             FROM account_announcement_reactions
             GROUP BY announcement_id, reaction_name",
        )
        .bind_refs(&[D1Type::Text(account_id)])?
        .all()
        .await?
        .results::<AnnouncementReactionCountRow>()?;
    let mut state = HashMap::new();
    for row in rows {
        if row.announcement_id.is_empty() || row.reaction_name.is_empty() {
            continue;
        }
        state.insert(
            (row.announcement_id, row.reaction_name),
            (row.count, row.me > 0),
        );
    }
    Ok(state)
}

async fn save_announcement_dismissal(
    db: &worker::D1Database,
    account_id: &str,
    announcement_id: &str,
) -> Result<()> {
    db.prepare(
        "INSERT INTO account_announcement_dismissals (
            account_id,
            announcement_id
         ) VALUES (?1, ?2)
         ON CONFLICT(account_id, announcement_id)
         DO UPDATE SET updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(&[D1Type::Text(account_id), D1Type::Text(announcement_id)])?
    .run()
    .await?;
    Ok(())
}

async fn save_announcement_reaction(
    db: &worker::D1Database,
    account_id: &str,
    announcement_id: &str,
    reaction_name: &str,
) -> Result<()> {
    db.prepare(
        "INSERT INTO account_announcement_reactions (
            account_id,
            announcement_id,
            reaction_name
         ) VALUES (?1, ?2, ?3)
         ON CONFLICT(account_id, announcement_id, reaction_name)
         DO UPDATE SET updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(&[
        D1Type::Text(account_id),
        D1Type::Text(announcement_id),
        D1Type::Text(reaction_name),
    ])?
    .run()
    .await?;
    Ok(())
}

async fn delete_announcement_reaction(
    db: &worker::D1Database,
    account_id: &str,
    announcement_id: &str,
    reaction_name: &str,
) -> Result<()> {
    db.prepare(
        "DELETE FROM account_announcement_reactions
         WHERE account_id = ?1
           AND announcement_id = ?2
           AND reaction_name = ?3",
    )
    .bind_refs(&[
        D1Type::Text(account_id),
        D1Type::Text(announcement_id),
        D1Type::Text(reaction_name),
    ])?
    .run()
    .await?;
    Ok(())
}

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
    let db = ctx.d1(&config.database_binding)?;
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

    Response::from_json(&domains)
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

pub(crate) async fn instance_terms_of_service_version_response(
    ctx: RouteContext<()>,
) -> Result<Response> {
    instance_terms_of_service_response(ctx).await
}

pub(crate) async fn instance_domain_blocks_response(_ctx: RouteContext<()>) -> Result<Response> {
    Response::from_json(&Vec::<serde_json::Value>::new())
}

pub(crate) async fn instance_languages_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    Response::from_json(&configured_instance_languages(&config))
}

pub(crate) async fn instance_translation_languages_response(
    _ctx: RouteContext<()>,
) -> Result<Response> {
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn announcements_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "This method requires an authenticated user",
            }))?
            .with_status(422));
        }
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let read_ids = list_announcement_read_ids(&db, &account.id).await?;
    let reaction_state = load_announcement_reaction_state(&db, &account.id).await?;

    Response::from_json(&build_announcements_document(
        &config,
        &read_ids,
        &reaction_state,
    ))
}

pub(crate) async fn announcement_reaction_mutation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let Some(announcement_id) = ctx.param("announcement_id") else {
        return Response::error("announcement not found", 404);
    };
    let Some(reaction_name) = ctx.param("id") else {
        return Response::error("reaction not found", 404);
    };
    if !configured_announcement_exists(&config, announcement_id) {
        return Response::error("announcement not found", 404);
    }

    match req.method().as_ref() {
        "DELETE" => {
            delete_announcement_reaction(&db, &account.id, announcement_id, reaction_name).await?
        }
        _ => save_announcement_reaction(&db, &account.id, announcement_id, reaction_name).await?,
    }

    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn dismiss_announcement_mutation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let Some(announcement_id) = ctx.param("id") else {
        return Response::error("announcement not found", 404);
    };
    if !configured_announcement_exists(&config, announcement_id) {
        return Response::error("announcement not found", 404);
    }
    save_announcement_dismissal(&db, &account.id, announcement_id).await?;
    Ok(Response::empty()?.with_status(200))
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
