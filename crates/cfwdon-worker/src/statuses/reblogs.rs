use super::{
    build_local_action_status_response, build_remote_status_response, delete_reblog_by_target_uri,
    delete_reblog_wrapper_status_by_target_uri, find_reblog_activity_by_target_uri,
    local_status_target_uri, resolve_authenticated_status_action_context,
    resolve_visible_action_status, upsert_reblog_local_status, upsert_reblog_remote_status,
    upsert_reblog_wrapper_status,
};
use crate::delivery::{enqueue_announce_activity, enqueue_undo_announce_activity};
use crate::response_cache::invalidate_status_api_cache;
use crate::statuses::{AuthenticatedStatusActionContextResolution, ResolvedVisibleActionStatus};
use cfwdon_domain::Visibility;
use serde::Deserialize;
use worker::{Error, Request, Response, Result, RouteContext};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ReblogStatusRequest {
    pub(crate) visibility: Option<String>,
}

/// Mastodon's `StatusPolicy#reblog?` plus `ReblogService`'s visibility pick:
/// direct posts never, followers-only posts only by their author (and the
/// boost keeps that visibility), otherwise the requested visibility or the
/// booster's default. `None` means the boost is not permitted.
fn reblog_visibility(
    target: Visibility,
    owned: bool,
    requested: Option<&str>,
    default: Visibility,
) -> Option<String> {
    match target {
        Visibility::Direct => None,
        Visibility::FollowersOnly if !owned => None,
        Visibility::FollowersOnly => Some(target.as_str().to_owned()),
        Visibility::Public | Visibility::Unlisted => Some(
            requested
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|value| Visibility::parse(value).ok())
                .unwrap_or(default)
                .as_str()
                .to_owned(),
        ),
    }
}

fn reblog_not_permitted_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is not allowed",
    }))?
    .with_status(403))
}

