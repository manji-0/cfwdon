use crate::{
    AppConfig, D1Database, MastodonAccountResponse, RemoteActorProfile, Request, Response, Result,
    RouteContext, build_internal_cursor_link_header, build_reject_follow_activity,
    build_relationship_for_target, build_stored_accept_follow_activity, count_rows,
    find_account_by_id, find_remote_actor_by_actor_uri, find_remote_actor_by_username_domain,
    load_account_stats, load_config, parse_internal_pagination_id, parse_lookup_handle,
    remote_account_rest_id, remote_actor_uri_from_rest_id, require_authenticated_local_account,
    upsert_follower_by_inbox,
};
use serde::Deserialize;
use url::Url;
use worker::d1::D1Type;

#[derive(Debug, Default, Deserialize)]
struct FollowRequestsQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
}

async fn authenticated_follow_request_viewer(
    req: &Request,
    ctx: &RouteContext<()>,
    config: &AppConfig,
) -> Result<Option<(D1Database, cfwdon_domain::LocalAccount)>> {
    let db = ctx.d1(&config.database_binding)?;
    let Some(viewer) = require_authenticated_local_account(req, &db, config).await? else {
        return Ok(None);
    };
    Ok(Some((db, viewer)))
}

#[derive(Debug, Default, Deserialize)]
struct NotificationRequestsQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PendingLocalFollowRequestRow {
    cursor_id: i64,
    created_at: String,
    requester_account_id: String,
}

#[derive(Debug, Deserialize)]
struct PendingRemoteFollowRequestRow {
    cursor_id: i64,
    created_at: String,
    requester_actor_uri: String,
    requester_inbox_uri: String,
    requester_shared_inbox_uri: Option<String>,
    follow_activity_id: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingRemoteFollowRequest {
    requester_actor_uri: String,
    requester_inbox_uri: String,
    requester_shared_inbox_uri: Option<String>,
    pub(crate) follow_activity_id: Option<String>,
}

#[derive(Debug, Clone)]
enum PendingFollowRequest {
    Local {
        cursor_id: i64,
        created_at: String,
        requester_account_id: String,
    },
    Remote {
        cursor_id: i64,
        created_at: String,
        request: PendingRemoteFollowRequest,
    },
}

impl PendingFollowRequest {
    fn cursor_id(&self) -> i64 {
        match self {
            Self::Local { cursor_id, .. } | Self::Remote { cursor_id, .. } => *cursor_id,
        }
    }

    fn created_at(&self) -> &str {
        match self {
            Self::Local { created_at, .. } | Self::Remote { created_at, .. } => created_at,
        }
    }
}

fn notification_request_id(request: &PendingFollowRequest) -> String {
    request.cursor_id().to_string()
}

pub(crate) async fn count_pending_follow_requests(
    db: &D1Database,
    account_id: &str,
) -> Result<u64> {
    let local = count_rows(
        db,
        "SELECT COUNT(*) AS count FROM follows WHERE target_account_id = ?1 AND state = 'pending'",
        account_id,
    )
    .await?;
    let remote = count_rows(
        db,
        "SELECT COUNT(*) AS count FROM follow_requests WHERE account_id = ?1",
        account_id,
    )
    .await?;
    Ok(local + remote)
}

pub(crate) async fn has_pending_follow_request_from_account(
    db: &D1Database,
    account_id: &str,
    requester_account_id: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(requester_account_id)];
    let row = db
        .prepare(
            "SELECT 1 AS present
             FROM follows
             WHERE target_account_id = ?1
               AND follower_account_id = ?2
               AND state = 'pending'
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.is_some())
}

pub(crate) async fn has_pending_follow_request_from_actor(
    db: &D1Database,
    account_id: &str,
    requester_actor_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(requester_actor_uri)];
    let row = db
        .prepare(
            "SELECT 1 AS present
             FROM follow_requests
             WHERE account_id = ?1
               AND requester_actor_uri = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;
    Ok(row.is_some())
}

