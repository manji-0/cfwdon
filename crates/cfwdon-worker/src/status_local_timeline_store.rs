use crate::{D1Database, Result, StatusRow, normalize_hashtag};
use worker::d1::D1Type;

pub(crate) async fn list_local_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [
        D1Type::Text(viewer_account_id),
        D1Type::Text(viewer_account_id),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT DISTINCT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.created_at
             FROM statuses s
             LEFT JOIN follows f
               ON f.target_account_id = s.account_id
              AND f.follower_account_id = ?1
              AND f.state = 'accepted'
             WHERE s.account_id = ?2
                OR (
                    f.follower_account_id IS NOT NULL
                    AND s.visibility IN ('public', 'unlisted', 'private')
                )
             ORDER BY s.created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}

pub(crate) async fn list_local_public_timeline_statuses(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE visibility = 'public'
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<StatusRow>()
}

pub(crate) async fn list_local_public_statuses_by_tag(
    db: &D1Database,
    tag: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let pattern = format!("%#{}%", normalize_hashtag(tag));
    let bindings = [
        D1Type::Text(pattern.as_str()),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE visibility = 'public'
               AND lower(text_content) LIKE ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}
