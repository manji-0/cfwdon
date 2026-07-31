#[allow(unused_imports)]
pub(crate) use crate::*;

mod action_resolution;
mod bookmark_store;
mod bookmarks;
mod boost_targets;
mod counts;
mod detail_routes;
mod edits;
mod favourite_store;
mod favourites;
mod local_context;
mod local_timeline_store;
mod mutations;
mod outbox_activities;
mod pins;
mod placeholder_routes;
mod quotes_routes;
mod reblog_response;
mod reblog_store;
mod reblogs;
mod remote_context;
mod remote_mutations;
mod repository;
mod request_parsing;
mod response_builders;
mod response_mentions;
mod response_quotes;
mod store;
mod store_local;
mod store_remote;
mod thread_mutes;
mod usecases;
pub(crate) use action_resolution::*;
pub(crate) use bookmark_store::*;
pub(crate) use bookmarks::*;
pub(crate) use boost_targets::*;
pub(crate) use counts::*;
pub(crate) use detail_routes::*;
pub(crate) use edits::*;
pub(crate) use favourite_store::*;
pub(crate) use favourites::*;
pub(crate) use local_context::*;
pub(crate) use local_timeline_store::*;
pub(crate) use mutations::*;
pub(crate) use outbox_activities::*;
pub(crate) use pins::*;
pub(crate) use placeholder_routes::*;
pub(crate) use quotes_routes::*;
pub(crate) use reblog_store::*;
pub(crate) use reblogs::*;
pub(crate) use remote_context::*;
pub(crate) use remote_mutations::*;
pub(crate) use repository::*;
pub(crate) use request_parsing::*;
pub(crate) use response_builders::*;
pub(crate) use response_mentions::*;
pub(crate) use response_quotes::*;
pub(crate) use store::*;
pub(crate) use store_local::*;
pub(crate) use store_remote::*;
pub(crate) use thread_mutes::*;
pub(crate) use usecases::*;

use cfwdon_domain::{LocalAccount, QuoteApprovalPolicy, StatusDraft, Visibility};

struct CreateStatusAccess {
    account: LocalAccount,
    application_id: Option<i64>,
}

pub(crate) fn local_quote_policy_allows(policy: &str, is_owner: bool, is_follower: bool) -> bool {
    QuoteApprovalPolicy::parse(policy)
        .map(|policy| policy.allows_quote(is_owner, is_follower))
        .unwrap_or(false)
}

async fn validate_local_quote_creation(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    requester: &LocalAccount,
    draft: &StatusDraft,
    quote_of_uri: &str,
) -> Result<Option<&'static str>> {
    let Some(status) = find_local_status_by_object_uri(db, config, quote_of_uri).await? else {
        return Ok(None);
    };
    let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(Some("quoted_status_id references unknown status"));
    };
    if !can_view_local_status(db, &status, Some(requester), &owner).await? {
        return Ok(Some("quoted_status_id references unknown status"));
    }
    if status.visibility == Visibility::Direct {
        return Ok(Some("private mentions cannot be quoted"));
    }
    if status.visibility == Visibility::FollowersOnly
        && draft.visibility() != Visibility::FollowersOnly
    {
        return Ok(Some("private posts can only be quoted in private posts"));
    }
    let owner_actor_uri = actor_url(config, owner.username());
    let requester_actor_uri = actor_url(config, requester.username());
    if is_blocking_actor(db, owner.id(), &requester_actor_uri).await?
        || is_blocking_actor(db, requester.id(), &owner_actor_uri).await?
    {
        return Ok(Some("current user is not allowed to quote this status"));
    }

    let is_owner = requester.id() == owner.id();
    let is_follower = if is_owner {
        false
    } else {
        is_local_follower_authorized(db, requester.id(), owner.id()).await?
    };
    if !local_quote_policy_allows(
        effective_local_quote_approval_policy(&status),
        is_owner,
        is_follower,
    ) {
        return Ok(Some("current user is not allowed to quote this status"));
    }

    Ok(None)
}

async fn resolve_quoted_status_uri(
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
    value: Option<&str>,
) -> Result<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    if let Some(status) = find_status_by_id(db, value).await? {
        return Ok(Some(
            status
                .ap_id
                .clone()
                .unwrap_or_else(|| format!("local:{}", status.id)),
        ));
    }
    if let Some(status) = find_remote_status_by_id(db, value).await? {
        return Ok(Some(status.object_uri));
    }
    if let Some(status) = find_local_status_by_object_uri(db, config, value).await? {
        return Ok(Some(
            status
                .ap_id
                .clone()
                .unwrap_or_else(|| format!("local:{}", status.id)),
        ));
    }
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, value).await? {
        return Ok(Some(status.object_uri));
    }

    Ok(None)
}

