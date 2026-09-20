/// Every authenticated channel (`user`, `user:notification`, `direct`, `list`)
/// is served by one hub per account. Subscribers of those channels always
/// resolve to a single account, so a per-account session hub lets one socket
/// hold several subscriptions and keeps clients off other accounts' hubs.
pub(crate) fn stream_hub_session_id_name(account_id: &str) -> String {
    format!("user:{account_id}")
}

/// True when the channel is served by a per-account session hub.
pub(crate) fn stream_is_session_channel(stream: &str) -> bool {
    matches!(stream, "user" | "user:notification" | "direct" | "list")
}

/// Shared hub for a channel with an open audience.
pub(crate) fn stream_hub_channel_id_name(stream: &str, tag: Option<&str>) -> String {
    if stream.starts_with("hashtag") {
        return format!("hashtag:{}", tag.unwrap_or_default());
    }
    stream.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_channels_share_one_hub_per_account() {
        assert_eq!(stream_hub_session_id_name("acct-1"), "user:acct-1");
        assert_ne!(
            stream_hub_session_id_name("acct-1"),
            stream_hub_session_id_name("acct-2")
        );
    }

    #[test]
    fn stream_hub_channel_id_name_maps_open_channels() {
        assert_eq!(
            stream_hub_channel_id_name("hashtag", Some("rust")),
            "hashtag:rust"
        );
        assert_eq!(
            stream_hub_channel_id_name("hashtag:local", Some("rust")),
            "hashtag:rust"
        );
        assert_eq!(
            stream_hub_channel_id_name("public:local", None),
            "public:local"
        );
    }
}