pub(crate) async fn reblog_status(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let action = match resolve_authenticated_status_action_context(&*req, &ctx).await? {
        AuthenticatedStatusActionContextResolution::Ready(action) => action,
        AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        AuthenticatedStatusActionContextResolution::Unauthenticated => {
            return Response::error("Auth0 authentication required", 401);
        }
    };
    let request = parse_reblog_status_request(req)
        .await
        .map_err(Error::RustError)?;
    let viewer = &action.auth.viewer;

    match resolve_visible_action_status(
        &action.auth.db,
        &action.auth.config,
        viewer,
        &action.status_id,
        action.action_uri.as_deref(),
    )
    .await?
    {
        Some(ResolvedVisibleActionStatus::Local(subject)) => {
            let Some(visibility) = reblog_visibility(
                subject.status.visibility,
                viewer.id() == subject.account.id(),
                request.visibility.as_deref(),
                viewer.default_visibility(),
            ) else {
                return reblog_not_permitted_response();
            };
            let wrapper = upsert_reblog_wrapper_status(
                &action.auth.db,
                &action.auth.config,
                viewer,
                &local_status_target_uri(&subject.status),
                &visibility,
            )
            .await?;
            let existing = find_reblog_activity_by_target_uri(
                &action.auth.db,
                viewer.id(),
                &local_status_target_uri(&subject.status),
            )
            .await?;
            let mut outbound_activity_id = existing.and_then(|row| row.ap_activity_id);
            if outbound_activity_id.is_none() {
                outbound_activity_id = enqueue_announce_activity(
                    &action.auth.db,
                    &action.auth.config,
                    viewer,
                    &wrapper.id,
                    &local_status_target_uri(&subject.status),
                    &visibility,
                    None,
                )
                .await?;
            }
            upsert_reblog_local_status(
                &action.auth.db,
                &action.auth.config,
                Some(&ctx.env),
                viewer,
                &subject.status,
                &visibility,
                outbound_activity_id.as_deref(),
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            // The wrapper was just written by this request and has no media or reply.
            let wrapper_subject = super::LoadedLocalStatusResponseSubject {
                status: wrapper,
                account: viewer.clone(),
                preload: super::LocalStatusResponsePreload {
                    media: Vec::new(),
                    in_reply_to_account_id: None,
                },
            };
            let response = build_local_action_status_response(
                &action.auth.db,
                &action.auth.config,
                viewer,
                wrapper_subject,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(ResolvedVisibleActionStatus::Remote(status, actor)) => {
            let Some(visibility) = reblog_visibility(
                status.visibility,
                false,
                request.visibility.as_deref(),
                viewer.default_visibility(),
            ) else {
                return reblog_not_permitted_response();
            };
            let existing = find_reblog_activity_by_target_uri(
                &action.auth.db,
                viewer.id(),
                &status.object_uri,
            )
            .await?;
            let mut outbound_activity_id = existing.and_then(|row| row.ap_activity_id);
            let wrapper = upsert_reblog_wrapper_status(
                &action.auth.db,
                &action.auth.config,
                viewer,
                &status.object_uri,
                &visibility,
            )
            .await?;
            if outbound_activity_id.is_none() {
                outbound_activity_id = enqueue_announce_activity(
                    &action.auth.db,
                    &action.auth.config,
                    viewer,
                    &wrapper.id,
                    &status.object_uri,
                    &visibility,
                    Some(actor.actor_uri.as_str()),
                )
                .await?;
            }
            upsert_reblog_remote_status(
                &action.auth.db,
                viewer.id(),
                &status,
                &visibility,
                outbound_activity_id.as_deref(),
            )
            .await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            // The wrapper was just written by this request and has no media or reply.
            let wrapper_subject = super::LoadedLocalStatusResponseSubject {
                status: wrapper,
                account: viewer.clone(),
                preload: super::LocalStatusResponsePreload {
                    media: Vec::new(),
                    in_reply_to_account_id: None,
                },
            };
            let response = build_local_action_status_response(
                &action.auth.db,
                &action.auth.config,
                viewer,
                wrapper_subject,
            )
            .await?;
            Response::from_json(&response)
        }
        None => Response::error("status not found", 404),
    }
}

pub(crate) async fn unreblog_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let action = match resolve_authenticated_status_action_context(&req, &ctx).await? {
        AuthenticatedStatusActionContextResolution::Ready(action) => action,
        AuthenticatedStatusActionContextResolution::MissingStatusId => {
            return Response::error("missing status id route parameter", 400);
        }
        AuthenticatedStatusActionContextResolution::Unauthenticated => {
            return Response::error("Auth0 authentication required", 401);
        }
    };
    let viewer = &action.auth.viewer;

    match resolve_visible_action_status(
        &action.auth.db,
        &action.auth.config,
        viewer,
        &action.status_id,
        action.action_uri.as_deref(),
    )
    .await?
    {
        Some(ResolvedVisibleActionStatus::Local(subject)) => {
            let target_uri = local_status_target_uri(&subject.status);
            if let Some(row) =
                find_reblog_activity_by_target_uri(&action.auth.db, viewer.id(), &target_uri)
                    .await?
                && let Some(announce_activity_id) = row.ap_activity_id.as_deref()
            {
                enqueue_undo_announce_activity(
                    &action.auth.db,
                    &action.auth.config,
                    viewer,
                    &subject.status.id,
                    announce_activity_id,
                    &target_uri,
                    &row.visibility,
                    None,
                )
                .await?;
            }
            delete_reblog_by_target_uri(&action.auth.db, viewer.id(), &target_uri).await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            delete_reblog_wrapper_status_by_target_uri(&action.auth.db, viewer.id(), &target_uri)
                .await?;
            let response = build_local_action_status_response(
                &action.auth.db,
                &action.auth.config,
                viewer,
                subject,
            )
            .await?;
            Response::from_json(&response)
        }
        Some(ResolvedVisibleActionStatus::Remote(status, actor)) => {
            if let Some(row) =
                find_reblog_activity_by_target_uri(&action.auth.db, viewer.id(), &status.object_uri)
                    .await?
                && let Some(announce_activity_id) = row.ap_activity_id.as_deref()
            {
                enqueue_undo_announce_activity(
                    &action.auth.db,
                    &action.auth.config,
                    viewer,
                    &status.id,
                    announce_activity_id,
                    &status.object_uri,
                    &row.visibility,
                    Some(actor.actor_uri.as_str()),
                )
                .await?;
            }
            delete_reblog_by_target_uri(&action.auth.db, viewer.id(), &status.object_uri).await?;
            invalidate_status_api_cache(&ctx, &action.status_id).await;
            delete_reblog_wrapper_status_by_target_uri(
                &action.auth.db,
                viewer.id(),
                &status.object_uri,
            )
            .await?;
            let response = build_remote_status_response(
                &action.auth.db,
                &action.auth.config,
                Some(viewer),
                &status,
                &actor,
            )
            .await?;
            Response::from_json(&response)
        }
        None => Response::error("status not found", 404),
    }
}

async fn parse_reblog_status_request(
    req: &mut Request,
) -> std::result::Result<ReblogStatusRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.trim().is_empty() {
        ReblogStatusRequest::default()
    } else if content_type.contains("application/json") {
        req.json::<ReblogStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON reblog payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form reblog payload: {error}"))?;
        ReblogStatusRequest {
            visibility: form.get_field("visibility"),
        }
    };

    if let Some(visibility) = request.visibility.as_mut() {
        *visibility = visibility.trim().to_ascii_lowercase();
        if visibility.is_empty() {
            request.visibility = None;
        } else if super::Visibility::parse(visibility).is_err() {
            return Err("visibility must be one of: public, unlisted, private, direct".to_owned());
        }
    }

    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::reblog_visibility;
    use cfwdon_domain::Visibility;

    #[test]
    fn reblog_visibility_follows_mastodon_policy() {
        assert_eq!(
            reblog_visibility(Visibility::Direct, true, None, Visibility::Public),
            None
        );
        assert_eq!(
            reblog_visibility(Visibility::FollowersOnly, false, None, Visibility::Public),
            None
        );
        assert_eq!(
            reblog_visibility(
                Visibility::FollowersOnly,
                true,
                Some("public"),
                Visibility::Public
            )
            .as_deref(),
            Some("private")
        );
        assert_eq!(
            reblog_visibility(Visibility::Public, true, None, Visibility::Unlisted).as_deref(),
            Some("unlisted")
        );
        assert_eq!(
            reblog_visibility(
                Visibility::Public,
                false,
                Some("unlisted"),
                Visibility::Public
            )
            .as_deref(),
            Some("unlisted")
        );
    }
}
