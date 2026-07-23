#[allow(unused_imports)]
pub(crate) use crate::*;

mod documents;
mod identity;
mod nodeinfo_documents;
mod policy_documents;
mod store;
pub(crate) use documents::*;
pub(crate) use identity::*;
pub(crate) use nodeinfo_documents::*;
pub(crate) use policy_documents::*;
pub(crate) use store::*;

use crate::statuses::{
    configured_translation_provider, configured_translation_provider_from_env,
    load_translation_provider_languages,
};
use std::collections::{HashMap, HashSet};
use time::{Duration, OffsetDateTime, Time, format_description::well_known::Rfc3339};
use worker::Env;
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

#[derive(Debug, Default, serde::Deserialize)]
struct TrendsQuery {
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
struct TrendingLocalLinkRow {
    text_content: String,
    created_at: String,
    account_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct TrendingRemoteLinkRow {
    content_html: String,
    published_at: String,
    actor_uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrendingLinkCandidate {
    url: String,
    account_key: String,
    published_at: String,
}

#[derive(Debug, Default)]
struct TrendingLinkAggregate {
    latest_timestamp: i64,
    total_uses: u64,
    accounts: HashSet<String>,
    uses_by_day: HashMap<i64, u64>,
    accounts_by_day: HashMap<i64, HashSet<String>>,
}

#[derive(Debug, Default)]
struct TrendingTagAggregate {
    statuses_count: u64,
    accounts: HashSet<String>,
    last_status_at: String,
}

const TRENDING_LINK_HISTORY_DAYS: usize = 7;

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
        .filter_map(|item| announcement_document(item, read_ids, reaction_state))
        .collect()
}

fn announcement_document(
    item: serde_json::Value,
    read_ids: &HashSet<String>,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> Option<serde_json::Value> {
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
        .filter_map(|reaction| announcement_reaction_document(&id, reaction, reaction_state))
        .collect::<Vec<_>>();
    announcement.insert("reactions".to_owned(), serde_json::Value::Array(reactions));

    ensure_announcement_collection_fields(&mut announcement);

    Some(serde_json::Value::Object(announcement))
}

fn ensure_announcement_collection_fields(
    announcement: &mut serde_json::Map<String, serde_json::Value>,
) {
    for key in ["mentions", "statuses", "tags", "emojis"] {
        if !announcement.contains_key(key) {
            announcement.insert(key.to_owned(), serde_json::json!([]));
        }
    }
}

fn announcement_reaction_document(
    announcement_id: &str,
    reaction: serde_json::Value,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> Option<serde_json::Value> {
    let mut reaction = reaction.as_object()?.clone();
    let name = reaction.get("name")?.as_str()?.to_owned();
    let (count, me) =
        announcement_reaction_viewer_state(announcement_id, &name, &reaction, reaction_state);
    reaction.insert("count".to_owned(), serde_json::json!(count));
    reaction.insert("me".to_owned(), serde_json::Value::Bool(me));
    Some(serde_json::Value::Object(reaction))
}

fn announcement_reaction_viewer_state(
    announcement_id: &str,
    name: &str,
    reaction: &serde_json::Map<String, serde_json::Value>,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> (u64, bool) {
    reaction_state
        .get(&(announcement_id.to_owned(), name.to_owned()))
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
        })
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

pub(crate) async fn list_announcement_read_ids(
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

pub(crate) async fn load_announcement_reaction_state(
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
    instance_summary_response_for_config(&db, config).await
}

pub(crate) async fn instance_summary_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = env.d1(&config.database_binding)?;
    instance_summary_response_for_config(&db, config).await
}

async fn instance_summary_response_for_config(
    db: &worker::D1Database,
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
    let db = ctx.d1(&config.database_binding)?;
    instance_v2_response_for_config(&db, config, configured_translation_provider(&ctx).is_some())
        .await
}

pub(crate) async fn instance_v2_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = env.d1(&config.database_binding)?;
    instance_v2_response_for_config(
        &db,
        config,
        configured_translation_provider_from_env(env).is_some(),
    )
    .await
}

async fn instance_v2_response_for_config(
    db: &worker::D1Database,
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
    let db = ctx.d1(&config.database_binding)?;

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

    cache_public_response(Response::from_json(&domains)?, CACHE_TTL_INSTANCE_SUMMARY)
}

pub(crate) async fn instance_activity_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    // Prefer js_sys::Date via now_unix_timestamp — std SystemTime panics on wasm32.
    let now = OffsetDateTime::from_unix_timestamp(now_unix_timestamp()).map_err(|error| {
        worker::Error::RustError(format!("invalid current unix timestamp: {error}"))
    })?;
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

    cache_public_response(
        Response::from_json(&build_instance_activity_document(
            week_floor,
            &weekly_totals,
        ))?,
        300,
    )
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

pub(crate) async fn announcements_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "This method requires an authenticated user",
            }))?
            .with_status(422));
        }
    };
    let read_ids = list_announcement_read_ids(&db, account.id()).await?;
    let reaction_state = load_announcement_reaction_state(&db, account.id()).await?;

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
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
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
            delete_announcement_reaction(&db, account.id(), announcement_id, reaction_name).await?
        }
        _ => save_announcement_reaction(&db, account.id(), announcement_id, reaction_name).await?,
    }

    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn dismiss_announcement_mutation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(announcement_id) = ctx.param("id") else {
        return Response::error("announcement not found", 404);
    };
    if !configured_announcement_exists(&config, announcement_id) {
        return Response::error("announcement not found", 404);
    }
    save_announcement_dismissal(&db, account.id(), announcement_id).await?;
    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn trending_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: TrendsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(10).clamp(1, 20);
    let offset = query.offset.unwrap_or(0);
    let fetch_limit = limit.saturating_add(offset).clamp(limit, 200);
    let db = ctx.d1(&config.database_binding)?;
    let cursor = ResolvedTimelineCursor::default();
    let mut entries = Vec::<(String, String, serde_json::Value)>::new();

    for status in list_local_public_timeline_statuses(&db, &cursor, fetch_limit).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let response = build_local_status_response(
            &db,
            &config,
            None,
            &status,
            &account,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?;
        entries.push((
            status.created_at.clone(),
            status.id.clone(),
            serde_json::to_value(response)?,
        ));
    }

    for (status, actor) in list_remote_public_timeline_statuses(&db, &cursor, fetch_limit).await? {
        let response = build_remote_status_response(&db, &config, None, &status, &actor).await?;
        entries.push((
            status.published_at.clone(),
            status.id.clone(),
            serde_json::to_value(response)?,
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
    cache_public_response(
        Response::from_json(
            &entries
                .into_iter()
                .skip(offset as usize)
                .take(limit as usize)
                .map(|(_, _, status)| status)
                .collect::<Vec<_>>(),
        )?,
        CACHE_TTL_TRENDS,
    )
}

pub(crate) async fn trending_links_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: TrendsQuery = req.query().unwrap_or_default();
    let db = ctx.d1(&config.database_binding)?;
    cache_public_response(
        Response::from_json(
            &list_trending_link_documents(
                &db,
                query.limit.unwrap_or(10).clamp(1, 20),
                query.offset.unwrap_or(0),
            )
            .await?,
        )?,
        CACHE_TTL_TRENDS,
    )
}

pub(crate) async fn trending_tags_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: TrendsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(10).clamp(1, 20);
    let offset = query.offset.unwrap_or(0);
    let db = ctx.d1(&config.database_binding)?;
    let cursor = ResolvedTimelineCursor::default();
    let mut tags = HashMap::<String, TrendingTagAggregate>::new();

    for status in list_local_public_timeline_statuses(&db, &cursor, 200).await? {
        for tag in extract_hashtags_from_text(&status.text) {
            let entry = tags.entry(tag).or_default();
            entry.statuses_count += 1;
            entry.accounts.insert(status.account_id.clone());
            if status.created_at > entry.last_status_at {
                entry.last_status_at = status.created_at.clone();
            }
        }
    }

    for (status, _actor) in list_remote_public_timeline_statuses(&db, &cursor, 200).await? {
        for tag in extract_hashtags_from_html(&status.content_html) {
            let entry = tags.entry(tag).or_default();
            entry.statuses_count += 1;
            entry.accounts.insert(status.actor_uri.clone());
            if status.published_at > entry.last_status_at {
                entry.last_status_at = status.published_at.clone();
            }
        }
    }

    let mut entries = tags.into_iter().collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .1
            .statuses_count
            .cmp(&left.1.statuses_count)
            .then_with(|| right.1.accounts.len().cmp(&left.1.accounts.len()))
            .then_with(|| right.1.last_status_at.cmp(&left.1.last_status_at))
            .then_with(|| left.0.cmp(&right.0))
    });

    let mut response = Vec::new();
    for (tag, _aggregate) in entries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
    {
        response.push(build_tag_response(&db, &config, &tag).await?);
    }
    cache_public_response(Response::from_json(&response)?, CACHE_TTL_TRENDS)
}

