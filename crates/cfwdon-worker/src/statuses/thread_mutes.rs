use super::{
    Error, Request, Response, Result, RouteContext, build_local_status_response,
    find_visible_local_status_response_subject, json_string_array, load_config,
    require_authenticated_local_account, sql_in_json_each, unique_ordered_refs,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use worker::d1::D1Type;

#[derive(Debug, Deserialize)]
struct StatusReplyLinkRow {
    id: String,
    in_reply_to_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ThreadRootStatusIdRow {
    thread_root_status_id: String,
}

async fn resolve_thread_root_status_id(
    db: &crate::D1Database,
    status: &crate::StatusRow,
) -> Result<String> {
    let roots = resolve_thread_root_status_ids(db, &[status]).await?;
    roots.get(&status.id).cloned().ok_or_else(|| {
        Error::RustError("failed to resolve thread root for local status".to_owned())
    })
}

/// Resolve thread-root status ids for many local statuses with batched parent fetches.
pub(crate) async fn resolve_thread_root_status_ids(
    db: &crate::D1Database,
    statuses: &[&crate::StatusRow],
) -> Result<HashMap<String, String>> {
    let mut parent_by_id = HashMap::new();
    for status in statuses {
        parent_by_id.insert(status.id.clone(), status.in_reply_to_id.clone());
    }

    loop {
        let mut missing = Vec::new();
        let mut seen_missing = HashSet::new();
        for parent in parent_by_id.values().filter_map(|parent| parent.as_ref()) {
            if !parent_by_id.contains_key(parent) && seen_missing.insert(parent.as_str()) {
                missing.push(parent.clone());
            }
        }
        if missing.is_empty() {
            break;
        }

        let loaded = load_status_reply_links(db, &missing).await?;
        for parent_id in missing {
            if let Some(in_reply_to_id) = loaded.get(&parent_id) {
                parent_by_id.insert(parent_id, in_reply_to_id.clone());
            }
            // Missing DB rows stay absent so walk stops at the child (legacy behavior).
        }
    }

    let mut roots = HashMap::with_capacity(statuses.len());
    for status in statuses {
        roots.insert(
            status.id.clone(),
            thread_root_from_parent_map(&status.id, &parent_by_id)?,
        );
    }
    Ok(roots)
}

fn thread_root_from_parent_map(
    start_id: &str,
    parent_by_id: &HashMap<String, Option<String>>,
) -> Result<String> {
    let mut current_id = start_id.to_owned();
    let mut seen = HashSet::new();

    loop {
        if !seen.insert(current_id.clone()) {
            return Err(Error::RustError(
                "detected cycle while resolving thread root".to_owned(),
            ));
        }
        let Some(parent_id) = parent_by_id
            .get(&current_id)
            .and_then(|parent| parent.as_ref())
        else {
            return Ok(current_id);
        };
        if !parent_by_id.contains_key(parent_id) {
            return Ok(current_id);
        }
        current_id = parent_id.clone();
    }
}

async fn load_status_reply_links(
    db: &crate::D1Database,
    status_ids: &[String],
) -> Result<HashMap<String, Option<String>>> {
    let ids = unique_ordered_refs(status_ids);
    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let ids_json = json_string_array(&ids);
    let sql = format!(
        "SELECT id, in_reply_to_id
         FROM statuses
         WHERE id {}",
        sql_in_json_each(1)
    );
    let binding = D1Type::Text(ids_json.as_str());
    let result = db.prepare(&sql).bind_refs(&binding)?.all().await?;

    Ok(crate::d1_results::<StatusReplyLinkRow>(&result)?
        .into_iter()
        .map(|row| (row.id, row.in_reply_to_id))
        .collect())
}

async fn load_muted_thread_root_status_ids(
    db: &crate::D1Database,
    account_id: &str,
    root_status_ids: &[String],
) -> Result<HashSet<String>> {
    let roots = unique_ordered_refs(root_status_ids);
    if roots.is_empty() {
        return Ok(HashSet::new());
    }

    let roots_json = json_string_array(&roots);
    let sql = format!(
        "SELECT thread_root_status_id
         FROM thread_mutes
         WHERE account_id = ?1
           AND thread_root_status_id {}",
        sql_in_json_each(2)
    );
    let bindings = [D1Type::Text(account_id), D1Type::Text(roots_json.as_str())];
    let result = db.prepare(&sql).bind_refs(bindings.iter())?.all().await?;

    Ok(crate::d1_results::<ThreadRootStatusIdRow>(&result)?
        .into_iter()
        .map(|row| row.thread_root_status_id)
        .collect())
}

/// Return the subset of status ids whose thread root is muted by `account_id`.
pub(crate) async fn local_status_ids_thread_muted_by(
    db: &crate::D1Database,
    account_id: &str,
    statuses: &[&crate::StatusRow],
) -> Result<HashSet<String>> {
    if statuses.is_empty() {
        return Ok(HashSet::new());
    }

    let roots_by_status_id = resolve_thread_root_status_ids(db, statuses).await?;
    let mut seen_roots = HashSet::new();
    let root_ids = roots_by_status_id
        .values()
        .filter(|root| seen_roots.insert(root.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let muted_roots = load_muted_thread_root_status_ids(db, account_id, &root_ids).await?;

    Ok(roots_by_status_id
        .into_iter()
        .filter(|(_, root)| muted_roots.contains(root))
        .map(|(status_id, _)| status_id)
        .collect())
}

pub(crate) async fn is_local_status_thread_muted_by(
    db: &crate::D1Database,
    account_id: &str,
    status: &crate::StatusRow,
) -> Result<bool> {
    let muted = local_status_ids_thread_muted_by(db, account_id, &[status]).await?;
    Ok(muted.contains(&status.id))
}

pub(crate) async fn account_has_thread_mutes(
    db: &crate::D1Database,
    account_id: &str,
) -> Result<bool> {
    Ok(crate::load_account_capabilities(db, account_id)
        .await?
        .has_thread_mutes)
}

async fn mute_thread_for_status(
    db: &crate::D1Database,
    account_id: &str,
    status: &crate::StatusRow,
) -> Result<()> {
    let root_status_id = resolve_thread_root_status_id(db, status).await?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(root_status_id.as_str()),
    ];
    db.prepare(
        "INSERT INTO thread_mutes (account_id, thread_root_status_id)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, thread_root_status_id) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    crate::invalidate_account_capabilities(account_id).await;
    Ok(())
}

async fn unmute_thread_for_status(
    db: &crate::D1Database,
    account_id: &str,
    status: &crate::StatusRow,
) -> Result<()> {
    let root_status_id = resolve_thread_root_status_id(db, status).await?;
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(root_status_id.as_str()),
    ];
    db.prepare(
        "DELETE FROM thread_mutes
         WHERE account_id = ?1
           AND thread_root_status_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    crate::invalidate_account_capabilities(account_id).await;
    Ok(())
}

pub(crate) async fn mute_status_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(subject) =
        find_visible_local_status_response_subject(&db, Some(&viewer), &status_id).await?
    else {
        return Response::error("status not found", 404);
    };

    mute_thread_for_status(&db, viewer.id(), &subject.status).await?;
    Response::from_json(&thread_mute_status_response(&db, &config, &viewer, subject).await?)
}

pub(crate) async fn unmute_status_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| worker::Error::RustError("missing status id route parameter".to_owned()))?;
    let db = crate::bind_request_d1(&ctx, &config)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(viewer) => viewer,
        None => return Response::error("Auth0 authentication required", 401),
    };
    let Some(subject) =
        find_visible_local_status_response_subject(&db, Some(&viewer), &status_id).await?
    else {
        return Response::error("status not found", 404);
    };

    unmute_thread_for_status(&db, viewer.id(), &subject.status).await?;
    Response::from_json(&thread_mute_status_response(&db, &config, &viewer, subject).await?)
}

