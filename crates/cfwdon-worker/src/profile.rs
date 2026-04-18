use super::{
    AccountReference, AppConfig, Error, MastodonAccountResponse, ProfileField, Request, Response,
    Result, RouteContext, UpdateCredentialsField, build_preferences_document,
    extract_authenticated_user, load_account_stats, load_config, parse_update_credentials_request,
    resolve_account_reference, resolve_local_account, resolve_lookup_account,
};
use serde::Deserialize;
use worker::D1Database;

#[derive(Debug)]
pub(crate) struct ProfileMediaUpload {
    pub(crate) bytes: Vec<u8>,
    pub(crate) content_type: String,
    pub(crate) object_kind: &'static str,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccountLookupQuery {
    acct: String,
}

pub(crate) async fn account_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            let stats = load_account_stats(&db, &account.id).await?;
            Response::from_json(&MastodonAccountResponse::from_account_with_stats(
                &account, &config, &stats,
            ))
        }
        Some(AccountReference::Remote(actor)) => {
            Response::from_json(&MastodonAccountResponse::from_remote_actor(&actor))
        }
        None => Response::error("account not found", 404),
    }
}

pub(crate) async fn account_lookup(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    let query: AccountLookupQuery = req.query()?;
    let db = ctx.d1(&config.database_binding)?;
    match resolve_lookup_account(&db, &config, &query.acct).await {
        Ok(account) => Response::from_json(&account),
        Err(_) => Response::error("account not found", 404),
    }
}

pub(crate) async fn verify_credentials(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let stats = load_account_stats(&db, &account.id).await?;

    Response::from_json(&MastodonAccountResponse::from_credentials_account(
        &account, &config, &stats,
    ))
}

pub(crate) async fn preferences_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    Response::from_json(&build_preferences_document(&account))
}

pub(crate) async fn update_credentials(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let update = parse_update_credentials_request(req)
        .await
        .map_err(Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let account =
        super::apply_account_credentials_update(&db, &bucket, &config, &account, &update).await?;
    let stats = load_account_stats(&db, &account.id).await?;

    Response::from_json(&MastodonAccountResponse::from_credentials_account(
        &account, &config, &stats,
    ))
}

pub(crate) async fn require_authenticated_local_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<super::LocalAccount>> {
    let Some(user) = extract_authenticated_user(req, config).await? else {
        return Ok(None);
    };
    resolve_local_account(db, &user).await.map(Some)
}

pub(crate) fn profile_field_from_update(field: &UpdateCredentialsField) -> Option<ProfileField> {
    Some(ProfileField {
        name: field.name.clone()?,
        value: field.value.clone()?,
    })
}

pub(crate) fn parse_profile_fields_json(value: &str) -> Vec<ProfileField> {
    serde_json::from_str(value).unwrap_or_default()
}

pub(crate) fn activitypub_profile_attachments(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "type": "PropertyValue",
                "name": field.name,
                "value": super::render_profile_field_value_html(&field.value),
            })
        })
        .collect()
}
