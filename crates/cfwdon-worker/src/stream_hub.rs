mod client;
mod durable_object;
mod events;
mod fanout;
mod logging;
mod messages;
mod naming;
mod retry;
mod upgrade;

pub(crate) use events::{
    StreamHubEvent, publish_direct_stream_hub_event_soft, publish_list_stream_hub_event_soft,
    publish_notification_stream_hub_event_soft, publish_stream_hub_event_soft,
    publish_user_stream_hub_event_soft,
};
pub(crate) use logging::log_stream_hub_connect_event;
pub(crate) use naming::{stream_hub_channel_id_name, stream_hub_session_id_name};
pub(crate) use retry::stream_hub_sse_should_reconnect;
pub(crate) use upgrade::{
    StreamHubUpgradeParams, connect_stream_hub_websocket, upgrade_stream_hub_websocket,
};

const STREAM_HUB_WEBSOCKET_PATH: &str = "/websocket";
const STREAM_HUB_PUBLISH_PATH: &str = "/publish";
const STREAM_HUB_FORWARD_REGISTER_PATH: &str = "/forward/register";
const STREAM_HUB_INTERNAL_ORIGIN: &str = "https://stream-hub";
const STORAGE_FORWARD_TARGETS_KEY: &str = "forward_targets";
/// Optional override for the Durable Object binding used for hub-to-hub
/// forwarding. Keep in sync with `AppConfig::stream_hub_binding`.
const STREAM_HUB_BINDING_VAR: &str = "STREAM_HUB_BINDING";
const STREAM_HUB_DEFAULT_BINDING: &str = "STREAM_HUB";
/// Internal headers the Worker sets on hub requests. They are stripped from
/// forwarded client headers so a client cannot claim another account or channel.
const STREAM_HUB_STREAM_HEADER: &str = "X-Stream";
const STREAM_HUB_TAG_HEADER: &str = "X-Stream-Tag";
const STREAM_HUB_LIST_HEADER: &str = "X-Stream-List";
const STREAM_HUB_ACCOUNT_HEADER: &str = "X-Account-Id";
const STREAM_HUB_INTERNAL_HEADERS: [&str; 4] = [
    STREAM_HUB_STREAM_HEADER,
    STREAM_HUB_TAG_HEADER,
    STREAM_HUB_LIST_HEADER,
    STREAM_HUB_ACCOUNT_HEADER,
];
