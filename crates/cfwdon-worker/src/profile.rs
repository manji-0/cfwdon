#[allow(unused_imports)]
pub(crate) use crate::*;

mod request_parsing;
pub(crate) use request_parsing::*;

pub(crate) use self::request_parsing::{UpdateCredentialsField, UpdateCredentialsRequest};
use super::{
    AccountReference, AppConfig, CACHE_TTL_ACCOUNT_API, Error, MastodonAccountResponse,
    ProfileField, Request, Response, Result, RouteContext, app_bearer_token_from_request,
    apply_account_credentials_update, apply_remote_actor_social_counts, cache_account_api_response,
    cache_public_json_response, cached_account_api_response, count_pending_follow_requests,
    enqueue_profile_update_activities, extract_authenticated_user,
    fetch_remote_actor_profile_with_document, find_authenticated_local_account,
    find_remote_actor_by_actor_uri, invalidate_account_public_cache, load_account_stats,
    load_config, load_remote_actor_social_counts_from_document, load_remote_actor_status_summary,
    media_object_url, normalize_hashtag, oauth_access_token_has_any_scope_json,
    oauth_bearer_token_hash, render_profile_field_value_html, resolve_account_reference,
    resolve_lookup_account, sql_placeholders, upsert_remote_actor,
};
use cfwdon_domain::LocalAccount;
use serde::Deserialize;
use std::collections::HashMap;
use worker::d1::D1Type;
use worker::{Bucket, D1Database};

#[derive(Debug)]
pub(crate) struct ProfileMediaUpload {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: String,
    pub(crate) object_kind: &'static str,
}

#[derive(Debug, Default)]
struct AccountProfileSettings {
    hide_collections: Option<bool>,
    indexable: bool,
    show_media: bool,
    show_media_replies: bool,
    show_featured: bool,
    avatar_description: String,
    header_description: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccountLookupQuery {
    acct: String,
}

enum ProfileMediaField {
    Avatar,
    Header,
}

const ACCOUNT_PREFERENCES_SELECT_SQL: &str = "a.id, a.username, a.access_email,
            a.display_name, a.bio_html, a.bio_text, a.fields_json, a.locked, a.bot, a.discoverable,
            a.default_post_visibility, a.default_quote_policy, a.default_sensitive,
            a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key,
            a.header_content_type, '' AS private_key_jwk, a.public_key_pem, a.created_at,
            s.hide_collections, s.indexable, s.show_media, s.show_media_replies, s.show_featured,
            s.avatar_description, s.header_description";
const PREFERENCES_READ_SCOPES: &[&str] = &[
    "read",
    "read:accounts",
    "read:blocks",
    "read:bookmarks",
    "read:collections",
    "read:favourites",
    "read:filters",
    "read:follows",
    "read:lists",
    "read:mutes",
    "read:notifications",
    "read:search",
    "read:statuses",
];

#[derive(Debug)]
struct AccountPreferencesSubject {
    account: LocalAccount,
    settings: AccountProfileSettings,
}

pub(crate) async fn account_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let cacheable_account_id = account_api_cache_candidate(&account_id);
    if cacheable_account_id
        && let Some(response) = cached_account_api_response(&ctx, &account_id).await?
    {
        return Ok(response);
    }
    match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            let stats = load_account_stats(&db, account.id()).await?;
            let settings = load_account_profile_settings(&db, account.id()).await?;
            let response =
                MastodonAccountResponse::from_account_with_stats(&account, &config, &stats)
                    .with_profile_settings(
                        settings.indexable,
                        settings.hide_collections,
                        settings.show_media,
                        settings.show_media_replies,
                        settings.show_featured,
                    );
            if cacheable_account_id {
                cache_account_api_response(&ctx, &account_id, &response).await?;
                return cache_public_json_response(
                    &response,
                    "application/json; charset=utf-8",
                    CACHE_TTL_ACCOUNT_API,
                    &[("Cache-Tag", &format!("account-{}", response.username))],
                );
            }
            Response::from_json(&response)
        }
        Some(AccountReference::Remote(actor)) => {
            let response = remote_account_response(&db, &actor).await?;
            Response::from_json(&response)
        }
        None => Response::error("account not found", 404),
    }
}