pub(crate) async fn custom_emojis_response(_ctx: RouteContext<()>) -> Result<Response> {
    custom_emojis_response_direct()
}

pub(crate) fn custom_emojis_response_direct() -> Result<Response> {
    cache_public_response(Response::from_json(&serde_json::json!([]))?, 300)
}

pub(crate) async fn trending_link_target_is_known(
    db: &worker::D1Database,
    target_urls: &[String],
) -> Result<bool> {
    let known = list_trending_link_entries(db).await?;
    let targets = target_urls
        .iter()
        .filter_map(|url| canonicalize_link_timeline_url(url))
        .collect::<HashSet<_>>();
    Ok(known.iter().any(|entry| targets.contains(&entry.url)))
}

async fn list_trending_link_documents(
    db: &worker::D1Database,
    limit: u32,
    offset: u32,
) -> Result<Vec<serde_json::Value>> {
    Ok(list_trending_link_entries(db)
        .await?
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .map(|entry| entry.document)
        .collect())
}

async fn list_trending_link_entries(db: &worker::D1Database) -> Result<Vec<TrendingLinkEntry>> {
    // Prefer js_sys::Date via now_unix_timestamp — std SystemTime panics on wasm32.
    let now_ts = now_unix_timestamp();
    let now = OffsetDateTime::from_unix_timestamp(now_ts).map_err(|error| {
        worker::Error::RustError(format!("invalid current unix timestamp: {error}"))
    })?;
    let cutoff = (now - Duration::days(TRENDING_LINK_HISTORY_DAYS as i64))
        .format(&Rfc3339)
        .map_err(|error| {
            worker::Error::RustError(format!("failed to format trending link cutoff: {error}"))
        })?;
    let today = unix_day_bucket(now_ts);
    let mut candidates = Vec::new();

    for row in list_trending_local_link_rows(db, &cutoff).await? {
        if let Some(candidate) =
            build_trending_link_candidate(&row.text_content, &row.created_at, &row.account_id)
        {
            candidates.push(candidate);
        }
    }

    for row in list_trending_remote_link_rows(db, &cutoff).await? {
        if let Some(candidate) = build_trending_link_candidate(
            &strip_html_tags(&row.content_html),
            &row.published_at,
            &row.actor_uri,
        ) {
            candidates.push(candidate);
        }
    }

    Ok(build_trending_link_entries(candidates, today))
}

