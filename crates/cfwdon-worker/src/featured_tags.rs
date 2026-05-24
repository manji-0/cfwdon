use std::collections::{HashMap, HashSet};

use crate::auth::find_account_by_username;
use crate::instance::{actor_url, instance_base_url};
use crate::profile::require_authenticated_local_account;
use crate::remote::{AccountReference, resolve_account_reference};
use crate::runtime_config::load_config;
use crate::{normalize_hashtag, sql_placeholders};
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{Request, Response, Result, RouteContext};

const MAX_FEATURED_TAGS: usize = 10;

#[derive(Debug, Deserialize)]
struct FeaturedTagRow {
    tag_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct FeaturedTagStatusMetricsRow {
    #[serde(default)]
    tag_name: String,
    statuses_count: u64,
    last_status_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SuggestedFeaturedTagRow {
    tag_name: String,
}

fn featured_tag_profile_url(config: &cfwdon_core::AppConfig, username: &str, tag: &str) -> String {
    format!(
        "{}/tagged/{}",
        actor_url(config, username),
        normalize_hashtag(tag)
    )
}

async fn count_featured_tags(db: &worker::D1Database, account_id: &str) -> Result<u64> {
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM featured_tags
             WHERE account_id = ?1",
        )
        .bind_refs(&account_id)?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row
        .as_ref()
        .and_then(|row| row.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0))
}

async fn is_featured_tag_present(
    db: &worker::D1Database,
    account_id: &str,
    tag: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag)];
    Ok(db
        .prepare(
            "SELECT tag_name
             FROM featured_tags
             WHERE account_id = ?1
               AND tag_name = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?
        .is_some())
}

async fn list_featured_tags_for_account(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<Vec<FeaturedTagRow>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT tag_name
             FROM featured_tags
             WHERE account_id = ?1
             ORDER BY created_at DESC, tag_name ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;
    result.results::<FeaturedTagRow>()
}

async fn featured_tag_metrics(
    db: &worker::D1Database,
    account_id: &str,
    tag: &str,
) -> Result<FeaturedTagStatusMetricsRow> {
    let tag = normalize_hashtag(tag);
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag.as_str())];
    Ok(db
        .prepare(featured_tag_metrics_sql())
        .bind_refs(bindings.iter())?
        .first::<FeaturedTagStatusMetricsRow>(None)
        .await?
        .unwrap_or(FeaturedTagStatusMetricsRow {
            tag_name: tag,
            statuses_count: 0,
            last_status_at: None,
        }))
}

async fn featured_tag_metrics_by_tag(
    db: &worker::D1Database,
    account_id: &str,
    tags: &[String],
) -> Result<HashMap<String, FeaturedTagStatusMetricsRow>> {
    if tags.is_empty() {
        return Ok(HashMap::new());
    }

    let normalized_tags = tags
        .iter()
        .map(|tag| normalize_hashtag(tag))
        .collect::<Vec<_>>();
    let sql = featured_tag_metrics_by_tag_sql(normalized_tags.len());
    let mut bindings = Vec::with_capacity(normalized_tags.len() + 1);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(normalized_tags.iter().map(|tag| D1Type::Text(tag.as_str())));
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<FeaturedTagStatusMetricsRow>()?
        .into_iter()
        .map(|row| (row.tag_name.clone(), row))
        .collect())
}

fn featured_tag_metrics_sql() -> &'static str {
    "SELECT COUNT(*) AS statuses_count,
            MAX(created_at) AS last_status_at
     FROM status_hashtags
     WHERE account_id = ?1
       AND tag = ?2"
}

fn featured_tag_metrics_by_tag_sql(tag_count: usize) -> String {
    let placeholders = sql_placeholders(2, tag_count);
    format!(
        "SELECT tag AS tag_name,
                COUNT(*) AS statuses_count,
                MAX(created_at) AS last_status_at
         FROM status_hashtags
         WHERE account_id = ?1
           AND tag IN ({placeholders})
         GROUP BY tag"
    )
}

fn featured_tag_api_document(
    config: &cfwdon_core::AppConfig,
    username: &str,
    tag: &str,
    statuses_count: u64,
    last_status_at: Option<String>,
) -> serde_json::Value {
    let normalized = normalize_hashtag(tag);
    serde_json::json!({
        "id": normalized,
        "name": normalized,
        "url": featured_tag_profile_url(config, username, tag),
        "statuses_count": statuses_count,
        "last_status_at": last_status_at,
    })
}

fn featured_tag_suggestion_document(
    config: &cfwdon_core::AppConfig,
    tag: &str,
) -> serde_json::Value {
    let normalized = normalize_hashtag(tag);
    serde_json::json!({
        "id": normalized,
        "name": normalized,
        "url": format!("{}/tags/{}", instance_base_url(config), normalized),
        "history": [],
        "following": false,
    })
}

