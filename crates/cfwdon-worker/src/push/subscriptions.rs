use crate::profile::require_authenticated_local_account;
use crate::request_utils::parse_optional_bool;
use crate::runtime_config::load_config;
use serde::Deserialize;
use url::Url;
use worker::d1::D1Type;
use worker::{D1Database, Error, FormData};
use worker::{Request, Response, Result, RouteContext};

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

#[derive(Clone, Debug, Default)]
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

#[derive(Debug)]
struct PushSubscriptionSaveDraft {
    account_id: String,
    endpoint: String,
    p256dh_key: String,
    auth_key: String,
    standard: bool,
    alerts: PushAlerts,
    policy: String,
}

impl PushSubscriptionSaveDraft {
    fn new(account_id: &str, request: &CreatePushSubscriptionRequest) -> Self {
        Self {
            account_id: account_id.to_owned(),
            endpoint: request.endpoint.clone(),
            p256dh_key: request.p256dh.clone(),
            auth_key: request.auth.clone(),
            standard: request.standard,
            alerts: request.alerts.clone(),
            policy: request.policy.clone(),
        }
    }
}

#[derive(Debug)]
struct PushSubscriptionUpdateDraft {
    account_id: String,
    alerts: PushAlerts,
    policy: String,
}

impl PushSubscriptionUpdateDraft {
    fn new(account_id: &str, request: &UpdatePushSubscriptionRequest) -> Self {
        Self {
            account_id: account_id.to_owned(),
            alerts: request.alerts.clone(),
            policy: request.policy.clone(),
        }
    }
}

const PUSH_SUBSCRIPTION_SAVE_SQL: &str = "INSERT INTO push_subscriptions (
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
            updated_at = CURRENT_TIMESTAMP";

const PUSH_SUBSCRIPTION_UPDATE_SQL: &str = "UPDATE push_subscriptions
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
         WHERE account_id = ?1";

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

fn request_content_type(req: &Request) -> std::result::Result<String, String> {
    req.headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))
        .map(|value| value.unwrap_or_default().to_ascii_lowercase())
}

fn request_is_json(content_type: &str) -> bool {
    content_type.contains("application/json")
}

fn push_alerts_from_form(form: &FormData) -> std::result::Result<PushAlertsInput, String> {
    Ok(PushAlertsInput {
        mention: parse_optional_bool(form.get_field("data[alerts][mention]").as_deref())?,
        quote: parse_optional_bool(form.get_field("data[alerts][quote]").as_deref())?,
        status: parse_optional_bool(form.get_field("data[alerts][status]").as_deref())?,
        reblog: parse_optional_bool(form.get_field("data[alerts][reblog]").as_deref())?,
        follow: parse_optional_bool(form.get_field("data[alerts][follow]").as_deref())?,
        follow_request: parse_optional_bool(
            form.get_field("data[alerts][follow_request]").as_deref(),
        )?,
        favourite: parse_optional_bool(form.get_field("data[alerts][favourite]").as_deref())?,
        poll: parse_optional_bool(form.get_field("data[alerts][poll]").as_deref())?,
        update: parse_optional_bool(form.get_field("data[alerts][update]").as_deref())?,
        quoted_update: parse_optional_bool(
            form.get_field("data[alerts][quoted_update]").as_deref(),
        )?,
        admin_sign_up: parse_optional_bool(
            form.get_field("data[alerts][admin.sign_up]").as_deref(),
        )?,
        admin_report: parse_optional_bool(form.get_field("data[alerts][admin.report]").as_deref())?,
    })
}

fn push_data_from_form(form: &FormData) -> std::result::Result<PushDataInput, String> {
    Ok(PushDataInput {
        alerts: Some(push_alerts_from_form(form)?),
        policy: form.get_field("data[policy]"),
    })
}

