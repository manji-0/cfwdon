use crate::{
    AppConfig, Error, MastodonStatusResponse, Result, StatusRow, build_local_status_response,
    find_account_by_id, find_local_status_by_object_uri, find_media_attachments_by_status_id,
    find_status_by_id, find_status_poll_by_id, load_in_reply_to_account_id, load_push_subscription,
    local_status_target_uri, notification_timestamp_sort_token,
    publish_local_actor_notification_soft, push_subscription_alert_enabled,
};
use cfwdon_domain::LocalAccount;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use js_sys::Uint8Array;
use serde::Deserialize;
use serde_json::json;
use wasm_bindgen::JsValue;
use web_push_native::{
    Auth, WebPushBuilder, jwt_simple::algorithms::ES256KeyPair, p256::PublicKey,
};
use worker::{D1Database, Env, Fetch, Headers, Method, Request, RequestInit};

#[derive(Debug, Deserialize)]
struct AccountIdRow {
    account_id: String,
}

async fn load_account_ids(
    db: &D1Database,
    sql: &str,
    bindings: &[worker::d1::D1Type<'_>],
) -> Result<Vec<String>> {
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;
    Ok(result
        .results::<AccountIdRow>()?
        .into_iter()
        .map(|row| row.account_id)
        .collect())
}

async fn build_local_status_response_for_recipient_soft(
    db: &D1Database,
    config: &AppConfig,
    recipient_account_id: &str,
    status: &StatusRow,
    author: &LocalAccount,
) -> Option<MastodonStatusResponse> {
    let recipient = find_account_by_id(db, recipient_account_id)
        .await
        .ok()
        .flatten()?;
    let media = find_media_attachments_by_status_id(db, &status.id)
        .await
        .ok()?;
    let in_reply_to = load_in_reply_to_account_id(db, status).await.ok()?;
    build_local_status_response(
        db,
        config,
        Some(&recipient),
        status,
        author,
        in_reply_to,
        media,
    )
    .await
    .ok()
}

async fn publish_poll_end_stream_notifications_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    poll_id: &str,
    status_id: &str,
    account_id: &str,
    recipient_account_ids: &[String],
) {
    if env.is_none() {
        return;
    }

    let Ok(Some(actor)) = find_account_by_id(db, account_id).await else {
        return;
    };
    let Ok(Some(status)) = find_status_by_id(db, status_id).await else {
        return;
    };
    let expires_at = find_status_poll_by_id(db, poll_id)
        .await
        .ok()
        .flatten()
        .map(|poll| poll.expires_at)
        .unwrap_or_else(|| status.created_at.clone());
    let id = format!("poll-local-{}", poll_id);
    let mut sent = std::collections::HashSet::new();

    for recipient_account_id in recipient_account_ids {
        if !sent.insert(recipient_account_id.clone()) {
            continue;
        }
        let status_response = build_local_status_response_for_recipient_soft(
            db,
            config,
            recipient_account_id,
            &status,
            &actor,
        )
        .await;
        publish_local_actor_notification_soft(
            env,
            db,
            config,
            recipient_account_id,
            &actor,
            "poll",
            id.clone(),
            id.clone(),
            expires_at.clone(),
            status_response,
        )
        .await;
    }
}

async fn publish_local_status_update_stream_notifications_soft(
    env: Option<&Env>,
    db: &D1Database,
    config: &AppConfig,
    status: &StatusRow,
    reblog_recipient_ids: &[String],
    quote_recipient_ids: &[String],
) {
    if env.is_none() {
        return;
    }

    let Ok(Some(actor)) = find_account_by_id(db, &status.account_id).await else {
        return;
    };
    let updated_at = status.updated_at.as_deref().unwrap_or(&status.created_at);
    let update_token = notification_timestamp_sort_token(updated_at)
        .unwrap_or_else(|| updated_at.replace([':', ' '], "-"));
    let update_id =
        format!("update-local-{}-{}-{}", actor.id(), status.id, update_token);
    let quoted_update_id =
        format!("quoted-update-local-{}-{}-{}", actor.id(), status.id, update_token);
    let mut sent = std::collections::HashSet::new();

    for recipient_account_id in reblog_recipient_ids {
        if !sent.insert(format!("update:{recipient_account_id}")) {
            continue;
        }
        let status_response = build_local_status_response_for_recipient_soft(
            db,
            config,
            recipient_account_id,
            status,
            &actor,
        )
        .await;
        publish_local_actor_notification_soft(
            env,
            db,
            config,
            recipient_account_id,
            &actor,
            "update",
            update_id.clone(),
            update_id.clone(),
            updated_at.to_owned(),
            status_response,
        )
        .await;
    }

    for recipient_account_id in quote_recipient_ids {
        if !sent.insert(format!("quoted_update:{recipient_account_id}")) {
            continue;
        }
        let status_response = build_local_status_response_for_recipient_soft(
            db,
            config,
            recipient_account_id,
            status,
            &actor,
        )
        .await;
        publish_local_actor_notification_soft(
            env,
            db,
            config,
            recipient_account_id,
            &actor,
            "quoted_update",
            quoted_update_id.clone(),
            quoted_update_id.clone(),
            updated_at.to_owned(),
            status_response,
        )
        .await;
    }
}