pub(crate) async fn find_pending_remote_follow_request_by_actor(
    db: &D1Database,
    account_id: &str,
    requester_actor_uri: &str,
) -> Result<Option<PendingRemoteFollowRequest>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(requester_actor_uri)];
    let row = db
        .prepare(
            "SELECT requester_actor_uri, requester_inbox_uri, requester_shared_inbox_uri, follow_activity_id
             FROM follow_requests
             WHERE account_id = ?1
               AND requester_actor_uri = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<PendingRemoteFollowRequestRow>(None)
        .await?;

    Ok(row.map(|row| PendingRemoteFollowRequest {
        requester_actor_uri: row.requester_actor_uri,
        requester_inbox_uri: row.requester_inbox_uri,
        requester_shared_inbox_uri: row.requester_shared_inbox_uri,
        follow_activity_id: row.follow_activity_id,
    }))
}

pub(crate) async fn upsert_remote_follow_request(
    db: &D1Database,
    account_id: &str,
    remote_actor: &RemoteActorProfile,
    follow_activity_id: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(remote_actor.actor_uri.as_str()),
        D1Type::Text(remote_actor.inbox_uri.as_str()),
        match remote_actor.shared_inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match follow_activity_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO follow_requests (
            id,
            account_id,
            requester_account_id,
            requester_actor_uri,
            requester_inbox_uri,
            requester_shared_inbox_uri,
            follow_activity_id,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            NULL,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, requester_actor_uri) WHERE requester_actor_uri IS NOT NULL DO UPDATE SET
            requester_inbox_uri = excluded.requester_inbox_uri,
            requester_shared_inbox_uri = excluded.requester_shared_inbox_uri,
            follow_activity_id = excluded.follow_activity_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn delete_remote_follow_request_by_actor(
    db: &D1Database,
    account_id: &str,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(actor_uri),
        D1Type::Text(canonical_actor_uri),
    ];
    db.prepare(
        "DELETE FROM follow_requests
         WHERE account_id = ?1
           AND (requester_actor_uri = ?2 OR requester_actor_uri = ?3)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn list_pending_follow_requests(
    db: &D1Database,
    account_id: &str,
) -> Result<Vec<PendingFollowRequest>> {
    let account_id = D1Type::Text(account_id);
    let local_rows = db
        .prepare(
            "SELECT CAST(strftime('%s', created_at) AS INTEGER) * 1000000 + rowid AS cursor_id,
                    created_at,
                    follower_account_id AS requester_account_id
             FROM follows
             WHERE target_account_id = ?1
               AND state = 'pending'",
        )
        .bind_refs(&account_id)?
        .all()
        .await?
        .results::<PendingLocalFollowRequestRow>()?;
    let remote_rows = db
        .prepare(
            "SELECT CAST(strftime('%s', created_at) AS INTEGER) * 1000000 + rowid AS cursor_id,
                    created_at,
                    requester_actor_uri,
                    requester_inbox_uri,
                    requester_shared_inbox_uri,
                    follow_activity_id
             FROM follow_requests
             WHERE account_id = ?1",
        )
        .bind_refs(&account_id)?
        .all()
        .await?
        .results::<PendingRemoteFollowRequestRow>()?;

    let mut requests = Vec::with_capacity(local_rows.len() + remote_rows.len());
    for row in local_rows {
        requests.push(PendingFollowRequest::Local {
            cursor_id: row.cursor_id,
            created_at: row.created_at,
            requester_account_id: row.requester_account_id,
        });
    }
    for row in remote_rows {
        requests.push(PendingFollowRequest::Remote {
            cursor_id: row.cursor_id,
            created_at: row.created_at,
            request: PendingRemoteFollowRequest {
                requester_actor_uri: row.requester_actor_uri,
                requester_inbox_uri: row.requester_inbox_uri,
                requester_shared_inbox_uri: row.requester_shared_inbox_uri,
                follow_activity_id: row.follow_activity_id,
            },
        });
    }
    requests.sort_by(|left, right| {
        right
            .cursor_id()
            .cmp(&left.cursor_id())
            .then_with(|| right.created_at().cmp(left.created_at()))
    });
    Ok(requests)
}

async fn build_follow_request_remote_account_response(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let status_summary = crate::load_remote_actor_status_summary(db, actor_uri).await?;

    if let Some(actor) = find_remote_actor_by_actor_uri(db, actor_uri).await? {
        let mut response = MastodonAccountResponse::from_remote_actor(&actor);
        response.id = format!("{}@{}", actor.username, actor.domain);
        response.acct = format!("{}@{}", actor.username, actor.domain);
        response.statuses_count = status_summary.statuses_count;
        response.last_status_at = status_summary.last_status_at.clone();
        return Ok(Some(response));
    }

    let parsed = match Url::parse(actor_uri) {
        Ok(parsed) => parsed,
        Err(_) => return Ok(None),
    };
    let Some(domain) = parsed.host_str().map(str::to_owned) else {
        return Ok(None);
    };
    let Some(username) = parsed
        .path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
        .map(|segment| segment.trim_start_matches('@').to_owned())
        .filter(|segment| !segment.is_empty())
    else {
        return Ok(None);
    };

    Ok(Some(MastodonAccountResponse {
        id: remote_account_rest_id(actor_uri),
        username: username.clone(),
        acct: format!("{username}@{domain}"),
        uri: actor_uri.to_owned(),
        display_name: username.clone(),
        locked: false,
        bot: false,
        group: false,
        discoverable: true,
        indexable: true,
        noindex: None,
        hide_collections: None,
        show_media: None,
        show_media_replies: None,
        show_featured: None,
        last_status_at: status_summary.last_status_at,
        created_at: String::new(),
        note: String::new(),
        url: actor_uri.to_owned(),
        avatar: String::new(),
        avatar_static: String::new(),
        header: String::new(),
        header_static: String::new(),
        emojis: Vec::new(),
        fields: Vec::new(),
        roles: Vec::new(),
        followers_count: 0,
        following_count: 0,
        statuses_count: status_summary.statuses_count,
        source: None,
    }))
}

async fn build_follow_request_account_response(
    db: &D1Database,
    config: &AppConfig,
    request: &PendingFollowRequest,
) -> Result<Option<MastodonAccountResponse>> {
    match request {
        PendingFollowRequest::Local {
            requester_account_id,
            ..
        } => {
            let Some(account) = find_account_by_id(db, requester_account_id).await? else {
                return Ok(None);
            };
            let stats = load_account_stats(db, &account.id).await?;
            Ok(Some(MastodonAccountResponse::from_account_with_stats(
                &account, config, &stats,
            )))
        }
        PendingFollowRequest::Remote { request, .. } => {
            build_follow_request_remote_account_response(db, &request.requester_actor_uri).await
        }
    }
}

async fn build_notification_request_document(
    db: &D1Database,
    config: &AppConfig,
    request: &PendingFollowRequest,
) -> Result<Option<serde_json::Value>> {
    let Some(account) = build_follow_request_account_response(db, config, request).await? else {
        return Ok(None);
    };

    Ok(Some(serde_json::json!({
        "id": notification_request_id(request),
        "created_at": request.created_at(),
        "updated_at": request.created_at(),
        "account": account,
        "notifications_count": "1",
        "last_status": serde_json::Value::Null,
    })))
}

async fn pending_request_matches_identity(
    db: &D1Database,
    config: &AppConfig,
    request: &PendingFollowRequest,
    identity: &str,
) -> Result<bool> {
    let decoded = urlencoding::decode(identity)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| identity.to_owned());

    match request {
        PendingFollowRequest::Local {
            cursor_id,
            requester_account_id,
            ..
        } => {
            if identity == cursor_id.to_string() {
                return Ok(true);
            }
            if requester_account_id == identity || requester_account_id == &decoded {
                return Ok(true);
            }
            let Some(account) = find_account_by_id(db, requester_account_id).await? else {
                return Ok(false);
            };
            if account.username.eq_ignore_ascii_case(identity)
                || account.username.eq_ignore_ascii_case(&decoded)
            {
                return Ok(true);
            }
            if let Ok(handle) = parse_lookup_handle(identity, config) {
                return Ok(handle.is_local_to(&config.instance_domain)
                    && handle.username.eq_ignore_ascii_case(&account.username));
            }
            Ok(false)
        }
        PendingFollowRequest::Remote {
            cursor_id, request, ..
        } => {
            if identity == cursor_id.to_string() {
                return Ok(true);
            }
            if request.requester_actor_uri == identity || request.requester_actor_uri == decoded {
                return Ok(true);
            }
            if remote_actor_uri_from_rest_id(identity)
                .as_deref()
                .is_some_and(|value| value == request.requester_actor_uri)
            {
                return Ok(true);
            }
            let Ok(handle) = parse_lookup_handle(identity, config) else {
                return Ok(false);
            };
            let Some(domain) = handle.domain.as_deref() else {
                return Ok(false);
            };
            let Some(actor) =
                find_remote_actor_by_username_domain(db, &handle.username, domain).await?
            else {
                return Ok(false);
            };
            Ok(actor.actor_uri == request.requester_actor_uri)
        }
    }
}

async fn resolve_pending_follow_request(
    db: &D1Database,
    config: &AppConfig,
    account_id: &str,
    identity: &str,
) -> Result<Option<PendingFollowRequest>> {
    for request in list_pending_follow_requests(db, account_id).await? {
        if pending_request_matches_identity(db, config, &request, identity).await? {
            return Ok(Some(request));
        }
    }
    Ok(None)
}

async fn authorize_pending_follow_request(
    db: &D1Database,
    config: &AppConfig,
    viewer: &crate::LocalAccount,
    request: &PendingFollowRequest,
) -> Result<serde_json::Value> {
    match request {
        PendingFollowRequest::Local {
            requester_account_id,
            ..
        } => {
            let bindings = [
                D1Type::Text(viewer.id.as_str()),
                D1Type::Text(requester_account_id.as_str()),
            ];
            db.prepare(
                "UPDATE follows
                 SET state = 'accepted',
                     updated_at = CURRENT_TIMESTAMP
                 WHERE target_account_id = ?1
                   AND follower_account_id = ?2
                   AND state = 'pending'",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
            let requester = find_account_by_id(db, requester_account_id)
                .await?
                .ok_or_else(|| worker::Error::RustError("follow requester not found".to_owned()))?;
            Ok(serde_json::to_value(
                build_relationship_for_target(
                    db,
                    config,
                    viewer,
                    &requester.id,
                    &crate::actor_url(config, &requester.username),
                )
                .await?,
            )?)
        }
        PendingFollowRequest::Remote { request, .. } => {
            upsert_follower_by_inbox(
                db,
                &viewer.id,
                &request.requester_actor_uri,
                &request.requester_inbox_uri,
                request.requester_shared_inbox_uri.as_deref(),
                request.follow_activity_id.as_deref(),
            )
            .await?;
            delete_remote_follow_request_by_actor(
                db,
                &viewer.id,
                &request.requester_actor_uri,
                &request.requester_actor_uri,
            )
            .await?;
            if let Some(follow_activity_id) = request.follow_activity_id.as_deref() {
                let payload = build_stored_accept_follow_activity(
                    config,
                    viewer,
                    follow_activity_id,
                    &request.requester_actor_uri,
                )?;
                let _ = crate::queue_remote_actor_activity_required(
                    db,
                    &viewer.id,
                    &request.requester_actor_uri,
                    &payload,
                )
                .await;
            }
            Ok(serde_json::to_value(
                build_relationship_for_target(
                    db,
                    config,
                    viewer,
                    &remote_account_rest_id(&request.requester_actor_uri),
                    &request.requester_actor_uri,
                )
                .await?,
            )?)
        }
    }
}

async fn reject_pending_follow_request(
    db: &D1Database,
    config: &AppConfig,
    viewer: &crate::LocalAccount,
    request: &PendingFollowRequest,
) -> Result<serde_json::Value> {
    match request {
        PendingFollowRequest::Local {
            requester_account_id,
            ..
        } => {
            let bindings = [
                D1Type::Text(viewer.id.as_str()),
                D1Type::Text(requester_account_id.as_str()),
            ];
            db.prepare(
                "DELETE FROM follows
                 WHERE target_account_id = ?1
                   AND follower_account_id = ?2
                   AND state = 'pending'",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
            let requester = find_account_by_id(db, requester_account_id)
                .await?
                .ok_or_else(|| worker::Error::RustError("follow requester not found".to_owned()))?;
            Ok(serde_json::to_value(
                build_relationship_for_target(
                    db,
                    config,
                    viewer,
                    &requester.id,
                    &crate::actor_url(config, &requester.username),
                )
                .await?,
            )?)
        }
        PendingFollowRequest::Remote { request, .. } => {
            delete_remote_follow_request_by_actor(
                db,
                &viewer.id,
                &request.requester_actor_uri,
                &request.requester_actor_uri,
            )
            .await?;
            if let Some(follow_activity_id) = request.follow_activity_id.as_deref() {
                let payload = build_reject_follow_activity(
                    config,
                    viewer,
                    follow_activity_id,
                    &request.requester_actor_uri,
                )?;
                let _ = crate::queue_remote_actor_activity_required(
                    db,
                    &viewer.id,
                    &request.requester_actor_uri,
                    &payload,
                )
                .await;
            }
            Ok(serde_json::to_value(
                build_relationship_for_target(
                    db,
                    config,
                    viewer,
                    &remote_account_rest_id(&request.requester_actor_uri),
                    &request.requester_actor_uri,
                )
                .await?,
            )?)
        }
    }
}

async fn parse_notification_request_ids(
    req: &mut Request,
) -> std::result::Result<Vec<String>, worker::Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| {
            worker::Error::RustError(format!("failed to read Content-Type header: {error}"))
        })?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        let payload = req.json::<serde_json::Value>().await.map_err(|error| {
            worker::Error::RustError(format!(
                "invalid notification request JSON payload: {error}"
            ))
        })?;
        let mut ids = Vec::new();
        for key in [
            "id",
            "ids",
            "notification_request_ids",
            "notification_requests",
        ] {
            match payload.get(key) {
                Some(serde_json::Value::String(value)) => ids.push(value.clone()),
                Some(serde_json::Value::Array(values)) => {
                    ids.extend(
                        values
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(ToOwned::to_owned),
                    );
                }
                _ => {}
            }
        }
        return Ok(ids);
    }

    let form = req.form_data().await.map_err(|error| {
        worker::Error::RustError(format!(
            "invalid notification request form payload: {error}"
        ))
    })?;
    let mut ids = Vec::new();
    for key in [
        "id",
        "id[]",
        "ids",
        "ids[]",
        "notification_request_ids[]",
        "notification_requests[]",
    ] {
        if let Some(value) = form.get_field(key) {
            ids.push(value);
        }
        if let Some(values) = form.get_all(key) {
            ids.extend(values.into_iter().filter_map(|entry| match entry {
                worker::FormEntry::Field(value) => Some(value),
                worker::FormEntry::File(_) => None,
            }));
        }
    }
    Ok(ids)
}