async fn remote_account_response(
    db: &D1Database,
    actor: &crate::RemoteActorRow,
) -> Result<MastodonAccountResponse> {
    let fetched = match fetch_remote_actor_profile_with_document(&actor.actor_uri).await {
        Ok(fetched) => fetched,
        Err(_) => return Ok(MastodonAccountResponse::from_remote_actor(actor)),
    };
    let profile = fetched.profile;
    upsert_remote_actor(db, &profile).await?;
    let mut response = match find_remote_actor_by_actor_uri(db, &profile.actor_uri).await? {
        Some(actor) => MastodonAccountResponse::from_remote_actor(&actor),
        None => MastodonAccountResponse::from_remote_actor_profile(&profile),
    };
    if let Ok(counts) = load_remote_actor_social_counts_from_document(&fetched.document).await {
        apply_remote_actor_social_counts(&mut response, counts);
    }
    if let Ok(summary) = load_remote_actor_status_summary(db, &profile.actor_uri).await {
        if summary.statuses_count > 0 {
            response.statuses_count = summary.statuses_count;
        }
        response.last_status_at = summary.last_status_at;
    }
    Ok(response)
}

fn account_api_cache_candidate(account_id: &str) -> bool {
    account_id.len() == 32 && account_id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) async fn account_lookup(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    match find_authenticated_local_account(&req, &db, &config).await? {
        Some(_) => {}
        None => return Response::error("Auth0 authentication required", 401),
    }

    let query: AccountLookupQuery = req.query()?;
    match resolve_lookup_account(&db, &config, &query.acct).await {
        Ok(account) => Response::from_json(&account),
        Err(_) => Response::error("account not found", 404),
    }
}

pub(crate) async fn verify_credentials(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let (stats, settings, featured_tags, follow_requests_count) = futures_util::try_join!(
        load_account_stats(&db, account.id()),
        load_account_profile_settings(&db, account.id()),
        featured_tags_payload(&db, &config, &account),
        count_pending_follow_requests(&db, account.id()),
    )?;

    Response::from_json(&build_credentials_document(
        &account,
        &config,
        &stats,
        &settings,
        featured_tags,
        follow_requests_count,
    ))
}

pub(crate) async fn profile_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let settings = load_account_profile_settings(&db, account.id()).await?;
    let featured_tags = featured_tags_payload(&db, &config, &account).await?;

    Response::from_json(&build_profile_document(
        &account,
        &config,
        &settings,
        featured_tags,
    ))
}

pub(crate) async fn preferences_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let subject = match find_authenticated_preferences_subject(&req, &db, &config).await? {
        Some(subject) => subject,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let account = &subject.account;
    let settings = &subject.settings;

    Response::from_json(&serde_json::json!({
        "posting:default:visibility": account.default_visibility().as_str(),
        "posting:default:sensitive": account.default_sensitive(),
        "posting:default:language": account.default_language(),
        "posting:default:quote_policy": account.default_quote_policy().as_str(),
        "posting:default:privacy": account.default_visibility().as_str(),
        "posting:default:media_sensitive": account.default_sensitive(),
        "posting:default:content_type": "text/plain",
        "reading:expand:media": if settings.show_media { "show_all" } else { "hide_all" },
        "reading:expand:spoilers": false,
        "reading:autoplay:gifs": true,
        "reading:display:media": if settings.show_media_replies { "show_all" } else { "hide_all" },
        "reading:display:expand_media": if settings.show_media { "show_all" } else { "hide_all" },
        "reading:display:expand_spoilers": false,
        "notifications:follow": true,
        "notifications:favourite": true,
        "notifications:reblog": true,
        "notifications:mention": true,
        "notifications:poll": true,
        "web:theme": "default",
    }))
}

