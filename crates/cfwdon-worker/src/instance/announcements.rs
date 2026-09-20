use super::{
    Request, Response, Result, RouteContext, load_config,
    publish_announcement_reaction_user_stream_soft, publish_announcement_user_stream_soft,
    require_authenticated_local_account,
};
use std::collections::{HashMap, HashSet};
use worker::Env;
use worker::console_error;
use worker::d1::D1Type;

#[derive(Debug, serde::Deserialize)]
struct AnnouncementDismissalRow {
    announcement_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct AnnouncementReactionCountRow {
    announcement_id: String,
    reaction_name: String,
    count: u64,
    me: u64,
}

pub(crate) fn build_announcements_document(
    config: &cfwdon_core::AppConfig,
    read_ids: &HashSet<String>,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> Vec<serde_json::Value> {
    let Some(raw) = config.announcements_json.as_deref() else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };

    items
        .into_iter()
        .filter_map(|item| announcement_document(item, read_ids, reaction_state))
        .collect()
}

fn announcement_document(
    item: serde_json::Value,
    read_ids: &HashSet<String>,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> Option<serde_json::Value> {
    let mut announcement = item.as_object()?.clone();
    let id = announcement.get("id")?.as_str()?.to_owned();
    announcement.insert(
        "read".to_owned(),
        serde_json::Value::Bool(read_ids.contains(&id)),
    );

    let reactions = announcement
        .get("reactions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|reaction| announcement_reaction_document(&id, reaction, reaction_state))
        .collect::<Vec<_>>();
    announcement.insert("reactions".to_owned(), serde_json::Value::Array(reactions));

    ensure_announcement_collection_fields(&mut announcement);

    Some(serde_json::Value::Object(announcement))
}

fn ensure_announcement_collection_fields(
    announcement: &mut serde_json::Map<String, serde_json::Value>,
) {
    for key in ["mentions", "statuses", "tags", "emojis"] {
        if !announcement.contains_key(key) {
            announcement.insert(key.to_owned(), serde_json::json!([]));
        }
    }
}

fn announcement_reaction_document(
    announcement_id: &str,
    reaction: serde_json::Value,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> Option<serde_json::Value> {
    let mut reaction = reaction.as_object()?.clone();
    let name = reaction.get("name")?.as_str()?.to_owned();
    let (count, me) =
        announcement_reaction_viewer_state(announcement_id, &name, &reaction, reaction_state);
    reaction.insert("count".to_owned(), serde_json::json!(count));
    reaction.insert("me".to_owned(), serde_json::Value::Bool(me));
    Some(serde_json::Value::Object(reaction))
}

fn announcement_reaction_viewer_state(
    announcement_id: &str,
    name: &str,
    reaction: &serde_json::Map<String, serde_json::Value>,
    reaction_state: &HashMap<(String, String), (u64, bool)>,
) -> (u64, bool) {
    reaction_state
        .get(&(announcement_id.to_owned(), name.to_owned()))
        .copied()
        .unwrap_or_else(|| {
            let count = reaction
                .get("count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let me = reaction
                .get("me")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            (count, me)
        })
}

fn find_configured_announcement_item(
    config: &cfwdon_core::AppConfig,
    announcement_id: &str,
) -> Option<serde_json::Value> {
    let raw = config.announcements_json.as_deref()?;
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(raw) else {
        return None;
    };
    items.into_iter().find(|item| {
        item.get("id")
            .and_then(serde_json::Value::as_str)
            .map(|id| id == announcement_id)
            .unwrap_or(false)
    })
}

fn configured_announcement_exists(config: &cfwdon_core::AppConfig, announcement_id: &str) -> bool {
    find_configured_announcement_item(config, announcement_id).is_some()
}

async fn build_viewer_announcement_stream_payload(
    config: &cfwdon_core::AppConfig,
    db: &crate::D1Database,
    account_id: &str,
    announcement_id: &str,
) -> Result<Option<String>> {
    let read_ids = list_announcement_read_ids(db, account_id).await?;
    let reaction_state = load_announcement_reaction_state(db, account_id).await?;
    let announcements = build_announcements_document(config, &read_ids, &reaction_state);
    for announcement in announcements {
        if announcement.get("id").and_then(serde_json::Value::as_str) == Some(announcement_id) {
            return Ok(Some(serde_json::to_string(&announcement)?));
        }
    }
    Ok(None)
}

async fn publish_announcement_reaction_stream_soft(
    env: &Env,
    config: &cfwdon_core::AppConfig,
    db: &crate::D1Database,
    account_id: &str,
    announcement_id: &str,
    reaction_name: &str,
) {
    let reaction_state = match load_announcement_reaction_state(db, account_id).await {
        Ok(state) => state,
        Err(error) => {
            console_error!(
                "failed to load announcement reaction state for stream hub publish (account {account_id}): {error}"
            );
            return;
        }
    };
    let (count, _) = reaction_state
        .get(&(announcement_id.to_owned(), reaction_name.to_owned()))
        .copied()
        .unwrap_or((0, false));
    publish_announcement_reaction_user_stream_soft(
        env,
        &config.stream_hub_binding,
        account_id,
        announcement_id,
        reaction_name,
        count,
    )
    .await;
}

async fn publish_announcement_stream_soft(
    env: &Env,
    config: &cfwdon_core::AppConfig,
    db: &crate::D1Database,
    account_id: &str,
    announcement_id: &str,
) {
    let payload = match build_viewer_announcement_stream_payload(
        config,
        db,
        account_id,
        announcement_id,
    )
    .await
    {
        Ok(Some(payload)) => payload,
        Ok(None) => return,
        Err(error) => {
            console_error!(
                "failed to build announcement stream payload (account {account_id}, announcement {announcement_id}): {error}"
            );
            return;
        }
    };
    publish_announcement_user_stream_soft(
        env,
        &config.stream_hub_binding,
        account_id,
        announcement_id,
        &payload,
    )
    .await;
}

pub(crate) async fn list_announcement_read_ids(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<HashSet<String>> {
    let rows = db
        .prepare(
            "SELECT announcement_id
             FROM account_announcement_dismissals
             WHERE account_id = ?1",
        )
        .bind_refs(&[D1Type::Text(account_id)])?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<AnnouncementDismissalRow>(&__d1))?;
    Ok(rows.into_iter().map(|row| row.announcement_id).collect())
}

pub(crate) async fn load_announcement_reaction_state(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<HashMap<(String, String), (u64, bool)>> {
    let rows = db
        .prepare(
            "SELECT
                announcement_id,
                reaction_name,
                COUNT(*) AS count,
                MAX(CASE WHEN account_id = ?1 THEN 1 ELSE 0 END) AS me
             FROM account_announcement_reactions
             GROUP BY announcement_id, reaction_name",
        )
        .bind_refs(&[D1Type::Text(account_id)])?
        .all()
        .await
        .and_then(|__d1| crate::d1_results::<AnnouncementReactionCountRow>(&__d1))?;
    let mut state = HashMap::new();
    for row in rows {
        if row.announcement_id.is_empty() || row.reaction_name.is_empty() {
            continue;
        }
        state.insert(
            (row.announcement_id, row.reaction_name),
            (row.count, row.me > 0),
        );
    }
    Ok(state)
}

async fn save_announcement_dismissal(
    db: &crate::D1Database,
    account_id: &str,
    announcement_id: &str,
) -> Result<()> {
    db.prepare(
        "INSERT INTO account_announcement_dismissals (
            account_id,
            announcement_id
         ) VALUES (?1, ?2)
         ON CONFLICT(account_id, announcement_id)
         DO UPDATE SET updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(&[D1Type::Text(account_id), D1Type::Text(announcement_id)])?
    .run()
    .await?;
    Ok(())
}

async fn save_announcement_reaction(
    db: &crate::D1Database,
    account_id: &str,
    announcement_id: &str,
    reaction_name: &str,
) -> Result<()> {
    db.prepare(
        "INSERT INTO account_announcement_reactions (
            account_id,
            announcement_id,
            reaction_name
         ) VALUES (?1, ?2, ?3)
         ON CONFLICT(account_id, announcement_id, reaction_name)
         DO UPDATE SET updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(&[
        D1Type::Text(account_id),
        D1Type::Text(announcement_id),
        D1Type::Text(reaction_name),
    ])?
    .run()
    .await?;
    Ok(())
}

async fn delete_announcement_reaction(
    db: &crate::D1Database,
    account_id: &str,
    announcement_id: &str,
    reaction_name: &str,
) -> Result<()> {
    db.prepare(
        "DELETE FROM account_announcement_reactions
         WHERE account_id = ?1
           AND announcement_id = ?2
           AND reaction_name = ?3",
    )
    .bind_refs(&[
        D1Type::Text(account_id),
        D1Type::Text(announcement_id),
        D1Type::Text(reaction_name),
    ])?
    .run()
    .await?;
    Ok(())
}

pub(crate) async fn announcements_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => {
            return Ok(Response::from_json(&serde_json::json!({
                "error": "This method requires an authenticated user",
            }))?
            .with_status(422));
        }
    };
    let read_ids = list_announcement_read_ids(&db, account.id()).await?;
    let reaction_state = load_announcement_reaction_state(&db, account.id()).await?;

    Response::from_json(&build_announcements_document(
        &config,
        &read_ids,
        &reaction_state,
    ))
}

pub(crate) async fn announcement_reaction_mutation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(announcement_id) = ctx.param("announcement_id") else {
        return Response::error("announcement not found", 404);
    };
    let Some(reaction_name) = ctx.param("id") else {
        return Response::error("reaction not found", 404);
    };
    if !configured_announcement_exists(&config, announcement_id) {
        return Response::error("announcement not found", 404);
    }

    match req.method().as_ref() {
        "DELETE" => {
            delete_announcement_reaction(&db, account.id(), announcement_id, reaction_name).await?
        }
        _ => save_announcement_reaction(&db, account.id(), announcement_id, reaction_name).await?,
    }

    publish_announcement_reaction_stream_soft(
        &ctx.env,
        &config,
        &db,
        account.id(),
        announcement_id,
        reaction_name,
    )
    .await;

    Ok(Response::empty()?.with_status(200))
}

pub(crate) async fn dismiss_announcement_mutation_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = crate::bind_request_d1(&ctx, &config)?;
    let account = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(announcement_id) = ctx.param("id") else {
        return Response::error("announcement not found", 404);
    };
    if !configured_announcement_exists(&config, announcement_id) {
        return Response::error("announcement not found", 404);
    }
    save_announcement_dismissal(&db, account.id(), announcement_id).await?;
    publish_announcement_stream_soft(&ctx.env, &config, &db, account.id(), announcement_id).await;
    Ok(Response::empty()?.with_status(200))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcement_document_adds_required_empty_collections() {
        let document = announcement_document(
            serde_json::json!({
                "id": "announcement-1",
                "content": "<p>Hello</p>"
            }),
            &HashSet::new(),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(document["read"], serde_json::json!(false));
        assert_eq!(document["mentions"], serde_json::json!([]));
        assert_eq!(document["statuses"], serde_json::json!([]));
        assert_eq!(document["tags"], serde_json::json!([]));
        assert_eq!(document["emojis"], serde_json::json!([]));
        assert_eq!(document["reactions"], serde_json::json!([]));
    }

    #[test]
    fn announcement_reaction_document_preserves_payload_defaults_without_viewer_state() {
        let reaction = announcement_reaction_document(
            "announcement-1",
            serde_json::json!({
                "name": "wave",
                "count": 5
            }),
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(reaction["count"], serde_json::json!(5));
        assert_eq!(reaction["me"], serde_json::json!(false));
    }

    #[test]
    fn announcement_reaction_document_prefers_viewer_state() {
        let reaction_state =
            HashMap::from([(("announcement-1".to_owned(), "wave".to_owned()), (7, true))]);
        let reaction = announcement_reaction_document(
            "announcement-1",
            serde_json::json!({
                "name": "wave",
                "count": 5,
                "me": false
            }),
            &reaction_state,
        )
        .unwrap();

        assert_eq!(reaction["count"], serde_json::json!(7));
        assert_eq!(reaction["me"], serde_json::json!(true));
    }
}