pub(crate) async fn follow_requests_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: FollowRequestsQuery = req.query()?;
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;

    let mut requests = list_pending_follow_requests(&db, &viewer.id).await?;
    requests.retain(|entry| max_id.is_none_or(|value| entry.cursor_id() < value));
    requests.retain(|entry| since_id.is_none_or(|value| entry.cursor_id() > value));
    if requests.len() > limit as usize {
        requests.truncate(limit as usize);
    }

    let first_id = requests.first().map(PendingFollowRequest::cursor_id);
    let last_id = requests.last().map(PendingFollowRequest::cursor_id);
    let mut accounts = Vec::with_capacity(requests.len());
    for request in &requests {
        if let Some(account) = build_follow_request_account_response(&db, &config, request).await? {
            accounts.push(account);
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        first_id,
        last_id,
        accounts.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }
    builder.from_json(&accounts)
}

pub(crate) async fn follow_request_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing follow request id route parameter".to_owned())
        })?;

    let Some(request) =
        resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
    else {
        return Response::error("follow request not found", 404);
    };
    let Some(account) = build_follow_request_account_response(&db, &config, &request).await? else {
        return Response::error("follow request not found", 404);
    };
    Response::from_json(&account)
}

pub(crate) async fn authorize_follow_request_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing follow request id route parameter".to_owned())
        })?;

    let Some(request) =
        resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
    else {
        return Response::error("follow request not found", 404);
    };
    Response::from_json(&authorize_pending_follow_request(&db, &config, &viewer, &request).await?)
}