async fn find_authenticated_preferences_subject(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<AccountPreferencesSubject>> {
    if let Some(token) = app_bearer_token_from_request(req)? {
        let Some((scopes_json, subject)) =
            load_oauth_preferences_subject(db, token.as_str()).await?
        else {
            return Ok(None);
        };
        if !oauth_access_token_has_any_scope_json(&scopes_json, PREFERENCES_READ_SCOPES) {
            return Ok(None);
        }
        return Ok(Some(subject));
    }

    let Some(user) = extract_authenticated_user(req, config).await? else {
        return Ok(None);
    };

    load_access_preferences_subject(db, user.email.as_str()).await
}

async fn load_oauth_preferences_subject(
    db: &D1Database,
    token: &str,
) -> Result<Option<(String, AccountPreferencesSubject)>> {
    let token_hash = oauth_bearer_token_hash(token);
    let token_hash = D1Type::Text(token_hash.as_str());
    let token = D1Type::Text(token);
    let sql = format!(
        "SELECT t.scopes_json, {ACCOUNT_PREFERENCES_SELECT_SQL}
         FROM oauth_access_tokens t
         JOIN accounts a ON a.id = t.account_id
         LEFT JOIN account_profile_settings s ON s.account_id = a.id
         WHERE t.access_token_hash = ?1
            OR t.access_token = ?2
         LIMIT 1"
    );
    let Some(row) = db
        .prepare(&sql)
        .bind_refs(&[token_hash, token])?
        .first::<serde_json::Value>(None)
        .await?
    else {
        return Ok(None);
    };
    let scopes_json = row
        .get("scopes_json")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Ok(Some((
        scopes_json,
        account_preferences_subject_from_value(&row)?,
    )))
}

async fn load_access_preferences_subject(
    db: &D1Database,
    email: &str,
) -> Result<Option<AccountPreferencesSubject>> {
    let email = D1Type::Text(email);
    let sql = format!(
        "SELECT {ACCOUNT_PREFERENCES_SELECT_SQL}
         FROM accounts a
         LEFT JOIN account_profile_settings s ON s.account_id = a.id
         WHERE a.access_email = ?1
         LIMIT 1"
    );
    db.prepare(&sql)
        .bind_refs(&email)?
        .first::<serde_json::Value>(None)
        .await?
        .map(|row| account_preferences_subject_from_value(&row))
        .transpose()
}

fn account_preferences_subject_from_value(
    row: &serde_json::Value,
) -> Result<AccountPreferencesSubject> {
    let account = serde_json::from_value::<crate::AccountRow>(row.clone())
        .map(LocalAccount::from_record)
        .map_err(|error| {
            Error::RustError(format!("failed to decode account preferences row: {error}"))
        })?;
    Ok(AccountPreferencesSubject {
        account,
        settings: account_profile_settings_from_value(row),
    })
}

pub(crate) async fn update_credentials(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let (account, settings, stats, featured_tags) =
        update_profile_internal(req, &ctx, &config, &account).await?;
    let follow_requests_count = count_pending_follow_requests(&db, account.id()).await?;
    Response::from_json(&build_credentials_document(
        &account,
        &config,
        &stats,
        &settings,
        featured_tags,
        follow_requests_count,
    ))
}

pub(crate) async fn update_profile_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let (account, settings, _stats, featured_tags) =
        update_profile_internal(req, &ctx, &config, &account).await?;
    invalidate_account_public_cache(&ctx, account.id(), account.username()).await;
    Response::from_json(&build_profile_document(
        &account,
        &config,
        &settings,
        featured_tags,
    ))
}

pub(crate) async fn delete_profile_avatar_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    delete_profile_media_response(req, ctx, ProfileMediaField::Avatar).await
}

pub(crate) async fn delete_profile_header_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    delete_profile_media_response(req, ctx, ProfileMediaField::Header).await
}

