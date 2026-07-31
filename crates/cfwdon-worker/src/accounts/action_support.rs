use crate::{
    AccountReference, AppConfig, D1Database, Error, FollowAccountRequest, FormEntry, LocalAccount,
    Request, ResolvedRelationshipTarget, Response, Result, RouteContext, SocialActionError,
    actor_url, now_iso_string, parse_optional_bool, publish_local_actor_notification_soft,
    require_authenticated_local_account, resolve_account_reference, send_push_notification,
};
use cfwdon_domain::{LocalFollowState, initial_local_follow_state, local_follow_notification_type};
use worker::{Env, d1::D1Type};

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct MuteAccountRequest {
    pub(crate) notifications: Option<bool>,
    pub(crate) duration: Option<u32>,
}

#[derive(Debug)]
struct LocalFollowUpsertDraft {
    follower_account_id: String,
    target_account_id: String,
    target_actor_uri: String,
    state: LocalFollowState,
    show_reblogs: bool,
    notify: bool,
    languages_json: Option<String>,
}

impl LocalFollowUpsertDraft {
    fn new(
        follower: &LocalAccount,
        target: &LocalAccount,
        target_actor_uri: String,
        request: &FollowAccountRequest,
    ) -> Result<Self> {
        Ok(Self {
            follower_account_id: follower.id().to_owned(),
            target_account_id: target.id().to_owned(),
            target_actor_uri,
            state: initial_local_follow_state(target.is_locked()),
            show_reblogs: request.reblogs.unwrap_or(true),
            notify: request.notify.unwrap_or(false),
            languages_json: serialize_follow_languages(request)?,
        })
    }
}

const LOCAL_FOLLOW_UPSERT_SQL: &str = "INSERT INTO follows (
            id,
            follower_account_id,
            target_account_id,
            target_actor_uri,
            target_inbox_uri,
            target_shared_inbox_uri,
            follow_activity_id,
            state,
            show_reblogs,
            notify,
            languages_json,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            NULL,
            NULL,
            NULL,
            ?4,
            ?5,
            ?6,
            ?7,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(follower_account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            state = excluded.state,
            show_reblogs = excluded.show_reblogs,
            notify = excluded.notify,
            languages_json = excluded.languages_json,
            updated_at = CURRENT_TIMESTAMP";

pub(crate) async fn parse_mute_account_request(
    req: &mut Request,
) -> std::result::Result<MuteAccountRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.trim().is_empty() {
        return Ok(MuteAccountRequest::default());
    }

    if content_type.contains("application/json") {
        return req
            .json::<MuteAccountRequest>()
            .await
            .map_err(|error| format!("invalid JSON mute payload: {error}"));
    }

    let form = req
        .form_data()
        .await
        .map_err(|error| format!("invalid form mute payload: {error}"))?;
    Ok(MuteAccountRequest {
        notifications: parse_optional_bool(form.get_field("notifications").as_deref())?,
        duration: form
            .get_field("duration")
            .and_then(|value| value.trim().parse::<u32>().ok()),
    })
}

pub(crate) async fn parse_follow_account_request(
    req: &mut Request,
) -> std::result::Result<FollowAccountRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.trim().is_empty() {
        return Ok(FollowAccountRequest::default());
    }

    let mut request = if content_type.contains("application/json") {
        req.json::<FollowAccountRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON follow payload: {error}")))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| Error::RustError(format!("invalid form follow payload: {error}")))?;
        FollowAccountRequest {
            reblogs: parse_optional_bool(form.get_field("reblogs").as_deref())
                .map_err(Error::RustError)?,
            notify: parse_optional_bool(form.get_field("notify").as_deref())
                .map_err(Error::RustError)?,
            languages: form.get_all("languages[]").map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|entry| match entry {
                        FormEntry::Field(value) => {
                            let value = value.trim().to_ascii_lowercase();
                            (!value.is_empty()).then_some(value)
                        }
                        FormEntry::File(_) => None,
                    })
                    .collect()
            }),
        }
    };

    if let Some(languages) = request.languages.as_mut() {
        languages.sort();
        languages.dedup();
        if languages.is_empty() {
            request.languages = None;
        }
    }

    Ok(request)
}

