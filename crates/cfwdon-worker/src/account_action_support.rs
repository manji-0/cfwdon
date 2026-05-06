use super::{
    AppConfig, D1Database, Error, FollowAccountRequest, FormEntry, LocalAccount, Result, actor_url,
    parse_optional_bool, send_push_notification,
};
use worker::Request;
use worker::d1::D1Type;

#[derive(Debug, Default, serde::Deserialize)]
pub(crate) struct MuteAccountRequest {
    pub(crate) notifications: Option<bool>,
    pub(crate) duration: Option<u32>,
}

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
    follower: &LocalAccount,
    target: &LocalAccount,
    request: &FollowAccountRequest,
) -> Result<()> {
    let target_actor_uri = actor_url(config, &target.username);
    let notification_type = if target.locked {
        "follow_request"
    } else {
        "follow"
    };
    let languages_json = request
        .languages
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            Error::RustError(format!("failed to serialize follow languages: {error}"))
        })?;
    let state = if target.locked { "pending" } else { "accepted" };
    let bindings = [
        D1Type::Text(follower.id.as_str()),
        D1Type::Text(target.id.as_str()),
        D1Type::Text(target_actor_uri.as_str()),
        D1Type::Text(state),
        D1Type::Integer(if request.reblogs.unwrap_or(true) {
            1
        } else {
            0
        }),
        D1Type::Integer(if request.notify.unwrap_or(false) {
            1
        } else {
            0
        }),
        match languages_json.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO follows (
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
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let _ = send_push_notification(
        db,
        config,
        &target.id,
        notification_type,
        serde_json::json!({
            "follower_account_id": follower.id,
            "target_account_id": target.id,
            "state": state,
        }),
    )
    .await;

    Ok(())
}
