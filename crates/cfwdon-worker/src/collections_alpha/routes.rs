use super::notifications::{
    insert_added_to_collection_notification, insert_collection_update_notifications,
};
use super::{
    CollectionsQuery, DEFAULT_COLLECTIONS_LIMIT, InCollectionPageEntry, MAX_COLLECTION_ITEMS,
    MAX_COLLECTIONS_LIMIT, account_blocks_viewer, account_reference_featureable_by_owner,
    action_not_allowed_response, actor_url, can_revoke_collection_item, collection_document,
    collection_item_by_id, collection_item_document, collection_item_response_document,
    collection_list_document, collection_response_document, collection_row_by_id,
    collection_update_is_significant, collection_update_requires_activity,
    collection_with_accounts_document, count_collection_rows_for_account, count_in_collection_rows,
    count_remote_collection_rows_for_actor, count_remote_in_collection_rows, delete_collection,
    delete_collection_item, enqueue_collection_add_activity,
    enqueue_collection_feature_request_activity, enqueue_collection_item_add_activity,
    enqueue_collection_item_remove_activity, enqueue_collection_remove_activity,
    enqueue_collection_update_activity, enqueue_delete_feature_authorization_activity,
    insert_collection, insert_collection_item, is_blocking_actor, is_owner, list_collection_items,
    list_collection_rows_for_account, list_local_in_collection_rows, list_remote_collection_items,
    list_remote_collection_rows_for_actor, list_remote_in_collection_rows,
    optional_collection_viewer, parse_collection_request, remote_collection_document,
    remote_collection_item_by_id, remote_collection_item_document, remote_collection_row_by_id,
    remote_collection_with_accounts_document, require_collection_reader, require_collection_writer,
    revalidate_remote_collection_item_approvals, revoke_collection_item,
    revoke_remote_collection_item, sort_in_collection_page_entries, update_collection,
    validate_collection_request, validation_failed_response,
};
use crate::{
    AccountReference, Request, Response, Result, RouteContext, find_account_by_id,
    find_remote_actor_by_actor_uri, load_config, resolve_account_reference,
};

fn route_param(ctx: &RouteContext<()>, name: &str) -> Result<String> {
    ctx.param(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError(format!("missing {name} route parameter")))
}

pub(in crate::collections_alpha) fn build_collection_offset_link(
    url: &url::Url,
    limit: u32,
    offset: u32,
    rel: &str,
) -> String {
    let mut url = url.clone();
    let query_pairs = url
        .query_pairs()
        .filter(|(key, _)| key != "limit" && key != "offset")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut serializer = url.query_pairs_mut();
        for (key, value) in query_pairs {
            serializer.append_pair(&key, &value);
        }
        serializer.append_pair("limit", &limit.to_string());
        serializer.append_pair("offset", &offset.to_string());
    }
    format!("<{}>; rel=\"{rel}\"", url.as_str())
}

pub(in crate::collections_alpha) fn build_collection_offset_link_header_for_url(
    url: &url::Url,
    limit: u32,
    offset: u32,
    page_size: usize,
    total_count: u64,
) -> Option<String> {
    let mut links = Vec::new();
    if (offset as u64).saturating_add(page_size as u64) < total_count {
        links.push(build_collection_offset_link(
            url,
            limit,
            offset.saturating_add(limit),
            "next",
        ));
    }
    if offset > 0 {
        links.push(build_collection_offset_link(
            url,
            limit,
            offset.saturating_sub(limit),
            "prev",
        ));
    }
    (!links.is_empty()).then(|| links.join(", "))
}

fn build_collection_offset_link_header(
    req: &Request,
    limit: u32,
    offset: u32,
    page_size: usize,
    total_count: u64,
) -> Result<Option<String>> {
    Ok(build_collection_offset_link_header_for_url(
        &req.url()?,
        limit,
        offset,
        page_size,
        total_count,
    ))
}