pub(crate) async fn upsert_local_follow(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    follower: &LocalAccount,
    target: &LocalAccount,
    request: &FollowAccountRequest,
) -> Result<()> {
    let target_actor_uri = actor_url(config, target.username());
    let draft = LocalFollowUpsertDraft::new(follower, target, target_actor_uri, request)?;
    upsert_local_follow_row(db, &draft).await?;

    let notification_type = local_follow_notification_type(draft.state);
    let _ = send_push_notification(
        db,
        config,
        &draft.target_account_id,
        notification_type,
        local_follow_notification_payload(&draft),
    )
    .await;

    if let Ok(created_at) = now_iso_string() {
        let id = local_follow_stream_notification_id(notification_type, &draft.follower_account_id);
        publish_local_actor_notification_soft(
            env,
            db,
            config,
            &draft.target_account_id,
            follower,
            notification_type,
            id.clone(),
            id,
            created_at,
            None,
        )
        .await;
    }

    Ok(())
}

async fn upsert_local_follow_row(db: &D1Database, draft: &LocalFollowUpsertDraft) -> Result<()> {
    let bindings = local_follow_upsert_bindings(draft);
    db.prepare(LOCAL_FOLLOW_UPSERT_SQL)
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    Ok(())
}

fn serialize_follow_languages(request: &FollowAccountRequest) -> Result<Option<String>> {
    request
        .languages
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| Error::RustError(format!("failed to serialize follow languages: {error}")))
}

fn bool_d1(value: bool) -> D1Type<'static> {
    D1Type::Integer(if value { 1 } else { 0 })
}

pub(crate) struct ResolvedSocialActionContext {
    pub(crate) db: D1Database,
    pub(crate) config: AppConfig,
    pub(crate) viewer: LocalAccount,
    pub(crate) target_account_id: Option<String>,
    pub(crate) target_id: String,
    pub(crate) target_actor_uri: String,
}

impl ResolvedSocialActionContext {
    pub(crate) fn relationship_target(&self) -> ResolvedRelationshipTarget<'_> {
        ResolvedRelationshipTarget {
            viewer: &self.viewer,
            target_account_id: self.target_account_id.as_deref(),
            target_id: &self.target_id,
            target_actor_uri: &self.target_actor_uri,
        }
    }
}

pub(crate) enum SocialActionContextError {
    NotFound,
    Worker(Error),
}

impl From<Error> for SocialActionContextError {
    fn from(error: Error) -> Self {
        Self::Worker(error)
    }
}

pub(crate) async fn resolve_social_action_context(
    req: &Request,
    ctx: &RouteContext<()>,
) -> std::result::Result<Option<ResolvedSocialActionContext>, SocialActionContextError> {
    let config = crate::load_config(ctx);
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Ok(None),
    };
    let (target_account_id, target_id, target_actor_uri) =
        match resolve_account_reference(&db, &target_account_id).await? {
            Some(AccountReference::Local(target)) => (
                Some(target.id().to_owned()),
                target.id().to_owned(),
                actor_url(&config, target.username()),
            ),
            Some(AccountReference::Remote(actor)) => (
                None,
                crate::remote_account_rest_id(&actor.actor_uri),
                actor.actor_uri,
            ),
            None => return Err(SocialActionContextError::NotFound),
        };
    Ok(Some(ResolvedSocialActionContext {
        db,
        config,
        viewer,
        target_account_id,
        target_id,
        target_actor_uri,
    }))
}

pub(crate) fn social_action_usecase_response<T: serde::Serialize>(
    result: std::result::Result<T, SocialActionError>,
) -> Result<Response> {
    match result {
        Ok(value) => Response::from_json(&value),
        Err(SocialActionError::FollowRequired) => Response::error(
            "Validation failed: You must be already following the person you want to endorse",
            422,
        ),
        Err(SocialActionError::Worker(error)) => Err(error),
    }
}

fn local_follow_upsert_bindings(draft: &LocalFollowUpsertDraft) -> [D1Type<'_>; 7] {
    [
        D1Type::Text(draft.follower_account_id.as_str()),
        D1Type::Text(draft.target_account_id.as_str()),
        D1Type::Text(draft.target_actor_uri.as_str()),
        D1Type::Text(draft.state.as_str()),
        bool_d1(draft.show_reblogs),
        bool_d1(draft.notify),
        draft
            .languages_json
            .as_deref()
            .map_or(D1Type::Null, D1Type::Text),
    ]
}