fn push_subscription_from_form(
    form: &FormData,
) -> std::result::Result<PushSubscriptionInput, String> {
    Ok(PushSubscriptionInput {
        endpoint: form.get_field("subscription[endpoint]"),
        keys: Some(PushKeysInput {
            p256dh: form.get_field("subscription[keys][p256dh]"),
            auth: form.get_field("subscription[keys][auth]"),
        }),
        standard: parse_optional_bool(form.get_field("subscription[standard]").as_deref())?,
    })
}

fn create_push_subscription_request_from_input(
    subscription: PushSubscriptionInput,
    data: PushDataInput,
) -> std::result::Result<CreatePushSubscriptionRequest, String> {
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

async fn parse_create_push_subscription_request(
    req: &mut Request,
) -> std::result::Result<CreatePushSubscriptionRequest, String> {
    let content_type = request_content_type(req)?;

    let (subscription, data) = if request_is_json(&content_type) {
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
            push_subscription_from_form(&form)?,
            push_data_from_form(&form)?,
        )
    };

    create_push_subscription_request_from_input(subscription, data)
}

async fn parse_update_push_subscription_request(
    req: &mut Request,
) -> std::result::Result<UpdatePushSubscriptionRequest, String> {
    let content_type = request_content_type(req)?;

    let data = if request_is_json(&content_type) {
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
        push_data_from_form(&form)?
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
    let draft = PushSubscriptionSaveDraft::new(account_id, request);
    save_push_subscription_row(db, &draft).await
}

async fn save_push_subscription_row(
    db: &D1Database,
    draft: &PushSubscriptionSaveDraft,
) -> Result<()> {
    let bindings = push_subscription_save_bindings(draft);
    db.prepare(PUSH_SUBSCRIPTION_SAVE_SQL)
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    Ok(())
}

fn push_subscription_save_bindings(draft: &PushSubscriptionSaveDraft) -> [D1Type<'_>; 18] {
    [
        D1Type::Text(draft.account_id.as_str()),
        D1Type::Text(draft.endpoint.as_str()),
        D1Type::Text(draft.p256dh_key.as_str()),
        D1Type::Text(draft.auth_key.as_str()),
        D1Type::Integer(bool_to_i32(draft.standard)),
        D1Type::Integer(bool_to_i32(draft.alerts.follow)),
        D1Type::Integer(bool_to_i32(draft.alerts.favourite)),
        D1Type::Integer(bool_to_i32(draft.alerts.reblog)),
        D1Type::Integer(bool_to_i32(draft.alerts.mention)),
        D1Type::Integer(bool_to_i32(draft.alerts.poll)),
        D1Type::Integer(bool_to_i32(draft.alerts.status)),
        D1Type::Integer(bool_to_i32(draft.alerts.update)),
        D1Type::Integer(bool_to_i32(draft.alerts.follow_request)),
        D1Type::Integer(bool_to_i32(draft.alerts.quote)),
        D1Type::Integer(bool_to_i32(draft.alerts.quoted_update)),
        D1Type::Integer(bool_to_i32(draft.alerts.admin_sign_up)),
        D1Type::Integer(bool_to_i32(draft.alerts.admin_report)),
        D1Type::Text(draft.policy.as_str()),
    ]
}

async fn update_push_subscription(
    db: &D1Database,
    account_id: &str,
    request: &UpdatePushSubscriptionRequest,
) -> Result<()> {
    let draft = PushSubscriptionUpdateDraft::new(account_id, request);
    update_push_subscription_row(db, &draft).await
}

async fn update_push_subscription_row(
    db: &D1Database,
    draft: &PushSubscriptionUpdateDraft,
) -> Result<()> {
    let bindings = push_subscription_update_bindings(draft);
    db.prepare(PUSH_SUBSCRIPTION_UPDATE_SQL)
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    Ok(())
}

fn push_subscription_update_bindings(draft: &PushSubscriptionUpdateDraft) -> [D1Type<'_>; 14] {
    [
        D1Type::Text(draft.account_id.as_str()),
        D1Type::Integer(bool_to_i32(draft.alerts.follow)),
        D1Type::Integer(bool_to_i32(draft.alerts.favourite)),
        D1Type::Integer(bool_to_i32(draft.alerts.reblog)),
        D1Type::Integer(bool_to_i32(draft.alerts.mention)),
        D1Type::Integer(bool_to_i32(draft.alerts.poll)),
        D1Type::Integer(bool_to_i32(draft.alerts.status)),
        D1Type::Integer(bool_to_i32(draft.alerts.update)),
        D1Type::Integer(bool_to_i32(draft.alerts.follow_request)),
        D1Type::Integer(bool_to_i32(draft.alerts.quote)),
        D1Type::Integer(bool_to_i32(draft.alerts.quoted_update)),
        D1Type::Integer(bool_to_i32(draft.alerts.admin_sign_up)),
        D1Type::Integer(bool_to_i32(draft.alerts.admin_report)),
        D1Type::Text(draft.policy.as_str()),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_json_matches_json_content_types() {
        assert!(request_is_json("application/json"));
        assert!(request_is_json("application/json; charset=utf-8"));
        assert!(!request_is_json("application/x-www-form-urlencoded"));
    }

    #[test]
    fn create_push_subscription_request_from_input_normalizes_fields() {
        let request = create_push_subscription_request_from_input(
            PushSubscriptionInput {
                endpoint: Some(" https://push.example/subscription ".to_owned()),
                keys: Some(PushKeysInput {
                    p256dh: Some(" p256dh-key ".to_owned()),
                    auth: Some(" auth-key ".to_owned()),
                }),
                standard: Some(true),
            },
            PushDataInput {
                alerts: Some(PushAlertsInput {
                    mention: Some(true),
                    quote: Some(true),
                    ..Default::default()
                }),
                policy: Some(" FOLLOWED ".to_owned()),
            },
        )
        .unwrap();

        assert_eq!(request.endpoint, "https://push.example/subscription");
        assert_eq!(request.p256dh, "p256dh-key");
        assert_eq!(request.auth, "auth-key");
        assert!(request.standard);
        assert!(request.alerts.mention);
        assert!(request.alerts.quote);
        assert!(!request.alerts.follow);
        assert_eq!(request.policy, "followed");
    }

    #[test]
    fn create_push_subscription_request_from_input_rejects_blank_required_fields() {
        let error = create_push_subscription_request_from_input(
            PushSubscriptionInput {
                endpoint: Some("   ".to_owned()),
                keys: Some(PushKeysInput {
                    p256dh: Some("p256dh-key".to_owned()),
                    auth: Some("auth-key".to_owned()),
                }),
                standard: None,
            },
            PushDataInput::default(),
        )
        .unwrap_err();

        assert_eq!(error, "Validation failed: Endpoint can't be blank");
    }

    #[test]
    fn push_subscription_save_draft_copies_request_storage_fields() {
        let request = CreatePushSubscriptionRequest {
            endpoint: "https://push.example/subscription".to_owned(),
            p256dh: "p256dh-key".to_owned(),
            auth: "auth-key".to_owned(),
            standard: true,
            alerts: PushAlerts {
                follow: true,
                mention: true,
                quote: true,
                admin_report: true,
                ..Default::default()
            },
            policy: "followed".to_owned(),
        };

        let draft = PushSubscriptionSaveDraft::new("account-1", &request);

        assert_eq!(draft.account_id, "account-1");
        assert_eq!(draft.endpoint, "https://push.example/subscription");
        assert_eq!(draft.p256dh_key, "p256dh-key");
        assert_eq!(draft.auth_key, "auth-key");
        assert!(draft.standard);
        assert!(draft.alerts.follow);
        assert!(draft.alerts.mention);
        assert!(draft.alerts.quote);
        assert!(draft.alerts.admin_report);
        assert!(!draft.alerts.reblog);
        assert_eq!(draft.policy, "followed");
    }

    #[test]
    fn push_subscription_save_bindings_keep_sql_slot_order_stable() {
        let draft = PushSubscriptionSaveDraft {
            account_id: "account-1".to_owned(),
            endpoint: "https://push.example/subscription".to_owned(),
            p256dh_key: "p256dh-key".to_owned(),
            auth_key: "auth-key".to_owned(),
            standard: true,
            alerts: PushAlerts {
                follow: true,
                favourite: false,
                reblog: true,
                mention: false,
                poll: true,
                status: false,
                update: true,
                follow_request: false,
                quote: true,
                quoted_update: false,
                admin_sign_up: true,
                admin_report: false,
            },
            policy: "all".to_owned(),
        };
        let bindings = push_subscription_save_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("account-1")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("https://push.example/subscription")
        ));
        assert!(matches!(bindings[2], D1Type::Text("p256dh-key")));
        assert!(matches!(bindings[3], D1Type::Text("auth-key")));
        assert!(matches!(bindings[4], D1Type::Integer(1)));
        assert!(matches!(bindings[5], D1Type::Integer(1)));
        assert!(matches!(bindings[6], D1Type::Integer(0)));
        assert!(matches!(bindings[7], D1Type::Integer(1)));
        assert!(matches!(bindings[8], D1Type::Integer(0)));
        assert!(matches!(bindings[9], D1Type::Integer(1)));
        assert!(matches!(bindings[10], D1Type::Integer(0)));
        assert!(matches!(bindings[11], D1Type::Integer(1)));
        assert!(matches!(bindings[12], D1Type::Integer(0)));
        assert!(matches!(bindings[13], D1Type::Integer(1)));
        assert!(matches!(bindings[14], D1Type::Integer(0)));
        assert!(matches!(bindings[15], D1Type::Integer(1)));
        assert!(matches!(bindings[16], D1Type::Integer(0)));
        assert!(matches!(bindings[17], D1Type::Text("all")));
    }

    #[test]
    fn push_subscription_update_draft_copies_request_storage_fields() {
        let request = UpdatePushSubscriptionRequest {
            alerts: PushAlerts {
                status: true,
                update: true,
                follow_request: true,
                quoted_update: true,
                ..Default::default()
            },
            policy: "none".to_owned(),
        };

        let draft = PushSubscriptionUpdateDraft::new("account-2", &request);

        assert_eq!(draft.account_id, "account-2");
        assert!(draft.alerts.status);
        assert!(draft.alerts.update);
        assert!(draft.alerts.follow_request);
        assert!(draft.alerts.quoted_update);
        assert!(!draft.alerts.follow);
        assert_eq!(draft.policy, "none");
    }

    #[test]
    fn push_subscription_update_bindings_keep_sql_slot_order_stable() {
        let draft = PushSubscriptionUpdateDraft {
            account_id: "account-2".to_owned(),
            alerts: PushAlerts {
                follow: false,
                favourite: true,
                reblog: false,
                mention: true,
                poll: false,
                status: true,
                update: false,
                follow_request: true,
                quote: false,
                quoted_update: true,
                admin_sign_up: false,
                admin_report: true,
            },
            policy: "followed".to_owned(),
        };
        let bindings = push_subscription_update_bindings(&draft);

        assert!(matches!(bindings[0], D1Type::Text("account-2")));
        assert!(matches!(bindings[1], D1Type::Integer(0)));
        assert!(matches!(bindings[2], D1Type::Integer(1)));
        assert!(matches!(bindings[3], D1Type::Integer(0)));
        assert!(matches!(bindings[4], D1Type::Integer(1)));
        assert!(matches!(bindings[5], D1Type::Integer(0)));
        assert!(matches!(bindings[6], D1Type::Integer(1)));
        assert!(matches!(bindings[7], D1Type::Integer(0)));
        assert!(matches!(bindings[8], D1Type::Integer(1)));
        assert!(matches!(bindings[9], D1Type::Integer(0)));
        assert!(matches!(bindings[10], D1Type::Integer(1)));
        assert!(matches!(bindings[11], D1Type::Integer(0)));
        assert!(matches!(bindings[12], D1Type::Integer(1)));
        assert!(matches!(bindings[13], D1Type::Text("followed")));
    }
}