fn build_featured_tags_collection_document(
    config: &cfwdon_core::AppConfig,
    username: &str,
    tags: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/collections/tags", actor_url(config, username)),
        "type": "OrderedCollection",
        "totalItems": tags.len(),
        "orderedItems": tags.iter().map(|tag| {
            let normalized = normalize_hashtag(tag);
            serde_json::json!({
                "type": "Hashtag",
                "name": format!("#{normalized}"),
                "href": featured_tag_profile_url(config, username, &normalized),
            })
        }).collect::<Vec<_>>(),
    })
}

pub(crate) fn build_featured_collection_document(
    config: &cfwdon_core::AppConfig,
    username: &str,
    pinned_status_uris: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{}/collections/featured", actor_url(config, username)),
        "type": "OrderedCollection",
        "totalItems": pinned_status_uris.len(),
        "orderedItems": pinned_status_uris.iter().map(|uri| serde_json::json!({ "id": uri })).collect::<Vec<_>>(),
    })
}

async fn insert_featured_tag(db: &worker::D1Database, account_id: &str, tag: &str) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag)];
    db.prepare(
        "INSERT INTO featured_tags (account_id, tag_name)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, tag_name) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn delete_featured_tag(db: &worker::D1Database, account_id: &str, tag: &str) -> Result<bool> {
    if !is_featured_tag_present(db, account_id, tag).await? {
        return Ok(false);
    }
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag)];
    db.prepare(
        "DELETE FROM featured_tags
         WHERE account_id = ?1
           AND tag_name = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(true)
}

async fn suggested_featured_tag_names(
    db: &worker::D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    let featured = list_featured_tags_for_account(db, account_id)
        .await?
        .into_iter()
        .map(|row| row.tag_name)
        .collect::<HashSet<_>>();
    let featured_tags = featured.iter().collect::<Vec<_>>();
    let sql = suggested_featured_tag_names_sql(featured_tags.len());
    let mut bindings = Vec::with_capacity(featured_tags.len() + 2);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(featured_tags.iter().map(|tag| D1Type::Text(tag.as_str())));
    bindings.push(D1Type::Integer(MAX_FEATURED_TAGS as i32));
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<SuggestedFeaturedTagRow>()?
        .into_iter()
        .map(|row| row.tag_name)
        .collect())
}

fn suggested_featured_tag_names_sql(featured_tag_count: usize) -> String {
    let exclusion_sql = if featured_tag_count == 0 {
        String::new()
    } else {
        format!(
            " AND tag NOT IN ({})",
            sql_placeholders(2, featured_tag_count)
        )
    };
    format!(
        "SELECT tag AS tag_name
         FROM status_hashtags
         WHERE account_id = ?1{exclusion_sql}
         GROUP BY tag
         ORDER BY COUNT(*) DESC, MAX(created_at) DESC, tag ASC
         LIMIT ?{}",
        featured_tag_count + 2
    )
}

#[derive(Debug, Default, Deserialize)]
struct FeatureTagRequest {
    name: Option<String>,
}

async fn parse_feature_tag_request(req: &mut Request) -> std::result::Result<String, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let raw_name = if content_type.contains("application/json") {
        req.json::<FeatureTagRequest>()
            .await
            .map_err(|error| format!("invalid featured tag JSON payload: {error}"))?
            .name
    } else {
        req.form_data()
            .await
            .map_err(|error| format!("invalid featured tag form payload: {error}"))?
            .get_field("name")
    };

    let normalized = normalize_hashtag(raw_name.as_deref().unwrap_or_default());
    if normalized.is_empty() {
        return Err("featured tag name must not be empty".to_owned());
    }

    Ok(normalized)
}

pub(crate) async fn featured_tags_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let rows = list_featured_tags_for_account(&db, &account.id).await?;
    let tag_names = rows
        .iter()
        .map(|row| row.tag_name.clone())
        .collect::<Vec<_>>();
    let metrics_by_tag = featured_tag_metrics_by_tag(&db, &account.id, &tag_names).await?;
    let mut documents = Vec::new();
    for row in rows {
        let normalized = normalize_hashtag(&row.tag_name);
        let metrics =
            metrics_by_tag
                .get(&normalized)
                .cloned()
                .unwrap_or(FeaturedTagStatusMetricsRow {
                    tag_name: normalized,
                    statuses_count: 0,
                    last_status_at: None,
                });
        documents.push(featured_tag_api_document(
            &config,
            &account.username,
            &row.tag_name,
            metrics.statuses_count,
            metrics.last_status_at,
        ));
    }

    Response::from_json(&documents)
}

pub(crate) async fn account_featured_tags_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            let rows = list_featured_tags_for_account(&db, &account.id).await?;
            let tag_names = rows
                .iter()
                .map(|row| row.tag_name.clone())
                .collect::<Vec<_>>();
            let metrics_by_tag = featured_tag_metrics_by_tag(&db, &account.id, &tag_names).await?;
            let mut documents = Vec::new();
            for row in rows {
                let normalized = normalize_hashtag(&row.tag_name);
                let metrics = metrics_by_tag.get(&normalized).cloned().unwrap_or(
                    FeaturedTagStatusMetricsRow {
                        tag_name: normalized,
                        statuses_count: 0,
                        last_status_at: None,
                    },
                );
                documents.push(featured_tag_api_document(
                    &config,
                    &account.username,
                    &row.tag_name,
                    metrics.statuses_count,
                    metrics.last_status_at,
                ));
            }
            Response::from_json(&documents)
        }
        Some(AccountReference::Remote(_)) => Response::from_json(&Vec::<serde_json::Value>::new()),
        None => Response::error("account not found", 404),
    }
}