async fn list_trending_local_link_rows(
    db: &worker::D1Database,
    cutoff: &str,
) -> Result<Vec<TrendingLocalLinkRow>> {
    let bindings = [D1Type::Text(cutoff)];
    db.prepare(
        "SELECT s.text_content, s.created_at, s.account_id
         FROM statuses s
         JOIN accounts a ON a.id = s.account_id
         WHERE s.visibility = 'public'
           AND a.discoverable = 1
           AND s.created_at >= ?1",
    )
    .bind_refs(&bindings)?
    .all()
    .await?
    .results::<TrendingLocalLinkRow>()
}

async fn list_trending_remote_link_rows(
    db: &worker::D1Database,
    cutoff: &str,
) -> Result<Vec<TrendingRemoteLinkRow>> {
    let bindings = [D1Type::Text(cutoff)];
    db.prepare(
        "SELECT rs.content_html, rs.published_at, rs.actor_uri
         FROM remote_statuses rs
         JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
         WHERE rs.visibility = 'public'
           AND ra.discoverable = 1
           AND rs.published_at >= ?1",
    )
    .bind_refs(&bindings)?
    .all()
    .await?
    .results::<TrendingRemoteLinkRow>()
}

fn build_trending_link_candidate(
    text: &str,
    published_at: &str,
    account_key: &str,
) -> Option<TrendingLinkCandidate> {
    let url = build_status_card_value(text)?
        .get("url")
        .and_then(serde_json::Value::as_str)
        .and_then(canonicalize_link_timeline_url)?;
    Some(TrendingLinkCandidate {
        url,
        account_key: account_key.to_owned(),
        published_at: published_at.to_owned(),
    })
}

