use crate::content_helpers::{
    extract_account_handles_from_text, extract_hashtags_from_html, extract_hashtags_from_text,
    extract_mentions_from_text,
};
use cfwdon_core::AppConfig;

#[test]
fn extract_hashtags_from_text_deduplicates_and_normalizes() {
    assert_eq!(
        extract_hashtags_from_text("Hello #Rust #rust and #fediverse_test"),
        vec!["rust".to_owned(), "fediverse_test".to_owned()]
    );
}

#[test]
fn extract_hashtags_from_html_ignores_markup() {
    assert_eq!(
        extract_hashtags_from_html(
            "<p><a href=\"https://example/tags/rust\">#<span>Rust</span></a> and #Workers</p>"
        ),
        vec!["rust".to_owned(), "workers".to_owned()]
    );
}

#[test]
fn extract_mentions_from_text_finds_local_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_mentions_from_text(
        "@alice hi @bob@social.example and @carol@remote.example",
        &config,
    );
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].username, "alice");
    assert_eq!(mentions[1].username, "bob");
}

#[test]
fn extract_mentions_from_text_deduplicates_local_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_mentions_from_text("@alice @alice@social.example", &config);
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].username, "alice");
}

#[test]
fn extract_account_handles_from_text_keeps_remote_mentions() {
    let config = AppConfig::new("social.example", "cfwdon", "test");
    let mentions = extract_account_handles_from_text("@alice @bob@remote.example @alice", &config);
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].username, "alice");
    assert_eq!(mentions[0].domain.as_deref(), Some("social.example"));
    assert_eq!(mentions[1].username, "bob");
    assert_eq!(mentions[1].domain.as_deref(), Some("remote.example"));
}
