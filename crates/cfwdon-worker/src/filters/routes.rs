use super::{
    create_filter_keyword_row, create_filter_row, create_filter_status_row,
    delete_filter_keyword_row, delete_filter_row, delete_filter_status_row,
    expires_at_from_seconds, find_filter, find_filter_keyword, find_filter_status, find_v1_filter,
    keyword_document, list_filter_keywords, list_filter_keywords_for_filters, list_filter_statuses,
    list_filter_statuses_for_filters, list_filters, list_v1_filters, normalize_contexts,
    normalize_filter_action, normalize_keyword, parse_keyword_request, parse_status_filter_request,
    parse_v1_filter_request, parse_v2_filter_request, replace_filter_keywords,
    split_filter_context, status_filter_document, update_filter_keyword_row, update_filter_row,
    v1_filter_document, v2_filter_document, v2_filter_document_from_parts,
};
use crate::{
    Error, Request, Response, Result, RouteContext, load_config,
    publish_user_stream_hub_event_soft, require_authenticated_local_account,
};

async fn publish_filters_changed_soft(
    env: &worker::Env,
    binding: &str,
    account_id: &str,
    event_id: &str,
) {
    publish_user_stream_hub_event_soft(
        env,
        binding,
        account_id,
        "filters_changed",
        "",
        Some(event_id),
    )
    .await;
}

pub(crate) async fn filters_v1_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let filters = list_v1_filters(&db, viewer.id())
        .await?
        .into_iter()
        .map(|row| v1_filter_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&filters)
}

pub(crate) async fn filter_v1_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;

    match find_v1_filter(&db, viewer.id(), &filter_id).await? {
        Some(row) => Response::from_json(&v1_filter_document(&row)),
        None => Response::error("filter not found", 404),
    }
}

pub(crate) async fn create_filter_v1_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let request = parse_v1_filter_request(req).await?;
    let phrase = normalize_keyword(request.phrase.as_deref())?;
    let contexts = normalize_contexts(request.context.unwrap_or_default())?;
    let expires_at = expires_at_from_seconds(request.expires_in)?;
    let filter_action = if request.irreversible.unwrap_or(false) {
        "hide".to_owned()
    } else {
        "warn".to_owned()
    };
    let whole_word = request.whole_word.unwrap_or(false);

    let filter_id = create_filter_row(
        &db,
        viewer.id(),
        &phrase,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;
    replace_filter_keywords(&db, &filter_id, &[(phrase.clone(), whole_word)]).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &filter_id,
    )
    .await;
    let Some(row) = list_v1_filters(&db, viewer.id())
        .await?
        .into_iter()
        .find(|row| row.phrase == phrase)
    else {
        return Response::error("failed to load filter", 500);
    };
    Response::from_json(&v1_filter_document(&row))
}

pub(crate) async fn update_filter_v1_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    let Some(existing_keyword) = find_filter_keyword(&db, viewer.id(), &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    let Some(existing_filter) = find_filter(&db, viewer.id(), &existing_keyword.filter_id).await?
    else {
        return Response::error("filter not found", 404);
    };

    let request = parse_v1_filter_request(req).await?;
    let keyword_count = list_filter_keywords(&db, &existing_filter.id).await?.len();
    if keyword_count > 1
        && (request.context.is_some()
            || request.expires_in.is_some()
            || request.irreversible.is_some())
    {
        return Response::error(
            "cannot update context, expires_in, or irreversible on a v1 filter backed by multiple keywords",
            422,
        );
    }
    let phrase = match request.phrase.as_deref() {
        Some(value) => normalize_keyword(Some(value))?,
        None => existing_keyword.keyword.clone(),
    };
    let contexts = match request.context {
        Some(context) => normalize_contexts(context)?,
        None => split_filter_context(&existing_filter.context_csv),
    };
    let expires_at = match request.expires_in {
        Some(seconds) => expires_at_from_seconds(Some(seconds))?,
        None => existing_filter.expires_at.clone(),
    };
    let filter_action = if request
        .irreversible
        .unwrap_or(existing_filter.filter_action == "hide")
    {
        "hide".to_owned()
    } else {
        "warn".to_owned()
    };
    let whole_word = request
        .whole_word
        .unwrap_or(existing_keyword.whole_word != 0);

    update_filter_row(
        &db,
        viewer.id(),
        &existing_filter.id,
        &phrase,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;
    update_filter_keyword_row(&db, &existing_keyword.id, &phrase, whole_word).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &existing_filter.id,
    )
    .await;

    let Some(row) = find_v1_filter(&db, viewer.id(), &existing_keyword.id).await? else {
        return Response::error("filter not found", 404);
    };
    Response::from_json(&v1_filter_document(&row))
}

