use crate::{
    Request, Response, Result, RouteContext, load_config, parse_optional_bool,
    require_authenticated_local_account,
};
use serde::Deserialize;
use url::Url;
use worker::d1::D1Type;
use worker::{D1Database, Error};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PushSubscriptionRow {
    pub(crate) id: i64,
    pub(crate) endpoint: String,
    pub(crate) p256dh_key: String,
    pub(crate) auth_key: String,
    pub(crate) standard: i32,
    pub(crate) alert_follow: i32,
    pub(crate) alert_favourite: i32,
    pub(crate) alert_reblog: i32,
    pub(crate) alert_mention: i32,
    pub(crate) alert_poll: i32,
    pub(crate) alert_status: i32,
    pub(crate) alert_update: i32,
    pub(crate) alert_follow_request: i32,
    pub(crate) alert_quote: i32,
    pub(crate) alert_quoted_update: i32,
    pub(crate) alert_admin_sign_up: i32,
    pub(crate) alert_admin_report: i32,
    #[serde(rename = "policy")]
    pub(crate) _policy: String,
}

#[derive(Debug, Default, Deserialize)]
struct PushAlertsInput {
    follow: Option<bool>,
    favourite: Option<bool>,
    reblog: Option<bool>,
    mention: Option<bool>,
    poll: Option<bool>,
    status: Option<bool>,
    update: Option<bool>,
    follow_request: Option<bool>,
    quote: Option<bool>,
    quoted_update: Option<bool>,
    #[serde(rename = "admin.sign_up")]
    admin_sign_up: Option<bool>,
    #[serde(rename = "admin.report")]
    admin_report: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct PushDataInput {
    alerts: Option<PushAlertsInput>,
    policy: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PushKeysInput {
    p256dh: Option<String>,
    auth: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PushSubscriptionInput {
    endpoint: Option<String>,
    keys: Option<PushKeysInput>,
    standard: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct CreatePushSubscriptionInput {
    subscription: Option<PushSubscriptionInput>,
    data: Option<PushDataInput>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdatePushSubscriptionInput {
    data: Option<PushDataInput>,
}

#[derive(Debug, Default)]
struct PushAlerts {
    follow: bool,
    favourite: bool,
    reblog: bool,
    mention: bool,
    poll: bool,
    status: bool,
    update: bool,
    follow_request: bool,
    quote: bool,
    quoted_update: bool,
    admin_sign_up: bool,
    admin_report: bool,
}

#[derive(Debug)]
struct CreatePushSubscriptionRequest {
    endpoint: String,
    p256dh: String,
    auth: String,
    standard: bool,
    alerts: PushAlerts,
    policy: String,
}

#[derive(Debug)]
struct UpdatePushSubscriptionRequest {
    alerts: PushAlerts,
    policy: String,
}

fn push_subscription_document(
    row: &PushSubscriptionRow,
    config: &cfwdon_core::AppConfig,
) -> serde_json::Value {
    serde_json::json!({
        "id": row.id.to_string(),
        "endpoint": row.endpoint,
        "standard": row.standard != 0,
        "alerts": {
            "follow": row.alert_follow != 0,
            "favourite": row.alert_favourite != 0,
            "reblog": row.alert_reblog != 0,
            "mention": row.alert_mention != 0,
            "poll": row.alert_poll != 0,
            "status": row.alert_status != 0,
            "update": row.alert_update != 0,
            "follow_request": row.alert_follow_request != 0,
            "quote": row.alert_quote != 0,
            "quoted_update": row.alert_quoted_update != 0,
            "admin.sign_up": row.alert_admin_sign_up != 0,
            "admin.report": row.alert_admin_report != 0,
        },
        "server_key": config.web_push_vapid_public_key.as_deref().unwrap_or(""),
    })
}

pub(crate) fn push_subscription_alert_enabled(
    row: &PushSubscriptionRow,
    notification_type: &str,
) -> bool {
    match notification_type {
        "follow" => row.alert_follow != 0,
        "favourite" => row.alert_favourite != 0,
        "reblog" => row.alert_reblog != 0,
        "mention" => row.alert_mention != 0,
        "poll" => row.alert_poll != 0,
        "status" => row.alert_status != 0,
        "update" => row.alert_update != 0,
        "follow_request" => row.alert_follow_request != 0,
        "quote" => row.alert_quote != 0,
        "quoted_update" => row.alert_quoted_update != 0,
        "admin.sign_up" => row.alert_admin_sign_up != 0,
        "admin.report" => row.alert_admin_report != 0,
        _ => false,
    }
}

fn bool_to_i32(value: bool) -> i32 {
    i32::from(value)
}

fn normalize_required_string(
    value: Option<String>,
    field: &str,
) -> std::result::Result<String, String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Validation failed: {field} can't be blank"))
}

fn validate_endpoint_url(value: &str) -> std::result::Result<String, String> {
    let url = Url::parse(value)
        .map_err(|_| "Validation failed: Endpoint is not a valid URL".to_owned())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Validation failed: Endpoint is not a valid URL".to_owned());
    }
    Ok(value.to_owned())
}

fn normalize_policy(value: Option<String>) -> std::result::Result<String, String> {
    let policy = value
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "all".to_owned());
    match policy.as_str() {
        "all" | "followed" | "follower" | "none" => Ok(policy),
        _ => Err("Validation failed: Policy is not included in the list".to_owned()),
    }
}

fn alerts_from_input(input: Option<PushAlertsInput>) -> PushAlerts {
    let input = input.unwrap_or_default();
    PushAlerts {
        follow: input.follow.unwrap_or(false),
        favourite: input.favourite.unwrap_or(false),
        reblog: input.reblog.unwrap_or(false),
        mention: input.mention.unwrap_or(false),
        poll: input.poll.unwrap_or(false),
        status: input.status.unwrap_or(false),
        update: input.update.unwrap_or(false),
        follow_request: input.follow_request.unwrap_or(false),
        quote: input.quote.unwrap_or(false),
        quoted_update: input.quoted_update.unwrap_or(false),
        admin_sign_up: input.admin_sign_up.unwrap_or(false),
        admin_report: input.admin_report.unwrap_or(false),
    }
}

async fn parse_create_push_subscription_request(
    req: &mut Request,
) -> std::result::Result<CreatePushSubscriptionRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (subscription, data) = if content_type.contains("application/json") {
        let payload = req
            .json::<CreatePushSubscriptionInput>()
            .await
            .map_err(|error| format!("invalid JSON push subscription payload: {error}"))?;
        (
            payload.subscription.unwrap_or_default(),
            payload.data.unwrap_or_default(),
        )
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form push subscription payload: {error}"))?;
        (
            PushSubscriptionInput {
                endpoint: form.get_field("subscription[endpoint]"),
                keys: Some(PushKeysInput {
                    p256dh: form.get_field("subscription[keys][p256dh]"),
                    auth: form.get_field("subscription[keys][auth]"),
                }),
                standard: parse_optional_bool(form.get_field("subscription[standard]").as_deref())?,
            },
            PushDataInput {
                alerts: Some(PushAlertsInput {
                    mention: parse_optional_bool(
                        form.get_field("data[alerts][mention]").as_deref(),
                    )?,
                    quote: parse_optional_bool(form.get_field("data[alerts][quote]").as_deref())?,
                    status: parse_optional_bool(form.get_field("data[alerts][status]").as_deref())?,
                    reblog: parse_optional_bool(form.get_field("data[alerts][reblog]").as_deref())?,
                    follow: parse_optional_bool(form.get_field("data[alerts][follow]").as_deref())?,
                    follow_request: parse_optional_bool(
                        form.get_field("data[alerts][follow_request]").as_deref(),
                    )?,
                    favourite: parse_optional_bool(
                        form.get_field("data[alerts][favourite]").as_deref(),
                    )?,
                    poll: parse_optional_bool(form.get_field("data[alerts][poll]").as_deref())?,
                    update: parse_optional_bool(form.get_field("data[alerts][update]").as_deref())?,
                    quoted_update: parse_optional_bool(
                        form.get_field("data[alerts][quoted_update]").as_deref(),
                    )?,
                    admin_sign_up: parse_optional_bool(
                        form.get_field("data[alerts][admin.sign_up]").as_deref(),
                    )?,
                    admin_report: parse_optional_bool(
                        form.get_field("data[alerts][admin.report]").as_deref(),
                    )?,
                }),
                policy: form.get_field("data[policy]"),
            },
        )
    };

    let keys = subscription.keys.unwrap_or_default();
    Ok(CreatePushSubscriptionRequest {
        endpoint: validate_endpoint_url(&normalize_required_string(
            subscription.endpoint,
            "Endpoint",
        )?)?,
        p256dh: normalize_required_string(keys.p256dh, "P256dh")?,
        auth: normalize_required_string(keys.auth, "Auth")?,
        standard: subscription.standard.unwrap_or(false),
        alerts: alerts_from_input(data.alerts),
        policy: normalize_policy(data.policy)?,
    })
}

async fn parse_update_push_subscription_request(
    req: &mut Request,
) -> std::result::Result<UpdatePushSubscriptionRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let data = if content_type.contains("application/json") {
        req.json::<UpdatePushSubscriptionInput>()
            .await
            .map_err(|error| format!("invalid JSON push subscription payload: {error}"))?
            .data
            .unwrap_or_default()
    } else if content_type.trim().is_empty() {
        PushDataInput::default()
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form push subscription payload: {error}"))?;
        PushDataInput {
            alerts: Some(PushAlertsInput {
                mention: parse_optional_bool(form.get_field("data[alerts][mention]").as_deref())?,
                quote: parse_optional_bool(form.get_field("data[alerts][quote]").as_deref())?,
                status: parse_optional_bool(form.get_field("data[alerts][status]").as_deref())?,
                reblog: parse_optional_bool(form.get_field("data[alerts][reblog]").as_deref())?,
                follow: parse_optional_bool(form.get_field("data[alerts][follow]").as_deref())?,
                follow_request: parse_optional_bool(
                    form.get_field("data[alerts][follow_request]").as_deref(),
                )?,
                favourite: parse_optional_bool(
                    form.get_field("data[alerts][favourite]").as_deref(),
                )?,
                poll: parse_optional_bool(form.get_field("data[alerts][poll]").as_deref())?,
                update: parse_optional_bool(form.get_field("data[alerts][update]").as_deref())?,
                quoted_update: parse_optional_bool(
                    form.get_field("data[alerts][quoted_update]").as_deref(),
                )?,
                admin_sign_up: parse_optional_bool(
                    form.get_field("data[alerts][admin.sign_up]").as_deref(),
                )?,
                admin_report: parse_optional_bool(
                    form.get_field("data[alerts][admin.report]").as_deref(),
                )?,
            }),
            policy: form.get_field("data[policy]"),
        }
    };