fn local_follow_stream_notification_id(
    notification_type: &str,
    follower_account_id: &str,
) -> String {
    match notification_type {
        "follow_request" => format!("follow-request-local-{}", follower_account_id),
        _ => format!("follow-local-{}", follower_account_id),
    }
}

fn local_follow_notification_payload(draft: &LocalFollowUpsertDraft) -> serde_json::Value {
    serde_json::json!({
        "follower_account_id": draft.follower_account_id,
        "target_account_id": draft.target_account_id,
        "state": draft.state.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::LocalAccountRecord;

    fn local_account(id: &str, username: &str, locked: bool) -> LocalAccount {
        let mut record = LocalAccountRecord::test_fixture(id, username);
        record.locked = i32::from(locked);
        LocalAccount::from_record(record)
    }

    #[test]
    fn local_follow_upsert_draft_uses_defaults_for_unlocked_target() {
        let follower = local_account("viewer", "alice", false);
        let target = local_account("target", "bob", false);
        let draft = LocalFollowUpsertDraft::new(
            &follower,
            &target,
            "https://local.example/users/bob".to_owned(),
            &FollowAccountRequest::default(),
        )
        .expect("draft");

        assert_eq!(draft.follower_account_id, "viewer");
        assert_eq!(draft.target_account_id, "target");
        assert_eq!(draft.target_actor_uri, "https://local.example/users/bob");
        assert_eq!(draft.state, LocalFollowState::Accepted);
        assert_eq!(local_follow_notification_type(draft.state), "follow");
        assert!(draft.show_reblogs);
        assert!(!draft.notify);
        assert_eq!(draft.languages_json, None);
    }

    #[test]
    fn local_follow_upsert_draft_maps_request_and_locked_target() {
        let follower = local_account("viewer", "alice", false);
        let target = local_account("target", "bob", true);
        let request = FollowAccountRequest {
            reblogs: Some(false),
            notify: Some(true),
            languages: Some(vec!["en".to_owned(), "ja".to_owned()]),
        };
        let draft = LocalFollowUpsertDraft::new(
            &follower,
            &target,
            "https://local.example/users/bob".to_owned(),
            &request,
        )
        .expect("draft");

        assert_eq!(draft.state, LocalFollowState::Pending);
        assert_eq!(
            local_follow_notification_type(draft.state),
            "follow_request"
        );
        assert!(!draft.show_reblogs);
        assert!(draft.notify);
        assert_eq!(draft.languages_json, Some("[\"en\",\"ja\"]".to_owned()));
    }

    #[test]
    fn local_follow_upsert_bindings_keep_sql_slot_order_stable() {
        let draft = LocalFollowUpsertDraft {
            follower_account_id: "viewer".to_owned(),
            target_account_id: "target".to_owned(),
            target_actor_uri: "https://local.example/users/bob".to_owned(),
            state: LocalFollowState::Pending,
            show_reblogs: false,
            notify: true,
            languages_json: Some("[\"ja\"]".to_owned()),
        };
        let bindings = local_follow_upsert_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("viewer")));
        assert!(matches!(bindings[1], D1Type::Text("target")));
        assert!(matches!(
            bindings[2],
            D1Type::Text("https://local.example/users/bob")
        ));
        assert!(matches!(bindings[3], D1Type::Text("pending")));
        assert!(matches!(bindings[4], D1Type::Integer(0)));
        assert!(matches!(bindings[5], D1Type::Integer(1)));
        assert!(matches!(bindings[6], D1Type::Text("[\"ja\"]")));
    }

    #[test]
    fn local_follow_stream_notification_id_matches_collectors() {
        assert_eq!(
            local_follow_stream_notification_id("follow", "viewer"),
            "follow-local-viewer"
        );
        assert_eq!(
            local_follow_stream_notification_id("follow_request", "viewer"),
            "follow-request-local-viewer"
        );
    }

    #[test]
    fn local_follow_notification_payload_matches_stored_relationship() {
        let draft = LocalFollowUpsertDraft {
            follower_account_id: "viewer".to_owned(),
            target_account_id: "target".to_owned(),
            target_actor_uri: "https://local.example/users/bob".to_owned(),
            state: LocalFollowState::Accepted,
            show_reblogs: true,
            notify: false,
            languages_json: None,
        };

        assert_eq!(
            local_follow_notification_payload(&draft),
            serde_json::json!({
                "follower_account_id": "viewer",
                "target_account_id": "target",
                "state": "accepted",
            })
        );
    }
}