pub(crate) async fn alpha_account_collections_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionsQuery = req.query().unwrap_or_default();
    let limit = query
        .limit
        .unwrap_or(DEFAULT_COLLECTIONS_LIMIT)
        .clamp(1, MAX_COLLECTIONS_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let account_id = route_param(&ctx, "account_id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match optional_collection_viewer(&req, &db, &config).await? {
        Ok(viewer) => viewer,
        Err(response) => return Ok(response),
    };

    let owner = match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => account,
        Some(AccountReference::Remote(_)) => {
            let Some(AccountReference::Remote(owner)) =
                resolve_account_reference(&db, &account_id).await?
            else {
                return Response::error("account not found", 404);
            };
            if viewer
                .account
                .as_ref()
                .is_some_and(|viewer| viewer.id() == account_id)
                || (if let Some(viewer) = viewer.account.as_ref() {
                    is_blocking_actor(&db, viewer.id(), &owner.actor_uri).await?
                } else {
                    false
                })
            {
                return Response::from_json(&collection_list_document(Vec::new()));
            }
            let rows =
                list_remote_collection_rows_for_actor(&db, &owner.actor_uri, offset, limit).await?;
            let total_count = count_remote_collection_rows_for_actor(&db, &owner.actor_uri).await?;
            let mut response = Vec::new();
            for row in rows.iter() {
                revalidate_remote_collection_item_approvals(&db, &config, row).await?;
                let items = list_remote_collection_items(&db, &row.id, false).await?;
                let mut item_documents = Vec::new();
                for item in &items {
                    item_documents.push(remote_collection_item_document(&db, &config, item).await?);
                }
                response.push(remote_collection_document(
                    &config,
                    &owner,
                    row,
                    item_documents,
                ));
            }
            let mut builder = Response::from_json(&collection_list_document(response))?;
            if let Some(link_header) =
                build_collection_offset_link_header(&req, limit, offset, rows.len(), total_count)?
            {
                builder.headers_mut().set("Link", &link_header)?;
            }
            return Ok(builder);
        }
        None => return Response::error("account not found", 404),
    };
    let include_private = is_owner(viewer.account.as_ref(), owner.id());
    let collections_hidden =
        account_blocks_viewer(&db, &config, &owner, viewer.account.as_ref()).await?;
    let (rows, total_count) = if collections_hidden {
        (Vec::new(), 0)
    } else {
        (
            list_collection_rows_for_account(&db, owner.id(), include_private, offset, limit)
                .await?,
            count_collection_rows_for_account(&db, owner.id(), include_private).await?,
        )
    };
    let mut response = Vec::new();
    for row in rows.iter() {
        let include_pending = include_private;
        let items = list_collection_items(&db, &row.id, include_pending)
            .await?
            .iter()
            .map(collection_item_document)
            .collect::<Vec<_>>();
        response.push(collection_document(&config, &owner, row, items));
    }
    let mut builder = Response::from_json(&collection_list_document(response))?;
    if let Some(link_header) =
        build_collection_offset_link_header(&req, limit, offset, rows.len(), total_count)?
    {
        builder.headers_mut().set("Link", &link_header)?;
    }
    Ok(builder)
}

pub(crate) async fn alpha_account_in_collections_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionsQuery = req.query().unwrap_or_default();
    let limit = query
        .limit
        .unwrap_or(DEFAULT_COLLECTIONS_LIMIT)
        .clamp(1, MAX_COLLECTIONS_LIMIT);
    let offset = query.offset.unwrap_or(0);
    let account_id = route_param(&ctx, "account_id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_collection_reader(&req, &db, &config).await? {
        Ok(viewer) => viewer,
        Err(response) => return Ok(response),
    };
    let target_account_id = match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) if account.id() == viewer.id() => {
            account.id().to_owned()
        }
        Some(_) => return action_not_allowed_response(),
        None => return Response::error("account not found", 404),
    };

    let target_actor_uri = actor_url(&config, viewer.username());
    let local_total_count = count_in_collection_rows(&db, &target_account_id).await?;
    let remote_total_count = count_remote_in_collection_rows(&db, &target_actor_uri).await?;
    let total_count = local_total_count.saturating_add(remote_total_count);
    let page_window = offset.saturating_add(limit);
    let local_rows = list_local_in_collection_rows(&db, &target_account_id, page_window).await?;
    let remote_rows = list_remote_in_collection_rows(&db, &target_actor_uri, page_window).await?;
    let mut entries = local_rows
        .into_iter()
        .map(InCollectionPageEntry::Local)
        .chain(remote_rows.into_iter().map(InCollectionPageEntry::Remote))
        .collect::<Vec<_>>();
    sort_in_collection_page_entries(&mut entries);

    let page_entries = entries
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect::<Vec<_>>();
    let page_size = page_entries.len();
    let mut response = Vec::new();
    for entry in page_entries {
        match entry {
            InCollectionPageEntry::Local(row) => {
                let Some(owner) = find_account_by_id(&db, &row.account_id).await? else {
                    continue;
                };
                let include_pending = viewer.id() == owner.id();
                let items = list_collection_items(&db, &row.id, include_pending)
                    .await?
                    .iter()
                    .map(collection_item_document)
                    .collect::<Vec<_>>();
                response.push(collection_document(&config, &owner, &row, items));
            }
            InCollectionPageEntry::Remote(row) => {
                let Some(owner) = find_remote_actor_by_actor_uri(&db, &row.actor_uri).await? else {
                    continue;
                };
                revalidate_remote_collection_item_approvals(&db, &config, &row).await?;
                let items = list_remote_collection_items(&db, &row.id, true).await?;
                let mut item_documents = Vec::new();
                for item in &items {
                    item_documents.push(remote_collection_item_document(&db, &config, item).await?);
                }
                response.push(remote_collection_document(
                    &config,
                    &owner,
                    &row,
                    item_documents,
                ));
            }
        }
    }
    let mut builder = Response::from_json(&collection_list_document(response))?;
    if let Some(link_header) =
        build_collection_offset_link_header(&req, limit, offset, page_size, total_count)?
    {
        builder.headers_mut().set("Link", &link_header)?;
    }
    Ok(builder)
}