async fn resolve_create_status_access(
    req: &Request,
    db: &worker::D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<Option<CreateStatusAccess>> {
    if let Some(token) = app_bearer_token_from_request(req)? {
        if let Some(auth) = find_oauth_access_token_with_account_by_bearer_token(db, &token).await?
        {
            if !oauth_access_token_has_any_scope(&auth.token, &["write:statuses", "write"]) {
                return Err(worker::Error::RustError(
                    "status token outside authorized scopes".to_owned(),
                ));
            }
            let Some(account) = auth.account else {
                return Ok(None);
            };
            return Ok(Some(CreateStatusAccess {
                account,
                application_id: Some(auth.token.oauth_app_id),
            }));
        }
        if let Some(app) = find_oauth_app_by_bearer_token(db, &token).await? {
            let Some(user) = extract_authenticated_user(req, config).await? else {
                return Ok(None);
            };
            let account = resolve_local_account(db, config, &user).await?;
            return Ok(Some(CreateStatusAccess {
                account,
                application_id: Some(app.id),
            }));
        }
        return Ok(None);
    }

    let Some(account) = find_authenticated_local_account(req, db, config).await? else {
        return Ok(None);
    };
    Ok(Some(CreateStatusAccess {
        account,
        application_id: None,
    }))
}

pub(crate) async fn create_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let config = config_with_resolved_custom_emojis(&db, &config).await?;
    let parsed = match parse_status_draft(&mut req, &config).await {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };
    let super::ParsedStatusDraft {
        mut draft,
        idempotency_key,
        scheduled_at,
        quoted_status_id,
    } = parsed;
    draft = match sanitize_status_draft(draft, &config) {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };
    let access = match resolve_create_status_access(&req, &db, &config).await {
        Ok(Some(access)) => access,
        Ok(None) => {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "The access token is invalid",
            }))?
            .with_status(401));
        }
        Err(worker::Error::RustError(message))
            if message == "status token outside authorized scopes" =>
        {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "This action is outside the authorized scopes",
            }))?
            .with_status(403));
        }
        Err(error) => return Err(error),
    };
    let pending_media =
        match resolve_attachable_media(&db, &access.account, draft.media_ids()).await {
            Ok(media) => media,
            Err(message) => return Response::error(message, 422),
        };
    let in_reply_to_account_id = match draft.in_reply_to_id() {
        Some(status_id) => match find_local_status_owner_id(&db, status_id).await? {
            Some(account_id) => Some(account_id),
            None => return Response::error("in_reply_to_id references unknown local status", 422),
        },
        None => None,
    };
    let quote_of_uri =
        match resolve_quoted_status_uri(&db, &config, quoted_status_id.as_deref()).await? {
            Some(uri) => Some(uri),
            None if quoted_status_id.is_some() => {
                return Response::error("quoted_status_id references unknown status", 422);
            }
            None => None,
        };
    if let Some(quote_of_uri) = quote_of_uri.as_deref()
        && let Some(message) =
            validate_local_quote_creation(&db, &config, &access.account, &draft, quote_of_uri)
                .await?
    {
        return Response::error(message, 422);
    }
    if let Some(scheduled_at) = scheduled_at.as_deref() {
        if let Err(message) = validate_scheduled_at_minimum_offset(scheduled_at) {
            return Response::error(message, 422);
        }
        return Response::from_json(
            &crate::create_scheduled_status(
                &db,
                &config,
                access.account.id(),
                &draft,
                idempotency_key.as_deref(),
                access.application_id,
                quote_of_uri.as_deref(),
                scheduled_at,
            )
            .await?,
        );
    }

    let response = create_published_status_and_response(
        &db,
        &config,
        Some(&ctx.env),
        CreatePublishedStatusInput {
            account: &access.account,
            application_id: access.application_id,
            draft: &draft,
            pending_media: &pending_media,
            in_reply_to_account_id,
            quote_of_uri: quote_of_uri.as_deref(),
        },
    )
    .await?;
    invalidate_account_dynamic_public_cache(&ctx, access.account.id(), access.account.username())
        .await;
    Response::from_json(&response)
}

