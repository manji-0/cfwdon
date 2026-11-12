//! Live subscriptions recorded in D1 (`stream_subscriptions`), so publishers
//! skip hubs nobody listens on.
//!
//! Each hub is the only writer of its own rows: whenever its sockets'
//! subscriptions change, it replaces its rows with what its live sockets hold.
//! A hub that receives a publish it delivers to no socket does the same, which
//! clears rows left behind by sockets that died without a close event.
//!
//! Publishers read the whole table once per [`STREAM_AUDIENCE_TTL_MS`] per
//! isolate. Streaming is best effort: an isolate holding an older snapshot
//! drops events for a subscription made within that window.

use super::messages::SocketSubscriptionState;
use super::naming::{stream_hub_channel_id_name, stream_is_session_channel};
use crate::runtime_config::load_config_from_env;
use crate::tracked_d1::D1Database;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;
use worker::d1::D1Type;
use worker::{Env, Result, console_error};

const STREAM_AUDIENCE_TTL_MS: f64 = 5_000.0;
/// Above this many rows the snapshot is not kept and every publish goes out,
/// as before the registry existed.
const STREAM_AUDIENCE_ROW_LIMIT: usize = 2_000;

/// One channel a hub has a subscriber for, keyed by the hub a publisher sends
/// that channel's events to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub(super) struct RegistryKey {
    target: String,
    stream: String,
    param: String,
}

impl RegistryKey {
    fn new(target: &str, stream: &str, tag: Option<&str>, list: Option<&str>) -> Self {
        Self {
            target: target.to_owned(),
            stream: stream.to_owned(),
            param: subscription_param(stream, tag, list).to_owned(),
        }
    }
}

/// The part of a subscription key `StreamSubscription::matches` compares
/// besides the stream name.
fn subscription_param<'a>(stream: &str, tag: Option<&'a str>, list: Option<&'a str>) -> &'a str {
    if stream.starts_with("hashtag") {
        tag.unwrap_or_default()
    } else if stream == "list" {
        list.unwrap_or_default()
    } else {
        ""
    }
}

/// Keys a socket on `hub` can receive events for. Session channels arrive on
/// the socket's own session hub; open channels arrive on their channel hub,
/// directly for a socket on that hub or by forwarding to a session hub.
fn socket_registry_keys(
    hub: &str,
    state: &SocketSubscriptionState,
) -> impl Iterator<Item = RegistryKey> {
    let on_session_hub = state.session_hub.as_deref() == Some(hub);
    state.subscriptions.iter().filter_map(move |subscription| {
        let target = if stream_is_session_channel(&subscription.stream) {
            hub.to_owned()
        } else {
            stream_hub_channel_id_name(&subscription.stream, subscription.tag.as_deref())
        };
        let reachable = if stream_is_session_channel(&subscription.stream) {
            on_session_hub
        } else {
            target == hub || on_session_hub
        };
        reachable.then(|| {
            RegistryKey::new(
                &target,
                &subscription.stream,
                subscription.tag.as_deref(),
                subscription.list.as_deref(),
            )
        })
    })
}

pub(super) fn hub_registry_keys<'a>(
    hub: &str,
    sockets: impl IntoIterator<Item = &'a SocketSubscriptionState>,
) -> BTreeSet<RegistryKey> {
    sockets
        .into_iter()
        .flat_map(|state| socket_registry_keys(hub, state).collect::<Vec<_>>())
        .collect()
}