#[derive(Debug, Clone)]
struct TrendingLinkEntry {
    url: String,
    latest_timestamp: i64,
    total_uses: u64,
    total_accounts: usize,
    document: serde_json::Value,
}

fn build_trending_link_entries(
    candidates: Vec<TrendingLinkCandidate>,
    today_bucket: i64,
) -> Vec<TrendingLinkEntry> {
    let mut aggregates = HashMap::<String, TrendingLinkAggregate>::new();

    for candidate in candidates {
        let Some(published_at) = parse_unix_timestamp(&candidate.published_at) else {
            continue;
        };
        let day = unix_day_bucket(published_at);
        let entry = aggregates.entry(candidate.url).or_default();
        entry.latest_timestamp = entry.latest_timestamp.max(published_at);
        entry.total_uses += 1;
        entry.accounts.insert(candidate.account_key.clone());
        *entry.uses_by_day.entry(day).or_insert(0) += 1;
        entry
            .accounts_by_day
            .entry(day)
            .or_default()
            .insert(candidate.account_key);
    }

    let mut entries = aggregates
        .into_iter()
        .filter_map(|(url, aggregate)| {
            let mut document = build_status_card_value(&url)?;
            document["history"] =
                serde_json::json!(build_trending_link_history(&aggregate, today_bucket,));
            Some(TrendingLinkEntry {
                url,
                latest_timestamp: aggregate.latest_timestamp,
                total_uses: aggregate.total_uses,
                total_accounts: aggregate.accounts.len(),
                document,
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        right
            .total_uses
            .cmp(&left.total_uses)
            .then_with(|| right.total_accounts.cmp(&left.total_accounts))
            .then_with(|| right.latest_timestamp.cmp(&left.latest_timestamp))
            .then_with(|| left.url.cmp(&right.url))
    });
    entries
}

fn build_trending_link_history(
    aggregate: &TrendingLinkAggregate,
    today_bucket: i64,
) -> Vec<serde_json::Value> {
    (0..TRENDING_LINK_HISTORY_DAYS)
        .map(|offset| {
            let day = today_bucket - (offset as i64 * 86_400);
            let uses = aggregate.uses_by_day.get(&day).copied().unwrap_or(0);
            let accounts = aggregate
                .accounts_by_day
                .get(&day)
                .map(|accounts| accounts.len() as u64)
                .unwrap_or(0);
            serde_json::json!({
                "day": day.to_string(),
                "accounts": accounts.to_string(),
                "uses": uses.to_string(),
            })
        })
        .collect()
}

fn parse_unix_timestamp(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|timestamp| timestamp.unix_timestamp())
}

fn unix_day_bucket(timestamp: i64) -> i64 {
    timestamp.div_euclid(86_400) * 86_400
}

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
    let db = ctx.d1(&config.database_binding)?;
    nodeinfo_response_for_config(&db, config).await
}