pub(crate) async fn delete_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let query: DeleteStatusQuery = req.query().unwrap_or_default();

    let db = ctx.d1(&config.database_binding)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(deleted) = delete_owned_local_status(
        &db,
        &config,
        Some(&ctx.env),
        &requester,
        &status_id,
    )
    .await?
    else {
        return Response::error("status not found", 404);
    };
    invalidate_status_api_cache(&ctx, &deleted.status_id).await;
    invalidate_account_dynamic_public_cache(&ctx, requester.id(), requester.username()).await;
    if query.delete_media.unwrap_or(false) {
        let bucket = ctx.bucket(&config.media_binding)?;
        delete_media_attachments(&db, &bucket, &deleted.media).await?;
    }
    Response::from_json(&deleted.response)
}

pub(crate) async fn update_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let request: UpdateStatusRequest = match parse_update_status_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    let db = ctx.d1(&config.database_binding)?;
    let config = config_with_resolved_custom_emojis(&db, &config).await?;
    let account = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(status) = find_owned_local_status(&db, &status_id, account.id()).await? else {
        return Response::error("status not found", 404);
    };
    let LocalStatusResponsePreload {
        media: current_media,
        in_reply_to_account_id: current_in_reply_to_account_id,
    } = load_local_status_response_preload(&db, &status).await?;

    let next_text = sanitize_emoji_shortcodes(
        &request.status.unwrap_or_else(|| status.text.clone()),
        &config,
    );
    let next_spoiler_text = sanitize_emoji_shortcodes(
        &request
            .spoiler_text
            .unwrap_or_else(|| status.spoiler_text.clone()),
        &config,
    );
    let next_sensitive = request.sensitive.unwrap_or(status.sensitive);
    let next_language = request.language.or(status.language.clone());
    let next_poll = match request.poll {
        Some(poll) => match normalize_status_poll(Some(poll), &config) {
            Ok(poll) => poll,
            Err(message) => return Response::error(message, 422),
        },
        None => None,
    };
    let requested_media = match request.media_ids.as_ref() {
        Some(media_ids) => match resolve_editable_media(&db, &account, &status.id, media_ids).await
        {
            Ok(media) => Some(media),
            Err(message) => return Response::error(message, 422),
        },
        None => None,
    };
    let existing_poll = find_status_poll_by_status_id(&db, &status.id).await?;
    let resulting_media = requested_media.as_ref().unwrap_or(&current_media);
    if next_poll.is_some() && !resulting_media.is_empty() {
        return Response::error("poll and media_ids cannot be used together", 422);
    }
    if next_poll.is_some()
        && let Some(existing_poll) = existing_poll.as_ref()
    {
        let has_votes = crate::count_poll_voters(&db, &existing_poll.id).await? > 0
            || list_status_poll_options(&db, &existing_poll.id)
                .await?
                .into_iter()
                .any(|option| option.votes_count > 0);
        let expired = crate::is_iso_timestamp_in_past(&existing_poll.expires_at).unwrap_or(false);
        if has_votes || expired {
            return Response::error(
                "cannot edit a poll after voting has started or it has expired",
                422,
            );
        }
    }
    let changed = next_text != status.text
        || next_spoiler_text != status.spoiler_text
        || next_sensitive != (status.sensitive)
        || next_language != status.language
        || next_poll.is_some()
        || requested_media.is_some()
        || request
            .media_attributes
            .as_ref()
            .is_some_and(|attributes| !attributes.is_empty());
    if next_text.is_empty()
        && resulting_media.is_empty()
        && existing_poll.is_none()
        && next_poll.is_none()
    {
        return Response::error("status must include text, media_ids, or poll", 422);
    }
    if !changed {
        let response = build_local_status_response(
            &db,
            &config,
            Some(&account),
            &status,
            &account,
            current_in_reply_to_account_id,
            current_media,
        )
        .await?;
        return Response::from_json(&response);
    }

    let updated = match apply_local_status_update(
        &db,
        &config,
        UpdateLocalStatusInput {
            account: &account,
            status: &status,
            current_media: current_media.clone(),
            current_in_reply_to_account_id,
            next_text: &next_text,
            next_spoiler_text: &next_spoiler_text,
            next_sensitive,
            next_language: next_language.as_deref(),
            next_poll: next_poll.as_ref(),
            requested_media: requested_media.as_deref(),
            media_attributes: request.media_attributes.as_deref(),
        },
    )
    .await
    {
        Ok(updated) => updated,
        Err(worker::Error::RustError(message))
            if message.starts_with("unknown media attachment in media_attributes:") =>
        {
            return Response::error(message, 422);
        }
        Err(error) => return Err(error),
    };
    invalidate_status_api_cache(&ctx, &updated.status_id).await;
    Response::from_json(&updated.response)
}