pub(crate) async fn delete_filter_v1_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;

    let Some(keyword) = find_filter_keyword(&db, viewer.id(), &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    delete_filter_keyword_row(&db, &keyword.id).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &keyword.filter_id,
    )
    .await;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn filters_v2_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };

    let filters = list_filters(&db, viewer.id()).await?;
    let (keywords_by_filter_id, statuses_by_filter_id) = futures_util::try_join!(
        list_filter_keywords_for_filters(&db, &filters),
        list_filter_statuses_for_filters(&db, &filters),
    )?;
    let response = filters
        .iter()
        .map(|row| {
            v2_filter_document_from_parts(
                row,
                keywords_by_filter_id
                    .get(&row.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                statuses_by_filter_id
                    .get(&row.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    Response::from_json(&response)
}

pub(crate) async fn filter_v2_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;

    match find_filter(&db, viewer.id(), &filter_id).await? {
        Some(row) => Response::from_json(&v2_filter_document(&db, &row).await?),
        None => Response::error("filter not found", 404),
    }
}

pub(crate) async fn create_filter_v2_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let request = parse_v2_filter_request(req).await?;
    let contexts = normalize_contexts(request.context.unwrap_or_default())?;
    let filter_action = normalize_filter_action(request.filter_action.as_deref())?;
    let expires_at = expires_at_from_seconds(request.expires_in)?;
    let keywords = match request.keywords {
        Some(entries) => entries
            .into_iter()
            .filter_map(|entry| {
                entry.keyword.map(|keyword| {
                    let keyword = keyword.trim().to_owned();
                    (!keyword.is_empty()).then_some((keyword, entry.whole_word.unwrap_or(false)))
                })
            })
            .flatten()
            .collect::<Vec<_>>(),
        None => match request
            .phrase
            .as_deref()
            .or(request.title.as_deref())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            Some(value) => vec![(value.to_owned(), request.whole_word.unwrap_or(false))],
            None => Vec::new(),
        },
    };
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| keywords.first().map(|(keyword, _)| keyword.clone()))
        .ok_or_else(|| Error::RustError("title or keyword is required".to_owned()))?;

    let filter_id = create_filter_row(
        &db,
        viewer.id(),
        &title,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;
    replace_filter_keywords(&db, &filter_id, &keywords).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &filter_id,
    )
    .await;
    let Some(row) = find_filter(&db, viewer.id(), &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    Response::from_json(&v2_filter_document(&db, &row).await?)
}

pub(crate) async fn update_filter_v2_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    let Some(existing) = find_filter(&db, viewer.id(), &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    let request = parse_v2_filter_request(req).await?;

    let contexts = match request.context {
        Some(contexts) => normalize_contexts(contexts)?,
        None => split_filter_context(&existing.context_csv),
    };
    let filter_action = match request.filter_action.as_deref() {
        Some(value) => normalize_filter_action(Some(value))?,
        None => existing.filter_action.clone(),
    };
    let expires_at = match request.expires_in {
        Some(seconds) => expires_at_from_seconds(Some(seconds))?,
        None => existing.expires_at.clone(),
    };
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or(existing.title.clone());

    update_filter_row(
        &db,
        viewer.id(),
        &filter_id,
        &title,
        &contexts,
        expires_at.as_deref(),
        &filter_action,
    )
    .await?;

    if let Some(entries) = request.keywords {
        let keywords = entries
            .into_iter()
            .filter_map(|entry| {
                entry.keyword.map(|keyword| {
                    let keyword = keyword.trim().to_owned();
                    (!keyword.is_empty()).then_some((keyword, entry.whole_word.unwrap_or(false)))
                })
            })
            .flatten()
            .collect::<Vec<_>>();
        replace_filter_keywords(&db, &filter_id, &keywords).await?;
    }

    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &filter_id,
    )
    .await;

    let Some(row) = find_filter(&db, viewer.id(), &filter_id).await? else {
        return Response::error("filter not found", 404);
    };
    Response::from_json(&v2_filter_document(&db, &row).await?)
}