/// Replaces `hub`'s rows with `keys`. Rows that already match cost a read,
/// not a write.
pub(super) async fn replace_hub_registry_rows(
    db: &D1Database,
    hub: &str,
    keys: &BTreeSet<RegistryKey>,
) -> Result<()> {
    let keys_json = serde_json::to_string(
        &keys
            .iter()
            .map(|key| [&key.target, &key.stream, &key.param])
            .collect::<Vec<_>>(),
    )
    .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let bindings = [D1Type::Text(hub), D1Type::Text(&keys_json)];
    let delete_stale = db
        .prepare(
            "DELETE FROM stream_subscriptions
             WHERE hub = ?1
               AND NOT EXISTS (
                 SELECT 1 FROM json_each(?2) AS k
                 WHERE json_extract(k.value, '$[0]') = stream_subscriptions.target
                   AND json_extract(k.value, '$[1]') = stream_subscriptions.stream
                   AND json_extract(k.value, '$[2]') = stream_subscriptions.param
               )",
        )
        .bind_refs(bindings.iter())?;
    if keys.is_empty() {
        delete_stale.run().await?;
        return Ok(());
    }
    let insert_new = db
        .prepare(
            "INSERT OR IGNORE INTO stream_subscriptions (hub, target, stream, param)
             SELECT ?1, json_extract(k.value, '$[0]'), json_extract(k.value, '$[1]'), json_extract(k.value, '$[2]')
             FROM json_each(?2) AS k",
        )
        .bind_refs(bindings.iter())?;
    db.batch(vec![delete_stale, insert_new]).await?;
    Ok(())
}

/// Which publish targets have a subscriber, as of the last registry read.
/// `None` when the registry could not be read or is too large to hold: then
/// every target counts as subscribed.
#[derive(Debug)]
pub(crate) struct StreamAudience(Option<HashSet<RegistryKey>>);

impl StreamAudience {
    pub(crate) fn reaches(
        &self,
        hub: &str,
        stream: &str,
        tag: Option<&str>,
        list: Option<&str>,
    ) -> bool {
        self.0
            .as_ref()
            .is_none_or(|keys| keys.contains(&RegistryKey::new(hub, stream, tag, list)))
    }

    /// True when some hub has a subscriber for `stream`, whatever its key.
    /// Lets fan-out skip the D1 reads that find recipients.
    pub(crate) fn has_stream(&self, stream: &str) -> bool {
        self.0
            .as_ref()
            .is_none_or(|keys| keys.iter().any(|key| key.stream == stream))
    }

    fn from_rows(rows: Vec<RegistryKey>) -> Self {
        Self((rows.len() <= STREAM_AUDIENCE_ROW_LIMIT).then(|| rows.into_iter().collect()))
    }
}

thread_local! {
    static STREAM_AUDIENCE: RefCell<Option<(f64, Rc<StreamAudience>)>> = const { RefCell::new(None) };
}

/// The current audience, read from D1 at most once per TTL per isolate.
pub(crate) async fn stream_audience(env: &Env) -> Rc<StreamAudience> {
    let now = js_sys::Date::now();
    let cached = STREAM_AUDIENCE.with(|cell| {
        cell.borrow()
            .as_ref()
            .filter(|(read_at, _)| now - read_at < STREAM_AUDIENCE_TTL_MS)
            .map(|(_, audience)| audience.clone())
    });
    if let Some(audience) = cached {
        return audience;
    }
    let audience = Rc::new(match load_stream_audience(env).await {
        Ok(audience) => audience,
        Err(error) => {
            console_error!("failed to read stream subscriptions: {error}");
            StreamAudience(None)
        }
    });
    STREAM_AUDIENCE.with(|cell| *cell.borrow_mut() = Some((now, audience.clone())));
    audience
}

async fn load_stream_audience(env: &Env) -> Result<StreamAudience> {
    let db = D1Database::new(env.d1(&load_config_from_env(env).database_binding)?);
    let limit = (STREAM_AUDIENCE_ROW_LIMIT + 1).to_string();
    let rows = db
        .prepare("SELECT target, stream, param FROM stream_subscriptions LIMIT ?1")
        .bind_refs(&D1Type::Text(&limit))?
        .all()
        .await?
        .results::<RegistryKey>()?;
    Ok(StreamAudience::from_rows(rows))
}

#[cfg(test)]
mod tests {
    use super::super::messages::StreamSubscription;
    use super::*;

