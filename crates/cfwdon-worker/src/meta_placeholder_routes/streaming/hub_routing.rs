use crate::{stream_hub_channel_id_name, stream_hub_session_id_name};

pub(super) fn stream_hub_proxy_target(
    stream: &str,
    viewer: Option<&cfwdon_domain::LocalAccount>,
    tag: Option<&str>,
    list: Option<&str>,
) -> Option<(String, Option<String>)> {
    let tag = tag.map(str::trim).filter(|value| !value.is_empty());
    let list = list.map(str::trim).filter(|value| !value.is_empty());

    match stream {
        // Authenticated channels all live on the viewer's own session hub, so a
        // list id from another account can never reach that account's events.
        "user" | "user:notification" | "direct" => viewer.map(|viewer| {
            (
                stream_hub_session_id_name(viewer.id()),
                Some(viewer.id().to_owned()),
            )
        }),
        "list" => match (viewer, list) {
            (Some(viewer), Some(_)) => Some((
                stream_hub_session_id_name(viewer.id()),
                Some(viewer.id().to_owned()),
            )),
            _ => None,
        },
        // Authenticated clients always land on their session hub; the open
        // channel hub forwards matching events there, so one socket can mix
        // session and open channels.
        "public"
        | "public:media"
        | "public:local"
        | "public:local:media"
        | "public:remote"
        | "public:remote:media" => {
            if let Some(viewer) = viewer {
                return Some((
                    stream_hub_session_id_name(viewer.id()),
                    Some(viewer.id().to_owned()),
                ));
            }
            Some((stream_hub_channel_id_name(stream, None), None))
        }
        "hashtag" | "hashtag:local" => {
            let tag_value = tag?;
            if let Some(viewer) = viewer {
                return Some((
                    stream_hub_session_id_name(viewer.id()),
                    Some(viewer.id().to_owned()),
                ));
            }
            Some((stream_hub_channel_id_name(stream, Some(tag_value)), None))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_hub_proxy_test_account() -> cfwdon_domain::LocalAccount {
        cfwdon_domain::LocalAccount::from_record(cfwdon_domain::LocalAccountRecord::test_fixture(
            "acct-1", "alice",
        ))
    }

    #[test]
    fn stream_hub_proxy_target_maps_authenticated_channels() {
        let viewer = stream_hub_proxy_test_account();
        assert_eq!(
            stream_hub_proxy_target("user", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert_eq!(
            stream_hub_proxy_target("user:notification", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert_eq!(
            stream_hub_proxy_target("direct", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        // A list id from another account still resolves to the viewer's own hub.
        assert_eq!(
            stream_hub_proxy_target("list", Some(&viewer), None, Some("list-of-someone-else")),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert!(stream_hub_proxy_target("user", None, None, None).is_none());
        assert!(stream_hub_proxy_target("list", Some(&viewer), None, None).is_none());
        assert!(stream_hub_proxy_target("list", None, None, Some("list-1")).is_none());
    }

    #[test]
    fn stream_hub_proxy_target_maps_public_and_hashtag_channels() {
        let viewer = stream_hub_proxy_test_account();
        assert_eq!(
            stream_hub_proxy_target("public:local", None, None, None),
            Some(("public:local".to_owned(), None))
        );
        assert_eq!(
            stream_hub_proxy_target("public", Some(&viewer), None, None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert_eq!(
            stream_hub_proxy_target("hashtag", None, Some("rust"), None),
            Some(("hashtag:rust".to_owned(), None))
        );
        assert_eq!(
            stream_hub_proxy_target("hashtag:local", Some(&viewer), Some("rust"), None),
            Some(("user:acct-1".to_owned(), Some("acct-1".to_owned())))
        );
        assert!(stream_hub_proxy_target("hashtag", None, None, None).is_none());
        assert!(stream_hub_proxy_target("unknown", None, None, None).is_none());
    }
}
