use super::email_confirmation::{
    send_email_confirmation_message, update_pending_email_confirmation_sent_at,
    upsert_pending_email_confirmation,
};
use super::{
    invalid_access_token_response, outside_authorized_scopes_response, validation_failed_response,
};
use crate::auth::{find_account_by_email, find_account_by_username, store_account_private_key};
use crate::crypto_keys::generate_account_key_material;
use crate::db_session::bind_request_d1;
use crate::id_utils::generate_entity_id;
use crate::oauth_apps::{
    build_oauth_token_document, find_oauth_app_id_by_bearer_token, issue_oauth_access_token,
    oauth_app_has_any_scope, oauth_app_scopes, store_account_password,
};
use crate::oauth_store::{
    app_bearer_token_from_request, find_oauth_app_by_bearer_token, link_oauth_app_to_account,
};
use crate::push::send_push_notification;
use crate::request_utils::parse_optional_bool;
use crate::runtime_config::load_config;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::collections::BTreeMap;
use worker::{Request, Response, Result, RouteContext, d1::D1Type};

#[derive(Debug, Default, Deserialize)]
struct AccountRegistrationRequest {
    username: Option<String>,
    email: Option<String>,
    password: Option<String>,
    agreement: Option<String>,
    locale: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AccountRegistrationValidation {
    pub(crate) username: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) password_present: bool,
    pub(crate) agreement: Option<bool>,
}

fn normalized_registration_field(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalized_registration_username(value: Option<String>) -> Option<String> {
    normalized_registration_field(value).map(|value| value.to_ascii_lowercase())
}

pub(in crate::meta_placeholder_routes) fn normalized_registration_email(
    value: Option<String>,
) -> Option<String> {
    normalized_registration_field(value).map(|value| value.to_ascii_lowercase())
}

fn account_registration_composing(
    validation: &AccountRegistrationValidation,
) -> cfwdon_domain::ComposingRegistration {
    cfwdon_domain::ComposingRegistration {
        username: validation.username.clone(),
        email: validation.email.clone(),
        password_present: validation.password_present,
        agreement: validation.agreement,
    }
}

fn account_registration_api_details(
    validation: &AccountRegistrationValidation,
    uniqueness: cfwdon_domain::RegistrationUniquenessFacts,
) -> BTreeMap<&'static str, Vec<String>> {
    match cfwdon_domain::finalize_registration_validation(
        account_registration_composing(validation).validate(),
        uniqueness,
    ) {
        Ok(_) => BTreeMap::new(),
        Err(errors) => errors.into_api_details(),
    }
}

async fn parse_account_registration_request(
    req: &mut Request,
) -> std::result::Result<AccountRegistrationRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<AccountRegistrationRequest>()
            .await
            .map_err(|error| format!("invalid JSON account registration payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form account registration payload: {error}"))?;
        AccountRegistrationRequest {
            username: form.get_field("username"),
            email: form.get_field("email"),
            password: form.get_field("password"),
            agreement: form.get_field("agreement"),
            locale: form.get_field("locale"),
            reason: form.get_field("reason"),
        }
    };

    request.username = normalized_registration_username(request.username);
    request.email = normalized_registration_email(request.email);
    request.password = normalized_registration_field(request.password);
    request.locale = normalized_registration_field(request.locale);
    request.reason = normalized_registration_field(request.reason);
    request.agreement = request
        .agreement
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    Ok(request)
}

async fn insert_registered_account(
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
    username: &str,
    email: &str,
) -> Result<String> {
    let id = generate_entity_id(16)?;
    let display_name = username.to_owned();
    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(id.as_str()),
        D1Type::Text(username),
        D1Type::Text(email),
        D1Type::Text(display_name.as_str()),
        D1Type::Text(""),
        D1Type::Text(key_material.public_key_pem.as_str()),
    ];
    db.prepare(
        "INSERT INTO accounts (
            id,
            username,
            access_email,
            display_name,
            fields_json,
            discoverable,
            default_quote_policy,
            private_key_jwk,
            public_key_pem,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            '[]',
            0,
            'public',
            ?5,
            ?6,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    store_account_private_key(db, config, &id, &key_material.private_key_jwk).await?;

    for admin_email in &config.admin_emails {
        if let Some(admin) = find_account_by_email(db, admin_email).await? {
            let _ = send_push_notification(
                db,
                config,
                admin.id(),
                "admin.sign_up",
                serde_json::json!({
                    "account_id": id,
                    "username": username,
                    "email": email,
                }),
            )
            .await;
        }
    }
    Ok(id)
}

pub(crate) async fn create_account_placeholder_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    let Some(token) = app_bearer_token_from_request(&req)? else {
        return invalid_access_token_response();
    };
    let Some(app) = find_oauth_app_by_bearer_token(&db, &token).await? else {
        return invalid_access_token_response();
    };
    if !oauth_app_has_any_scope(&app, &["write:accounts", "write"]) {
        return outside_authorized_scopes_response();
    }

    let request = match parse_account_registration_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    let agreement = parse_optional_bool(request.agreement.as_deref()).unwrap_or_default();
    let validation = AccountRegistrationValidation {
        username: request.username.clone(),
        email: request.email.clone(),
        password_present: request.password.is_some(),
        agreement,
    };
    let uniqueness = cfwdon_domain::RegistrationUniquenessFacts {
        username_taken: if let Some(username) = request.username.as_deref() {
            find_account_by_username(&db, username).await?.is_some()
        } else {
            false
        },
        email_taken: if let Some(email) = request.email.as_deref() {
            find_account_by_email(&db, email).await?.is_some()
        } else {
            false
        },
    };
    let details = account_registration_api_details(&validation, uniqueness);
    if !details.is_empty() {
        return validation_failed_response(details);
    }
    let account_id = insert_registered_account(
        &db,
        &config,
        request
            .username
            .as_deref()
            .expect("validated username presence"),
        request.email.as_deref().expect("validated email presence"),
    )
    .await?;
    store_account_password(
        &db,
        &account_id,
        request
            .password
            .as_deref()
            .expect("validated password presence"),
    )
    .await?;
    let app_id = find_oauth_app_id_by_bearer_token(&db, &token)
        .await?
        .expect("loaded app must have an id");
    link_oauth_app_to_account(&db, app_id, &account_id).await?;
    let confirmation_token = generate_entity_id(32)?;
    let pending_email = request.email.as_deref().expect("validated email presence");
    upsert_pending_email_confirmation(&db, &account_id, app_id, pending_email, &confirmation_token)
        .await?;
    if send_email_confirmation_message(&ctx, &config, pending_email, &confirmation_token).await? {
        update_pending_email_confirmation_sent_at(&db, &account_id, &confirmation_token).await?;
    }
    let app_scopes = oauth_app_scopes(&app);
    let access_token = issue_oauth_access_token(&db, app_id, &account_id, &app_scopes).await?;

    Response::from_json(&build_oauth_token_document(
        &access_token.access_token,
        &app_scopes.join(" "),
    ))
}