async fn send_push_notifications_to_accounts(
    db: &D1Database,
    config: &AppConfig,
    account_ids: Vec<String>,
    notification_type: &str,
    details: serde_json::Value,
) -> Result<()> {
    let mut sent = std::collections::HashSet::new();
    for account_id in account_ids {
        if !sent.insert(account_id.clone()) {
            continue;
        }
        let _ = send_push_notification(db, config, &account_id, notification_type, details.clone())
            .await;
    }
    Ok(())
}

fn vapid_subject(config: &AppConfig) -> String {
    config.web_push_vapid_subject.clone().unwrap_or_else(|| {
        config
            .contact_email
            .as_ref()
            .map(|value| format!("mailto:{value}"))
            .unwrap_or_else(|| format!("mailto:admin@{}", config.instance_domain))
    })
}

fn decode_urlsafe_bytes(value: &str, field: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .map_err(|error| Error::RustError(format!("invalid base64url {field}: {error}")))
}

fn notification_payload(
    notification_type: &str,
    account_id: &str,
    details: serde_json::Value,
) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "type": notification_type,
        "account_id": account_id,
        "details": details,
    }))
    .unwrap_or_default()
}

async fn build_push_request(
    config: &AppConfig,
    endpoint: &str,
    p256dh_key: &str,
    auth_key: &str,
    payload: Vec<u8>,
) -> Result<(String, Headers, Vec<u8>)> {
    let private_key = config
        .web_push_vapid_private_key
        .as_deref()
        .ok_or_else(|| Error::RustError("missing WEB_PUSH_VAPID_PRIVATE_KEY".to_owned()))?;
    let key_pair =
        ES256KeyPair::from_bytes(&decode_urlsafe_bytes(private_key, "VAPID private key")?)
            .map_err(|error| {
                Error::RustError(format!("failed to load VAPID private key: {error}"))
            })?;
    let subject = vapid_subject(config);
    let builder = WebPushBuilder::new(
        endpoint
            .parse()
            .map_err(|error| Error::RustError(format!("invalid push endpoint URL: {error}")))?,
        PublicKey::from_sec1_bytes(&decode_urlsafe_bytes(
            p256dh_key,
            "subscription public key",
        )?)
        .map_err(|error| Error::RustError(format!("failed to load push public key: {error}")))?,
        Auth::clone_from_slice(&decode_urlsafe_bytes(auth_key, "subscription auth secret")?),
    )
    .with_vapid(&key_pair, &subject);

    let http_request = builder
        .build(payload)
        .map_err(|error| Error::RustError(format!("failed to build push request: {error}")))?;

    let headers = Headers::new();
    for (name, value) in http_request.headers() {
        headers.set(
            name.as_str(),
            value
                .to_str()
                .map_err(|error| Error::RustError(format!("invalid push header value: {error}")))?,
        )?;
    }

    let body = http_request.body().to_vec();
    Ok((http_request.uri().to_string(), headers, body))
}

async fn send_push_request(endpoint: String, headers: Headers, body: Vec<u8>) -> Result<()> {
    let mut init = RequestInit::new();
    init.with_method(Method::Post).with_headers(headers);
    init.with_body(Some(JsValue::from(Uint8Array::from(body.as_slice()))));

    let request = Request::new_with_init(&endpoint, &init)?;
    let response = Fetch::Request(request).send().await?;
    let status = response.status_code();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(Error::RustError(format!(
            "push endpoint rejected request with HTTP {status}"
        )))
    }
}

pub(crate) async fn send_push_notification(
    db: &D1Database,
    config: &AppConfig,
    account_id: &str,
    notification_type: &str,
    details: serde_json::Value,
) -> Result<()> {
    let Some(subscription) = load_push_subscription(db, account_id).await? else {
        return Ok(());
    };
    if !push_subscription_alert_enabled(&subscription, notification_type) {
        return Ok(());
    }

    let payload = notification_payload(notification_type, account_id, details);
    let (endpoint, headers, body) = build_push_request(
        config,
        &subscription.endpoint,
        &subscription.p256dh_key,
        &subscription.auth_key,
        payload,
    )
    .await?;

    send_push_request(endpoint, headers, body).await
}

pub(crate) async fn send_status_quote_notification(
    db: &D1Database,
    config: &AppConfig,
    quote_status: &StatusRow,
) -> Result<()> {
    let Some(quote_of_uri) = quote_status.quote_of_uri.as_deref() else {
        return Ok(());
    };
    if quote_status.quote_state != cfwdon_domain::QuoteState::Accepted {
        return Ok(());
    }
    let Some(target) = find_local_status_by_object_uri(db, config, quote_of_uri).await? else {
        return Ok(());
    };
    if target.account_id == quote_status.account_id {
        return Ok(());
    }
    send_push_notifications_to_accounts(
        db,
        config,
        vec![target.account_id],
        "quote",
        json!({
            "account_id": quote_status.account_id,
            "status_id": quote_status.id,
            "quoted_status_id": target.id,
        }),
    )
    .await
}

