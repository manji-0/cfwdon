use crate::{
    Request, Response, Result, RouteContext, build_internal_cursor_link_header, build_tag_response,
    extract_authenticated_user, load_config, normalize_hashtag, parse_internal_pagination_id,
    resolve_local_account,
};
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Error};

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 200;
const MAX_FEATURED_TAGS: u64 = 10;

#[derive(Debug, Deserialize)]
struct FollowedTagRow {
    id: i64,
    tag_name: String,
    created_at: String,
}

#[derive(Debug, Default, Deserialize)]
struct TagCollectionQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

fn tag_from_context(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("id")
        .or_else(|| ctx.param("name"))
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing tag route parameter".to_owned()))
}

async fn find_followed_tag(
    db: &D1Database,
    account_id: &str,
    tag: &str,
) -> Result<Option<FollowedTagRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag)];
    db.prepare(
        "SELECT id, tag_name, created_at
         FROM followed_tags
         WHERE account_id = ?1
           AND tag_name = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<FollowedTagRow>(None)
    .await
}

pub(crate) async fn list_followed_tag_names(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let rows = db
        .prepare(
            "SELECT id, tag_name, created_at
             FROM followed_tags
             WHERE account_id = ?1
             ORDER BY created_at DESC, id DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?
        .results::<FollowedTagRow>()?;
    Ok(rows.into_iter().map(|row| row.tag_name).collect())
}

async fn is_following_tag(db: &D1Database, account_id: &str, tag: &str) -> Result<bool> {
    Ok(find_followed_tag(db, account_id, tag).await?.is_some())
}

async fn follow_tag(db: &D1Database, account_id: &str, tag: &str) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag)];
    db.prepare(
        "INSERT INTO followed_tags (account_id, tag_name)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, tag_name) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn unfollow_tag(db: &D1Database, account_id: &str, tag: &str) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag)];
    db.prepare(
        "DELETE FROM followed_tags
         WHERE account_id = ?1
           AND tag_name = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn is_featured_tag(db: &D1Database, account_id: &str, tag: &str) -> Result<bool> {
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

async fn count_featured_tags(db: &D1Database, account_id: &str) -> Result<u64> {
    let binding = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM featured_tags
             WHERE account_id = ?1",
        )
        .bind_refs(&binding)?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0))
}

async fn feature_tag(db: &D1Database, account_id: &str, tag: &str) -> Result<()> {
    if count_featured_tags(db, account_id).await? >= MAX_FEATURED_TAGS
        && !is_featured_tag(db, account_id, tag).await?
    {
        return Err(Error::RustError("featured tags limit reached".to_owned()));
    }

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

async fn unfeature_tag(db: &D1Database, account_id: &str, tag: &str) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(tag)];
    db.prepare(
        "DELETE FROM featured_tags
         WHERE account_id = ?1
           AND tag_name = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn build_authenticated_tag_response(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    account_id: &str,
    tag: &str,
) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(build_tag_response(db, config, tag).await?)?;
    value["following"] = serde_json::json!(is_following_tag(db, account_id, tag).await?);
    value["featured"] = serde_json::json!(is_featured_tag(db, account_id, tag).await?);
    Ok(value)
}

async fn resolve_authenticated_account(
    req: &Request,
    ctx: &RouteContext<()>,
) -> Result<
    Option<(
        D1Database,
        cfwdon_core::AppConfig,
        cfwdon_domain::LocalAccount,
    )>,
> {
    let config = load_config(ctx);
    let Some(user) = extract_authenticated_user(req, &config).await? else {
        return Ok(None);
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    Ok(Some((db, config, account)))
}

pub(crate) async fn follow_tag_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some((db, config, account)) = resolve_authenticated_account(&req, &ctx).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };
    let tag = tag_from_context(&ctx)?;
    follow_tag(&db, &account.id, &tag).await?;
    Response::from_json(&build_authenticated_tag_response(&db, &config, &account.id, &tag).await?)
}

pub(crate) async fn unfollow_tag_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let Some((db, config, account)) = resolve_authenticated_account(&req, &ctx).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };
    let tag = tag_from_context(&ctx)?;
    unfollow_tag(&db, &account.id, &tag).await?;
    Response::from_json(&build_authenticated_tag_response(&db, &config, &account.id, &tag).await?)
}

pub(crate) async fn feature_tag_v1_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some((db, config, account)) = resolve_authenticated_account(&req, &ctx).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };
    let tag = tag_from_context(&ctx)?;
    match feature_tag(&db, &account.id, &tag).await {
        Ok(()) => {}
        Err(Error::RustError(message)) if message == "featured tags limit reached" => {
            return Response::error(&message, 422);
        }
        Err(error) => return Err(error),
    }
    Response::from_json(&build_authenticated_tag_response(&db, &config, &account.id, &tag).await?)
}

pub(crate) async fn unfeature_tag_v1_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some((db, config, account)) = resolve_authenticated_account(&req, &ctx).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };
    let tag = tag_from_context(&ctx)?;
    unfeature_tag(&db, &account.id, &tag).await?;
    Response::from_json(&build_authenticated_tag_response(&db, &config, &account.id, &tag).await?)
}

pub(crate) async fn followed_tags_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let Some((db, config, account)) = resolve_authenticated_account(&req, &ctx).await? else {
        return Response::error("Cloudflare Access authentication required", 401);
    };

    let query = req.query::<TagCollectionQuery>()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;

    let account_id = D1Type::Text(&account.id);
    let rows = db
        .prepare(
            "SELECT id, tag_name, created_at
             FROM followed_tags
             WHERE account_id = ?1
             ORDER BY created_at DESC, id DESC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?
        .results::<FollowedTagRow>()?;

    let mut rows = rows
        .into_iter()
        .filter(|row| max_id.is_none_or(|value| row.id < value))
        .filter(|row| since_id.is_none_or(|value| row.id > value))
        .filter(|row| min_id.is_none_or(|value| row.id > value))
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });

    let has_next = rows.len() > limit as usize;
    if has_next {
        rows.truncate(limit as usize);
    }

    let first_id = rows.first().map(|row| row.id);
    let last_id = rows.last().map(|row| row.id);
    let mut documents = Vec::with_capacity(rows.len());
    for row in rows {
        documents.push(
            build_authenticated_tag_response(&db, &config, &account.id, &row.tag_name).await?,
        );
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        first_id,
        last_id,
        has_next,
        max_id.is_some() || since_id.is_some() || min_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }
    builder.from_json(&documents)
}
