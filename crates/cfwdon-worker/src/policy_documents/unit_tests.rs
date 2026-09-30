use crate::policy_documents::configured_html_document;

#[test]
fn configured_html_document_builds_privacy_and_terms_shapes() {
    let privacy = configured_html_document(
        Some("<p>Privacy</p>"),
        Some("2026-01-01T00:00:00Z"),
        "1970-01-01T00:00:00Z",
        false,
    )
    .unwrap();
    assert_eq!(
        privacy,
        serde_json::json!({
            "updated_at": "2026-01-01T00:00:00Z",
            "content": "<p>Privacy</p>",
        })
    );

    let terms =
        configured_html_document(Some("<p>Terms</p>"), Some("2026-02-01"), "1970-01-01", true)
            .unwrap();
    assert_eq!(
        terms,
        serde_json::json!({
            "effective_date": "2026-02-01",
            "effective": true,
            "content": "<p>Terms</p>",
            "succeeded_by": serde_json::Value::Null,
        })
    );
}