async fn update_profile_internal(
    req: &mut Request,
    ctx: &RouteContext<()>,
    config: &AppConfig,
    account: &cfwdon_domain::LocalAccount,
) -> Result<(
    cfwdon_domain::LocalAccount,
    AccountProfileSettings,
    crate::AccountStats,
    Vec<serde_json::Value>,
)> {
    let update = parse_update_credentials_request(req)
        .await
        .map_err(Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    let account = apply_account_credentials_update(&db, &bucket, config, account, &update).await?;
    save_account_profile_settings(&db, account.id(), &update).await?;
    let stats = load_account_stats(&db, account.id()).await?;
    let settings = load_account_profile_settings(&db, account.id()).await?;
    let featured_tags = featured_tags_payload(&db, config, &account).await?;

    Ok((account, settings, stats, featured_tags))
}

async fn delete_profile_media_response(
    req: Request,
    ctx: RouteContext<()>,
    field: ProfileMediaField,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    clear_profile_media(&db, &bucket, &account, field).await?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    enqueue_profile_update_activities(&db, &config, &account).await?;
    invalidate_account_public_cache(&ctx, account.id(), account.username()).await;
    enqueue_outbox_process_queue_best_effort(&ctx.env, "profile_media_delete").await;
    let stats = load_account_stats(&db, account.id()).await?;
    let settings = load_account_profile_settings(&db, account.id()).await?;
    let featured_tags = featured_tags_payload(&db, &config, &account).await?;
    let follow_requests_count = count_pending_follow_requests(&db, account.id()).await?;

    Response::from_json(&build_credentials_document(
        &account,
        &config,
        &stats,
        &settings,
        featured_tags,
        follow_requests_count,
    ))
}

async fn clear_profile_media(
    db: &D1Database,
    bucket: &Bucket,
    account: &cfwdon_domain::LocalAccount,
    field: ProfileMediaField,
) -> Result<()> {
    let avatar_key = account.avatar_object_key().map(str::to_owned);
    let header_key = account.header_object_key().map(str::to_owned);
    let (existing_key, object_key_column, content_type_column) = match field {
        ProfileMediaField::Avatar => (
            avatar_key.as_deref(),
            "avatar_object_key",
            "avatar_content_type",
        ),
        ProfileMediaField::Header => (
            header_key.as_deref(),
            "header_object_key",
            "header_content_type",
        ),
    };

    if let Some(object_key) = existing_key {
        bucket.delete(object_key).await?;
    }

    let account_id = D1Type::Text(account.id());
    db.prepare(format!(
        "UPDATE accounts
         SET {object_key_column} = NULL,
             {content_type_column} = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1"
    ))
    .bind_refs(&account_id)?
    .run()
    .await?;

    let settings = load_account_profile_settings(db, account.id()).await?;
    let updated = AccountProfileSettings {
        avatar_description: if matches!(field, ProfileMediaField::Avatar) {
            String::new()
        } else {
            settings.avatar_description.clone()
        },
        header_description: if matches!(field, ProfileMediaField::Header) {
            String::new()
        } else {
            settings.header_description.clone()
        },
        ..settings
    };
    upsert_account_profile_settings(db, account.id(), &updated).await
}

async fn load_account_profile_settings(
    db: &D1Database,
    account_id: &str,
) -> Result<AccountProfileSettings> {
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT hide_collections,
                    indexable,
                    show_media,
                    show_media_replies,
                    show_featured,
                    avatar_description,
                    header_description
             FROM account_profile_settings
             WHERE account_id = ?1
             LIMIT 1",
        )
        .bind_refs(&account_id)?
        .first::<serde_json::Value>(None)
        .await?;

    let Some(row) = row else {
        return Ok(AccountProfileSettings::default());
    };

    Ok(account_profile_settings_from_value(&row))
}

