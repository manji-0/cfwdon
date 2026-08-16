use super::{
    add_accounts_to_list, create_list_row, delete_list_row, list_document, list_id_from_context,
    list_membership_refs, list_row_by_id, list_rows_for_account, parse_list_accounts_request,
    parse_list_request, remove_accounts_from_list, requested_account_membership_variants,
    resolve_list_member_document, update_list_row,
};
use crate::profile::require_authenticated_local_account;
use crate::runtime_config::load_config;
use std::collections::HashSet;
use worker::{Request, Response, Result, RouteContext};

pub(crate) async fn account_lists_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let account_ref = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing account id route parameter".to_owned()))?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(target_refs) =
        requested_account_membership_variants(&db, &config, &account_ref).await?
    else {
        return Response::error("account not found", 404);
    };

    let target_refs = target_refs.into_iter().collect::<HashSet<_>>();
    let mut documents = Vec::new();
    for row in list_rows_for_account(&db, account.id()).await? {
        let memberships = list_membership_refs(&db, &row.id).await?;
        if memberships
            .into_iter()
            .any(|membership| target_refs.contains(&membership.target_account_ref))
        {
            documents.push(list_document(&row));
        }
    }

    Response::from_json(&documents)
}

pub(crate) async fn lists_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let documents = list_rows_for_account(&db, account.id())
        .await?
        .into_iter()
        .map(|row| list_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&documents)
}

pub(crate) async fn create_list_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let request = parse_list_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let row = create_list_row(&db, account.id(), &request).await?;
    Response::from_json(&list_document(&row))
}

pub(crate) async fn list_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let list_id = list_id_from_context(&ctx)?;
    match list_row_by_id(&db, account.id(), &list_id).await? {
        Some(row) => Response::from_json(&list_document(&row)),
        None => Response::error("list not found", 404),
    }
}

pub(crate) async fn update_list_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let request = parse_list_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    match update_list_row(&db, account.id(), &list_id, &request).await? {
        Some(row) => Response::from_json(&list_document(&row)),
        None => Response::error("list not found", 404),
    }
}

pub(crate) async fn delete_list_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if !delete_list_row(&db, account.id(), &list_id).await? {
        return Response::error("list not found", 404);
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn list_accounts_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let list_id = list_id_from_context(&ctx)?;
    if list_row_by_id(&db, account.id(), &list_id).await?.is_none() {
        return Response::error("list not found", 404);
    }

    let max_id = req
        .url()?
        .query_pairs()
        .find(|(key, _)| key == "max_id")
        .map(|(_, value)| value.into_owned());
    let mut documents = Vec::new();
    for row in list_membership_refs(&db, &list_id).await? {
        if let Some(document) =
            resolve_list_member_document(&db, &config, &row.target_account_ref).await?
        {
            documents.push(document);
        }
    }
    if let Some(max_id) = max_id {
        documents.retain(|document| {
            document
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(|value| value < max_id.as_str())
                .unwrap_or(false)
        });
    }
    Response::from_json(&documents)
}

pub(crate) async fn add_list_accounts_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let request = parse_list_accounts_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if list_row_by_id(&db, account.id(), &list_id).await?.is_none() {
        return Response::error("list not found", 404);
    }

    add_accounts_to_list(&db, &list_id, &request.account_ids.unwrap_or_default()).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn delete_list_accounts_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let list_id = list_id_from_context(&ctx)?;
    let request = parse_list_accounts_request(req)
        .await
        .map_err(worker::Error::RustError)?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    if list_row_by_id(&db, account.id(), &list_id).await?.is_none() {
        return Response::error("list not found", 404);
    }

    for account_ref in request.account_ids.unwrap_or_default() {
        let Some(variants) =
            requested_account_membership_variants(&db, &config, &account_ref).await?
        else {
            continue;
        };
        remove_accounts_from_list(&db, &list_id, &variants).await?;
    }

    Response::from_json(&serde_json::json!({}))
}