    fn subscription(stream: &str, tag: Option<&str>, list: Option<&str>) -> StreamSubscription {
        StreamSubscription::new(
            stream.to_owned(),
            tag.map(str::to_owned),
            list.map(str::to_owned),
        )
    }

    fn audience_of(hub: &str, sockets: &[SocketSubscriptionState]) -> StreamAudience {
        StreamAudience::from_rows(hub_registry_keys(hub, sockets).into_iter().collect())
    }

    #[test]
    fn session_socket_registers_its_channels_under_the_hub_that_publishes_them() {
        let socket = SocketSubscriptionState {
            subscriptions: vec![
                subscription("user", None, None),
                subscription("list", None, Some("list-1")),
                subscription("public:local", None, None),
                subscription("hashtag", Some("rust"), None),
            ],
            session_hub: Some("user:acct-1".to_owned()),
            ..Default::default()
        };
        let audience = audience_of("user:acct-1", &[socket]);

        assert!(audience.reaches("user:acct-1", "user", None, None));
        assert!(audience.reaches("user:acct-1", "list", None, Some("list-1")));
        assert!(audience.reaches("public:local", "public:local", None, None));
        assert!(audience.reaches("hashtag:rust", "hashtag", Some("rust"), None));

        assert!(!audience.reaches("user:acct-1", "user:notification", None, None));
        assert!(!audience.reaches("user:acct-1", "list", None, Some("list-2")));
        assert!(!audience.reaches("user:acct-2", "user", None, None));
        assert!(!audience.reaches("public", "public", None, None));
        assert!(!audience.reaches("hashtag:rust", "hashtag:local", Some("rust"), None));
        assert!(!audience.reaches("hashtag:wasm", "hashtag", Some("wasm"), None));
    }

    #[test]
    fn anonymous_socket_registers_only_the_channel_its_hub_serves() {
        let socket = SocketSubscriptionState {
            subscriptions: vec![
                subscription("hashtag:local", Some("rust"), None),
                // Nothing forwards this to an anonymous socket on another hub.
                subscription("public", None, None),
                subscription("user", None, None),
            ],
            ..Default::default()
        };
        let audience = audience_of("hashtag:rust", &[socket]);

        assert!(audience.reaches("hashtag:rust", "hashtag:local", Some("rust"), None));
        assert!(!audience.reaches("public", "public", None, None));
        assert!(!audience.has_stream("user"));
    }

    #[test]
    fn hub_without_live_subscriptions_registers_nothing() {
        let audience = audience_of("user:acct-1", &[SocketSubscriptionState::default()]);
        assert!(!audience.has_stream("user"));
        assert!(!audience.reaches("user:acct-1", "user", None, None));
    }

    #[test]
    fn several_sockets_with_one_subscription_share_one_key() {
        let socket = || SocketSubscriptionState {
            subscriptions: vec![subscription("user", None, None)],
            session_hub: Some("user:acct-1".to_owned()),
            ..Default::default()
        };
        assert_eq!(
            hub_registry_keys("user:acct-1", &[socket(), socket()]).len(),
            1
        );
    }

    #[test]
    fn has_stream_answers_for_any_key_of_that_stream() {
        let audience = audience_of(
            "user:acct-1",
            &[SocketSubscriptionState {
                subscriptions: vec![subscription("list", None, Some("list-1"))],
                session_hub: Some("user:acct-1".to_owned()),
                ..Default::default()
            }],
        );
        assert!(audience.has_stream("list"));
        assert!(!audience.has_stream("user"));
    }

    #[test]
    fn oversized_or_unreadable_registry_publishes_everywhere() {
        let rows = (0..=STREAM_AUDIENCE_ROW_LIMIT)
            .map(|index| RegistryKey::new(&format!("user:{index}"), "user", None, None))
            .collect();
        let audience = StreamAudience::from_rows(rows);
        assert!(audience.reaches("public", "public", None, None));
        assert!(audience.has_stream("direct"));
    }
}