fn account_profile_settings_from_value(row: &serde_json::Value) -> AccountProfileSettings {
    AccountProfileSettings {
        hide_collections: value_as_option_bool(row.get("hide_collections")),
        indexable: value_as_bool(row.get("indexable"), true),
        show_media: value_as_bool(row.get("show_media"), true),
        show_media_replies: value_as_bool(row.get("show_media_replies"), true),
        show_featured: value_as_bool(row.get("show_featured"), true),
        avatar_description: row
            .get("avatar_description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        header_description: row
            .get("header_description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    }
}

async fn save_account_profile_settings(
    db: &D1Database,
    account_id: &str,
    update: &UpdateCredentialsRequest,
) -> Result<()> {
    let current = load_account_profile_settings(db, account_id).await?;
    let merged = AccountProfileSettings {
        hide_collections: update.hide_collections.or(current.hide_collections),
        indexable: update.indexable.unwrap_or(current.indexable),
        show_media: update.show_media.unwrap_or(current.show_media),
        show_media_replies: update
            .show_media_replies
            .unwrap_or(current.show_media_replies),
        show_featured: update.show_featured.unwrap_or(current.show_featured),
        avatar_description: update
            .avatar_description
            .clone()
            .unwrap_or(current.avatar_description),
        header_description: update
            .header_description
            .clone()
            .unwrap_or(current.header_description),
    };
    upsert_account_profile_settings(db, account_id, &merged).await
}

async fn upsert_account_profile_settings(
    db: &D1Database,
    account_id: &str,
    settings: &AccountProfileSettings,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        match settings.hide_collections {
            Some(value) => D1Type::Integer(i32::from(value)),
            None => D1Type::Null,
        },
        D1Type::Integer(i32::from(settings.indexable)),
        D1Type::Integer(i32::from(settings.show_media)),
        D1Type::Integer(i32::from(settings.show_media_replies)),
        D1Type::Integer(i32::from(settings.show_featured)),
        D1Type::Text(settings.avatar_description.as_str()),
        D1Type::Text(settings.header_description.as_str()),
    ];
    db.prepare(
        "INSERT INTO account_profile_settings (
            account_id,
            hide_collections,
            indexable,
            show_media,
            show_media_replies,
            show_featured,
            avatar_description,
            header_description,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id) DO UPDATE SET
            hide_collections = excluded.hide_collections,
            indexable = excluded.indexable,
            show_media = excluded.show_media,
            show_media_replies = excluded.show_media_replies,
            show_featured = excluded.show_featured,
            avatar_description = excluded.avatar_description,
            header_description = excluded.header_description,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn featured_tags_payload(
    db: &D1Database,
    config: &AppConfig,
    account: &cfwdon_domain::LocalAccount,
) -> Result<Vec<serde_json::Value>> {
    let account_id = D1Type::Text(account.id());
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

    let rows = result.results::<FeaturedTagRow>()?;
    let tag_names = rows
        .iter()
        .map(|row| row.tag_name.clone())
        .collect::<Vec<_>>();
    let metrics_by_tag = featured_tag_metrics_by_tag(db, account.id(), &tag_names).await?;
    let mut documents = Vec::with_capacity(rows.len());
    for row in rows {
        let normalized = normalize_hashtag(&row.tag_name);
        let metrics = metrics_by_tag
            .get(&normalized)
            .cloned()
            .unwrap_or(FeaturedTagMetricsRow {
                tag_name: normalized.clone(),
                statuses_count: 0,
                last_status_at: None,
            });
        documents.push(serde_json::json!({
            "id": normalized,
            "name": normalized,
            "url": format!("{}/tagged/{}", super::actor_url(config, account.username()), normalized),
            "statuses_count": metrics.statuses_count.to_string(),
            "last_status_at": metrics.last_status_at,
        }));
    }
    Ok(documents)
}

async fn featured_tag_metrics_by_tag(
    db: &D1Database,
    account_id: &str,
    tags: &[String],
) -> Result<HashMap<String, FeaturedTagMetricsRow>> {
    if tags.is_empty() {
        return Ok(HashMap::new());
    }

    let normalized_tags = tags
        .iter()
        .map(|tag| normalize_hashtag(tag))
        .collect::<Vec<_>>();
    let placeholders = sql_placeholders(2, normalized_tags.len());
    let sql = format!(
        "SELECT tag AS tag_name,
                COUNT(*) AS statuses_count,
                MAX(created_at) AS last_status_at
         FROM status_hashtags
         WHERE account_id = ?1
           AND tag IN ({placeholders})
         GROUP BY tag"
    );
    let mut bindings = Vec::with_capacity(normalized_tags.len() + 1);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(normalized_tags.iter().map(|tag| D1Type::Text(tag.as_str())));
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<FeaturedTagMetricsRow>()?
        .into_iter()
        .map(|row| (row.tag_name.clone(), row))
        .collect())
}

fn build_credentials_document(
    account: &cfwdon_domain::LocalAccount,
    config: &AppConfig,
    stats: &crate::AccountStats,
    settings: &AccountProfileSettings,
    featured_tags: Vec<serde_json::Value>,
    follow_requests_count: u64,
) -> serde_json::Value {
    let mut value = serde_json::to_value(
        MastodonAccountResponse::from_credentials_account(account, config, stats)
            .with_profile_settings(
                settings.indexable,
                settings.hide_collections,
                settings.show_media,
                settings.show_media_replies,
                settings.show_featured,
            ),
    )
    .unwrap_or_else(|_| serde_json::json!({}));

    if let Some(object) = value.as_object_mut() {
        object.insert("bot".to_owned(), serde_json::json!(account.is_bot()));
        object.insert("locked".to_owned(), serde_json::json!(account.is_locked()));
        object.insert(
            "discoverable".to_owned(),
            serde_json::json!(account.is_discoverable()),
        );
        object.insert(
            "avatar_description".to_owned(),
            serde_json::json!(settings.avatar_description),
        );
        object.insert(
            "header_description".to_owned(),
            serde_json::json!(settings.header_description),
        );
        object.insert("attribution_domains".to_owned(), serde_json::json!([]));
        object.insert(
            "featured_tags".to_owned(),
            serde_json::Value::Array(featured_tags),
        );

        if let Some(source) = object
            .get_mut("source")
            .and_then(serde_json::Value::as_object_mut)
        {
            source.insert(
                "follow_requests_count".to_owned(),
                serde_json::json!(follow_requests_count),
            );
            source.insert(
                "discoverable".to_owned(),
                serde_json::json!(account.is_discoverable()),
            );
        }
    }

    value
}

fn build_profile_document(
    account: &cfwdon_domain::LocalAccount,
    config: &AppConfig,
    settings: &AccountProfileSettings,
    featured_tags: Vec<serde_json::Value>,
) -> serde_json::Value {
    let avatar = account
        .avatar_object_key()
        .map(|object_key| media_object_url(config, object_key));
    let header = account
        .header_object_key()
        .map(|object_key| media_object_url(config, object_key));

    serde_json::json!({
        "id": account.id(),
        "display_name": account.display_name(),
        "note": account.bio_text(),
        "fields": profile_fields_for_profile(account.fields()),
        "avatar": avatar,
        "avatar_static": avatar,
        "avatar_description": settings.avatar_description,
        "header": header,
        "header_static": header,
        "header_description": settings.header_description,
        "locked": account.is_locked(),
        "bot": account.is_bot(),
        "hide_collections": settings.hide_collections,
        "discoverable": account.is_discoverable(),
        "indexable": settings.indexable,
        "show_media": settings.show_media,
        "show_media_replies": settings.show_media_replies,
        "show_featured": settings.show_featured,
        "attribution_domains": [],
        "featured_tags": featured_tags,
    })
}

fn profile_fields_for_profile(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "name": field.name,
                "value": field.value,
                "verified_at": serde_json::Value::Null,
            })
        })
        .collect()
}

fn value_as_bool(value: Option<&serde_json::Value>, default: bool) -> bool {
    value
        .and_then(|value| value.as_i64())
        .map(|value| value != 0)
        .unwrap_or(default)
}

fn value_as_option_bool(value: Option<&serde_json::Value>) -> Option<bool> {
    value
        .and_then(|value| value.as_i64())
        .map(|value| value != 0)
}

pub(crate) async fn require_authenticated_local_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<super::LocalAccount>> {
    find_authenticated_local_account(req, db, config).await
}

pub(crate) fn profile_field_from_update(field: &UpdateCredentialsField) -> Option<ProfileField> {
    Some(ProfileField {
        name: field.name.clone()?,
        value: field.value.clone()?,
    })
}

pub(crate) fn activitypub_profile_attachments(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "type": "PropertyValue",
                "name": field.name,
                "value": render_profile_field_value_html(&field.value),
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct FeaturedTagRow {
    tag_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct FeaturedTagMetricsRow {
    #[serde(default)]
    tag_name: String,
    statuses_count: u64,
    last_status_at: Option<String>,
}