pub(crate) async fn nodeinfo_response_from_env(env: &Env) -> Result<Response> {
    let config = load_config_from_env(env);
    let db = env.d1(&config.database_binding)?;
    nodeinfo_response_for_config(&db, config).await
}

async fn nodeinfo_response_for_config(
    db: &worker::D1Database,
    config: super::AppConfig,
) -> Result<Response> {
    let summary = load_instance_summary(db, config.clone()).await?;
    let active_month = load_active_month_users(db).await?;
    let user_count = load_total_local_accounts(db).await?;
    let status_count = load_total_local_statuses(db).await?;

    cache_public_response(
        Response::from_json(&build_nodeinfo_document(
            &summary,
            &config,
            user_count,
            active_month,
            status_count,
        ))?,
        300,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        TrendingLinkCandidate, announcement_document, announcement_reaction_document,
        build_trending_link_candidate, build_trending_link_entries, parse_unix_timestamp,
        unix_day_bucket,
    };
    use std::collections::{HashMap, HashSet};

    #[test]
    fn announcement_document_adds_required_empty_collections() {
        let document = announcement_document(
            serde_json::json!({
                "id": "announcement-1",
                "content": "<p>Hello</p>"
            }),
            &HashSet::new(),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(document["read"], serde_json::json!(false));
        assert_eq!(document["mentions"], serde_json::json!([]));
        assert_eq!(document["statuses"], serde_json::json!([]));
        assert_eq!(document["tags"], serde_json::json!([]));
        assert_eq!(document["emojis"], serde_json::json!([]));
        assert_eq!(document["reactions"], serde_json::json!([]));
    }

    #[test]
    fn announcement_reaction_document_preserves_payload_defaults_without_viewer_state() {
        let reaction = announcement_reaction_document(
            "announcement-1",
            serde_json::json!({
                "name": "wave",
                "count": 5
            }),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(reaction["count"], serde_json::json!(5));
        assert_eq!(reaction["me"], serde_json::json!(false));
    }

    #[test]
    fn announcement_reaction_document_prefers_viewer_state() {
        let reaction_state =
            HashMap::from([(("announcement-1".to_owned(), "wave".to_owned()), (7, true))]);
        let reaction = announcement_reaction_document(
            "announcement-1",
            serde_json::json!({
                "name": "wave",
                "count": 5,
                "me": false
            }),
            &reaction_state,
        )
        .unwrap();

        assert_eq!(reaction["count"], serde_json::json!(7));
        assert_eq!(reaction["me"], serde_json::json!(true));
    }

    #[test]
    fn build_trending_link_candidate_normalizes_tracking_and_fragment() {
        let candidate = build_trending_link_candidate(
            "see https://example.com/post?utm_source=feed#part",
            "2026-04-21T12:00:00Z",
            "acct-1",
        )
        .unwrap();
        assert_eq!(candidate.url, "https://example.com/post");
    }

    #[test]
    fn build_trending_link_entries_sorts_by_usage_then_accounts() {
        let today_bucket = unix_day_bucket(parse_unix_timestamp("2026-04-21T12:00:00Z").unwrap());
        let entries = build_trending_link_entries(
            vec![
                TrendingLinkCandidate {
                    url: "https://example.com/b".to_owned(),
                    account_key: "acct-1".to_owned(),
                    published_at: "2026-04-21T12:00:00Z".to_owned(),
                },
                TrendingLinkCandidate {
                    url: "https://example.com/a".to_owned(),
                    account_key: "acct-1".to_owned(),
                    published_at: "2026-04-21T11:00:00Z".to_owned(),
                },
                TrendingLinkCandidate {
                    url: "https://example.com/a".to_owned(),
                    account_key: "acct-2".to_owned(),
                    published_at: "2026-04-21T10:00:00Z".to_owned(),
                },
            ],
            today_bucket,
        );

        assert_eq!(entries[0].url, "https://example.com/a");
        assert_eq!(entries[0].total_uses, 2);
        assert_eq!(
            entries[0].document["history"][0]["uses"],
            serde_json::json!("2")
        );
        assert_eq!(entries[1].url, "https://example.com/b");
    }
}