async fn thread_mute_status_response(
    db: &crate::D1Database,
    config: &cfwdon_core::AppConfig,
    viewer: &cfwdon_domain::LocalAccount,
    subject: super::LoadedLocalStatusResponseSubject,
) -> Result<crate::MastodonStatusResponse> {
    let super::LoadedLocalStatusResponseSubject {
        status,
        account,
        preload,
    } = subject;
    build_local_status_response(
        db,
        config,
        Some(viewer),
        &status,
        &account,
        preload.in_reply_to_account_id,
        preload.media,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::thread_root_from_parent_map;
    use std::collections::HashMap;

    #[test]
    fn thread_root_from_parent_map_walks_to_root() {
        let parent_by_id = HashMap::from([
            ("reply-2".to_owned(), Some("reply-1".to_owned())),
            ("reply-1".to_owned(), Some("root".to_owned())),
            ("root".to_owned(), None),
        ]);

        assert_eq!(
            thread_root_from_parent_map("reply-2", &parent_by_id).unwrap(),
            "root"
        );
        assert_eq!(
            thread_root_from_parent_map("root", &parent_by_id).unwrap(),
            "root"
        );
    }

    #[test]
    fn thread_root_from_parent_map_stops_when_parent_row_is_missing() {
        let parent_by_id = HashMap::from([("reply".to_owned(), Some("missing-parent".to_owned()))]);

        assert_eq!(
            thread_root_from_parent_map("reply", &parent_by_id).unwrap(),
            "reply"
        );
    }

    #[test]
    fn thread_root_from_parent_map_detects_cycles() {
        let parent_by_id = HashMap::from([
            ("a".to_owned(), Some("b".to_owned())),
            ("b".to_owned(), Some("a".to_owned())),
        ]);

        assert!(thread_root_from_parent_map("a", &parent_by_id).is_err());
    }
}