pub(crate) async fn alpha_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match optional_collection_viewer(&req, &db, &config).await? {
        Ok(viewer) => viewer,
        Err(response) => return Ok(response),
    };
    let Some(row) = collection_row_by_id(&db, &collection_id).await? else {
        let Some(row) = remote_collection_row_by_id(&db, &collection_id).await? else {
            return Response::error("collection not found", 404);
        };
        let Some(owner) = find_remote_actor_by_actor_uri(&db, &row.actor_uri).await? else {
            return Response::error("collection not found", 404);
        };
        if let Some(viewer) = viewer.account.as_ref()
            && is_blocking_actor(&db, viewer.id(), &owner.actor_uri).await?
        {
            return Response::error("collection not found", 404);
        }
        let document = remote_collection_with_accounts_document(
            &db,
            &config,
            &owner,
            &row,
            false,
            viewer.account.as_ref(),
        )
        .await?;
        return Response::from_json(&document);
    };
    let Some(owner) = find_account_by_id(&db, &row.account_id).await? else {
        return Response::error("collection not found", 404);
    };
    if account_blocks_viewer(&db, &config, &owner, viewer.account.as_ref()).await? {
        return Response::error("collection not found", 404);
    }
    let document = collection_with_accounts_document(
        &db,
        &config,
        &owner,
        &row,
        is_owner(viewer.account.as_ref(), &row.account_id),
        viewer.account.as_ref(),
    )
    .await?;
    Response::from_json(&document)
}

pub(crate) async fn create_alpha_collection_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let request = match parse_collection_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let details = validate_collection_request(&request, true);
    if !details.is_empty() {
        return validation_failed_response(details);
    }

    let mut targets = Vec::new();
    if let Some(account_ids) = request.account_ids.as_ref() {
        for account_id in account_ids.iter().take(MAX_COLLECTION_ITEMS) {
            let Some(target) = resolve_account_reference(&db, account_id).await? else {
                return Response::error("account not found", 404);
            };
            if !account_reference_featureable_by_owner(&db, &config, &owner, &target).await? {
                return action_not_allowed_response();
            }
            targets.push(target);
        }
    }

    let row = insert_collection(&db, owner.id(), &request).await?;
    for target in targets {
        let item = insert_collection_item(&db, &row.id, &target).await?;
        match target {
            AccountReference::Local(target) => {
                insert_added_to_collection_notification(
                    &db, &config, &owner, &target, &row.id, &item.id,
                )
                .await?;
            }
            AccountReference::Remote(actor) => {
                enqueue_collection_feature_request_activity(
                    &db,
                    &config,
                    &owner,
                    &row.id,
                    &item,
                    &actor.actor_uri,
                )
                .await?;
            }
        }
    }
    let row = collection_row_by_id(&db, &row.id)
        .await?
        .ok_or_else(|| worker::Error::RustError("failed to reload collection".to_owned()))?;
    enqueue_collection_add_activity(&db, &config, &owner, &row).await?;
    let items = list_collection_items(&db, &row.id, true)
        .await?
        .iter()
        .map(collection_item_document)
        .collect::<Vec<_>>();
    Response::from_json(&collection_response_document(collection_document(
        &config, &owner, &row, items,
    )))
}