pub(crate) async fn delete_filter_v2_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if !delete_filter_row(&db, viewer.id(), &filter_id).await? {
        return Response::error("filter not found", 404);
    }
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &filter_id,
    )
    .await;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn filter_keywords_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, viewer.id(), &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let response = list_filter_keywords(&db, &filter_id)
        .await?
        .into_iter()
        .map(|row| keyword_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&response)
}

pub(crate) async fn create_filter_keyword_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, viewer.id(), &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let request = parse_keyword_request(req).await?;
    let keyword = normalize_keyword(request.keyword.as_deref())?;
    let whole_word = request.whole_word.unwrap_or(false);
    let keyword_id = create_filter_keyword_row(&db, &filter_id, &keyword, whole_word).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &filter_id,
    )
    .await;
    Response::from_json(&serde_json::json!({
        "id": keyword_id,
        "keyword": keyword,
        "whole_word": whole_word,
    }))
}

pub(crate) async fn filter_keyword_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let keyword_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing keyword id route parameter".to_owned()))?;
    match find_filter_keyword(&db, viewer.id(), &keyword_id).await? {
        Some(row) => Response::from_json(&keyword_document(&row)),
        None => Response::error("filter keyword not found", 404),
    }
}

pub(crate) async fn update_filter_keyword_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let keyword_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing keyword id route parameter".to_owned()))?;
    let Some(existing) = find_filter_keyword(&db, viewer.id(), &keyword_id).await? else {
        return Response::error("filter keyword not found", 404);
    };
    let request = parse_keyword_request(req).await?;
    let keyword = match request.keyword.as_deref() {
        Some(value) => normalize_keyword(Some(value))?,
        None => existing.keyword.clone(),
    };
    let whole_word = request.whole_word.unwrap_or(existing.whole_word != 0);
    update_filter_keyword_row(&db, &keyword_id, &keyword, whole_word).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &existing.filter_id,
    )
    .await;
    Response::from_json(&serde_json::json!({
        "id": keyword_id,
        "keyword": keyword,
        "whole_word": whole_word,
    }))
}

pub(crate) async fn delete_filter_keyword_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let keyword_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing keyword id route parameter".to_owned()))?;
    let Some(keyword) = find_filter_keyword(&db, viewer.id(), &keyword_id).await? else {
        return Response::error("filter keyword not found", 404);
    };
    delete_filter_keyword_row(&db, &keyword.id).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &keyword.filter_id,
    )
    .await;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn filter_statuses_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, viewer.id(), &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let response = list_filter_statuses(&db, &filter_id)
        .await?
        .into_iter()
        .map(|row| status_filter_document(&row))
        .collect::<Vec<_>>();
    Response::from_json(&response)
}

pub(crate) async fn create_filter_status_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter id route parameter".to_owned()))?;
    if find_filter(&db, viewer.id(), &filter_id).await?.is_none() {
        return Response::error("filter not found", 404);
    }
    let request = parse_status_filter_request(req).await?;
    let status_id = normalize_keyword(request.status_id.as_deref())?;
    let status_filter_id = create_filter_status_row(&db, &filter_id, &status_id).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &filter_id,
    )
    .await;
    Response::from_json(&serde_json::json!({
        "id": status_filter_id,
        "status_id": status_id,
    }))
}

pub(crate) async fn filter_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let status_filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter status id route parameter".to_owned()))?;
    match find_filter_status(&db, viewer.id(), &status_filter_id).await? {
        Some(row) => Response::from_json(&status_filter_document(&row)),
        None => Response::error("filter status not found", 404),
    }
}

pub(crate) async fn delete_filter_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let status_filter_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing filter status id route parameter".to_owned()))?;
    if find_filter_status(&db, viewer.id(), &status_filter_id)
        .await?
        .is_none()
    {
        return Response::error("filter status not found", 404);
    }
    delete_filter_status_row(&db, &status_filter_id).await?;
    publish_filters_changed_soft(
        &ctx.env,
        &config.stream_hub_binding,
        viewer.id(),
        &status_filter_id,
    )
    .await;
    Response::from_json(&serde_json::json!({}))
}