pub(crate) async fn reject_follow_request_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing follow request id route parameter".to_owned())
        })?;

    let Some(request) =
        resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
    else {
        return Response::error("follow request not found", 404);
    };
    Response::from_json(&reject_pending_follow_request(&db, &config, &viewer, &request).await?)
}

pub(crate) async fn notification_requests_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: NotificationRequestsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let mut requests = list_pending_follow_requests(&db, &viewer.id).await?;
    requests.retain(|entry| max_id.is_none_or(|value| entry.cursor_id() < value));
    requests.retain(|entry| since_id.is_none_or(|value| entry.cursor_id() > value));
    requests.retain(|entry| min_id.is_none_or(|value| entry.cursor_id() > value));
    if requests.len() > limit as usize {
        requests.truncate(limit as usize);
    }

    let first_id = requests.first().map(PendingFollowRequest::cursor_id);
    let last_id = requests.last().map(PendingFollowRequest::cursor_id);
    let mut documents = Vec::new();
    for request in requests {
        if let Some(document) = build_notification_request_document(&db, &config, &request).await? {
            documents.push(document);
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        first_id,
        last_id,
        documents.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some() || min_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }
    builder.from_json(&documents)
}

pub(crate) async fn notification_request_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing notification request id route parameter".to_owned())
        })?;
    let Some(request) =
        resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
    else {
        return Response::error("notification request not found", 404);
    };
    let Some(document) = build_notification_request_document(&db, &config, &request).await? else {
        return Response::error("notification request not found", 404);
    };
    Response::from_json(&document)
}