pub(crate) async fn send_remote_status_quote_notification(
    db: &D1Database,
    config: &AppConfig,
    quote_status_id: &str,
    quote_author_id: &str,
    quote_state: &str,
    quote_of_uri: Option<&str>,
) -> Result<()> {
    let Some(quote_of_uri) = quote_of_uri else {
        return Ok(());
    };
    if quote_state != "accepted" {
        return Ok(());
    }
    let Some(target) = find_local_status_by_object_uri(db, config, quote_of_uri).await? else {
        return Ok(());
    };
    if target.account_id == quote_author_id {
        return Ok(());
    }
    send_push_notifications_to_accounts(
        db,
        config,
        vec![target.account_id],
        "quote",
        json!({
            "account_id": quote_author_id,
            "status_id": quote_status_id,
            "quoted_status_id": target.id,
        }),
    )
    .await
}

pub(crate) async fn send_status_update_notifications(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    status: &StatusRow,
) -> Result<()> {
    let bindings = [worker::d1::D1Type::Text(status.id.as_str())];
    let reblog_accounts = load_account_ids(
        db,
        "SELECT DISTINCT account_id
         FROM reblogs
         WHERE status_id = ?1",
        &bindings,
    )
    .await?;
    let _ = send_push_notifications_to_accounts(
        db,
        config,
        reblog_accounts.clone(),
        "update",
        json!({
            "account_id": status.account_id,
            "status_id": status.id,
        }),
    )
    .await;

    let target_uri = local_status_target_uri(status);
    let quote_bindings = [
        worker::d1::D1Type::Text(target_uri.as_str()),
        worker::d1::D1Type::Text(status.account_id.as_str()),
    ];
    let quote_recipients = load_account_ids(
        db,
        "SELECT DISTINCT account_id
         FROM statuses
         WHERE quote_of_uri = ?1
           AND quote_state != 'revoked'
           AND account_id != ?2",
        &quote_bindings,
    )
    .await?;
    let _ = send_push_notifications_to_accounts(
        db,
        config,
        quote_recipients.clone(),
        "quoted_update",
        json!({
            "account_id": status.account_id,
            "status_id": status.id,
            "quoted_status_id": status.id,
        }),
    )
    .await;

    publish_local_status_update_stream_notifications_soft(
        env,
        db,
        config,
        status,
        &reblog_accounts,
        &quote_recipients,
    )
    .await;

    Ok(())
}

pub(crate) async fn send_remote_status_update_notifications(
    db: &D1Database,
    config: &AppConfig,
    status_id: &str,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = [worker::d1::D1Type::Text(status_id)];
    let reblog_accounts = load_account_ids(
        db,
        "SELECT DISTINCT account_id
         FROM reblogs
         WHERE remote_status_id = ?1",
        &bindings,
    )
    .await?;
    let _ = send_push_notifications_to_accounts(
        db,
        config,
        reblog_accounts,
        "update",
        json!({
            "account_id": account_id,
            "status_id": status_id,
        }),
    )
    .await;

    let quote_bindings = [
        worker::d1::D1Type::Text(target_uri),
        worker::d1::D1Type::Text(account_id),
    ];
    let quote_recipients = load_account_ids(
        db,
        "SELECT DISTINCT account_id
         FROM statuses
         WHERE quote_of_uri = ?1
           AND quote_state != 'revoked'
           AND account_id != ?2",
        &quote_bindings,
    )
    .await?;
    let _ = send_push_notifications_to_accounts(
        db,
        config,
        quote_recipients,
        "quoted_update",
        json!({
            "account_id": account_id,
            "status_id": status_id,
            "quoted_status_id": target_uri,
        }),
    )
    .await;

    Ok(())
}

pub(crate) async fn send_poll_end_notifications(
    db: &D1Database,
    config: &AppConfig,
    env: Option<&Env>,
    poll_id: &str,
    status_id: &str,
    account_id: &str,
) -> Result<()> {
    let mut account_ids = load_account_ids(
        db,
        "SELECT account_id
         FROM status_poll_votes
         WHERE poll_id = ?1",
        &[worker::d1::D1Type::Text(poll_id)],
    )
    .await?;
    account_ids.push(account_id.to_owned());
    send_push_notifications_to_accounts(
        db,
        config,
        account_ids.clone(),
        "poll",
        json!({
            "account_id": account_id,
            "status_id": status_id,
            "poll_id": poll_id,
        }),
    )
    .await?;
    publish_poll_end_stream_notifications_soft(
        env,
        db,
        config,
        poll_id,
        status_id,
        account_id,
        &account_ids,
    )
    .await;
    Ok(())
}