pub(crate) async fn update_alpha_collection_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(existing) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if existing.account_id != owner.id() {
        return action_not_allowed_response();
    }
    let request = match parse_collection_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let details = validate_collection_request(&request, false);
    if !details.is_empty() {
        return validation_failed_response(details);
    }
    let distribute_update = collection_update_requires_activity(&existing, &request);
    let significant_update = collection_update_is_significant(&existing, &request);
    let row = update_collection(&db, &collection_id, &request)
        .await?
        .ok_or_else(|| worker::Error::RustError("updated collection disappeared".to_owned()))?;
    if distribute_update {
        enqueue_collection_update_activity(&db, &config, &owner, &row).await?;
    }
    if significant_update {
        insert_collection_update_notifications(&db, &config, &owner, &row.id).await?;
    }
    let items = list_collection_items(&db, &row.id, true)
        .await?
        .iter()
        .map(collection_item_document)
        .collect::<Vec<_>>();
    Response::from_json(&collection_response_document(collection_document(
        &config, &owner, &row, items,
    )))
}

pub(crate) async fn delete_alpha_collection_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(existing) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if existing.account_id != owner.id() {
        return action_not_allowed_response();
    }
    enqueue_collection_remove_activity(&db, &config, &owner, &existing).await?;
    let _ = delete_collection(&db, &collection_id).await?;
    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn create_alpha_collection_item_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "collection_id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(collection) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if collection.account_id != owner.id() {
        return action_not_allowed_response();
    }
    let request = match parse_collection_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let Some(account_id) = request.account_id.clone().or_else(|| {
        request
            .account_ids
            .as_ref()
            .and_then(|ids| ids.first())
            .cloned()
    }) else {
        return Response::from_json(&serde_json::json!({
            "error": "`account_id` parameter is missing",
        }))
        .map(|response| response.with_status(422));
    };
    let Some(target) = resolve_account_reference(&db, &account_id).await? else {
        return Response::error("account not found", 404);
    };
    if !account_reference_featureable_by_owner(&db, &config, &owner, &target).await? {
        return action_not_allowed_response();
    }
    let item = insert_collection_item(&db, &collection_id, &target).await?;
    match target {
        AccountReference::Local(target) => {
            enqueue_collection_item_add_activity(&db, &config, &owner, &collection_id, &item)
                .await?;
            insert_added_to_collection_notification(
                &db,
                &config,
                &owner,
                &target,
                &collection_id,
                &item.id,
            )
            .await?;
        }
        AccountReference::Remote(actor) => {
            enqueue_collection_feature_request_activity(
                &db,
                &config,
                &owner,
                &collection_id,
                &item,
                &actor.actor_uri,
            )
            .await?;
        }
    }
    Response::from_json(&collection_item_response_document(
        collection_item_document(&item),
    ))
}

pub(crate) async fn delete_alpha_collection_item_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "collection_id")?;
    let item_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let owner = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(collection) = collection_row_by_id(&db, &collection_id).await? else {
        return Response::error("collection not found", 404);
    };
    if collection.account_id != owner.id() {
        return action_not_allowed_response();
    }
    let Some(item) = collection_item_by_id(&db, &collection_id, &item_id).await? else {
        return Response::error("collection item not found", 404);
    };
    enqueue_collection_item_remove_activity(&db, &config, &owner, &collection_id, &item).await?;
    if !delete_collection_item(&db, &collection_id, &item_id).await? {
        return Response::error("collection item not found", 404);
    }
    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn revoke_alpha_collection_item_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let collection_id = route_param(&ctx, "collection_id")?;
    let item_id = route_param(&ctx, "id")?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let requester = match require_collection_writer(&req, &db, &config).await? {
        Ok(account) => account,
        Err(response) => return Ok(response),
    };
    let Some(_collection) = collection_row_by_id(&db, &collection_id).await? else {
        let Some(remote_collection) = remote_collection_row_by_id(&db, &collection_id).await?
        else {
            return Response::error("collection not found", 404);
        };
        let Some(item) = remote_collection_item_by_id(&db, &collection_id, &item_id).await? else {
            return Response::error("collection item not found", 404);
        };
        if item.target_actor_uri != actor_url(&config, requester.username()) {
            return action_not_allowed_response();
        }
        enqueue_delete_feature_authorization_activity(
            &db,
            &config,
            &requester,
            &remote_collection,
            &item,
        )
        .await?;
        if !revoke_remote_collection_item(&db, &collection_id, &item_id).await? {
            return Response::error("collection item not found", 404);
        }
        return Ok(Response::empty()?.with_status(200));
    };
    let Some(item) = collection_item_by_id(&db, &collection_id, &item_id).await? else {
        return Response::error("collection item not found", 404);
    };
    if !can_revoke_collection_item(&requester, &item) {
        return action_not_allowed_response();
    }
    if !revoke_collection_item(&db, &collection_id, &item_id).await? {
        return Response::error("collection item not found", 404);
    }
    Ok(Response::empty()?.with_status(200))
}