    Ok(UpdatePushSubscriptionRequest {
        alerts: alerts_from_input(data.alerts),
        policy: normalize_policy(data.policy)?,
    })
}

async fn find_push_subscription(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<PushSubscriptionRow>> {
    let binding = D1Type::Text(account_id);
    db.prepare(
        "SELECT
            id,
            endpoint,
            p256dh_key,
            auth_key,
            standard,
            alert_follow,
            alert_favourite,
            alert_reblog,
            alert_mention,
            alert_poll,
            alert_status,
            alert_update,
            alert_follow_request,
            alert_quote,
            alert_quoted_update,
            alert_admin_sign_up,
            alert_admin_report,
            policy
         FROM push_subscriptions
         WHERE account_id = ?1
         LIMIT 1",
    )
    .bind_refs(&binding)?
    .first::<PushSubscriptionRow>(None)
    .await
}

pub(crate) async fn load_push_subscription(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<PushSubscriptionRow>> {
    find_push_subscription(db, account_id).await
}

async fn save_push_subscription(
    db: &D1Database,
    account_id: &str,
    request: &CreatePushSubscriptionRequest,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(request.endpoint.as_str()),
        D1Type::Text(request.p256dh.as_str()),
        D1Type::Text(request.auth.as_str()),
        D1Type::Integer(bool_to_i32(request.standard)),
        D1Type::Integer(bool_to_i32(request.alerts.follow)),
        D1Type::Integer(bool_to_i32(request.alerts.favourite)),
        D1Type::Integer(bool_to_i32(request.alerts.reblog)),
        D1Type::Integer(bool_to_i32(request.alerts.mention)),
        D1Type::Integer(bool_to_i32(request.alerts.poll)),
        D1Type::Integer(bool_to_i32(request.alerts.status)),
        D1Type::Integer(bool_to_i32(request.alerts.update)),
        D1Type::Integer(bool_to_i32(request.alerts.follow_request)),
        D1Type::Integer(bool_to_i32(request.alerts.quote)),
        D1Type::Integer(bool_to_i32(request.alerts.quoted_update)),
        D1Type::Integer(bool_to_i32(request.alerts.admin_sign_up)),
        D1Type::Integer(bool_to_i32(request.alerts.admin_report)),
        D1Type::Text(request.policy.as_str()),
    ];
    db.prepare(
        "INSERT INTO push_subscriptions (
            account_id,
            endpoint,
            p256dh_key,
            auth_key,
            standard,
            alert_follow,
            alert_favourite,
            alert_reblog,
            alert_mention,
            alert_poll,
            alert_status,
            alert_update,
            alert_follow_request,
            alert_quote,
            alert_quoted_update,
            alert_admin_sign_up,
            alert_admin_report,
            policy,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id) DO UPDATE SET
            endpoint = excluded.endpoint,
            p256dh_key = excluded.p256dh_key,
            auth_key = excluded.auth_key,
            standard = excluded.standard,
            alert_follow = excluded.alert_follow,
            alert_favourite = excluded.alert_favourite,
            alert_reblog = excluded.alert_reblog,
            alert_mention = excluded.alert_mention,
            alert_poll = excluded.alert_poll,
            alert_status = excluded.alert_status,
            alert_update = excluded.alert_update,
            alert_follow_request = excluded.alert_follow_request,
            alert_quote = excluded.alert_quote,
            alert_quoted_update = excluded.alert_quoted_update,
            alert_admin_sign_up = excluded.alert_admin_sign_up,
            alert_admin_report = excluded.alert_admin_report,
            policy = excluded.policy,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn update_push_subscription(
    db: &D1Database,
    account_id: &str,
    request: &UpdatePushSubscriptionRequest,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Integer(bool_to_i32(request.alerts.follow)),
        D1Type::Integer(bool_to_i32(request.alerts.favourite)),
        D1Type::Integer(bool_to_i32(request.alerts.reblog)),
        D1Type::Integer(bool_to_i32(request.alerts.mention)),
        D1Type::Integer(bool_to_i32(request.alerts.poll)),
        D1Type::Integer(bool_to_i32(request.alerts.status)),
        D1Type::Integer(bool_to_i32(request.alerts.update)),
        D1Type::Integer(bool_to_i32(request.alerts.follow_request)),
        D1Type::Integer(bool_to_i32(request.alerts.quote)),
        D1Type::Integer(bool_to_i32(request.alerts.quoted_update)),
        D1Type::Integer(bool_to_i32(request.alerts.admin_sign_up)),
        D1Type::Integer(bool_to_i32(request.alerts.admin_report)),
        D1Type::Text(request.policy.as_str()),
    ];
    db.prepare(
        "UPDATE push_subscriptions
         SET alert_follow = ?2,
             alert_favourite = ?3,
             alert_reblog = ?4,
             alert_mention = ?5,
             alert_poll = ?6,
             alert_status = ?7,
             alert_update = ?8,
             alert_follow_request = ?9,
             alert_quote = ?10,
             alert_quoted_update = ?11,
             alert_admin_sign_up = ?12,
             alert_admin_report = ?13,
             policy = ?14,
             updated_at = CURRENT_TIMESTAMP
         WHERE account_id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn delete_push_subscription(db: &D1Database, account_id: &str) -> Result<()> {
    let binding = D1Type::Text(account_id);
    db.prepare(
        "DELETE FROM push_subscriptions
         WHERE account_id = ?1",
    )
    .bind_refs(&binding)?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn push_subscription_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(subscription) = find_push_subscription(&db, &account.id).await? else {
        return Response::error("Record not found", 404);
    };
    Response::from_json(&push_subscription_document(&subscription, &config))
}

pub(crate) async fn create_push_subscription_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request = match parse_create_push_subscription_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    save_push_subscription(&db, &account.id, &request).await?;
    let subscription = find_push_subscription(&db, &account.id)
        .await?
        .ok_or_else(|| Error::RustError("push subscription missing after save".to_owned()))?;
    Response::from_json(&push_subscription_document(&subscription, &config))
}

pub(crate) async fn update_push_subscription_response(
    mut req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    if find_push_subscription(&db, &account.id).await?.is_none() {
        return Response::error("Record not found", 404);
    }
    let request = match parse_update_push_subscription_request(&mut req).await {
        Ok(request) => request,
        Err(message) => return Response::error(&message, 422),
    };
    update_push_subscription(&db, &account.id, &request).await?;
    let subscription = find_push_subscription(&db, &account.id)
        .await?
        .ok_or_else(|| Error::RustError("push subscription missing after update".to_owned()))?;
    Response::from_json(&push_subscription_document(&subscription, &config))
}

pub(crate) async fn delete_push_subscription_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    delete_push_subscription(&db, &account.id).await?;
    Response::from_json(&serde_json::json!({}))
}