pub(crate) async fn notification_requests_merged_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(_) => Response::from_json(&serde_json::json!({
            "merged": true,
        })),
        None => Response::error("Cloudflare Access authentication required", 401),
    }
}

pub(crate) async fn accept_notification_requests_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let ids = parse_notification_request_ids(req).await?;
    for request_id in ids {
        if let Some(request) =
            resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
        {
            let _ = authorize_pending_follow_request(&db, &config, &viewer, &request).await?;
        }
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn dismiss_notification_requests_response(
    req: &mut Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let ids = parse_notification_request_ids(req).await?;
    for request_id in ids {
        if let Some(request) =
            resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
        {
            let _ = reject_pending_follow_request(&db, &config, &viewer, &request).await?;
        }
    }
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn accept_notification_request_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing notification request id route parameter".to_owned())
        })?;
    let Some(request) =
        resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
    else {
        return Response::error("notification request not found", 404);
    };
    let _ = authorize_pending_follow_request(&db, &config, &viewer, &request).await?;
    Response::from_json(&serde_json::json!({}))
}

pub(crate) async fn dismiss_notification_request_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let (db, viewer) = match authenticated_follow_request_viewer(&req, &ctx, &config).await? {
        Some(auth) => auth,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            worker::Error::RustError("missing notification request id route parameter".to_owned())
        })?;
    let Some(request) =
        resolve_pending_follow_request(&db, &config, &viewer.id, &request_id).await?
    else {
        return Response::error("notification request not found", 404);
    };
    let _ = reject_pending_follow_request(&db, &config, &viewer, &request).await?;
    Response::from_json(&serde_json::json!({}))
}