pub(crate) async fn featured_tag_suggestions_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let documents = suggested_featured_tag_names(&db, &account.id)
        .await?
        .into_iter()
        .map(|tag| featured_tag_suggestion_document(&config, &tag))
        .collect::<Vec<_>>();
    Response::from_json(&documents)
}

pub(crate) async fn feature_tag_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = parse_feature_tag_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if count_featured_tags(&db, &account.id).await? >= MAX_FEATURED_TAGS as u64
        && !is_featured_tag_present(&db, &account.id, &tag).await?
    {
        return Response::error("featured tags limit reached", 422);
    }

    insert_featured_tag(&db, &account.id, &tag).await?;
    let metrics = featured_tag_metrics(&db, &account.id, &tag).await?;
    Response::from_json(&featured_tag_api_document(
        &config,
        &account.username,
        &tag,
        metrics.statuses_count,
        metrics.last_status_at,
    ))
}

pub(crate) async fn unfeature_tag_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("id")
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing featured tag id".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if !delete_featured_tag(&db, &account.id, &tag).await? {
        return Response::error("featured tag not found", 404);
    }

    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn featured_tags_collection_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing username route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let account = find_account_by_username(&db, &username)
        .await?
        .ok_or_else(|| worker::Error::RustError("account not found".to_owned()))?;
    let tags = list_featured_tags_for_account(&db, &account.id)
        .await?
        .into_iter()
        .map(|row| row.tag_name)
        .collect::<Vec<_>>();

    Response::from_json(&build_featured_tags_collection_document(
        &config,
        &account.username,
        &tags,
    ))
}

pub(crate) async fn featured_collection_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing username route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let account = find_account_by_username(&db, &username)
        .await?
        .ok_or_else(|| worker::Error::RustError("account not found".to_owned()))?;
    let pinned_status_uris = crate::list_pinned_statuses_for_account(&db, &account.id)
        .await?
        .into_iter()
        .filter_map(|status| status.ap_id)
        .collect::<Vec<_>>();

    Response::from_json(&build_featured_collection_document(
        &config,
        &account.username,
        &pinned_status_uris,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        featured_tag_metrics_by_tag_sql, featured_tag_metrics_sql, suggested_featured_tag_names_sql,
    };

    #[test]
    fn featured_tag_metrics_sql_uses_indexed_status_hashtags() {
        let sql = featured_tag_metrics_sql();

        assert!(sql.contains("FROM status_hashtags"));
        assert!(sql.contains("account_id = ?1"));
        assert!(sql.contains("tag = ?2"));
        assert!(sql.contains("MAX(created_at) AS last_status_at"));
        assert!(!sql.contains("FROM statuses"));
        assert!(!sql.to_ascii_lowercase().contains("like"));
        assert!(!sql.contains("text_content"));
    }

    #[test]
    fn featured_tag_metrics_by_tag_sql_keeps_placeholder_slots_stable() {
        let sql = featured_tag_metrics_by_tag_sql(3);

        assert!(sql.contains("FROM status_hashtags"));
        assert!(sql.contains("account_id = ?1"));
        assert!(sql.contains("tag IN (?2, ?3, ?4)"));
        assert!(sql.contains("GROUP BY tag"));
        assert_eq!(sql.matches('?').count(), 4);
        assert!(!sql.to_ascii_lowercase().contains("like"));
        assert!(!sql.contains("text_content"));
    }

    #[test]
    fn suggested_featured_tag_names_sql_excludes_featured_tags_before_limit() {
        let sql = suggested_featured_tag_names_sql(2);

        assert!(sql.contains("FROM status_hashtags"));
        assert!(sql.contains("account_id = ?1"));
        assert!(sql.contains("tag NOT IN (?2, ?3)"));
        assert!(sql.contains("ORDER BY COUNT(*) DESC, MAX(created_at) DESC, tag ASC"));
        assert!(sql.contains("LIMIT ?4"));
        assert_eq!(sql.matches('?').count(), 4);
        assert!(!sql.to_ascii_lowercase().contains("like"));
        assert!(!sql.contains("text_content"));
    }

    #[test]
    fn suggested_featured_tag_names_sql_omits_empty_exclusion_list() {
        let sql = suggested_featured_tag_names_sql(0);

        assert!(sql.contains("account_id = ?1"));
        assert!(!sql.contains("NOT IN"));
        assert!(sql.contains("LIMIT ?2"));
        assert_eq!(sql.matches('?').count(), 2);
    }
}
