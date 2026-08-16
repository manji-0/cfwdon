use super::{
    JOB_CARD_UNFURL, StatusRow, build_status_card_value, card_unfurl_payload, delete_status_poll,
    insert_status_poll, render_status_html, replace_local_status_hashtags,
    replace_local_status_mentions, require_status_by_id, soft_enqueue_background_job,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::PollDraft;
use worker::Result;
use worker::d1::D1Type;

use crate::D1Database;

pub(crate) async fn replace_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &PollDraft,
    updated_at: &str,
) -> Result<()> {
    delete_status_poll(db, status_id).await?;
    insert_status_poll(db, status_id, poll, updated_at).await
}

fn local_status_update_bindings<'a>(
    content_html: &'a str,
    text: &'a str,
    spoiler_text: &'a str,
    sensitive: bool,
    language: Option<&'a str>,
    card_json: Option<&'a str>,
    updated_at: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 8] {
    [
        D1Type::Text(content_html),
        D1Type::Text(text),
        D1Type::Text(spoiler_text),
        D1Type::Integer(if sensitive { 1 } else { 0 }),
        language.map_or(D1Type::Null, D1Type::Text),
        card_json.map_or(D1Type::Null, D1Type::Text),
        D1Type::Text(updated_at),
        D1Type::Text(status_id),
    ]
}

fn local_status_quote_policy_update_bindings<'a>(
    quote_approval_policy: &'a str,
    updated_at: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 3] {
    [
        D1Type::Text(quote_approval_policy),
        D1Type::Text(updated_at),
        D1Type::Text(status_id),
    ]
}

pub(crate) async fn update_local_status(
    db: &D1Database,
    config: &AppConfig,
    status: &StatusRow,
    text: &str,
    spoiler_text: &str,
    sensitive: bool,
    language: Option<&str>,
    updated_at: &str,
) -> Result<StatusRow> {
    let content_html = render_status_html(text);
    let card_json =
        build_status_card_value(text).and_then(|value| serde_json::to_string(&value).ok());
    let bindings = local_status_update_bindings(
        &content_html,
        text,
        spoiler_text,
        sensitive,
        language,
        card_json.as_deref(),
        updated_at,
        &status.id,
    );
    db.prepare(
        "UPDATE statuses
         SET content_html = ?1,
             text_content = ?2,
             spoiler_text = ?3,
             sensitive = ?4,
             language = ?5,
             card_json = ?6,
             updated_at = ?7
         WHERE id = ?8",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    replace_local_status_hashtags(db, &status.id, &status.account_id, &status.created_at, text)
        .await?;

    replace_local_status_mentions(db, config, &status.id, &status.created_at, text).await?;

    if card_json.is_some() {
        let _ = soft_enqueue_background_job(
            db,
            JOB_CARD_UNFURL,
            &card_unfurl_payload("local", &status.id),
            updated_at,
        )
        .await;
    }

    require_status_by_id(db, &status.id).await
}

pub(crate) async fn update_local_status_quote_approval_policy(
    db: &D1Database,
    status: &StatusRow,
    quote_approval_policy: &str,
    updated_at: &str,
) -> Result<StatusRow> {
    let bindings =
        local_status_quote_policy_update_bindings(quote_approval_policy, updated_at, &status.id);
    db.prepare(
        "UPDATE statuses
         SET quote_approval_policy = ?1,
             updated_at = ?2
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_status_by_id(db, &status.id).await
}

fn local_status_quote_state_update_bindings<'a>(
    quote_state: &'a str,
    updated_at: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 3] {
    [
        D1Type::Text(quote_state),
        D1Type::Text(updated_at),
        D1Type::Text(status_id),
    ]
}

pub(crate) async fn update_local_status_quote_state(
    db: &D1Database,
    status: &StatusRow,
    quote_state: cfwdon_domain::QuoteState,
    updated_at: &str,
) -> Result<StatusRow> {
    let bindings = local_status_quote_state_update_bindings(
        quote_state.as_str(),
        updated_at,
        status.id.as_str(),
    );
    db.prepare(
        "UPDATE statuses
         SET quote_state = ?1,
             updated_at = ?2
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_status_by_id(db, &status.id).await
}

pub(crate) async fn clear_local_status_quote(
    db: &D1Database,
    status: &StatusRow,
    updated_at: &str,
) -> Result<StatusRow> {
    update_local_status_quote_state(
        db,
        status,
        cfwdon_domain::QuoteState::quote_state_after_revoke(status.quote_state),
        updated_at,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_status_update_bindings_keep_sql_slot_order_stable() {
        let bindings = local_status_update_bindings(
            "<p>hello</p>",
            "hello",
            "cw",
            true,
            Some("ja"),
            Some("{\"url\":\"https://example.com\"}"),
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[0], D1Type::Text("<p>hello</p>")));
        assert!(matches!(bindings[1], D1Type::Text("hello")));
        assert!(matches!(bindings[2], D1Type::Text("cw")));
        assert!(matches!(bindings[3], D1Type::Integer(1)));
        assert!(matches!(bindings[4], D1Type::Text("ja")));
        assert!(matches!(
            bindings[5],
            D1Type::Text("{\"url\":\"https://example.com\"}")
        ));
        assert!(matches!(
            bindings[6],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(bindings[7], D1Type::Text("status-1")));
    }

    #[test]
    fn local_status_update_bindings_use_null_for_missing_language() {
        let bindings = local_status_update_bindings(
            "<p>hello</p>",
            "hello",
            "",
            false,
            None,
            None,
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[3], D1Type::Integer(0)));
        assert!(matches!(bindings[4], D1Type::Null));
        assert!(matches!(bindings[5], D1Type::Null));
    }

    #[test]
    fn local_status_quote_policy_update_bindings_keep_sql_slot_order_stable() {
        let bindings = local_status_quote_policy_update_bindings(
            "followers",
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[0], D1Type::Text("followers")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(bindings[2], D1Type::Text("status-1")));
    }

    #[test]
    fn local_status_quote_state_update_bindings_keep_sql_slot_order_stable() {
        let bindings = local_status_quote_state_update_bindings(
            "accepted",
            "2026-01-02T03:04:05.000Z",
            "status-1",
        );

        assert!(matches!(bindings[0], D1Type::Text("accepted")));
        assert!(matches!(
            bindings[1],
            D1Type::Text("2026-01-02T03:04:05.000Z")
        ));
        assert!(matches!(bindings[2], D1Type::Text("status-1")));
    }
}
