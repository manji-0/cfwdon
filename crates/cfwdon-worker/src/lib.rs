use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cfwdon_core::{AppConfig, AuthenticatedUser, BuildMetadata};
use cfwdon_domain::{
    AccountHandle, InstanceCapabilities, InstanceSummary, LocalAccount, PollDraft, ProfileField,
    SoftwareInfo, StatusDraft, Visibility,
};
use js_sys::{Array, Object, Reflect, Uint8Array};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::net::IpAddr;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Algorithm, CryptoKey, CryptoKeyPair, RsaHashedImportParams, WorkerGlobalScope};
use worker::d1::D1Type;
use worker::*;

#[derive(Debug, Serialize)]
struct RootDocument {
    service: String,
    version: String,
    runtime: String,
    endpoints: Vec<&'static str>,
}

#[derive(Debug, Deserialize)]
struct AccountRow {
    id: String,
    username: String,
    access_email: String,
    display_name: String,
    bio_html: String,
    bio_text: String,
    fields_json: String,
    discoverable: i32,
    default_post_visibility: String,
    default_sensitive: i32,
    default_language: Option<String>,
    avatar_object_key: Option<String>,
    avatar_content_type: Option<String>,
    header_object_key: Option<String>,
    header_content_type: Option<String>,
    private_key_jwk: String,
    public_key_pem: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct InstanceSettingsRow {
    domain: String,
    title: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct ActiveMonthCountRow {
    count: u64,
}

#[derive(Debug, Default)]
struct AccountStats {
    followers_count: u64,
    following_count: u64,
    statuses_count: u64,
}

#[derive(Debug, Serialize)]
struct MastodonAccountResponse {
    id: String,
    username: String,
    acct: String,
    display_name: String,
    locked: bool,
    bot: bool,
    created_at: String,
    note: String,
    url: String,
    avatar: String,
    avatar_static: String,
    header: String,
    header_static: String,
    fields: Vec<serde_json::Value>,
    followers_count: u64,
    following_count: u64,
    statuses_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<MastodonAccountSource>,
}

#[derive(Debug, Serialize)]
struct MastodonAccountSource {
    note: String,
    fields: Vec<serde_json::Value>,
    privacy: String,
    sensitive: bool,
    language: String,
    follow_requests_count: u64,
    hide_collections: Option<bool>,
    discoverable: Option<bool>,
}

#[derive(Debug, Serialize)]
struct MastodonNotificationResponse {
    id: String,
    #[serde(rename = "type")]
    notification_type: String,
    group_key: String,
    created_at: String,
    account: MastodonAccountResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<MastodonStatusResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    report: Option<serde_json::Value>,
}

#[derive(Debug)]
struct NotificationEntry {
    id: String,
    created_at: String,
    value: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct WebFingerQuery {
    resource: String,
}

#[derive(Debug, Serialize)]
struct WebFingerResponse {
    subject: String,
    links: Vec<WebFingerLink>,
}

#[derive(Debug, Serialize)]
struct WebFingerLink {
    rel: &'static str,
    #[serde(rename = "type")]
    link_type: &'static str,
    href: String,
}

#[derive(Debug, Serialize)]
struct ActivityPubActorResponse {
    #[serde(rename = "@context")]
    context: Vec<&'static str>,
    id: String,
    #[serde(rename = "type")]
    actor_type: &'static str,
    #[serde(rename = "preferredUsername")]
    preferred_username: String,
    name: String,
    summary: String,
    inbox: String,
    outbox: String,
    followers: String,
    following: String,
    url: String,
    endpoints: ActivityPubActorEndpoints,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<ActivityPubImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<ActivityPubImage>,
    attachment: Vec<serde_json::Value>,
    #[serde(rename = "publicKey")]
    public_key: ActivityPubPublicKey,
    #[serde(rename = "manuallyApprovesFollowers")]
    manually_approves_followers: bool,
    discoverable: bool,
    published: String,
}

#[derive(Debug, Serialize)]
struct ActivityPubActorEndpoints {
    #[serde(rename = "sharedInbox")]
    shared_inbox: String,
}

#[derive(Debug, Serialize)]
struct ActivityPubPublicKey {
    id: String,
    owner: String,
    #[serde(rename = "publicKeyPem")]
    public_key_pem: String,
}

#[derive(Debug, Serialize)]
struct ActivityPubImage {
    #[serde(rename = "type")]
    image_type: &'static str,
    #[serde(rename = "mediaType")]
    media_type: String,
    url: String,
}

#[derive(Debug)]
struct AccountKeyMaterial {
    private_key_jwk: String,
    public_key_pem: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaKind {
    Image,
    Video,
    Audio,
}

#[derive(Debug)]
struct MediaUploadDraft {
    bytes: Vec<u8>,
    content_type: String,
    description: String,
    kind: MediaKind,
}

#[derive(Debug)]
struct ProfileMediaUpload {
    bytes: Vec<u8>,
    content_type: String,
    object_kind: &'static str,
}

#[derive(Debug, Default, Deserialize)]
struct CreateStatusRequest {
    status: Option<String>,
    media_ids: Option<Vec<String>>,
    poll: Option<CreateStatusPollRequest>,
    in_reply_to_id: Option<String>,
    sensitive: Option<bool>,
    spoiler_text: Option<String>,
    visibility: Option<String>,
    language: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CreateStatusPollRequest {
    options: Option<Vec<String>>,
    expires_in: Option<u64>,
    multiple: Option<bool>,
    hide_totals: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateCredentialsRequest {
    display_name: Option<String>,
    note: Option<String>,
    fields_attributes: Option<Vec<UpdateCredentialsField>>,
    discoverable: Option<bool>,
    source: Option<UpdateCredentialsSource>,
    #[serde(skip_deserializing)]
    avatar: Option<ProfileMediaUpload>,
    #[serde(skip_deserializing)]
    header: Option<ProfileMediaUpload>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateCredentialsSource {
    privacy: Option<String>,
    sensitive: Option<bool>,
    language: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateCredentialsField {
    name: Option<String>,
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StatusRow {
    id: String,
    account_id: String,
    ap_id: Option<String>,
    in_reply_to_id: Option<String>,
    content_html: String,
    #[serde(rename = "text_content")]
    _text_content: String,
    spoiler_text: String,
    visibility: String,
    sensitive: i32,
    language: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct StatusPollRow {
    id: String,
    status_id: String,
    multiple: i32,
    hide_totals: i32,
    expires_at: String,
}

#[derive(Debug, Deserialize)]
struct StatusPollOptionRow {
    title: String,
    votes_count: i64,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusPollRow {
    id: String,
    status_id: String,
    multiple: i32,
    expires_at: Option<String>,
    voters_count: Option<i64>,
    votes_count: i64,
    expired: i32,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusPollOptionRow {
    title: String,
    votes_count: i64,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusPollVoteRow {
    option_position: i64,
    option_title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusPollVoteWithIdRow {
    id: String,
    option_position: i64,
    option_title: Option<String>,
}

#[derive(Debug, Serialize)]
struct MastodonStatusResponse {
    id: String,
    created_at: String,
    in_reply_to_id: Option<String>,
    in_reply_to_account_id: Option<String>,
    sensitive: bool,
    spoiler_text: String,
    visibility: String,
    language: Option<String>,
    uri: String,
    url: String,
    replies_count: u64,
    reblogs_count: u64,
    favourites_count: u64,
    favourited: bool,
    reblogged: bool,
    muted: bool,
    bookmarked: bool,
    pinned: bool,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    reblog: Option<serde_json::Value>,
    application: Option<serde_json::Value>,
    account: MastodonAccountResponse,
    media_attachments: Vec<serde_json::Value>,
    mentions: Vec<serde_json::Value>,
    tags: Vec<serde_json::Value>,
    emojis: Vec<serde_json::Value>,
    card: Option<serde_json::Value>,
    poll: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct MastodonPollResponse {
    id: String,
    expires_at: String,
    expired: bool,
    multiple: bool,
    votes_count: u64,
    voters_count: Option<u64>,
    voted: bool,
    own_votes: Vec<u32>,
    options: Vec<MastodonPollOptionResponse>,
    emojis: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct MastodonPollOptionResponse {
    title: String,
    votes_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PollVoteRequest {
    choices: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct PollVoteTargetRow {
    poll_id: String,
    status_id: String,
    status_account_id: String,
    option_position: i64,
}

#[derive(Debug, Deserialize)]
struct PollVoteIdRow {
    id: String,
}

#[derive(Debug)]
struct RemotePollDraft {
    multiple: bool,
    expires_at: Option<String>,
    voters_count: Option<u64>,
    votes_count: u64,
    expired: bool,
    options: Vec<RemotePollOptionDraft>,
}

#[derive(Debug)]
struct RemotePollOptionDraft {
    title: String,
    votes_count: u64,
}

#[derive(Debug, Default, Deserialize)]
struct CreateReportRequest {
    account_id: String,
    status_ids: Option<Vec<String>>,
    comment: Option<String>,
    category: Option<String>,
    forward: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ReportRow {
    id: String,
    account_id: String,
    target_account_id: String,
    #[serde(rename = "target_remote_actor_uri")]
    _target_remote_actor_uri: Option<String>,
    comment: String,
    category: String,
    forward: i32,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct MastodonReportResponse {
    id: String,
    action_taken: bool,
    action_taken_at: Option<String>,
    category: String,
    comment: String,
    forwarded: bool,
    created_at: String,
    status_ids: Option<Vec<String>>,
    target_account: MastodonAccountResponse,
    rule_ids: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize)]
struct MediaAttachmentRow {
    id: String,
    account_id: String,
    status_id: Option<String>,
    object_key: String,
    content_type: String,
    description: String,
    focus_x: Option<f64>,
    focus_y: Option<f64>,
    #[serde(rename = "created_at")]
    _created_at: String,
}

#[derive(Debug, Serialize)]
struct MastodonMediaAttachmentResponse {
    id: String,
    #[serde(rename = "type")]
    media_type: &'static str,
    url: String,
    preview_url: String,
    remote_url: Option<String>,
    text_url: Option<String>,
    meta: MastodonMediaMeta,
    description: Option<String>,
    blurhash: Option<String>,
}

#[derive(Debug, Serialize)]
struct MastodonMediaMeta {
    original: Option<MastodonMediaMetaDetails>,
    small: Option<MastodonMediaMetaDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus: Option<MastodonMediaFocus>,
}

#[derive(Debug, Serialize)]
struct MastodonMediaMetaDetails {
    width: Option<u32>,
    height: Option<u32>,
    size: Option<String>,
    aspect: Option<f64>,
}

#[derive(Debug, Serialize)]
struct MastodonMediaFocus {
    x: f64,
    y: f64,
}

#[derive(Debug, Default, Deserialize)]
struct UpdateMediaRequest {
    description: Option<String>,
    focus: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DeleteStatusQuery {
    delete_media: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct AccountStatusesQuery {
    limit: Option<u32>,
    only_media: Option<bool>,
    exclude_replies: Option<bool>,
    exclude_reblogs: Option<bool>,
    pinned: Option<bool>,
    tagged: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccountLookupQuery {
    acct: String,
}

#[derive(Debug, Default, Deserialize)]
struct AccountSearchQuery {
    q: String,
    limit: Option<u32>,
    offset: Option<u32>,
    resolve: Option<bool>,
    following: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct DirectoryQuery {
    limit: Option<u32>,
    offset: Option<u32>,
    local: Option<bool>,
    order: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TagTimelineQuery {
    limit: Option<u32>,
    only_media: Option<bool>,
    local: Option<bool>,
    remote: Option<bool>,
    #[serde(rename = "any[]")]
    any: Option<Vec<String>>,
    #[serde(rename = "all[]")]
    all: Option<Vec<String>>,
    #[serde(rename = "none[]")]
    none: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct HomeTimelineQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    _max_id: Option<String>,
    #[serde(rename = "since_id")]
    _since_id: Option<String>,
    #[serde(rename = "min_id")]
    _min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CollectionPagingQuery {
    page: Option<bool>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct FavouritesQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    _max_id: Option<String>,
    #[serde(rename = "since_id")]
    _since_id: Option<String>,
    #[serde(rename = "min_id")]
    _min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct BookmarksQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    _max_id: Option<String>,
    #[serde(rename = "since_id")]
    _since_id: Option<String>,
    #[serde(rename = "min_id")]
    _min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct NotificationsQuery {
    limit: Option<u32>,
    account_id: Option<String>,
    #[serde(rename = "types[]")]
    types: Option<Vec<String>>,
    #[serde(rename = "exclude_types[]")]
    exclude_types: Option<Vec<String>>,
    #[serde(rename = "max_id")]
    _max_id: Option<String>,
    #[serde(rename = "since_id")]
    _since_id: Option<String>,
    #[serde(rename = "min_id")]
    _min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct MuteAccountRequest {
    notifications: Option<bool>,
    duration: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct MutesQuery {
    limit: Option<u32>,
    #[serde(rename = "max_id")]
    max_id: Option<String>,
    #[serde(rename = "since_id")]
    since_id: Option<String>,
    #[serde(rename = "min_id")]
    min_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ReblogStatusRequest {
    visibility: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SearchV2Query {
    q: String,
    #[serde(rename = "type")]
    search_type: Option<String>,
    resolve: Option<bool>,
    following: Option<bool>,
    account_id: Option<String>,
    #[serde(rename = "exclude_unreviewed")]
    _exclude_unreviewed: Option<bool>,
    #[serde(rename = "max_id")]
    _max_id: Option<String>,
    #[serde(rename = "min_id")]
    _min_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SearchCategoryFlags {
    accounts: bool,
    statuses: bool,
    hashtags: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirectoryOrder {
    Active,
    New,
}

#[derive(Debug, Default, Deserialize)]
struct FollowAccountRequest {
    reblogs: Option<bool>,
    notify: Option<bool>,
    languages: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct FollowerTargetRow {
    target_inbox: String,
}

#[derive(Debug, Deserialize)]
struct UsernameRow {
    username: String,
}

#[derive(Debug, Deserialize)]
struct FollowRow {
    follower_account_id: String,
    #[serde(rename = "target_account_id")]
    _target_account_id: Option<String>,
    target_actor_uri: String,
    #[serde(rename = "follow_activity_id")]
    _follow_activity_id: Option<String>,
    state: String,
    show_reblogs: i32,
    notify: i32,
    languages_json: Option<String>,
}

#[derive(Debug, Serialize)]
struct RelationshipResponse {
    id: String,
    following: bool,
    showing_reblogs: bool,
    notifying: bool,
    languages: Option<Vec<String>>,
    followed_by: bool,
    blocking: bool,
    blocked_by: bool,
    muting: bool,
    muting_notifications: bool,
    muting_expires_at: Option<String>,
    requested: bool,
    requested_by: bool,
    domain_blocking: bool,
    endorsed: bool,
    note: String,
}

#[derive(Debug, Default, Serialize)]
struct MastodonSearchResponse {
    accounts: Vec<MastodonAccountResponse>,
    statuses: Vec<MastodonStatusResponse>,
    hashtags: Vec<MastodonTagResponse>,
}

#[derive(Debug, Default, Serialize)]
struct MastodonContextResponse {
    ancestors: Vec<MastodonStatusResponse>,
    descendants: Vec<MastodonStatusResponse>,
}

#[derive(Debug, Serialize)]
struct MastodonTagResponse {
    id: String,
    name: String,
    url: String,
    history: Vec<MastodonTagHistoryEntry>,
    following: bool,
    featured: bool,
}

#[derive(Debug, Serialize)]
struct MastodonTagHistoryEntry {
    day: String,
    uses: String,
    accounts: String,
}

enum AccountReference {
    Local(LocalAccount),
    Remote(RemoteActorRow),
}

#[derive(Debug, Deserialize)]
struct OutboxDeliveryRow {
    id: String,
    account_id: String,
    status_id: String,
    activity_id: String,
    activity_type: String,
    target_inbox: Option<String>,
    payload_json: String,
    attempt_count: i32,
}

#[derive(Debug, Deserialize)]
struct OutboundActivityRow {
    id: String,
    account_id: String,
    activity_id: String,
    activity_type: String,
    target_actor_uri: Option<String>,
    target_inbox: String,
    payload_json: String,
    attempt_count: i32,
}

#[derive(Debug, Default, Serialize)]
struct OutboxProcessResponse {
    expanded: u32,
    delivered: u32,
    failed: u32,
    completed_without_targets: u32,
}

#[derive(Debug, Default, Serialize)]
struct OrphanMediaPruneResponse {
    deleted: u32,
}

#[derive(Debug, Default, Serialize)]
struct PollExpirationProcessResponse {
    queued: u32,
}

#[derive(Debug)]
struct RemoteActorProfile {
    actor_uri: String,
    username: String,
    domain: String,
    inbox_uri: String,
    shared_inbox_uri: Option<String>,
    public_key_id: String,
    public_key_pem: String,
    display_name: String,
    summary_html: String,
    profile_url: Option<String>,
    avatar_url: Option<String>,
    header_url: Option<String>,
}

#[derive(Debug)]
struct ParsedSignatureHeader {
    key_id: String,
    headers: Vec<String>,
    signature: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct RemoteActorRow {
    actor_uri: String,
    username: String,
    domain: String,
    display_name: String,
    summary_html: String,
    profile_url: Option<String>,
    avatar_url: Option<String>,
    header_url: Option<String>,
}

impl RemoteActorRow {
    fn from_value(value: &serde_json::Value) -> Self {
        Self {
            actor_uri: value
                .get("actor_uri")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            username: value
                .get("username")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            domain: value
                .get("domain")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            display_name: value
                .get("display_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            summary_html: value
                .get("summary_html")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            profile_url: value
                .get("profile_url")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            avatar_url: value
                .get("avatar_url")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            header_url: value
                .get("header_url")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RemoteStatusRow {
    id: String,
    actor_uri: String,
    object_uri: String,
    url: Option<String>,
    in_reply_to_uri: Option<String>,
    content_html: String,
    spoiler_text: String,
    visibility: String,
    sensitive: i32,
    language: Option<String>,
    published_at: String,
}

#[derive(Debug, Deserialize)]
struct OrphanMediaRow {
    id: String,
    object_key: String,
}

#[derive(Debug, Deserialize)]
struct AccessJwtHeader {
    alg: String,
    kid: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AccessAudClaim {
    Single(String),
    Multiple(Vec<String>),
}

impl AccessAudClaim {
    fn contains(&self, expected: &str) -> bool {
        match self {
            Self::Single(value) => value == expected,
            Self::Multiple(values) => values.iter().any(|value| value == expected),
        }
    }
}

#[derive(Debug, Deserialize)]
struct AccessJwtClaims {
    iss: String,
    aud: AccessAudClaim,
    email: Option<String>,
    exp: Option<u64>,
    nbf: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AccessJwk {
    kid: String,
    kty: String,
    alg: String,
    #[serde(rename = "use")]
    use_: String,
    e: String,
    n: String,
}

#[derive(Debug, Deserialize)]
struct AccessCertsResponse {
    keys: Vec<AccessJwk>,
}

#[derive(Debug, Deserialize)]
struct DnsJsonResponse {
    #[serde(rename = "Status")]
    status: u32,
    #[serde(rename = "Answer")]
    answer: Option<Vec<DnsJsonAnswer>>,
}

#[derive(Debug, Deserialize)]
struct DnsJsonAnswer {
    data: String,
}

#[derive(Debug, Deserialize)]
struct FavouriteEntryRow {
    status_id: Option<String>,
    remote_status_id: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct InteractionActivityRow {
    ap_activity_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReblogActivityRow {
    ap_activity_id: Option<String>,
    visibility: String,
}

#[derive(Debug, Deserialize)]
struct ExpiredPollQueueRow {
    poll_id: String,
    status_id: String,
    account_id: String,
}

#[derive(Debug, Deserialize)]
struct LocalFollowNotificationRow {
    follower_account_id: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct RemoteFollowNotificationRow {
    actor_uri: String,
    created_at: String,
}

#[derive(Debug, PartialEq, Eq)]
struct OutboundActivityDescriptor {
    activity_id: String,
    activity_type: String,
}

#[derive(Debug, Deserialize)]
struct MuteRow {
    notifications: i32,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MuteEntryRow {
    cursor_id: i64,
    target_account_id: Option<String>,
    target_actor_uri: String,
}

#[derive(Debug, Deserialize)]
struct FavouriteNotificationRow {
    account_id: String,
    status_id: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct ReblogNotificationRow {
    account_id: String,
    status_id: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct MentionNotificationRow {
    id: String,
    account_id: String,
    ap_id: Option<String>,
    in_reply_to_id: Option<String>,
    content_html: String,
    #[serde(rename = "text_content")]
    text_content: String,
    spoiler_text: String,
    visibility: String,
    sensitive: i32,
    language: Option<String>,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct RemoteMentionNotificationRow {
    id: String,
    actor_uri: String,
    object_uri: String,
    url: Option<String>,
    in_reply_to_uri: Option<String>,
    content_html: String,
    spoiler_text: String,
    visibility: String,
    sensitive: i32,
    language: Option<String>,
    published_at: String,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusInteractionRow {
    remote_actor_uri: String,
    status_id: String,
    created_at: String,
}

#[derive(Debug, Deserialize)]
struct RemoteStatusNotificationRow {
    id: String,
    actor_uri: String,
    object_uri: String,
    url: Option<String>,
    in_reply_to_uri: Option<String>,
    content_html: String,
    spoiler_text: String,
    visibility: String,
    sensitive: i32,
    language: Option<String>,
    published_at: String,
}

#[derive(Debug, Deserialize)]
struct PollNotificationRow {
    poll_id: String,
    status_id: String,
    account_id: String,
    expires_at: String,
}

#[derive(Debug, Deserialize)]
struct NotificationDismissalRow {
    notification_id: String,
}

#[derive(Debug, Deserialize)]
struct NotificationClearMarkerRow {
    cleared_at: String,
}

#[event(fetch, respond_with_errors)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get("/", |_req, _ctx| Response::from_json(&root_document()))
        .get("/healthz", |_req, _ctx| Response::ok("ok"))
        .get_async("/api/v1/instance", |_req, ctx| async move {
            instance_summary_response(ctx).await
        })
        .get_async("/api/v1/instance/peers", |_req, ctx| async move {
            instance_peers_response(ctx).await
        })
        .get_async(
            "/api/v1/instance/extended_description",
            |_req, ctx| async move { instance_extended_description_response(ctx).await },
        )
        .get_async("/api/v1/instance/privacy_policy", |_req, ctx| async move {
            instance_privacy_policy_response(ctx).await
        })
        .get_async(
            "/api/v1/instance/terms_of_service",
            |_req, ctx| async move { instance_terms_of_service_response(ctx).await },
        )
        .get_async("/api/v2/instance", |_req, ctx| async move {
            instance_v2_response(ctx).await
        })
        .get_async("/api/v1/timelines/home", |req, ctx| async move {
            home_timeline_response(req, ctx).await
        })
        .get_async("/api/v1/timelines/public", |_req, ctx| async move {
            public_timeline_response(ctx).await
        })
        .get_async("/api/v1/timelines/tag/:hashtag", |req, ctx| async move {
            tag_timeline_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id", |req, ctx| async move {
            status_api_response(req, ctx).await
        })
        .get_async("/api/v1/statuses/:id/context", |req, ctx| async move {
            status_context_response(req, ctx).await
        })
        .get_async("/api/v1/tags/:name", |_req, ctx| async move {
            tag_response(ctx).await
        })
        .get_async("/.well-known/webfinger", |req, ctx| async move {
            webfinger_response(req, ctx).await
        })
        .get_async("/.well-known/nodeinfo", |_req, ctx| async move {
            nodeinfo_links_response(ctx).await
        })
        .get_async("/nodeinfo/2.0", |_req, ctx| async move {
            nodeinfo_response(ctx).await
        })
        .get_async("/users/:username", |_req, ctx| async move {
            actor_response(ctx).await
        })
        .get_async("/users/:username/followers", |req, ctx| async move {
            followers_collection_response(req, ctx).await
        })
        .get_async("/users/:username/following", |req, ctx| async move {
            following_collection_response(req, ctx).await
        })
        .post_async("/inbox", |req, ctx| async move {
            shared_inbox_response(req, ctx).await
        })
        .post_async("/users/:username/inbox", |req, ctx| async move {
            inbox_response(req, ctx).await
        })
        .get_async("/users/:username/outbox", |_req, ctx| async move {
            outbox_response(ctx).await
        })
        .get_async("/users/:username/statuses/:id", |_req, ctx| async move {
            status_object_response(ctx).await
        })
        .get_async("/media/:id", |_req, ctx| async move {
            media_content_response(ctx).await
        })
        .post_async("/api/v1/statuses", |req, ctx| async move {
            create_status(req, ctx).await
        })
        .delete_async("/api/v1/statuses/:id", |req, ctx| async move {
            delete_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/favourite", |req, ctx| async move {
            favourite_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unfavourite", |req, ctx| async move {
            unfavourite_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/reblog", |mut req, ctx| async move {
            reblog_status(&mut req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unreblog", |req, ctx| async move {
            unreblog_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/bookmark", |req, ctx| async move {
            bookmark_status(req, ctx).await
        })
        .post_async("/api/v1/statuses/:id/unbookmark", |req, ctx| async move {
            unbookmark_status(req, ctx).await
        })
        .post_async("/internal/outbox/process", |req, ctx| async move {
            process_outbox_deliveries(req, ctx).await
        })
        .post_async("/internal/media/prune-orphans", |req, ctx| async move {
            prune_orphan_media(req, ctx).await
        })
        .post_async("/internal/polls/process-expired", |req, ctx| async move {
            process_expired_polls(req, ctx).await
        })
        .post_async("/api/v2/media", |req, ctx| async move {
            create_media_attachment(req, ctx).await
        })
        .get_async("/api/v1/media/:id", |_req, ctx| async move {
            media_metadata_response(ctx).await
        })
        .put_async("/api/v1/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
        .patch_async("/api/v1/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
        .put_async("/api/v2/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
        .patch_async("/api/v2/media/:id", |req, ctx| async move {
            update_media_attachment(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/follow", |req, ctx| async move {
            follow_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unfollow", |req, ctx| async move {
            unfollow_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/block", |req, ctx| async move {
            block_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unblock", |req, ctx| async move {
            unblock_account(req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/mute", |mut req, ctx| async move {
            mute_account(&mut req, ctx).await
        })
        .post_async("/api/v1/accounts/:id/unmute", |req, ctx| async move {
            unmute_account(req, ctx).await
        })
        .get_async("/api/v1/accounts/relationships", |req, ctx| async move {
            account_relationships(req, ctx).await
        })
        .get_async("/api/v1/accounts/lookup", |req, ctx| async move {
            account_lookup(req, ctx).await
        })
        .get_async("/api/v1/accounts/search", |req, ctx| async move {
            account_search(req, ctx).await
        })
        .get_async("/api/v1/directory", |req, ctx| async move {
            account_directory(req, ctx).await
        })
        .get_async("/api/v1/favourites", |req, ctx| async move {
            favourites_response(req, ctx).await
        })
        .get_async("/api/v1/bookmarks", |req, ctx| async move {
            bookmarks_response(req, ctx).await
        })
        .get_async("/api/v1/mutes", |req, ctx| async move {
            mutes_response(req, ctx).await
        })
        .get_async("/api/v1/notifications", |req, ctx| async move {
            notifications_response(req, ctx).await
        })
        .get_async(
            "/api/v1/notifications/unread_count",
            |req, ctx| async move { notifications_unread_count_response(req, ctx).await },
        )
        .get_async("/api/v1/notifications/:id", |req, ctx| async move {
            notification_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/clear", |req, ctx| async move {
            notifications_clear_response(req, ctx).await
        })
        .post_async("/api/v1/notifications/:id/dismiss", |req, ctx| async move {
            notification_dismiss_response(req, ctx).await
        })
        .get_async("/api/v2/search", |req, ctx| async move {
            search_v2(req, ctx).await
        })
        .get_async("/api/v1/polls/:id", |req, ctx| async move {
            poll_response(req, ctx).await
        })
        .post_async("/api/v1/polls/:id/votes", |mut req, ctx| async move {
            vote_in_poll(&mut req, ctx).await
        })
        .post_async("/api/v1/reports", |mut req, ctx| async move {
            create_report(&mut req, ctx).await
        })
        .get_async(
            "/api/v1/accounts/verify_credentials",
            |req, ctx| async move { verify_credentials(req, ctx).await },
        )
        .patch_async(
            "/api/v1/accounts/update_credentials",
            |mut req, ctx| async move { update_credentials(&mut req, ctx).await },
        )
        .get_async("/api/v1/accounts/:id/statuses", |req, ctx| async move {
            account_statuses_response(req, ctx).await
        })
        .get_async("/api/v1/accounts/:id", |_req, ctx| async move {
            account_response(ctx).await
        })
        .run(req, env)
        .await
}

async fn verify_credentials(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let stats = load_account_stats(&db, &account.id).await?;

    Response::from_json(&MastodonAccountResponse::from_credentials_account(
        &account, &config, &stats,
    ))
}

async fn update_credentials(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let update = parse_update_credentials_request(req)
        .await
        .map_err(Error::RustError)?;
    let db = ctx.d1(&config.database_binding)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let account =
        apply_account_credentials_update(&db, &bucket, &config, &account, &update).await?;
    let stats = load_account_stats(&db, &account.id).await?;

    Response::from_json(&MastodonAccountResponse::from_credentials_account(
        &account, &config, &stats,
    ))
}

async fn account_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            let stats = load_account_stats(&db, &account.id).await?;
            Response::from_json(&MastodonAccountResponse::from_account_with_stats(
                &account, &config, &stats,
            ))
        }
        Some(AccountReference::Remote(actor)) => {
            Response::from_json(&MastodonAccountResponse::from_remote_actor(&actor))
        }
        None => Response::error("account not found", 404),
    }
}

async fn account_statuses_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let query: AccountStatusesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    match resolve_account_reference(&db, &account_id).await? {
        Some(AccountReference::Local(account)) => {
            let statuses = list_account_statuses(&db, &account.id, limit).await?;
            let mut response = Vec::new();

            for status in statuses {
                if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
                    continue;
                }
                if query.pinned.unwrap_or(false) {
                    continue;
                }
                if let Some(tag) = query.tagged.as_deref()
                    && !status_contains_tag(&status, tag)
                {
                    continue;
                }
                if query.exclude_reblogs.unwrap_or(false) {
                    // Local reblog support does not exist yet, so this filter is effectively a no-op.
                }
                if query.exclude_replies.unwrap_or(false)
                    && status_is_reply_to_other_account(&db, &status, &account.id).await?
                {
                    continue;
                }

                let media = find_media_attachments_by_status_id(&db, &status.id).await?;
                if query.only_media.unwrap_or(false) && media.is_empty() {
                    continue;
                }

                response.push(
                    build_local_status_response(
                        &db,
                        &config,
                        viewer.as_ref(),
                        &status,
                        &account,
                        load_in_reply_to_account_id(&db, &status).await?,
                        media,
                    )
                    .await?,
                );
            }

            Response::from_json(&response)
        }
        Some(AccountReference::Remote(actor)) => {
            let mut response = Vec::new();
            for status in list_remote_statuses_by_actor_uri(&db, &actor.actor_uri, limit).await? {
                if !is_public_activitypub_visibility(&status.visibility) {
                    continue;
                }
                if query.pinned.unwrap_or(false) {
                    continue;
                }
                if let Some(tag) = query.tagged.as_deref()
                    && !status
                        .content_html
                        .to_ascii_lowercase()
                        .contains(&tag.to_ascii_lowercase())
                {
                    continue;
                }
                if query.exclude_reblogs.unwrap_or(false) {
                    // Remote reblog parsing is not implemented yet.
                }
                if query.exclude_replies.unwrap_or(false) && status.in_reply_to_uri.is_some() {
                    continue;
                }
                if query.only_media.unwrap_or(false) {
                    continue;
                }

                response.push(
                    build_remote_status_response(&db, &config, viewer.as_ref(), &status, &actor)
                        .await?,
                );
            }
            Response::from_json(&response)
        }
        None => Response::error("account not found", 404),
    }
}

async fn account_lookup(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    let query: AccountLookupQuery = req.query()?;
    let db = ctx.d1(&config.database_binding)?;
    match resolve_lookup_account(&db, &config, &query.acct).await {
        Ok(account) => Response::from_json(&account),
        Err(_) => Response::error("account not found", 404),
    }
}

async fn account_search(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: AccountSearchQuery = req.query().unwrap_or_default();
    let q = query.q.trim();
    if q.is_empty() {
        return Response::from_json(&Vec::<MastodonAccountResponse>::new());
    }

    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let only_following = query.following.unwrap_or(false);
    let mut results = search_cached_accounts(
        &db,
        &config,
        Some(&viewer),
        q,
        limit,
        offset,
        only_following,
    )
    .await?;

    if query.resolve.unwrap_or(false)
        && results.is_empty()
        && let Some(account) = resolve_search_account(&db, &config, q).await?
    {
        results.push(account);
    }

    Response::from_json(&results)
}

async fn account_directory(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: DirectoryQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(40).clamp(1, 80);
    let offset = query.offset.unwrap_or(0);
    let db = ctx.d1(&config.database_binding)?;

    // Current directory support only exposes local discoverable accounts.
    if matches!(query.local, Some(false)) {
        return Response::from_json(&Vec::<MastodonAccountResponse>::new());
    }

    let mut response = Vec::new();
    for account in
        list_discoverable_accounts(&db, limit, offset, directory_order(query.order.as_deref()))
            .await?
    {
        let stats = load_account_stats(&db, &account.id).await?;
        response.push(MastodonAccountResponse::from_account_with_stats(
            &account, &config, &stats,
        ));
    }

    Response::from_json(&response)
}

async fn search_v2(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: SearchV2Query = req.query().unwrap_or_default();
    let q = query.q.trim();
    if q.is_empty() {
        return Response::from_json(&MastodonSearchResponse::default());
    }

    let db = ctx.d1(&config.database_binding)?;
    let requires_auth = search_v2_requires_auth(&query);
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => Some(account),
        None if requires_auth => {
            return Response::error("Cloudflare Access authentication required", 401);
        }
        None => None,
    };

    let search_flags = search_category_flags(query.search_type.as_deref());
    let limit = search_v2_limit(query.limit);
    let offset = query.offset.unwrap_or(0);
    let resolve_enabled = query.resolve.unwrap_or(false);
    let mut response = MastodonSearchResponse::default();

    if search_flags.accounts {
        response.accounts = search_cached_accounts(
            &db,
            &config,
            viewer.as_ref(),
            q,
            limit,
            offset,
            query.following.unwrap_or(false),
        )
        .await?;
        if resolve_enabled
            && response.accounts.is_empty()
            && let Some(account) = resolve_search_account(&db, &config, q).await?
        {
            response.accounts.push(account);
            response.accounts.truncate(limit as usize);
        }
    }

    if search_flags.statuses
        && let Some(viewer) = viewer.as_ref()
    {
        response.statuses = search_statuses_for_v2(
            &db,
            &config,
            viewer,
            q,
            limit,
            offset,
            query.account_id.as_deref(),
        )
        .await?;
        if resolve_enabled
            && response.statuses.is_empty()
            && let Some(status) = resolve_search_status(&db, &config, viewer, q).await?
        {
            response.statuses.push(status);
            response.statuses.truncate(limit as usize);
        }
    }

    if search_flags.hashtags {
        response.hashtags = search_tags_for_v2(&db, &config, q, limit).await?;
        if resolve_enabled
            && response.hashtags.is_empty()
            && let Some(tag) = resolve_search_tag(&db, &config, q).await?
        {
            response.hashtags.push(tag);
            response.hashtags.truncate(limit as usize);
        }
    }

    Response::from_json(&response)
}

async fn webfinger_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: WebFingerQuery = req.query()?;
    let handle = parse_webfinger_resource(&query.resource)?;

    if !handle.is_local_to(&config.instance_domain) {
        return Response::error("resource not found", 404);
    }

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &handle.username).await? else {
        return Response::error("resource not found", 404);
    };

    let instance_host = instance_host(&config);
    let response = WebFingerResponse {
        subject: format!("acct:{}@{}", account.username, instance_host),
        links: vec![WebFingerLink {
            rel: "self",
            link_type: "application/activity+json",
            href: actor_url(&config, &account.username),
        }],
    };

    json_response(
        &response,
        "application/jrd+json",
        &[("Access-Control-Allow-Origin", "*")],
    )
}

async fn actor_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let account = ensure_account_keys(&db, account).await?;

    let response = build_activitypub_actor_document(&config, &account);

    json_response(&response, "application/activity+json", &[])
}

async fn shared_inbox_response(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let body = req.bytes().await?;
    let activity: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| Error::RustError(format!("invalid activitypub payload: {error}")))?;

    if activity.get("type").and_then(serde_json::Value::as_str) == Some("Delete") {
        return handle_inbox_request(&req, &db, &config, None, &body, &activity).await;
    }

    let Some(account) = resolve_inbox_target_account(&db, &config, None, &activity).await? else {
        return Ok(Response::empty()?.with_status(202));
    };
    handle_inbox_request(&req, &db, &config, Some(&account), &body, &activity).await
}

async fn home_timeline_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query: HomeTimelineQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let mut entries = Vec::new();

    for status in
        list_local_home_timeline_statuses(&db, &viewer.id, limit.saturating_mul(3)).await?
    {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        if is_muted_actor(&db, &viewer.id, &actor_url(&config, &account.username)).await? {
            continue;
        }
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            build_local_status_response(
                &db,
                &config,
                Some(&viewer),
                &status,
                &account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?,
        ));
    }

    for (status, actor) in
        list_remote_home_timeline_statuses(&db, &viewer.id, limit.saturating_mul(3)).await?
    {
        if is_muted_actor(&db, &viewer.id, &actor.actor_uri).await? {
            continue;
        }
        entries.push((
            status.published_at.clone(),
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?,
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &entries
            .into_iter()
            .map(|(_, value)| value)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}

async fn public_timeline_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let mut entries = Vec::new();

    for status in list_local_public_timeline_statuses(&db, 20).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            continue;
        };
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        entries.push((
            status.created_at.clone(),
            serde_json::to_value(
                build_local_status_response(&db, &config, None, &status, &account, None, media)
                    .await?,
            )
            .unwrap_or(serde_json::Value::Null),
        ));
    }

    for (status, actor) in list_remote_public_timeline_statuses(&db, 20).await? {
        entries.push((
            status.published_at.clone(),
            serde_json::to_value(
                build_remote_status_response(&db, &config, None, &status, &actor).await?,
            )
            .unwrap_or(serde_json::Value::Null),
        ));
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    let response = entries
        .into_iter()
        .map(|(_, value)| value)
        .take(20)
        .collect::<Vec<_>>();

    Response::from_json(&response)
}

async fn tag_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("name")
        .or_else(|| ctx.param("hashtag"))
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing tag route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;

    Response::from_json(&build_tag_response(&db, &config, &tag).await?)
}

async fn tag_timeline_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let tag = ctx
        .param("hashtag")
        .map(|value| normalize_hashtag(value))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing hashtag route parameter".to_owned()))?;
    let query: TagTimelineQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let query_limit = limit.saturating_mul(4).clamp(limit, 160);
    let include_local = include_local_source(query.local, query.remote);
    let include_remote = include_remote_source(query.local, query.remote);
    let db = ctx.d1(&config.database_binding)?;
    let mut entries = Vec::new();

    if include_local {
        for status in list_local_public_statuses_by_tag(&db, &tag, query_limit).await? {
            let status_tags = extract_hashtags_from_text(&status._text_content);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
                continue;
            };
            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            if query.only_media.unwrap_or(false) && media.is_empty() {
                continue;
            }
            entries.push((
                status.created_at.clone(),
                build_local_status_response(
                    &db,
                    &config,
                    None,
                    &status,
                    &account,
                    load_in_reply_to_account_id(&db, &status).await?,
                    media,
                )
                .await?,
            ));
        }
    }

    if include_remote {
        for (status, actor) in list_remote_public_statuses_by_tag(&db, &tag, query_limit).await? {
            let status_tags = extract_hashtags_from_html(&status.content_html);
            if !matches_tag_timeline_filters(&status_tags, &tag, &query) {
                continue;
            }
            if query.only_media.unwrap_or(false) {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                build_remote_status_response(&db, &config, None, &status, &actor).await?,
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &entries
            .into_iter()
            .map(|(_, status)| status)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}

async fn followers_collection_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionPagingQuery = req.query().unwrap_or_default();
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let mut ordered_items = list_follower_actor_uris(&db, &account.id).await?;
    let mut seen = ordered_items.iter().cloned().collect::<HashSet<_>>();
    for username in list_local_follower_usernames(&db, &account.id).await? {
        let actor_uri = actor_url(&config, &username);
        if seen.insert(actor_uri.clone()) {
            ordered_items.push(actor_uri);
        }
    }
    let collection_id = format!("{}/followers", actor_url(&config, &account.username));
    json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        &[],
    )
}

async fn following_collection_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: CollectionPagingQuery = req.query().unwrap_or_default();
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let ordered_items = list_following_actor_uris(&db, &account.id).await?;
    let collection_id = format!("{}/following", actor_url(&config, &account.username));

    json_response(
        &build_ordered_collection_document(&collection_id, &ordered_items, &query),
        "application/activity+json",
        &[],
    )
}

async fn outbox_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };

    let statuses = list_public_outbox_statuses(&db, &account.id, 20).await?;
    let actor = actor_url(&config, &account.username);
    let outbox = format!("{actor}/outbox");
    let ordered_items = build_outbox_activities(&db, &config, &account, &statuses).await?;

    json_response(
        &serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollection",
            "id": outbox,
            "totalItems": ordered_items.len(),
            "orderedItems": ordered_items,
        }),
        "application/activity+json",
        &[],
    )
}

async fn inbox_response(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;

    let body = req.bytes().await?;
    let activity: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| Error::RustError(format!("invalid activitypub payload: {error}")))?;
    let db = ctx.d1(&config.database_binding)?;
    let Some(account) =
        resolve_inbox_target_account(&db, &config, Some(username.as_str()), &activity).await?
    else {
        return Response::error("actor not found", 404);
    };
    handle_inbox_request(&req, &db, &config, Some(&account), &body, &activity).await
}

async fn status_object_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let username = ctx
        .param("username")
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing username route parameter".to_owned()))?;
    let status_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing status id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(account) = find_account_by_username(&db, &username).await? else {
        return Response::error("actor not found", 404);
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != account.id {
        return Response::error("status not found", 404);
    }
    if !is_public_activitypub_visibility(&status.visibility) {
        return Response::error("status not found", 404);
    }

    let note = build_activitypub_note(&db, &config, &account, &status, true).await?;
    json_response(&note, "application/activity+json", &[])
}

async fn build_local_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &StatusRow,
    account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    media_attachments: Vec<MediaAttachmentRow>,
) -> Result<MastodonStatusResponse> {
    let mut response = MastodonStatusResponse::from_row(
        status,
        account,
        config,
        in_reply_to_account_id,
        media_attachments,
    );
    response.poll = load_mastodon_poll_response(db, &status.id, viewer).await?;
    response.mentions = build_status_mentions(db, config, &status._text_content).await?;
    response.favourites_count = count_local_status_favourites(db, &status.id).await?;
    response.favourited = match viewer {
        Some(viewer) => is_local_status_favourited_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.reblogs_count = count_local_status_reblogs(db, &status.id).await?;
    response.reblogged = match viewer {
        Some(viewer) => is_local_status_reblogged_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.bookmarked = match viewer {
        Some(viewer) => is_local_status_bookmarked_by(db, &viewer.id, status).await?,
        None => false,
    };
    response.muted = match viewer {
        Some(viewer) => {
            is_muted_actor(db, &viewer.id, &actor_url(config, &account.username)).await?
        }
        None => false,
    };
    Ok(response)
}

async fn build_remote_status_response(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    status: &RemoteStatusRow,
    actor: &RemoteActorRow,
) -> Result<MastodonStatusResponse> {
    let mut response = MastodonStatusResponse::from_remote_row(status, actor, config);
    let text_content = strip_html_tags(&status.content_html);
    response.mentions = build_status_mentions(db, config, &text_content).await?;
    response.favourites_count = count_remote_status_favourites(db, &status.id).await?;
    response.favourited = match viewer {
        Some(viewer) => is_remote_status_favourited_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.reblogs_count = count_remote_status_reblogs(db, &status.id).await?;
    response.reblogged = match viewer {
        Some(viewer) => is_remote_status_reblogged_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.bookmarked = match viewer {
        Some(viewer) => is_remote_status_bookmarked_by(db, &viewer.id, &status.id).await?,
        None => false,
    };
    response.muted = match viewer {
        Some(viewer) => is_muted_actor(db, &viewer.id, &actor.actor_uri).await?,
        None => false,
    };
    response.poll = load_remote_mastodon_poll_response(db, status, viewer).await?;
    Ok(response)
}

async fn build_status_mentions(
    db: &D1Database,
    config: &AppConfig,
    text: &str,
) -> Result<Vec<serde_json::Value>> {
    let mut mentions = Vec::new();

    for handle in extract_account_handles_from_text(text, config) {
        if handle.is_local_to(&config.instance_domain) {
            let Some(account) = find_account_by_username(db, &handle.username).await? else {
                continue;
            };
            mentions.push(serde_json::json!({
                "id": account.id,
                "username": account.username,
                "url": actor_url(config, &account.username),
                "acct": account.acct(),
            }));
            continue;
        }

        let Some(domain) = handle.domain.as_deref() else {
            continue;
        };
        let Some(actor) =
            find_remote_actor_by_username_domain(db, &handle.username, domain).await?
        else {
            continue;
        };
        mentions.push(serde_json::json!({
            "id": remote_account_rest_id(&actor.actor_uri),
            "username": actor.username,
            "url": actor.profile_url.clone().unwrap_or_else(|| actor.actor_uri.clone()),
            "acct": format!("{}@{}", actor.username, actor.domain),
        }));
    }

    Ok(mentions)
}

async fn create_status(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let draft = match parse_status_draft(&mut req).await {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };
    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let pending_media = match resolve_attachable_media(&db, &account, &draft.media_ids).await {
        Ok(media) => media,
        Err(message) => return Response::error(message, 422),
    };
    let in_reply_to_account_id = match draft.in_reply_to_id.as_deref() {
        Some(status_id) => match find_status_by_id(&db, status_id).await? {
            Some(status) => Some(status.account_id),
            None => return Response::error("in_reply_to_id references unknown local status", 422),
        },
        None => None,
    };

    let status = insert_status(&db, &config, &account, &draft).await?;
    attach_media_to_status(&db, &status.id, &pending_media).await?;
    let attached_media = find_media_attachments_by_status_id(&db, &status.id).await?;
    enqueue_outbox_activity(&db, &config, &account, &status).await?;
    let response = build_local_status_response(
        &db,
        &config,
        Some(&account),
        &status,
        &account,
        in_reply_to_account_id,
        attached_media,
    )
    .await?;

    Response::from_json(&response)
}

async fn status_api_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
        return Response::error("status not found", 404);
    };
    if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
        return Response::error("status not found", 404);
    }

    let media = find_media_attachments_by_status_id(&db, &status.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
    Response::from_json(
        &build_local_status_response(
            &db,
            &config,
            viewer.as_ref(),
            &status,
            &account,
            in_reply_to_account_id,
            media,
        )
        .await?,
    )
}

async fn status_context_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(owner) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, viewer.as_ref(), &owner).await? {
            return Response::error("status not found", 404);
        }

        return Response::from_json(
            &build_local_status_context(&db, &config, viewer.as_ref(), &status, &owner).await?,
        );
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        return Response::from_json(
            &build_remote_status_context(&db, &config, &status, &actor).await?,
        );
    }

    Response::error("status not found", 404)
}

async fn delete_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let query: DeleteStatusQuery = req.query().unwrap_or_default();

    let db = ctx.d1(&config.database_binding)?;
    let requester = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let Some(status) = find_status_by_id(&db, &status_id).await? else {
        return Response::error("status not found", 404);
    };
    if status.account_id != requester.id {
        return Response::error("status not found", 404);
    }

    let media = find_media_attachments_by_status_id(&db, &status.id).await?;
    let in_reply_to_account_id = load_in_reply_to_account_id(&db, &status).await?;
    let mut response = MastodonStatusResponse::from_deleted_row(
        &status,
        &requester,
        &config,
        in_reply_to_account_id,
        media.clone(),
    );
    response.poll = load_mastodon_poll_response(&db, &status.id, Some(&requester)).await?;

    enqueue_outbox_delete(&db, &config, &requester, &status).await?;
    delete_status_by_id(&db, &status.id).await?;
    if query.delete_media.unwrap_or(false) {
        let bucket = ctx.bucket(&config.media_binding)?;
        delete_media_attachments(&db, &bucket, &media).await?;
    }

    Response::from_json(&response)
}

async fn favourite_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        upsert_favourite_local_status(&db, &viewer.id, &status).await?;
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let response = build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &account,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?;
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        let existing =
            find_favourite_activity_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
        let mut outbound_activity_id = existing.and_then(|row| row.ap_activity_id);
        if outbound_activity_id.is_none() {
            let (_, payload_json) =
                build_like_activity(&config, &viewer, &actor.actor_uri, &status.object_uri)?;
            outbound_activity_id =
                queue_remote_actor_activity(&db, &viewer.id, &actor.actor_uri, &payload_json)
                    .await?;
        }
        upsert_favourite_remote_status(&db, &viewer.id, &status, outbound_activity_id.as_deref())
            .await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

async fn unfavourite_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        delete_favourite_by_target_uri(&db, &viewer.id, &local_status_target_uri(&status)).await?;
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let response = build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &account,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?;
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        if let Some(row) =
            find_favourite_activity_by_target_uri(&db, &viewer.id, &status.object_uri).await?
            && let Some(like_activity_id) = row.ap_activity_id.as_deref()
        {
            let (_, payload_json) = build_undo_like_activity(
                &config,
                &viewer,
                like_activity_id,
                &actor.actor_uri,
                &status.object_uri,
            )?;
            let _ = queue_remote_actor_activity(&db, &viewer.id, &actor.actor_uri, &payload_json)
                .await?;
        }
        delete_favourite_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

async fn reblog_status(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let request = parse_reblog_status_request(req)
        .await
        .map_err(Error::RustError)?;
    let visibility = request.visibility.unwrap_or_else(|| "public".to_owned());
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        if viewer.id == account.id {
            return Response::error("cannot reblog your own status", 422);
        }
        upsert_reblog_local_status(&db, &viewer.id, &status, &visibility).await?;
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let response = build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &account,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?;
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        let existing =
            find_reblog_activity_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
        let mut outbound_activity_id = existing.and_then(|row| row.ap_activity_id);
        if outbound_activity_id.is_none() {
            let (_, payload_json) =
                build_announce_activity(&config, &viewer, &status.object_uri, &visibility)?;
            outbound_activity_id =
                queue_remote_actor_activity(&db, &viewer.id, &actor.actor_uri, &payload_json)
                    .await?;
        }
        upsert_reblog_remote_status(
            &db,
            &viewer.id,
            &status,
            &visibility,
            outbound_activity_id.as_deref(),
        )
        .await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

async fn unreblog_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        delete_reblog_by_target_uri(&db, &viewer.id, &local_status_target_uri(&status)).await?;
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let response = build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &account,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?;
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        if let Some(row) =
            find_reblog_activity_by_target_uri(&db, &viewer.id, &status.object_uri).await?
            && let Some(announce_activity_id) = row.ap_activity_id.as_deref()
        {
            let (_, payload_json) = build_undo_announce_activity(
                &config,
                &viewer,
                announce_activity_id,
                &actor.actor_uri,
                &status.object_uri,
                &row.visibility,
            )?;
            let _ = queue_remote_actor_activity(&db, &viewer.id, &actor.actor_uri, &payload_json)
                .await?;
        }
        delete_reblog_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

async fn favourites_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: FavouritesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mut entries = Vec::new();
    for entry in list_favourites_for_account(&db, &viewer.id, limit.saturating_mul(3)).await? {
        if let Some(status_id) = entry.status_id.as_deref()
            && let Some(status) = find_status_by_id(&db, status_id).await?
            && let Some(account) = find_account_by_id(&db, &status.account_id).await?
        {
            if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            let response = build_local_status_response(
                &db,
                &config,
                Some(&viewer),
                &status,
                &account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?;
            entries.push((
                entry.created_at,
                serde_json::to_value(response).unwrap_or_default(),
            ));
            continue;
        }

        if let Some(remote_status_id) = entry.remote_status_id.as_deref()
            && let Some(status) = find_remote_status_by_id(&db, remote_status_id).await?
            && let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await?
        {
            let response =
                build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
            entries.push((
                entry.created_at,
                serde_json::to_value(response).unwrap_or_default(),
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &entries
            .into_iter()
            .map(|(_, value)| value)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}

async fn bookmark_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        upsert_bookmark_local_status(&db, &viewer.id, &status).await?;
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let response = build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &account,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?;
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        upsert_bookmark_remote_status(&db, &viewer.id, &status).await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

async fn unbookmark_status(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let status_id = match status_id_from_context(&ctx) {
        Ok(status_id) => status_id,
        Err(_) => return Response::error("missing status id route parameter", 400),
    };
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    if let Some(status) = find_status_by_id(&db, &status_id).await? {
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("status not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("status not found", 404);
        }
        delete_bookmark_by_target_uri(&db, &viewer.id, &local_status_target_uri(&status)).await?;
        let media = find_media_attachments_by_status_id(&db, &status.id).await?;
        let response = build_local_status_response(
            &db,
            &config,
            Some(&viewer),
            &status,
            &account,
            load_in_reply_to_account_id(&db, &status).await?,
            media,
        )
        .await?;
        return Response::from_json(&response);
    }

    if let Some(status) = find_remote_status_by_id(&db, &status_id).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("status not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("status not found", 404);
        };
        delete_bookmark_by_target_uri(&db, &viewer.id, &status.object_uri).await?;
        let response =
            build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
        return Response::from_json(&response);
    }

    Response::error("status not found", 404)
}

async fn bookmarks_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: BookmarksQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mut entries = Vec::new();
    for entry in list_bookmarks_for_account(&db, &viewer.id, limit.saturating_mul(3)).await? {
        if let Some(status_id) = entry.status_id.as_deref()
            && let Some(status) = find_status_by_id(&db, status_id).await?
            && let Some(account) = find_account_by_id(&db, &status.account_id).await?
        {
            if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(&db, &status.id).await?;
            let response = build_local_status_response(
                &db,
                &config,
                Some(&viewer),
                &status,
                &account,
                load_in_reply_to_account_id(&db, &status).await?,
                media,
            )
            .await?;
            entries.push((
                entry.created_at,
                serde_json::to_value(response).unwrap_or_default(),
            ));
            continue;
        }

        if let Some(remote_status_id) = entry.remote_status_id.as_deref()
            && let Some(status) = find_remote_status_by_id(&db, remote_status_id).await?
            && let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await?
        {
            let response =
                build_remote_status_response(&db, &config, Some(&viewer), &status, &actor).await?;
            entries.push((
                entry.created_at,
                serde_json::to_value(response).unwrap_or_default(),
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Response::from_json(
        &entries
            .into_iter()
            .map(|(_, value)| value)
            .take(limit as usize)
            .collect::<Vec<_>>(),
    )
}

async fn notifications_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, limit.saturating_mul(4))
            .await?;

    Response::from_json(
        &entries
            .into_iter()
            .take(limit as usize)
            .map(|entry| entry.value)
            .collect::<Vec<_>>(),
    )
}

async fn notification_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let notification_id = ctx
        .param("id")
        .ok_or_else(|| Error::RustError("missing notification id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query = NotificationsQuery {
        limit: Some(200),
        ..NotificationsQuery::default()
    };

    let Some(entry) = collect_visible_notifications(&db, &config, &viewer, &query, 200)
        .await?
        .into_iter()
        .find(|entry| entry.id == notification_id.as_str())
    else {
        return Response::error("notification not found", 404);
    };

    Response::from_json(&entry.value)
}

async fn notification_dismiss_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let notification_id = ctx
        .param("id")
        .ok_or_else(|| Error::RustError("missing notification id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let query = NotificationsQuery {
        limit: Some(200),
        ..NotificationsQuery::default()
    };

    let exists = collect_visible_notifications(&db, &config, &viewer, &query, 200)
        .await?
        .into_iter()
        .any(|entry| entry.id == notification_id.as_str());
    if !exists {
        return Response::error("notification not found", 404);
    }

    dismiss_notification_for_account(&db, &viewer.id, &notification_id).await?;
    Response::from_json(&serde_json::json!({}))
}

async fn notifications_clear_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    clear_notifications_for_account(&db, &viewer.id).await?;
    Response::from_json(&serde_json::json!({}))
}

async fn notifications_unread_count_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let query: NotificationsQuery = req.query().unwrap_or_default();
    let per_type_limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let viewer = match require_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let entries =
        collect_visible_notifications(&db, &config, &viewer, &query, per_type_limit).await?;

    Response::from_json(&serde_json::json!({
        "count": entries.len(),
    }))
}

async fn poll_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let poll_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing poll id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = find_authenticated_local_account(&req, &db, &config).await?;
    if let Some(poll) = find_status_poll_by_id(&db, &poll_id).await? {
        let Some(status) = find_status_by_id(&db, &poll.status_id).await? else {
            return Response::error("poll not found", 404);
        };
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("poll not found", 404);
        };
        if !can_view_local_status(&db, &status, viewer.as_ref(), &account).await? {
            return Response::error("poll not found", 404);
        }

        return Response::from_json(
            &build_mastodon_poll_response(&db, &poll, viewer.as_ref())
                .await?
                .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
        );
    }

    let Some(poll) = find_remote_status_poll_by_id(&db, &poll_id).await? else {
        return Response::error("poll not found", 404);
    };
    let Some(status) = find_remote_status_by_id(&db, &poll.status_id).await? else {
        return Response::error("poll not found", 404);
    };
    if !is_public_activitypub_visibility(&status.visibility) {
        return Response::error("poll not found", 404);
    }

    Response::from_json(
        &build_remote_mastodon_poll_response(&db, &poll, viewer.as_ref())
            .await?
            .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
    )
}

async fn vote_in_poll(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let poll_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing poll id route parameter".to_owned()))?;
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let choices = match parse_poll_vote_request(req).await {
        Ok(choices) => choices,
        Err(message) => return Response::error(message, 422),
    };
    if let Some(poll) = find_status_poll_by_id(&db, &poll_id).await? {
        let Some(status) = find_status_by_id(&db, &poll.status_id).await? else {
            return Response::error("poll not found", 404);
        };
        let Some(account) = find_account_by_id(&db, &status.account_id).await? else {
            return Response::error("poll not found", 404);
        };
        if !can_view_local_status(&db, &status, Some(&viewer), &account).await? {
            return Response::error("poll not found", 404);
        }
        if is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false) {
            return Response::error("poll has already expired", 422);
        }

        if let Err(error) = apply_poll_vote(&db, &poll, &viewer.id, &choices).await {
            return match error {
                Error::RustError(message) => Response::error(message, 422),
                other => Err(other),
            };
        }
        let _ = enqueue_status_update_activity(&db, &config, &account, &status).await;
        return Response::from_json(
            &build_mastodon_poll_response(&db, &poll, Some(&viewer))
                .await?
                .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
        );
    }

    if let Some(poll) = find_remote_status_poll_by_id(&db, &poll_id).await? {
        let Some(status) = find_remote_status_by_id(&db, &poll.status_id).await? else {
            return Response::error("poll not found", 404);
        };
        if !is_public_activitypub_visibility(&status.visibility) {
            return Response::error("poll not found", 404);
        }
        let Some(actor) = find_remote_actor_by_actor_uri(&db, &status.actor_uri).await? else {
            return Response::error("poll not found", 404);
        };
        if poll.expired != 0
            || poll
                .expires_at
                .as_deref()
                .map(|value| is_iso_timestamp_in_past(value).unwrap_or(false))
                .unwrap_or(false)
        {
            return Response::error("poll has already expired", 422);
        }

        if let Err(error) =
            apply_remote_poll_vote(&db, &config, &viewer, &actor, &status, &poll, &choices).await
        {
            return match error {
                Error::RustError(message) => Response::error(message, 422),
                other => Err(other),
            };
        }
        return Response::from_json(
            &build_remote_mastodon_poll_response(&db, &poll, Some(&viewer))
                .await?
                .ok_or_else(|| Error::RustError("poll not found".to_owned()))?,
        );
    }

    Response::error("poll not found", 404)
}

async fn create_report(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let reporter = match find_authenticated_local_account(req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let request = match parse_create_report_request(req).await {
        Ok(request) => request,
        Err(message) => return Response::error(message, 422),
    };
    if request.account_id == reporter.id {
        return Response::error("cannot report your own account", 422);
    }

    let target = match resolve_account_reference(&db, &request.account_id).await? {
        Some(target) => target,
        None => return Response::error("account not found", 404),
    };
    let status_ids = request.status_ids.clone().unwrap_or_default();
    if let Err(message) = validate_report_status_ids(&db, &target, &status_ids).await {
        let status = if message == "status not found" {
            404
        } else {
            422
        };
        return Response::error(message, status);
    }
    let report = insert_report(&db, &reporter.id, &request, &target, &status_ids).await?;

    Response::from_json(&build_report_response(&db, &config, &report).await?)
}

async fn parse_create_report_request(
    req: &mut Request,
) -> std::result::Result<CreateReportRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<CreateReportRequest>()
            .await
            .map_err(|error| format!("invalid JSON report payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form report payload: {error}"))?;
        CreateReportRequest {
            account_id: form.get_field("account_id").unwrap_or_default(),
            status_ids: form.get_all("status_ids[]").map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|entry| match entry {
                        FormEntry::Field(value) => Some(value),
                        FormEntry::File(_) => None,
                    })
                    .collect()
            }),
            comment: form.get_field("comment"),
            category: form.get_field("category"),
            forward: parse_optional_bool(form.get_field("forward").as_deref())?,
        }
    };

    request.account_id = request.account_id.trim().to_owned();
    if request.account_id.is_empty() {
        return Err("account_id is required".to_owned());
    }
    if let Some(comment) = request.comment.as_mut() {
        *comment = comment.trim().to_owned();
    }
    if request
        .comment
        .as_deref()
        .map(|value| value.chars().count() > 1000)
        .unwrap_or(false)
    {
        return Err("comment must be at most 1000 characters".to_owned());
    }
    if let Some(category) = request.category.as_mut() {
        *category = category.trim().to_ascii_lowercase();
        if category.is_empty() {
            request.category = None;
        }
    }
    if let Some(status_ids) = request.status_ids.as_mut() {
        *status_ids = status_ids
            .iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        status_ids.sort();
        status_ids.dedup();
    }

    match request.category.as_deref().unwrap_or("other") {
        "spam" | "violation" | "other" | "legal" => {}
        _ => return Err("category must be one of: spam, legal, violation, other".to_owned()),
    }

    Ok(request)
}

async fn validate_report_status_ids(
    db: &D1Database,
    target: &AccountReference,
    status_ids: &[String],
) -> std::result::Result<(), String> {
    if status_ids.is_empty() {
        return Ok(());
    }
    let AccountReference::Local(target_account) = target else {
        return Err("status_ids are only supported for local accounts".to_owned());
    };
    for status_id in status_ids {
        let Some(status) = find_status_by_id(db, status_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("status not found".to_owned());
        };
        if status.account_id != target_account.id {
            return Err("status_ids must belong to the reported account".to_owned());
        }
    }
    Ok(())
}

async fn insert_report(
    db: &D1Database,
    reporter_account_id: &str,
    request: &CreateReportRequest,
    target: &AccountReference,
    status_ids: &[String],
) -> Result<ReportRow> {
    let report_id = generate_entity_id(16)?;
    let target_account_id = match target {
        AccountReference::Local(account) => account.id.clone(),
        AccountReference::Remote(actor) => remote_account_rest_id(&actor.actor_uri),
    };
    let target_remote_actor_uri = match target {
        AccountReference::Local(_) => None,
        AccountReference::Remote(actor) => Some(actor.actor_uri.clone()),
    };
    let category = request
        .category
        .clone()
        .unwrap_or_else(|| "other".to_owned());
    let comment = request.comment.clone().unwrap_or_default();
    let bindings = [
        D1Type::Text(report_id.as_str()),
        D1Type::Text(reporter_account_id),
        D1Type::Text(target_account_id.as_str()),
        match target_remote_actor_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(comment.as_str()),
        D1Type::Text(category.as_str()),
        D1Type::Integer(if request.forward.unwrap_or(false) {
            1
        } else {
            0
        }),
    ];
    db.prepare(
        "INSERT INTO reports (
            id,
            account_id,
            target_account_id,
            target_remote_actor_uri,
            comment,
            category,
            forward,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    for status_id in status_ids {
        let bindings = [
            D1Type::Text(report_id.as_str()),
            D1Type::Text(status_id.as_str()),
        ];
        db.prepare(
            "INSERT INTO report_statuses (report_id, status_id)
             VALUES (?1, ?2)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    find_report_by_id(db, &report_id)
        .await?
        .ok_or_else(|| Error::RustError("failed to load created report".to_owned()))
}

async fn build_report_response(
    db: &D1Database,
    config: &AppConfig,
    report: &ReportRow,
) -> Result<MastodonReportResponse> {
    let target_account = match resolve_account_reference(db, &report.target_account_id).await? {
        Some(AccountReference::Local(account)) => {
            let stats = load_account_stats(db, &account.id).await?;
            MastodonAccountResponse::from_account_with_stats(&account, config, &stats)
        }
        Some(AccountReference::Remote(actor)) => MastodonAccountResponse::from_remote_actor(&actor),
        None => {
            return Err(Error::RustError(
                "reported account could not be resolved".to_owned(),
            ));
        }
    };

    Ok(MastodonReportResponse {
        id: report.id.clone(),
        action_taken: false,
        action_taken_at: None,
        category: report.category.clone(),
        comment: report.comment.clone(),
        forwarded: report.forward != 0,
        created_at: report.created_at.clone(),
        status_ids: {
            let status_ids = list_report_status_ids(db, &report.id).await?;
            if status_ids.is_empty() {
                None
            } else {
                Some(status_ids)
            }
        },
        target_account,
        rule_ids: None,
    })
}

async fn require_authenticated_local_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<LocalAccount>> {
    find_authenticated_local_account(req, db, config).await
}

async fn collect_visible_notifications(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationEntry>> {
    let mut entries = collect_notifications(db, config, viewer, query, per_type_limit).await?;
    let dismissed_ids = load_dismissed_notification_ids(db, &viewer.id).await?;
    let cleared_at = load_notification_clear_marker(db, &viewer.id).await?;
    let cleared_at_token = cleared_at
        .as_deref()
        .and_then(notification_timestamp_sort_token);

    entries.retain(|entry| {
        if dismissed_ids.contains(&entry.id) {
            return false;
        }
        match (
            cleared_at_token.as_deref(),
            notification_timestamp_sort_token(&entry.created_at),
        ) {
            (Some(cleared_at), Some(created_at)) => created_at.as_str() > cleared_at,
            _ => true,
        }
    });
    entries.sort_by(|left, right| {
        notification_sort_key(&right.created_at)
            .cmp(&notification_sort_key(&left.created_at))
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(entries)
}

async fn collect_notifications(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<Vec<NotificationEntry>> {
    let mut entries = Vec::new();

    if is_admin_account(config, viewer) && notification_type_allowed(query, "admin.report") {
        for report in list_admin_report_notifications(db, per_type_limit).await? {
            let Some(account) = find_account_by_id(db, &report.account_id).await? else {
                continue;
            };
            if !notification_account_matches_filter(query.account_id.as_deref(), &account.id, None)
            {
                continue;
            }
            let stats = load_account_stats(db, &account.id).await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("admin-report-{}", report.id),
                    notification_type: "admin.report".to_owned(),
                    group_key: format!("admin-report-{}", report.id),
                    created_at: report.created_at.clone(),
                    account: MastodonAccountResponse::from_account_with_stats(
                        &account, config, &stats,
                    ),
                    status: None,
                    report: Some(serde_json::to_value(
                        build_report_response(db, config, &report).await?,
                    )?),
                },
            );
        }
    }

    if is_admin_account(config, viewer) && notification_type_allowed(query, "admin.sign_up") {
        for account in list_admin_sign_up_notifications(db, &viewer.id, per_type_limit).await? {
            if !notification_account_matches_filter(query.account_id.as_deref(), &account.id, None)
            {
                continue;
            }
            let stats = load_account_stats(db, &account.id).await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("admin-sign-up-{}", account.id),
                    notification_type: "admin.sign_up".to_owned(),
                    group_key: format!("admin-sign-up-{}", account.id),
                    created_at: account.created_at.clone(),
                    account: MastodonAccountResponse::from_account_with_stats(
                        &account, config, &stats,
                    ),
                    status: None,
                    report: None,
                },
            );
        }
    }

    if notification_type_allowed(query, "follow") {
        for follow in
            list_local_follow_notifications_for_account(db, &viewer.id, per_type_limit).await?
        {
            let Some(account) = find_account_by_id(db, &follow.follower_account_id).await? else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &account.username))
                .await?
                || !notification_account_matches_filter(
                    query.account_id.as_deref(),
                    &account.id,
                    None,
                )
            {
                continue;
            }
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("follow-local-{}", account.id),
                    notification_type: "follow".to_owned(),
                    group_key: format!("follow-local-{}", account.id),
                    created_at: follow.created_at,
                    account: MastodonAccountResponse::from_account(&account, config),
                    status: None,
                    report: None,
                },
            );
        }

        for follow in
            list_remote_follow_notifications_for_account(db, &viewer.id, per_type_limit).await?
        {
            let Some(actor) = find_remote_actor_by_actor_uri(db, &follow.actor_uri).await? else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            let remote_id = remote_account_rest_id(&actor.actor_uri);
            if !notification_account_matches_filter(
                query.account_id.as_deref(),
                &remote_id,
                Some(&actor.actor_uri),
            ) {
                continue;
            }
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("follow-remote-{}", remote_id),
                    notification_type: "follow".to_owned(),
                    group_key: format!("follow-remote-{}", remote_id),
                    created_at: follow.created_at,
                    account: MastodonAccountResponse::from_remote_actor(&actor),
                    status: None,
                    report: None,
                },
            );
        }
    }

    if notification_type_allowed(query, "favourite") {
        for favourite in
            list_favourite_notifications_for_account(db, &viewer.id, per_type_limit).await?
        {
            let Some(actor) = find_account_by_id(db, &favourite.account_id).await? else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &actor.username))
                .await?
                || !notification_account_matches_filter(
                    query.account_id.as_deref(),
                    &actor.id,
                    None,
                )
            {
                continue;
            }
            let Some(status) = find_status_by_id(db, &favourite.status_id).await? else {
                continue;
            };
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let status_response = build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                viewer,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("favourite-local-{}-{}", actor.id, status.id),
                    notification_type: "favourite".to_owned(),
                    group_key: format!("favourite-local-{}-{}", actor.id, status.id),
                    created_at: favourite.created_at,
                    account: MastodonAccountResponse::from_account(&actor, config),
                    status: Some(status_response),
                    report: None,
                },
            );
        }

        for favourite in
            list_remote_favourite_notifications_for_account(db, &viewer.id, per_type_limit).await?
        {
            let Some(actor) =
                find_remote_actor_by_actor_uri(db, &favourite.remote_actor_uri).await?
            else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            let remote_id = remote_account_rest_id(&actor.actor_uri);
            if !notification_account_matches_filter(
                query.account_id.as_deref(),
                &remote_id,
                Some(&actor.actor_uri),
            ) {
                continue;
            }
            let Some(status) = find_status_by_id(db, &favourite.status_id).await? else {
                continue;
            };
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let status_response = build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                viewer,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("favourite-remote-{}-{}", remote_id, status.id),
                    notification_type: "favourite".to_owned(),
                    group_key: format!("favourite-remote-{}-{}", remote_id, status.id),
                    created_at: favourite.created_at,
                    account: MastodonAccountResponse::from_remote_actor(&actor),
                    status: Some(status_response),
                    report: None,
                },
            );
        }
    }

    if notification_type_allowed(query, "mention") {
        for mention in
            list_local_mention_notifications_for_account(db, viewer, config, per_type_limit).await?
        {
            let Some(actor) = find_account_by_id(db, &mention.account_id).await? else {
                continue;
            };
            let status = StatusRow {
                id: mention.id,
                account_id: mention.account_id.clone(),
                ap_id: mention.ap_id,
                in_reply_to_id: mention.in_reply_to_id,
                content_html: mention.content_html,
                _text_content: mention.text_content,
                spoiler_text: mention.spoiler_text,
                visibility: mention.visibility,
                sensitive: mention.sensitive,
                language: mention.language,
                created_at: mention.created_at.clone(),
            };
            if !can_view_local_status(db, &status, Some(viewer), &actor).await?
                || muted_notifications_for_actor(
                    db,
                    &viewer.id,
                    &actor_url(config, &actor.username),
                )
                .await?
                || !notification_account_matches_filter(
                    query.account_id.as_deref(),
                    &actor.id,
                    None,
                )
            {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let status_response = build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &actor,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("mention-local-{}-{}", actor.id, status.id),
                    notification_type: "mention".to_owned(),
                    group_key: format!("mention-local-{}-{}", actor.id, status.id),
                    created_at: status.created_at,
                    account: MastodonAccountResponse::from_account(&actor, config),
                    status: Some(status_response),
                    report: None,
                },
            );
        }

        for mention in
            list_remote_mention_notifications_for_account(db, viewer, config, per_type_limit)
                .await?
        {
            if !is_public_activitypub_visibility(&mention.visibility) {
                continue;
            }
            let Some(actor) = find_remote_actor_by_actor_uri(db, &mention.actor_uri).await? else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            let remote_id = remote_account_rest_id(&actor.actor_uri);
            if !notification_account_matches_filter(
                query.account_id.as_deref(),
                &remote_id,
                Some(&actor.actor_uri),
            ) {
                continue;
            }
            let status = RemoteStatusRow {
                id: mention.id,
                actor_uri: mention.actor_uri.clone(),
                object_uri: mention.object_uri,
                url: mention.url,
                in_reply_to_uri: mention.in_reply_to_uri,
                content_html: mention.content_html,
                spoiler_text: mention.spoiler_text,
                visibility: mention.visibility,
                sensitive: mention.sensitive,
                language: mention.language,
                published_at: mention.published_at.clone(),
            };
            let status_response =
                build_remote_status_response(db, config, Some(viewer), &status, &actor).await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("mention-remote-{}-{}", remote_id, status.id),
                    notification_type: "mention".to_owned(),
                    group_key: format!("mention-remote-{}-{}", remote_id, status.id),
                    created_at: status.published_at,
                    account: MastodonAccountResponse::from_remote_actor(&actor),
                    status: Some(status_response),
                    report: None,
                },
            );
        }
    }

    if notification_type_allowed(query, "status") {
        for status in
            list_local_status_notifications_for_account(db, &viewer.id, per_type_limit).await?
        {
            let Some(actor) = find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if !can_view_local_status(db, &status, Some(viewer), &actor).await?
                || muted_notifications_for_actor(
                    db,
                    &viewer.id,
                    &actor_url(config, &actor.username),
                )
                .await?
                || !notification_account_matches_filter(
                    query.account_id.as_deref(),
                    &actor.id,
                    None,
                )
            {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let status_response = build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &actor,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("status-local-{}-{}", actor.id, status.id),
                    notification_type: "status".to_owned(),
                    group_key: format!("status-local-{}-{}", actor.id, status.id),
                    created_at: status.created_at,
                    account: MastodonAccountResponse::from_account(&actor, config),
                    status: Some(status_response),
                    report: None,
                },
            );
        }

        for status in
            list_remote_status_notifications_for_account(db, &viewer.id, per_type_limit).await?
        {
            if status.visibility == "direct" {
                continue;
            }
            let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            let remote_id = remote_account_rest_id(&actor.actor_uri);
            if !notification_account_matches_filter(
                query.account_id.as_deref(),
                &remote_id,
                Some(&actor.actor_uri),
            ) {
                continue;
            }
            let status_row = RemoteStatusRow {
                id: status.id.clone(),
                actor_uri: status.actor_uri.clone(),
                object_uri: status.object_uri.clone(),
                url: status.url.clone(),
                in_reply_to_uri: status.in_reply_to_uri.clone(),
                content_html: status.content_html.clone(),
                spoiler_text: status.spoiler_text.clone(),
                visibility: status.visibility.clone(),
                sensitive: status.sensitive,
                language: status.language.clone(),
                published_at: status.published_at.clone(),
            };
            let status_response =
                build_remote_status_response(db, config, Some(viewer), &status_row, &actor).await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("status-remote-{}-{}", remote_id, status.id),
                    notification_type: "status".to_owned(),
                    group_key: format!("status-remote-{}-{}", remote_id, status.id),
                    created_at: status.published_at,
                    account: MastodonAccountResponse::from_remote_actor(&actor),
                    status: Some(status_response),
                    report: None,
                },
            );
        }
    }

    if notification_type_allowed(query, "poll") {
        for poll in list_poll_notifications_for_account(db, &viewer.id, per_type_limit).await? {
            let Some(actor) = find_account_by_id(db, &poll.account_id).await? else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &actor.username))
                .await?
                || !notification_account_matches_filter(
                    query.account_id.as_deref(),
                    &actor.id,
                    None,
                )
            {
                continue;
            }
            let Some(status) = find_status_by_id(db, &poll.status_id).await? else {
                continue;
            };
            if !can_view_local_status(db, &status, Some(viewer), &actor).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let status_response = build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &actor,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("poll-local-{}", poll.poll_id),
                    notification_type: "poll".to_owned(),
                    group_key: format!("poll-local-{}", poll.poll_id),
                    created_at: poll.expires_at,
                    account: MastodonAccountResponse::from_account(&actor, config),
                    status: Some(status_response),
                    report: None,
                },
            );
        }
    }

    if notification_type_allowed(query, "reblog") {
        for reblog in list_reblog_notifications_for_account(db, &viewer.id, per_type_limit).await? {
            let Some(actor) = find_account_by_id(db, &reblog.account_id).await? else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor_url(config, &actor.username))
                .await?
                || !notification_account_matches_filter(
                    query.account_id.as_deref(),
                    &actor.id,
                    None,
                )
            {
                continue;
            }
            let Some(status) = find_status_by_id(db, &reblog.status_id).await? else {
                continue;
            };
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let status_response = build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                viewer,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("reblog-local-{}-{}", actor.id, status.id),
                    notification_type: "reblog".to_owned(),
                    group_key: format!("reblog-local-{}-{}", actor.id, status.id),
                    created_at: reblog.created_at,
                    account: MastodonAccountResponse::from_account(&actor, config),
                    status: Some(status_response),
                    report: None,
                },
            );
        }

        for reblog in
            list_remote_reblog_notifications_for_account(db, &viewer.id, per_type_limit).await?
        {
            let Some(actor) = find_remote_actor_by_actor_uri(db, &reblog.remote_actor_uri).await?
            else {
                continue;
            };
            if muted_notifications_for_actor(db, &viewer.id, &actor.actor_uri).await? {
                continue;
            }
            let remote_id = remote_account_rest_id(&actor.actor_uri);
            if !notification_account_matches_filter(
                query.account_id.as_deref(),
                &remote_id,
                Some(&actor.actor_uri),
            ) {
                continue;
            }
            let Some(status) = find_status_by_id(db, &reblog.status_id).await? else {
                continue;
            };
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let status_response = build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                viewer,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?;
            push_notification_entry(
                &mut entries,
                MastodonNotificationResponse {
                    id: format!("reblog-remote-{}-{}", remote_id, status.id),
                    notification_type: "reblog".to_owned(),
                    group_key: format!("reblog-remote-{}-{}", remote_id, status.id),
                    created_at: reblog.created_at,
                    account: MastodonAccountResponse::from_remote_actor(&actor),
                    status: Some(status_response),
                    report: None,
                },
            );
        }
    }

    Ok(entries)
}

fn push_notification_entry(
    entries: &mut Vec<NotificationEntry>,
    notification: MastodonNotificationResponse,
) {
    let id = notification.id.clone();
    let created_at = notification.created_at.clone();
    entries.push(NotificationEntry {
        id,
        created_at,
        value: serde_json::to_value(notification).unwrap_or_default(),
    });
}

fn profile_field_from_update(field: &UpdateCredentialsField) -> Option<ProfileField> {
    Some(ProfileField {
        name: field.name.clone()?,
        value: field.value.clone()?,
    })
}

fn parse_profile_fields_json(value: &str) -> Vec<ProfileField> {
    serde_json::from_str::<Vec<ProfileField>>(value).unwrap_or_default()
}

fn mastodon_account_fields(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "name": field.name,
                "value": render_profile_field_value_html(&field.value),
                "verified_at": serde_json::Value::Null,
            })
        })
        .collect()
}

fn activitypub_profile_attachments(fields: &[ProfileField]) -> Vec<serde_json::Value> {
    fields
        .iter()
        .map(|field| {
            serde_json::json!({
                "type": "PropertyValue",
                "name": field.name,
                "value": render_profile_field_value_html(&field.value),
            })
        })
        .collect()
}

fn render_profile_field_value_html(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if Url::parse(trimmed)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .is_some()
    {
        let escaped = escape_html(trimmed);
        format!(
            "<a href=\"{escaped}\" rel=\"nofollow noopener noreferrer me\" target=\"_blank\">{escaped}</a>"
        )
    } else {
        escape_html(trimmed)
    }
}

async fn mutes_response(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let query: MutesQuery = req.query().unwrap_or_default();
    let limit = query.limit.unwrap_or(20).clamp(1, 40);
    let max_id = parse_internal_pagination_id(query.max_id.as_deref(), "max_id")?;
    let since_id = parse_internal_pagination_id(query.since_id.as_deref(), "since_id")?;
    let min_id = parse_internal_pagination_id(query.min_id.as_deref(), "min_id")?;
    let since_id = since_id.or(min_id);
    let db = ctx.d1(&config.database_binding)?;
    let viewer = match find_authenticated_local_account(&req, &db, &config).await? {
        Some(account) => account,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let mutes = list_mutes_for_account(&db, &viewer.id, limit, max_id, since_id).await?;
    let mut response = Vec::new();
    for mute in &mutes {
        if let Some(target_account_id) = mute.target_account_id.as_deref()
            && let Some(account) = find_account_by_id(&db, target_account_id).await?
        {
            response.push(MastodonAccountResponse::from_account(&account, &config));
            continue;
        }

        if let Some(actor) = find_remote_actor_by_actor_uri(&db, &mute.target_actor_uri).await? {
            response.push(MastodonAccountResponse::from_remote_actor(&actor));
        }
    }

    let mut builder = Response::builder();
    if let Some(link_header) = build_internal_cursor_link_header(
        &req,
        limit,
        mutes.first().map(|mute| mute.cursor_id),
        mutes.last().map(|mute| mute.cursor_id),
        mutes.len() as u32 >= limit,
        max_id.is_some() || since_id.is_some(),
    )? {
        builder = builder.with_header("Link", &link_header)?;
    }

    builder.from_json(&response)
}

async fn prune_orphan_media(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    let db = ctx.d1(&config.database_binding)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    let orphans = list_orphan_media(&db, 24, 128).await?;
    let deleted = delete_orphan_media(&db, &bucket, &orphans).await?;

    Response::from_json(&OrphanMediaPruneResponse { deleted })
}

async fn process_expired_polls(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    let db = ctx.d1(&config.database_binding)?;
    let mut summary = PollExpirationProcessResponse::default();

    for row in list_expired_polls_requiring_federation_close(&db, 64).await? {
        let Some(status) = find_status_by_id(&db, &row.status_id).await? else {
            continue;
        };
        let Some(account) = find_account_by_id(&db, &row.account_id).await? else {
            continue;
        };
        if enqueue_status_update_activity(&db, &config, &account, &status)
            .await
            .is_ok()
        {
            mark_status_poll_federated_closed(&db, &row.poll_id).await?;
            summary.queued += 1;
        }
    }

    Response::from_json(&summary)
}

async fn follow_account(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let request = parse_follow_account_request(&mut req).await?;

    let db = ctx.d1(&config.database_binding)?;
    let follower = resolve_local_account(&db, &user).await?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if follower.id == target.id {
                return Response::error("cannot follow your own account", 422);
            }

            upsert_local_follow(&db, &config, &follower, &target, &request).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &follower,
                &target.id,
                &actor_url(&config, &target.username),
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            let relationship =
                follow_remote_account(&db, &config, &follower, &actor, &request).await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

async fn unfollow_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let follower = resolve_local_account(&db, &user).await?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, &target.username);
            delete_follow_by_target(&db, &follower.id, &target_actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &follower,
                &target.id,
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            let relationship = unfollow_remote_account(&db, &config, &follower, &actor).await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

async fn block_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let blocker = resolve_local_account(&db, &user).await?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if blocker.id == target.id {
                return Response::error("cannot block your own account", 422);
            }

            let target_actor_uri = actor_url(&config, &target.username);
            upsert_block(
                &db,
                &blocker.id,
                Some(target.id.as_str()),
                &target_actor_uri,
            )
            .await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &target.id,
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            upsert_block(&db, &blocker.id, None, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

async fn unblock_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let blocker = resolve_local_account(&db, &user).await?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, &target.username);
            delete_block_by_target(&db, &blocker.id, &target_actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &target.id,
                &target_actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            delete_block_by_target(&db, &blocker.id, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &blocker,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

async fn mute_account(req: &mut Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;
    let request = parse_mute_account_request(req)
        .await
        .map_err(Error::RustError)?;

    let db = ctx.d1(&config.database_binding)?;
    let muter = resolve_local_account(&db, &user).await?;
    let notifications = request.notifications.unwrap_or(true);
    let expires_at = request
        .duration
        .filter(|value| *value > 0)
        .map(expiry_from_duration_seconds)
        .transpose()?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            if muter.id == target.id {
                return Response::error("cannot mute your own account", 422);
            }
            let target_actor_uri = actor_url(&config, &target.username);
            upsert_mute(
                &db,
                &muter.id,
                Some(target.id.as_str()),
                &target_actor_uri,
                notifications,
                expires_at.as_deref(),
            )
            .await?;
            let relationship =
                build_relationship_for_target(&db, &config, &muter, &target.id, &target_actor_uri)
                    .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            upsert_mute(
                &db,
                &muter.id,
                None,
                &actor.actor_uri,
                notifications,
                expires_at.as_deref(),
            )
            .await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &muter,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

async fn unmute_account(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let target_account_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing account id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let muter = resolve_local_account(&db, &user).await?;
    match resolve_account_reference(&db, &target_account_id).await? {
        Some(AccountReference::Local(target)) => {
            let target_actor_uri = actor_url(&config, &target.username);
            delete_mute_by_target(&db, &muter.id, &target_actor_uri).await?;
            let relationship =
                build_relationship_for_target(&db, &config, &muter, &target.id, &target_actor_uri)
                    .await?;
            Response::from_json(&relationship)
        }
        Some(AccountReference::Remote(actor)) => {
            delete_mute_by_target(&db, &muter.id, &actor.actor_uri).await?;
            let relationship = build_relationship_for_target(
                &db,
                &config,
                &muter,
                &remote_account_rest_id(&actor.actor_uri),
                &actor.actor_uri,
            )
            .await?;
            Response::from_json(&relationship)
        }
        None => Response::error("account not found", 404),
    }
}

async fn account_relationships(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let db = ctx.d1(&config.database_binding)?;
    let viewer = resolve_local_account(&db, &user).await?;
    let mut relationships = Vec::new();

    for account_id in parse_relationship_query_ids(&req)? {
        match resolve_account_reference(&db, &account_id).await? {
            Some(AccountReference::Local(target)) => {
                relationships.push(
                    build_relationship_for_target(
                        &db,
                        &config,
                        &viewer,
                        &target.id,
                        &actor_url(&config, &target.username),
                    )
                    .await?,
                );
            }
            Some(AccountReference::Remote(actor)) => {
                relationships.push(
                    build_relationship_for_target(
                        &db,
                        &config,
                        &viewer,
                        &remote_account_rest_id(&actor.actor_uri),
                        &actor.actor_uri,
                    )
                    .await?,
                );
            }
            None => {}
        }
    }

    Response::from_json(&relationships)
}

async fn create_media_attachment(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };

    let draft = match parse_media_upload(&mut req).await {
        Ok(draft) => draft,
        Err(message) => return Response::error(message, 422),
    };

    let db = ctx.d1(&config.database_binding)?;
    let bucket = ctx.bucket(&config.media_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let media = store_media_attachment(&db, &bucket, &account, &draft).await?;

    Response::from_json(&MastodonMediaAttachmentResponse::from_row(&media, &config))
}

async fn process_outbox_deliveries(req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    match extract_authenticated_user(&req, &config).await? {
        Some(_) => {}
        None => return Response::error("Cloudflare Access authentication required", 401),
    }

    let db = ctx.d1(&config.database_binding)?;
    let mut summary = OutboxProcessResponse::default();

    for delivery in list_pending_generic_outbox_deliveries(&db, 16).await? {
        let targets = list_follower_delivery_targets(&db, &delivery.account_id).await?;
        if targets.is_empty() {
            mark_outbox_delivery_completed_without_targets(&db, &delivery.id).await?;
            summary.completed_without_targets += 1;
            continue;
        }

        summary.expanded += expand_outbox_delivery_targets(&db, &delivery, &targets).await? as u32;
        mark_outbox_delivery_expanded(&db, &delivery.id).await?;
    }

    for delivery in list_pending_target_outbox_deliveries(&db, 32).await? {
        let Some(target_inbox) = delivery.target_inbox.as_deref() else {
            continue;
        };
        let Some(account) = find_account_by_id(&db, &delivery.account_id).await? else {
            mark_outbox_delivery_terminal_failure(
                &db,
                &delivery.id,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        let account = ensure_account_keys(&db, account).await?;

        match send_signed_activity(&config, &account, target_inbox, &delivery.payload_json).await {
            Ok(()) => {
                mark_outbox_delivery_delivered(&db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(_error) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                if next_attempt >= 5 {
                    mark_outbox_delivery_terminal_failure(&db, &delivery.id, next_attempt).await?;
                } else {
                    reschedule_outbox_delivery(&db, &delivery.id, next_attempt).await?;
                }
                summary.failed += 1;
            }
        }
    }

    for delivery in list_pending_outbound_activities(&db, 32).await? {
        let Some(account) = find_account_by_id(&db, &delivery.account_id).await? else {
            reconcile_outbound_activity_terminal_failure(
                &db,
                &delivery,
                delivery.attempt_count.saturating_add(1) as u32,
            )
            .await?;
            summary.failed += 1;
            continue;
        };
        let account = ensure_account_keys(&db, account).await?;

        match send_signed_activity(
            &config,
            &account,
            &delivery.target_inbox,
            &delivery.payload_json,
        )
        .await
        {
            Ok(()) => {
                mark_outbound_activity_delivered(&db, &delivery.id).await?;
                summary.delivered += 1;
            }
            Err(_) => {
                let next_attempt = delivery.attempt_count.saturating_add(1) as u32;
                if next_attempt >= 5 {
                    reconcile_outbound_activity_terminal_failure(&db, &delivery, next_attempt)
                        .await?;
                } else {
                    reschedule_outbound_activity(&db, &delivery.id, next_attempt).await?;
                }
                summary.failed += 1;
            }
        }
    }

    Response::from_json(&summary)
}

async fn handle_inbox_request(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
    account: Option<&LocalAccount>,
    body: &[u8],
    activity: &serde_json::Value,
) -> Result<Response> {
    let remote_actor = match verify_incoming_activitypub_request(req, db, body, activity).await {
        Ok(remote_actor) => remote_actor,
        Err(_) => return Response::error("invalid activitypub signature", 401),
    };
    let activity_id = inbox_activity_id(activity);
    if let Some(activity_id) = activity_id.as_deref()
        && !begin_inbox_activity_processing(
            db,
            &remote_actor.actor_uri,
            activity_id,
            activity
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        )
        .await?
    {
        return Ok(Response::empty()?.with_status(202));
    }

    let result = match activity.get("type").and_then(serde_json::Value::as_str) {
        Some("Follow") => {
            if let Some(account) = account {
                handle_inbox_follow(db, config, account, activity, &remote_actor).await
            } else {
                Ok(())
            }
        }
        Some("Undo") => {
            if let Some(account) = account {
                handle_inbox_undo(db, account, activity, &remote_actor, config).await
            } else {
                Ok(())
            }
        }
        Some("Accept") => handle_inbox_accept(db, activity, &remote_actor).await,
        Some("Reject") => handle_inbox_reject(db, activity, &remote_actor).await,
        Some("Like") => {
            if let Some(account) = account {
                handle_inbox_like(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Create") => {
            if let Some(account) = account {
                handle_inbox_create(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Announce") => {
            if let Some(account) = account {
                handle_inbox_announce(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Update") => {
            if let Some(account) = account {
                handle_inbox_update(db, activity, &remote_actor, account, config).await
            } else {
                Ok(())
            }
        }
        Some("Delete") => handle_inbox_delete(db, activity, &remote_actor).await,
        _ => Ok(()),
    };

    if let Some(activity_id) = activity_id.as_deref() {
        match &result {
            Ok(()) => {
                mark_inbox_activity_processed(db, &remote_actor.actor_uri, activity_id).await?
            }
            Err(_) => {
                release_inbox_activity_processing(db, &remote_actor.actor_uri, activity_id).await?
            }
        }
    }
    result?;
    Ok(Response::empty()?.with_status(202))
}

async fn handle_inbox_follow(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    if !follow_targets_local_actor(
        activity.get("object"),
        &actor_url(config, &account.username),
    ) {
        return Ok(());
    }

    upsert_follower(db, &account.id, remote_actor).await?;

    let accept_activity =
        build_accept_activity(config, account, activity, &remote_actor.actor_uri)?;
    let _ = queue_remote_actor_activity_required(
        db,
        &account.id,
        &remote_actor.actor_uri,
        &accept_activity,
    )
    .await;

    Ok(())
}

async fn handle_inbox_undo(
    db: &D1Database,
    account: &LocalAccount,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    config: &AppConfig,
) -> Result<()> {
    let Some(actor_uri) = activity.get("actor").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    if !is_follow_undo(activity.get("object"), actor_uri, &remote_actor.actor_uri) {
        if handle_inbox_poll_vote_undo(db, activity, remote_actor, account, config).await? {
            return Ok(());
        }
        return handle_inbox_interaction_undo(db, activity, remote_actor).await;
    }

    delete_follower_by_actor(db, &account.id, actor_uri, &remote_actor.actor_uri).await
}

async fn handle_inbox_like(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    let Some(status) = find_local_status_by_object_uri(db, config, object_uri).await? else {
        return Ok(());
    };
    if status.account_id != account.id {
        return Ok(());
    }
    let activity_uri = activity.get("id").and_then(serde_json::Value::as_str);
    upsert_remote_favourite(
        db,
        &remote_actor.actor_uri,
        &status.id,
        object_uri,
        activity_uri,
    )
    .await
}

async fn handle_inbox_announce(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    let Some(status) = find_local_status_by_object_uri(db, config, object_uri).await? else {
        return Ok(());
    };
    if status.account_id != account.id {
        return Ok(());
    }
    let activity_uri = activity.get("id").and_then(serde_json::Value::as_str);
    upsert_remote_reblog(
        db,
        &remote_actor.actor_uri,
        &status.id,
        object_uri,
        activity_uri,
    )
    .await
}

async fn handle_inbox_interaction_undo(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(object) = activity.get("object") else {
        return Ok(());
    };
    let activity_type = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let target_uri = object
        .get("object")
        .and_then(|value| activity_object_id(Some(value)))
        .unwrap_or_default();
    let activity_uri = object.get("id").and_then(serde_json::Value::as_str);

    match activity_type {
        "Like" => {
            delete_remote_favourite(db, &remote_actor.actor_uri, target_uri, activity_uri).await?
        }
        "Announce" => {
            delete_remote_reblog(db, &remote_actor.actor_uri, target_uri, activity_uri).await?
        }
        _ => {}
    }

    Ok(())
}

async fn handle_inbox_poll_vote_undo(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<bool> {
    let Some(undo_target) = activity.get("object") else {
        return Ok(false);
    };
    let activity_uri = activity_object_id(Some(undo_target)).map(str::to_owned);
    let nested_object = undo_target.get("object").filter(|value| value.is_object());
    let choice_name = nested_object
        .and_then(|object| object.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let in_reply_to = nested_object
        .and_then(|object| object.get("inReplyTo"))
        .and_then(serde_json::Value::as_str);

    let (poll, status) = if let Some(activity_uri) = activity_uri.as_deref()
        && let Some(vote) = find_status_poll_vote_for_remote_actor_by_activity_uri(
            db,
            &remote_actor.actor_uri,
            activity_uri,
        )
        .await?
    {
        let Some(status) = find_status_by_id(db, &vote.status_id).await? else {
            return Ok(false);
        };
        let Some(poll) = find_status_poll_by_status_id(db, &vote.status_id).await? else {
            return Ok(false);
        };
        (poll, status)
    } else if let Some(in_reply_to) = in_reply_to
        && let Some(status) = find_local_status_by_object_uri(db, config, in_reply_to).await?
    {
        let Some(poll) = find_status_poll_by_status_id(db, &status.id).await? else {
            return Ok(false);
        };
        (poll, status)
    } else {
        return Ok(false);
    };

    if status.account_id != account.id {
        return Ok(false);
    }

    let deleted = delete_incoming_poll_vote(
        db,
        &poll,
        &remote_actor.actor_uri,
        activity_uri.as_deref(),
        choice_name,
    )
    .await?;
    if deleted {
        let _ = enqueue_status_update_activity(db, config, account, &status).await;
    }
    Ok(deleted)
}

async fn handle_inbox_create(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    if !is_supported_remote_status_object_type(
        object.get("type").and_then(serde_json::Value::as_str),
    ) {
        return Ok(());
    }

    let attributed_to = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
        .unwrap_or_default();
    if attributed_to != remote_actor.actor_uri {
        return Ok(());
    }
    if handle_inbox_poll_vote(
        db,
        object,
        remote_actor,
        account,
        config,
        activity.get("id").and_then(serde_json::Value::as_str),
    )
    .await?
    {
        return Ok(());
    }
    if !note_targets_account_or_followers(object, account, config) {
        return Ok(());
    }

    upsert_remote_actor(db, remote_actor).await?;
    upsert_remote_status(db, remote_actor, object).await
}

async fn handle_inbox_update(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
) -> Result<()> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    if is_activitypub_actor_type(object.get("type").and_then(serde_json::Value::as_str)) {
        return handle_inbox_actor_update(db, activity, remote_actor, Some(account)).await;
    }
    if !is_supported_remote_status_object_type(
        object.get("type").and_then(serde_json::Value::as_str),
    ) {
        return Ok(());
    }

    let attributed_to = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
        .unwrap_or_default();
    if attributed_to != remote_actor.actor_uri {
        return Ok(());
    }
    if !note_targets_account_or_followers(object, account, config) {
        return Ok(());
    }

    upsert_remote_actor(db, remote_actor).await?;
    upsert_remote_status(db, remote_actor, object).await
}

async fn handle_inbox_poll_vote(
    db: &D1Database,
    object: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: &LocalAccount,
    config: &AppConfig,
    activity_uri: Option<&str>,
) -> Result<bool> {
    let Some(in_reply_to) = object.get("inReplyTo").and_then(serde_json::Value::as_str) else {
        return Ok(false);
    };
    let Some(choice_name) = object.get("name").and_then(serde_json::Value::as_str) else {
        return Ok(false);
    };
    let Some(status) = find_local_status_by_object_uri(db, config, in_reply_to).await? else {
        return Ok(false);
    };
    if status.account_id != account.id {
        return Ok(false);
    }
    let Some(poll) = find_status_poll_by_status_id(db, &status.id).await? else {
        return Ok(false);
    };
    if is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false) {
        return Ok(true);
    }
    let options = list_status_poll_options(db, &poll.id).await?;
    let Some(choice) = options
        .iter()
        .position(|option| option.title == choice_name.trim())
    else {
        return Ok(true);
    };

    apply_incoming_poll_vote(
        db,
        &poll,
        &remote_actor.actor_uri,
        choice as u32,
        activity_uri,
    )
    .await?;
    let _ = enqueue_status_update_activity(db, config, account, &status).await;
    Ok(true)
}

async fn handle_inbox_actor_update(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    account: Option<&LocalAccount>,
) -> Result<()> {
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(());
    };
    if !is_activitypub_actor_type(object.get("type").and_then(serde_json::Value::as_str)) {
        return Ok(());
    }

    let object_actor_uri = activity_object_id(Some(object))
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
        .unwrap_or_default();
    if object_actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    let is_relevant = match account {
        Some(account) => {
            is_local_account_following_remote_actor(db, &account.id, &remote_actor.actor_uri)
                .await?
        }
        None => has_any_local_followers_for_remote_actor(db, &remote_actor.actor_uri).await?,
    };
    if !is_relevant {
        return Ok(());
    }

    let refreshed = parse_remote_actor_profile_document(object, &remote_actor.actor_uri)?;
    validate_remote_actor_profile_urls(&refreshed).await?;
    upsert_remote_actor(db, &refreshed).await
}

fn is_activitypub_actor_type(actor_type: Option<&str>) -> bool {
    matches!(
        actor_type,
        Some("Person" | "Application" | "Group" | "Organization" | "Service")
    )
}

async fn handle_inbox_accept(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    update_follow_state_from_response(db, activity, remote_actor, "accepted").await
}

async fn handle_inbox_reject(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    update_follow_state_from_response(db, activity, remote_actor, "rejected").await
}

async fn handle_inbox_delete(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let Some(object_uri) = activity_object_id(activity.get("object")) else {
        return Ok(());
    };
    let Some(status) = find_remote_status_by_object_uri(db, object_uri).await? else {
        return Ok(());
    };
    if status.actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    delete_remote_status_by_id(db, &status.id).await
}

async fn media_content_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let media_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing media id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(media) = find_media_attachment_by_id(&db, &media_id).await? else {
        return Response::error("media not found", 404);
    };

    if config.media_public_base_url.is_some() {
        return Response::redirect(
            Url::parse(&media_object_url(&config, &media.object_key))
                .map_err(|error| Error::RustError(format!("invalid media public url: {error}")))?,
        );
    }

    let bucket = ctx.bucket(&config.media_binding)?;
    let Some(object) = bucket.get(&media.object_key).execute().await? else {
        return Response::error("media object not found", 404);
    };
    let Some(body) = object.body() else {
        return Response::error("media object body missing", 500);
    };

    let mut response = Response::from_body(body.response_body()?)?;
    response
        .headers_mut()
        .set("Content-Type", &media.content_type)?;
    response.headers_mut().set("ETag", &object.http_etag())?;
    response
        .headers_mut()
        .set("Cache-Control", "public, max-age=31536000, immutable")?;

    Ok(response)
}

async fn media_metadata_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let media_id = ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing media id route parameter".to_owned()))?;

    let db = ctx.d1(&config.database_binding)?;
    let Some(media) = find_media_attachment_by_id(&db, &media_id).await? else {
        return Response::error("media not found", 404);
    };

    Response::from_json(&MastodonMediaAttachmentResponse::from_row(&media, &config))
}

async fn update_media_attachment(mut req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let user = match extract_authenticated_user(&req, &config).await? {
        Some(user) => user,
        None => return Response::error("Cloudflare Access authentication required", 401),
    };
    let media_id = match ctx
        .param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        Some(media_id) => media_id,
        None => return Response::error("missing media id route parameter", 400),
    };
    let update = match parse_media_update_request(&mut req).await {
        Ok(update) => update,
        Err(message) => return Response::error(message, 422),
    };

    let db = ctx.d1(&config.database_binding)?;
    let account = resolve_local_account(&db, &user).await?;
    let media = match find_media_attachment_by_id(&db, &media_id).await? {
        Some(media) if media.account_id == account.id => media,
        _ => return Response::error("media not found", 404),
    };

    let media = apply_media_update(&db, &media, update).await?;
    Response::from_json(&MastodonMediaAttachmentResponse::from_row(&media, &config))
}

fn root_document() -> RootDocument {
    let build = build_metadata();

    RootDocument {
        service: build.service_name.to_owned(),
        version: build.version.to_owned(),
        runtime: build.runtime.to_owned(),
        endpoints: vec![
            "/",
            "/healthz",
            "/api/v1/instance",
            "/api/v1/timelines/home",
            "/api/v1/timelines/public",
            "/api/v1/timelines/tag/:hashtag",
            "/api/v1/statuses/:id",
            "/api/v1/statuses/:id/favourite",
            "/api/v1/statuses/:id/unfavourite",
            "/api/v1/statuses/:id/reblog",
            "/api/v1/statuses/:id/unreblog",
            "/api/v1/statuses/:id/bookmark",
            "/api/v1/statuses/:id/unbookmark",
            "/api/v1/statuses/:id/context",
            "/api/v1/tags/:name",
            "/.well-known/webfinger",
            "/inbox",
            "/users/:username",
            "/users/:username/followers",
            "/users/:username/following",
            "/users/:username/inbox",
            "/users/:username/outbox",
            "/users/:username/statuses/:id",
            "/media/:id",
            "/api/v1/media/:id",
            "/api/v1/statuses",
            "/api/v2/media",
            "/api/v2/media/:id",
            "/internal/polls/process-expired",
            "/api/v1/accounts/verify_credentials",
            "/api/v1/accounts/update_credentials",
            "/api/v1/accounts/:id",
            "/api/v1/accounts/:id/statuses",
            "/api/v1/accounts/:id/follow",
            "/api/v1/accounts/:id/unfollow",
            "/api/v1/accounts/:id/block",
            "/api/v1/accounts/:id/unblock",
            "/api/v1/accounts/:id/mute",
            "/api/v1/accounts/:id/unmute",
            "/api/v1/accounts/relationships",
            "/api/v1/accounts/lookup",
            "/api/v1/accounts/search",
            "/api/v1/directory",
            "/api/v1/favourites",
            "/api/v1/bookmarks",
            "/api/v1/mutes",
            "/api/v1/notifications",
            "/api/v1/notifications/:id",
            "/api/v1/notifications/:id/dismiss",
            "/api/v1/notifications/clear",
            "/api/v1/notifications/unread_count",
            "/api/v1/polls/:id",
            "/api/v1/polls/:id/votes",
            "/api/v1/reports",
            "/api/v1/instance/peers",
            "/api/v1/instance/extended_description",
            "/api/v1/instance/privacy_policy",
            "/api/v1/instance/terms_of_service",
            "/api/v2/search",
            "/api/v2/instance",
            "/.well-known/nodeinfo",
            "/nodeinfo/2.0",
            "/internal/media/prune-orphans",
            "/internal/outbox/process",
        ],
    }
}

async fn instance_summary_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let summary = load_instance_summary(&db, config.clone()).await?;
    let active_month = load_active_month_users(&db).await?;
    let user_count = load_total_local_accounts(&db).await?;
    let status_count = load_total_local_statuses(&db).await?;
    let domain_count = load_known_peer_domains(&db, &config).await?.len() as u64;

    Response::from_json(&build_instance_v1_document(
        &summary,
        &config,
        active_month,
        user_count,
        status_count,
        domain_count,
    ))
}

async fn instance_v2_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let summary = load_instance_summary(&db, config.clone()).await?;
    let active_month = load_active_month_users(&db).await?;

    Response::from_json(&build_instance_v2_document(&summary, &config, active_month))
}

async fn instance_peers_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;

    Response::from_json(&load_known_peer_domains(&db, &config).await?)
}

async fn instance_extended_description_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let Some(content) = configured_html_document(
        config.instance_extended_description_html.as_deref(),
        config.instance_extended_description_updated_at.as_deref(),
        "1970-01-01T00:00:00Z",
        false,
    ) else {
        return Response::error("Record not found", 404);
    };

    Response::from_json(&content)
}

async fn instance_privacy_policy_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let Some(content) = configured_html_document(
        config.privacy_policy_html.as_deref(),
        config.privacy_policy_updated_at.as_deref(),
        "1970-01-01T00:00:00Z",
        false,
    ) else {
        return Response::error("Record not found", 404);
    };

    Response::from_json(&content)
}

async fn instance_terms_of_service_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let Some(content) = configured_html_document(
        config.terms_of_service_html.as_deref(),
        config.terms_of_service_effective_date.as_deref(),
        "1970-01-01",
        true,
    ) else {
        return Response::error("Record not found", 404);
    };

    Response::from_json(&content)
}

async fn nodeinfo_links_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    Response::from_json(&build_nodeinfo_links_document(&config))
}

async fn nodeinfo_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    let db = ctx.d1(&config.database_binding)?;
    let summary = load_instance_summary(&db, config.clone()).await?;
    let active_month = load_active_month_users(&db).await?;
    let user_count = load_total_local_accounts(&db).await?;
    let status_count = load_total_local_statuses(&db).await?;

    Response::from_json(&build_nodeinfo_document(
        &summary,
        &config,
        user_count,
        active_month,
        status_count,
    ))
}

async fn load_instance_summary(db: &D1Database, config: AppConfig) -> Result<InstanceSummary> {
    let build = build_metadata();
    let settings = db
        .prepare(
            "SELECT domain, title, description
             FROM instance_settings
             WHERE id = 1
             LIMIT 1",
        )
        .first::<InstanceSettingsRow>(None)
        .await?;

    let (domain, title, description) = match settings {
        Some(settings) => (settings.domain, settings.title, settings.description),
        None => (
            config.instance_domain,
            config.instance_name,
            config.instance_description,
        ),
    };

    Ok(InstanceSummary {
        domain: normalize_instance_domain(&domain),
        title,
        description,
        software: SoftwareInfo {
            name: build.service_name.to_owned(),
            version: build.version.to_owned(),
        },
        capabilities: InstanceCapabilities {
            federation: true,
            local_timeline: true,
            media_uploads: true,
        },
    })
}

async fn load_active_month_users(db: &D1Database) -> Result<u64> {
    let row = db
        .prepare(
            "SELECT COUNT(DISTINCT account_id) AS count
             FROM statuses
             WHERE created_at >= datetime('now', '-28 days')",
        )
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

async fn load_total_local_accounts(db: &D1Database) -> Result<u64> {
    let row = db
        .prepare("SELECT COUNT(*) AS count FROM accounts")
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

async fn load_total_local_statuses(db: &D1Database) -> Result<u64> {
    let row = db
        .prepare("SELECT COUNT(*) AS count FROM statuses")
        .first::<ActiveMonthCountRow>(None)
        .await?;

    Ok(row.map(|value| value.count).unwrap_or(0))
}

async fn load_known_peer_domains(db: &D1Database, config: &AppConfig) -> Result<Vec<String>> {
    let mut peers = BTreeSet::new();

    for value in db
        .prepare(
            "SELECT DISTINCT domain
             FROM remote_actors
             WHERE domain IS NOT NULL
               AND trim(domain) != ''",
        )
        .all()
        .await?
        .results::<serde_json::Value>()?
    {
        if let Some(domain) = value.get("domain").and_then(serde_json::Value::as_str) {
            let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
            if !domain.is_empty() && domain != instance_host(config) {
                peers.insert(domain);
            }
        }
    }

    for (sql, field) in [
        (
            "SELECT DISTINCT target_actor_uri AS actor_uri
             FROM follows
             WHERE target_actor_uri IS NOT NULL
               AND trim(target_actor_uri) != ''",
            "actor_uri",
        ),
        (
            "SELECT DISTINCT actor_uri
             FROM followers
             WHERE actor_uri IS NOT NULL
               AND trim(actor_uri) != ''",
            "actor_uri",
        ),
    ] {
        for value in db
            .prepare(sql)
            .all()
            .await?
            .results::<serde_json::Value>()?
        {
            if let Some(uri) = value.get(field).and_then(serde_json::Value::as_str)
                && let Some(peer) = peer_authority_from_uri(config, uri)
            {
                peers.insert(peer);
            }
        }
    }

    Ok(peers.into_iter().collect())
}

fn build_instance_v1_document(
    summary: &InstanceSummary,
    config: &AppConfig,
    _active_month: u64,
    user_count: u64,
    status_count: u64,
    domain_count: u64,
) -> serde_json::Value {
    serde_json::json!({
        "uri": summary.domain,
        "title": summary.title,
        "short_description": summary.description,
        "description": render_status_html(&summary.description),
        "email": config.contact_email.clone().unwrap_or_default(),
        "version": summary.software.version,
        "urls": {
            "streaming_api": serde_json::Value::Null,
        },
        "stats": {
            "user_count": user_count,
            "status_count": status_count,
            "domain_count": domain_count,
        },
        "thumbnail": config.instance_thumbnail_url,
        "languages": configured_instance_languages(config),
        "registrations": false,
        "approval_required": false,
        "invites_enabled": false,
        "configuration": {
            "statuses": {
                "max_characters": 500,
                "max_media_attachments": 4,
                "characters_reserved_per_url": 23,
            },
            "media_attachments": {
                "supported_mime_types": instance_supported_mime_types(),
                "description_limit": 1500,
                "image_size_limit": MAX_IMAGE_UPLOAD_BYTES,
                "video_size_limit": MAX_AV_UPLOAD_BYTES,
            },
            "polls": {
                "max_options": 0,
                "max_characters_per_option": 0,
                "min_expiration": 0,
                "max_expiration": 0,
            },
        },
        "contact_account": serde_json::Value::Null,
        "rules": Vec::<serde_json::Value>::new(),
    })
}

fn build_instance_v2_document(
    summary: &InstanceSummary,
    config: &AppConfig,
    active_month: u64,
) -> serde_json::Value {
    let about_url = config
        .instance_extended_description_html
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|_| extended_description_url(config));
    let privacy_policy_url = config
        .privacy_policy_html
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|_| privacy_policy_url(config));
    let terms_of_service_url = config
        .terms_of_service_html
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|_| terms_of_service_url(config));
    let mut response = serde_json::Map::new();
    response.insert("domain".to_owned(), serde_json::json!(summary.domain));
    response.insert("title".to_owned(), serde_json::json!(summary.title));
    response.insert(
        "version".to_owned(),
        serde_json::json!(summary.software.version),
    );
    response.insert(
        "description".to_owned(),
        serde_json::json!(summary.description),
    );
    response.insert(
        "usage".to_owned(),
        serde_json::json!({
            "users": {
                "active_month": active_month,
            },
        }),
    );
    response.insert(
        "icon".to_owned(),
        serde_json::json!(Vec::<serde_json::Value>::new()),
    );
    response.insert(
        "languages".to_owned(),
        serde_json::json!(configured_instance_languages(config)),
    );
    response.insert(
        "configuration".to_owned(),
        serde_json::json!({
            "urls": {
                "streaming": serde_json::Value::Null,
                "status": serde_json::Value::Null,
                "about": about_url,
                "privacy_policy": privacy_policy_url,
                "terms_of_service": terms_of_service_url,
            },
            "accounts": {
                "max_featured_tags": 0,
                "max_pinned_statuses": 0,
            },
            "statuses": {
                "max_characters": 500,
                "max_media_attachments": 4,
                "characters_reserved_per_url": 23,
            },
            "media_attachments": {
                "supported_mime_types": instance_supported_mime_types(),
                "description_limit": 1500,
                "image_size_limit": MAX_IMAGE_UPLOAD_BYTES,
                "video_size_limit": MAX_AV_UPLOAD_BYTES,
            },
            "polls": {
                "max_options": 0,
                "max_characters_per_option": 0,
                "min_expiration": 0,
                "max_expiration": 0,
            },
            "translation": {
                "enabled": false,
            },
            "timelines_access": {
                "live_feeds": {
                    "local": "public",
                    "remote": "public",
                },
                "hashtag_feeds": {
                    "local": "public",
                    "remote": "public",
                },
            },
            "limited_federation": false,
        }),
    );
    response.insert(
        "registrations".to_owned(),
        serde_json::json!({
            "enabled": false,
            "approval_required": false,
            "reason_required": false,
            "message": "Registration is handled by Cloudflare Access.",
            "min_age": serde_json::Value::Null,
            "url": serde_json::Value::Null,
        }),
    );
    response.insert(
        "api_versions".to_owned(),
        // Keep this deliberately conservative until a compatibility suite covers more of the surface.
        serde_json::json!({ "mastodon": 1 }),
    );
    response.insert(
        "rules".to_owned(),
        serde_json::json!(Vec::<serde_json::Value>::new()),
    );

    if let Some(source_url) = config.source_url.as_deref() {
        response.insert("source_url".to_owned(), serde_json::json!(source_url));
    }

    if let Some(thumbnail_url) = config.instance_thumbnail_url.as_deref() {
        response.insert(
            "thumbnail".to_owned(),
            serde_json::json!({
                "url": thumbnail_url,
            }),
        );
    }

    if let Some(contact_email) = config.contact_email.as_deref() {
        response.insert(
            "contact".to_owned(),
            serde_json::json!({
                "email": contact_email,
                "account": serde_json::Value::Null,
            }),
        );
    }

    serde_json::Value::Object(response)
}

fn build_nodeinfo_links_document(config: &AppConfig) -> serde_json::Value {
    serde_json::json!({
        "links": [
            {
                "rel": "http://nodeinfo.diaspora.software/ns/schema/2.0",
                "href": nodeinfo_url(config),
            }
        ]
    })
}

fn build_nodeinfo_document(
    summary: &InstanceSummary,
    _config: &AppConfig,
    user_count: u64,
    active_month: u64,
    status_count: u64,
) -> serde_json::Value {
    serde_json::json!({
        "version": "2.0",
        "software": {
            "name": summary.software.name,
            "version": summary.software.version,
        },
        "protocols": ["activitypub"],
        "services": {
            "inbound": [],
            "outbound": [],
        },
        "openRegistrations": false,
        "usage": {
            "users": {
                "total": user_count,
                "activeMonth": active_month,
            },
            "localPosts": status_count,
        },
        "metadata": {
            "nodeName": summary.title,
            "nodeDescription": summary.description,
        }
    })
}

fn configured_html_document(
    content: Option<&str>,
    metadata: Option<&str>,
    default_metadata: &str,
    is_terms: bool,
) -> Option<serde_json::Value> {
    let content = content?.trim();
    if content.is_empty() {
        return None;
    }

    let metadata = metadata
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_metadata);
    Some(if is_terms {
        serde_json::json!({
            "effective_date": metadata,
            "effective": true,
            "content": content,
            "succeeded_by": serde_json::Value::Null,
        })
    } else {
        serde_json::json!({
            "updated_at": metadata,
            "content": content,
        })
    })
}

fn load_config(ctx: &RouteContext<()>) -> AppConfig {
    let mut config = AppConfig::new(
        optional_var(ctx, "INSTANCE_DOMAIN").unwrap_or_else(|| "example.com".to_owned()),
        optional_var(ctx, "INSTANCE_NAME").unwrap_or_else(|| "cfwdon".to_owned()),
        optional_var(ctx, "INSTANCE_DESCRIPTION").unwrap_or_else(|| {
            "Cloudflare Workers + D1 + R2 based Mastodon-compatible server".to_owned()
        }),
    );

    if let Some(value) = optional_var(ctx, "SOURCE_URL") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.source_url = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_LANGUAGES") {
        let languages = parse_csv_list(&value);
        if !languages.is_empty() {
            config.instance_languages = languages;
        }
    }

    if let Some(value) = optional_var(ctx, "ADMIN_EMAILS") {
        config.admin_emails = parse_csv_list(&value);
    }

    if let Some(value) = optional_var(ctx, "CONTACT_EMAIL") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.contact_email = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_THUMBNAIL_URL") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.instance_thumbnail_url = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_EXTENDED_DESCRIPTION_HTML") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.instance_extended_description_html = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "INSTANCE_EXTENDED_DESCRIPTION_UPDATED_AT") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.instance_extended_description_updated_at = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "PRIVACY_POLICY_HTML") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.privacy_policy_html = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "PRIVACY_POLICY_UPDATED_AT") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.privacy_policy_updated_at = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "TERMS_OF_SERVICE_HTML") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.terms_of_service_html = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "TERMS_OF_SERVICE_EFFECTIVE_DATE") {
        let value = value.trim().to_owned();
        if !value.is_empty() {
            config.terms_of_service_effective_date = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "MEDIA_PUBLIC_BASE_URL") {
        let value = value.trim().trim_end_matches('/').to_owned();
        if !value.is_empty() {
            config.media_public_base_url = Some(value);
        }
    }

    if let Some(value) = optional_var(ctx, "ACCESS_EMAIL_HEADER") {
        config.access_email_header = value;
    }

    if let Some(value) = optional_var(ctx, "ACCESS_JWT_HEADER") {
        config.access_jwt_header = value;
    }

    if let Some(value) = optional_var(ctx, "ACCESS_TEAM_DOMAIN") {
        config.access_team_domain = value;
    }

    if let Some(value) = optional_var(ctx, "ACCESS_AUD") {
        config.access_audience = value;
    }

    config
}

fn optional_var(ctx: &RouteContext<()>, key: &str) -> Option<String> {
    ctx.var(key).ok().map(|value| value.to_string())
}

fn parse_csv_list(value: &str) -> Vec<String> {
    let mut values = value
        .split(',')
        .map(|entry| entry.trim().to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn parse_webfinger_resource(resource: &str) -> Result<AccountHandle> {
    let resource = resource.trim();
    let Some(acct) = resource.strip_prefix("acct:") else {
        return Err(Error::RustError(
            "WebFinger resource must use the acct: scheme".to_owned(),
        ));
    };

    let Some((username, domain)) = acct.split_once('@') else {
        return Err(Error::RustError(
            "WebFinger resource must be in acct:user@domain form".to_owned(),
        ));
    };

    let username = username.trim().to_ascii_lowercase();
    let domain = domain.trim().to_ascii_lowercase();
    if username.is_empty() || domain.is_empty() {
        return Err(Error::RustError(
            "WebFinger resource must include both username and domain".to_owned(),
        ));
    }

    Ok(AccountHandle {
        username,
        domain: Some(domain),
    })
}

fn parse_lookup_handle(value: &str, config: &AppConfig) -> Result<AccountHandle> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Error::RustError(
            "acct query parameter is required".to_owned(),
        ));
    }
    if value.starts_with("acct:") {
        return parse_webfinger_resource(value);
    }

    if let Some((username, domain)) = value.split_once('@') {
        let username = username.trim().to_ascii_lowercase();
        let domain = domain.trim().to_ascii_lowercase();
        if username.is_empty() || domain.is_empty() {
            return Err(Error::RustError(
                "acct must be in user@domain form".to_owned(),
            ));
        }
        return Ok(AccountHandle {
            username,
            domain: Some(domain),
        });
    }

    Ok(AccountHandle {
        username: value.trim().to_ascii_lowercase(),
        domain: Some(instance_host(config)),
    })
}

fn actor_url(config: &AppConfig, username: &str) -> String {
    format!("{}/users/{}", instance_base_url(config), username)
}

fn normalize_instance_domain(value: &str) -> String {
    value
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(value.trim())
        .to_owned()
}

fn configured_instance_languages(config: &AppConfig) -> Vec<String> {
    if config.instance_languages.is_empty() {
        vec!["en".to_owned()]
    } else {
        config.instance_languages.clone()
    }
}

fn instance_supported_mime_types() -> Vec<&'static str> {
    vec![
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/webp",
        "video/mp4",
        "video/webm",
        "audio/mpeg",
        "audio/mp3",
        "audio/ogg",
        "audio/webm",
    ]
}

fn media_object_url(config: &AppConfig, object_key: &str) -> String {
    let base = config
        .media_public_base_url
        .clone()
        .unwrap_or_else(|| instance_base_url(config));
    let base = base.trim_end_matches('/');
    let path = object_key.trim_start_matches('/');
    format!("{base}/{path}")
}

fn media_fallback_url(config: &AppConfig, media_id: &str) -> String {
    format!("{}/media/{}", instance_base_url(config), media_id)
}

fn public_key_id(config: &AppConfig, username: &str) -> String {
    format!("{}#main-key", actor_url(config, username))
}

fn shared_inbox_url(config: &AppConfig) -> String {
    format!("{}/inbox", instance_base_url(config))
}

fn remote_account_rest_id(actor_uri: &str) -> String {
    format!("r_{}", URL_SAFE_NO_PAD.encode(actor_uri.as_bytes()))
}

fn remote_actor_uri_from_rest_id(account_id: &str) -> Option<String> {
    let encoded = account_id.strip_prefix("r_")?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    String::from_utf8(bytes).ok()
}

fn instance_host(config: &AppConfig) -> String {
    config
        .instance_domain
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .split('/')
        .next()
        .unwrap_or(config.instance_domain.trim())
        .to_owned()
}

fn instance_base_url(config: &AppConfig) -> String {
    let domain = config.instance_domain.trim().trim_end_matches('/');
    if domain.starts_with("https://") || domain.starts_with("http://") {
        domain.to_owned()
    } else {
        format!("https://{}", instance_host(config))
    }
}

fn nodeinfo_url(config: &AppConfig) -> String {
    format!("{}/nodeinfo/2.0", instance_base_url(config))
}

fn extended_description_url(config: &AppConfig) -> String {
    format!(
        "{}/api/v1/instance/extended_description",
        instance_base_url(config)
    )
}

fn privacy_policy_url(config: &AppConfig) -> String {
    format!(
        "{}/api/v1/instance/privacy_policy",
        instance_base_url(config)
    )
}

fn terms_of_service_url(config: &AppConfig) -> String {
    format!(
        "{}/api/v1/instance/terms_of_service",
        instance_base_url(config)
    )
}

fn peer_authority_from_uri(config: &AppConfig, uri: &str) -> Option<String> {
    let parsed = Url::parse(uri).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let default_port = match parsed.scheme() {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    };
    let authority = match (parsed.port(), default_port) {
        (Some(port), Some(scheme_default)) if port == scheme_default => host,
        (Some(port), _) => format!("{host}:{port}"),
        (None, _) => host,
    };

    if authority == instance_host(config) {
        None
    } else {
        Some(authority)
    }
}

async fn generate_account_key_material() -> Result<AccountKeyMaterial> {
    let subtle = subtle_crypto()?;
    let algorithm = rsa_signing_algorithm(2048)?;
    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("sign"));
    key_usages.push(&JsValue::from_str("verify"));

    let key_pair =
        JsFuture::from(subtle.generate_key_with_object(&algorithm, true, &key_usages.into())?)
            .await?
            .dyn_into::<CryptoKeyPair>()
            .map_err(|_| Error::RustError("failed to generate RSA account key pair".to_owned()))?;

    let private_key = key_pair.get_private_key();
    let public_key = key_pair.get_public_key();

    Ok(AccountKeyMaterial {
        private_key_jwk: export_private_key_jwk(&subtle, &private_key).await?,
        public_key_pem: export_public_key_pem(&subtle, &public_key).await?,
    })
}

fn subtle_crypto() -> Result<web_sys::SubtleCrypto> {
    let global = js_sys::global()
        .dyn_into::<WorkerGlobalScope>()
        .map_err(|_| Error::RustError("failed to access WorkerGlobalScope".to_owned()))?;
    Ok(global.crypto().map_err(Error::from)?.subtle())
}

fn rsa_signing_algorithm(modulus_length: u32) -> Result<Object> {
    let algorithm = Object::new();
    Reflect::set(
        &algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )
    .map_err(Error::from)?;
    Reflect::set(
        &algorithm,
        &JsValue::from_str("modulusLength"),
        &JsValue::from_f64(modulus_length as f64),
    )
    .map_err(Error::from)?;

    let public_exponent = Uint8Array::from([1u8, 0, 1].as_slice());
    Reflect::set(
        &algorithm,
        &JsValue::from_str("publicExponent"),
        public_exponent.as_ref(),
    )
    .map_err(Error::from)?;

    let hash = Object::new();
    Reflect::set(
        &hash,
        &JsValue::from_str("name"),
        &JsValue::from_str("SHA-256"),
    )
    .map_err(Error::from)?;
    Reflect::set(&algorithm, &JsValue::from_str("hash"), &hash).map_err(Error::from)?;

    Ok(algorithm)
}

async fn export_private_key_jwk(subtle: &web_sys::SubtleCrypto, key: &CryptoKey) -> Result<String> {
    let exported = JsFuture::from(subtle.export_key("jwk", key)?).await?;
    js_sys::JSON::stringify(&exported)
        .map_err(Error::from)?
        .as_string()
        .ok_or_else(|| Error::RustError("failed to stringify private JWK".to_owned()))
}

async fn export_public_key_pem(subtle: &web_sys::SubtleCrypto, key: &CryptoKey) -> Result<String> {
    let exported = JsFuture::from(subtle.export_key("spki", key)?).await?;
    let bytes = Uint8Array::new(&exported).to_vec();
    Ok(spki_to_pem(&bytes))
}

fn spki_to_pem(bytes: &[u8]) -> String {
    let encoded = STANDARD.encode(bytes);
    let mut pem = String::from("-----BEGIN PUBLIC KEY-----\n");

    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        pem.push('\n');
    }

    pem.push_str("-----END PUBLIC KEY-----\n");
    pem
}

fn json_response<T>(
    value: &T,
    content_type: &str,
    extra_headers: &[(&str, &str)],
) -> Result<Response>
where
    T: Serialize,
{
    let body = serde_json::to_string(value)
        .map_err(|error| Error::RustError(format!("failed to serialize response: {error}")))?;
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response.headers_mut().set("Content-Type", content_type)?;

    for (name, value) in extra_headers {
        response.headers_mut().set(name, value)?;
    }

    Ok(response)
}

const MAX_IMAGE_UPLOAD_BYTES: usize = 10 * 1024 * 1024;
const MAX_AV_UPLOAD_BYTES: usize = 40 * 1024 * 1024;

async fn parse_media_upload(req: &mut Request) -> std::result::Result<MediaUploadDraft, String> {
    let form = req
        .form_data()
        .await
        .map_err(|error| format!("invalid multipart media payload: {error}"))?;

    let file = match form.get("file") {
        Some(FormEntry::File(file)) => file,
        Some(FormEntry::Field(_)) => {
            return Err("file field must be sent as multipart file data".to_owned());
        }
        None => return Err("file field is required".to_owned()),
    };

    let content_type = file.type_().trim().to_ascii_lowercase();
    if content_type.is_empty() {
        return Err("uploaded file is missing a content type".to_owned());
    }

    let kind = classify_media_kind(&content_type)
        .ok_or_else(|| format!("unsupported media content type: {content_type}"))?;
    let bytes = file
        .bytes()
        .await
        .map_err(|error| format!("failed to read uploaded file: {error}"))?;
    if bytes.is_empty() {
        return Err("uploaded file must not be empty".to_owned());
    }

    let size_limit = max_upload_size(kind);
    if bytes.len() > size_limit {
        return Err(format!(
            "uploaded file exceeds the {} byte limit for {} uploads",
            size_limit,
            media_kind_label(kind)
        ));
    }

    Ok(MediaUploadDraft {
        bytes,
        content_type,
        description: form
            .get_field("description")
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_default(),
        kind,
    })
}

async fn parse_media_update_request(
    req: &mut Request,
) -> std::result::Result<UpdateMediaRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let request = if content_type.contains("application/json") {
        req.json::<UpdateMediaRequest>()
            .await
            .map_err(|error| format!("invalid JSON media update payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form media update payload: {error}"))?;
        UpdateMediaRequest {
            description: form.get_field("description"),
            focus: form.get_field("focus"),
        }
    };

    Ok(request)
}

fn parse_media_focus(focus: Option<&str>) -> std::result::Result<Option<(f64, f64)>, String> {
    let Some(focus) = focus.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some((x, y)) = focus.split_once(',') else {
        return Err("focus must be in the form `x,y`".to_owned());
    };
    let x = x
        .trim()
        .parse::<f64>()
        .map_err(|_| "focus x must be a number".to_owned())?;
    let y = y
        .trim()
        .parse::<f64>()
        .map_err(|_| "focus y must be a number".to_owned())?;
    if !(-1.0..=1.0).contains(&x) || !(-1.0..=1.0).contains(&y) {
        return Err("focus coordinates must be between -1.0 and 1.0".to_owned());
    }
    Ok(Some((x, y)))
}

fn classify_media_kind(content_type: &str) -> Option<MediaKind> {
    if content_type.starts_with("image/") {
        Some(MediaKind::Image)
    } else if content_type.starts_with("video/") {
        Some(MediaKind::Video)
    } else if content_type.starts_with("audio/") {
        Some(MediaKind::Audio)
    } else {
        None
    }
}

const fn max_upload_size(kind: MediaKind) -> usize {
    match kind {
        MediaKind::Image => MAX_IMAGE_UPLOAD_BYTES,
        MediaKind::Video | MediaKind::Audio => MAX_AV_UPLOAD_BYTES,
    }
}

const fn media_kind_label(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => "image",
        MediaKind::Video => "video",
        MediaKind::Audio => "audio",
    }
}

async fn parse_status_draft(req: &mut Request) -> std::result::Result<StatusDraft, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let request = if content_type.contains("application/json") {
        req.json::<CreateStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON status payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form status payload: {error}"))?;

        CreateStatusRequest {
            status: form.get_field("status"),
            media_ids: parse_media_ids_from_form(&form),
            poll: parse_status_poll_from_form(&form)?,
            in_reply_to_id: form.get_field("in_reply_to_id"),
            sensitive: parse_optional_bool(form.get_field("sensitive").as_deref())?,
            spoiler_text: form.get_field("spoiler_text"),
            visibility: form.get_field("visibility"),
            language: form.get_field("language"),
        }
    };

    let text = request.status.unwrap_or_default().trim().to_owned();
    let poll = normalize_status_poll(request.poll)?;
    let media_ids = request
        .media_ids
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if text.is_empty() && media_ids.is_empty() && poll.is_none() {
        return Err("status, media_ids, or poll must be present".to_owned());
    }
    if media_ids.len() > 4 {
        return Err("a maximum of 4 media attachments is supported".to_owned());
    }
    if poll.is_some() && !media_ids.is_empty() {
        return Err("poll cannot be combined with media attachments yet".to_owned());
    }

    let visibility = match request.visibility.as_deref().map(str::trim) {
        Some("") | None => Visibility::Public,
        Some(value) => Visibility::parse(value).ok_or_else(|| {
            "visibility must be one of: public, unlisted, private, direct".to_owned()
        })?,
    };

    Ok(StatusDraft {
        text,
        visibility,
        spoiler_text: request.spoiler_text.unwrap_or_default().trim().to_owned(),
        sensitive: request.sensitive.unwrap_or(false),
        language: request
            .language
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty()),
        in_reply_to_id: request
            .in_reply_to_id
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        media_ids,
        poll,
    })
}

fn parse_status_poll_from_form(
    form: &FormData,
) -> std::result::Result<Option<CreateStatusPollRequest>, String> {
    let options = form.get_all("poll[options][]").map(|entries| {
        entries
            .into_iter()
            .filter_map(|entry| match entry {
                FormEntry::Field(value) => Some(value),
                FormEntry::File(_) => None,
            })
            .collect::<Vec<_>>()
    });
    let expires_in = form
        .get_field("poll[expires_in]")
        .and_then(|value| value.trim().parse::<u64>().ok());
    let multiple = parse_optional_bool(form.get_field("poll[multiple]").as_deref())?;
    let hide_totals = parse_optional_bool(form.get_field("poll[hide_totals]").as_deref())?;

    if options.is_none() && expires_in.is_none() && multiple.is_none() && hide_totals.is_none() {
        Ok(None)
    } else {
        Ok(Some(CreateStatusPollRequest {
            options,
            expires_in,
            multiple,
            hide_totals,
        }))
    }
}

fn normalize_status_poll(
    poll: Option<CreateStatusPollRequest>,
) -> std::result::Result<Option<PollDraft>, String> {
    let Some(poll) = poll else {
        return Ok(None);
    };
    let options = poll
        .options
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if options.len() < 2 || options.len() > 4 {
        return Err("poll must include between 2 and 4 non-empty options".to_owned());
    }
    let expires_in_seconds = poll
        .expires_in
        .filter(|value| *value >= 300)
        .ok_or_else(|| "poll[expires_in] must be at least 300 seconds".to_owned())?;

    Ok(Some(PollDraft {
        options,
        expires_in_seconds,
        multiple: poll.multiple.unwrap_or(false),
        hide_totals: poll.hide_totals.unwrap_or(false),
    }))
}

async fn parse_update_credentials_request(
    req: &mut Request,
) -> std::result::Result<UpdateCredentialsRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<UpdateCredentialsRequest>()
            .await
            .map_err(|error| format!("invalid JSON credentials payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form credentials payload: {error}"))?;
        let mut request = UpdateCredentialsRequest {
            display_name: form.get_field("display_name"),
            note: form.get_field("note"),
            fields_attributes: Some(parse_profile_fields_from_form(&form)),
            discoverable: parse_optional_bool(form.get_field("discoverable").as_deref())?,
            source: Some(UpdateCredentialsSource {
                privacy: form.get_field("source[privacy]"),
                sensitive: parse_optional_bool(form.get_field("source[sensitive]").as_deref())?,
                language: form.get_field("source[language]"),
            }),
            ..UpdateCredentialsRequest::default()
        };
        request.avatar = parse_profile_media_upload(form.get("avatar"), "avatar").await?;
        request.header = parse_profile_media_upload(form.get("header"), "header").await?;
        request
    };

    if let Some(display_name) = request.display_name.as_mut() {
        *display_name = display_name.trim().to_owned();
        if display_name.is_empty() {
            request.display_name = None;
        }
    }

    if let Some(note) = request.note.as_mut() {
        *note = note.trim().to_owned();
    }

    if let Some(fields) = request.fields_attributes.as_mut() {
        *fields = normalize_profile_fields(std::mem::take(fields));
        if fields.is_empty() {
            request.fields_attributes = None;
        }
    }

    if let Some(source) = request.source.as_mut() {
        if let Some(privacy) = source.privacy.as_mut() {
            *privacy = privacy.trim().to_ascii_lowercase();
            if privacy.is_empty() {
                source.privacy = None;
            } else if Visibility::parse(privacy).is_none() {
                return Err(
                    "source[privacy] must be one of: public, unlisted, private, direct".to_owned(),
                );
            }
        }

        if let Some(language) = source.language.as_mut() {
            *language = language.trim().to_ascii_lowercase();
            if language.is_empty() {
                source.language = None;
            }
        }
    }

    Ok(request)
}

fn parse_profile_fields_from_form(form: &FormData) -> Vec<UpdateCredentialsField> {
    (0..8)
        .filter_map(|index| {
            let name = form.get_field(&format!("fields_attributes[{index}][name]"));
            let value = form.get_field(&format!("fields_attributes[{index}][value]"));
            if name.is_none() && value.is_none() {
                None
            } else {
                Some(UpdateCredentialsField { name, value })
            }
        })
        .collect()
}

fn normalize_profile_fields(fields: Vec<UpdateCredentialsField>) -> Vec<UpdateCredentialsField> {
    fields
        .into_iter()
        .filter_map(|mut field| {
            if let Some(name) = field.name.as_mut() {
                *name = name.trim().to_owned();
            }
            if let Some(value) = field.value.as_mut() {
                *value = value.trim().to_owned();
            }
            let name = field.name.filter(|value| !value.is_empty());
            let value = field.value.filter(|value| !value.is_empty());
            match (name, value) {
                (Some(name), Some(value)) => Some(UpdateCredentialsField {
                    name: Some(name),
                    value: Some(value),
                }),
                _ => None,
            }
        })
        .take(4)
        .collect()
}

async fn parse_profile_media_upload(
    entry: Option<FormEntry>,
    object_kind: &'static str,
) -> std::result::Result<Option<ProfileMediaUpload>, String> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let file = match entry {
        FormEntry::File(file) => file,
        FormEntry::Field(_) => {
            return Err(format!("{object_kind} must be sent as multipart file data"));
        }
    };
    let content_type = file.type_().trim().to_ascii_lowercase();
    if content_type.is_empty() {
        return Err(format!("{object_kind} is missing a content type"));
    }
    let kind = classify_media_kind(&content_type)
        .ok_or_else(|| format!("unsupported {object_kind} content type: {content_type}"))?;
    if kind != MediaKind::Image {
        return Err(format!("{object_kind} must be an image"));
    }
    let bytes = file
        .bytes()
        .await
        .map_err(|error| format!("failed to read {object_kind} upload: {error}"))?;
    if bytes.is_empty() {
        return Err(format!("{object_kind} must not be empty"));
    }
    if bytes.len() > MAX_IMAGE_UPLOAD_BYTES {
        return Err(format!(
            "{object_kind} exceeds the {} byte image limit",
            MAX_IMAGE_UPLOAD_BYTES
        ));
    }

    Ok(Some(ProfileMediaUpload {
        bytes,
        content_type,
        object_kind,
    }))
}

async fn parse_reblog_status_request(
    req: &mut Request,
) -> std::result::Result<ReblogStatusRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.trim().is_empty() {
        ReblogStatusRequest::default()
    } else if content_type.contains("application/json") {
        req.json::<ReblogStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON reblog payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form reblog payload: {error}"))?;
        ReblogStatusRequest {
            visibility: form.get_field("visibility"),
        }
    };

    if let Some(visibility) = request.visibility.as_mut() {
        *visibility = visibility.trim().to_ascii_lowercase();
        if visibility.is_empty() {
            request.visibility = None;
        } else if Visibility::parse(visibility).is_none() {
            return Err("visibility must be one of: public, unlisted, private, direct".to_owned());
        }
    }

    Ok(request)
}

async fn parse_mute_account_request(
    req: &mut Request,
) -> std::result::Result<MuteAccountRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.trim().is_empty() {
        return Ok(MuteAccountRequest::default());
    }

    if content_type.contains("application/json") {
        return req
            .json::<MuteAccountRequest>()
            .await
            .map_err(|error| format!("invalid JSON mute payload: {error}"));
    }

    let form = req
        .form_data()
        .await
        .map_err(|error| format!("invalid form mute payload: {error}"))?;
    Ok(MuteAccountRequest {
        notifications: parse_optional_bool(form.get_field("notifications").as_deref())?,
        duration: form
            .get_field("duration")
            .and_then(|value| value.trim().parse::<u32>().ok()),
    })
}

async fn parse_follow_account_request(
    req: &mut Request,
) -> std::result::Result<FollowAccountRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.trim().is_empty() {
        return Ok(FollowAccountRequest::default());
    }

    let mut request = if content_type.contains("application/json") {
        req.json::<FollowAccountRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON follow payload: {error}")))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| Error::RustError(format!("invalid form follow payload: {error}")))?;
        FollowAccountRequest {
            reblogs: parse_optional_bool(form.get_field("reblogs").as_deref())
                .map_err(Error::RustError)?,
            notify: parse_optional_bool(form.get_field("notify").as_deref())
                .map_err(Error::RustError)?,
            languages: form.get_all("languages[]").map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|entry| match entry {
                        FormEntry::Field(value) => {
                            let value = value.trim().to_ascii_lowercase();
                            (!value.is_empty()).then_some(value)
                        }
                        FormEntry::File(_) => None,
                    })
                    .collect()
            }),
        }
    };

    if let Some(languages) = request.languages.as_mut() {
        languages.sort();
        languages.dedup();
        if languages.is_empty() {
            request.languages = None;
        }
    }

    Ok(request)
}

fn parse_media_ids_from_form(form: &FormData) -> Option<Vec<String>> {
    form.get_all("media_ids[]").map(|entries| {
        entries
            .into_iter()
            .filter_map(|entry| match entry {
                FormEntry::Field(value) => Some(value),
                FormEntry::File(_) => None,
            })
            .collect()
    })
}

fn parse_relationship_query_ids(req: &Request) -> Result<Vec<String>> {
    let url = req.url()?;
    let mut ids = Vec::new();

    for (key, value) in url.query_pairs() {
        if key == "id[]" || key == "id" {
            let value = value.trim().to_owned();
            if !value.is_empty() {
                ids.push(value);
            }
        }
    }

    Ok(ids)
}

fn parse_internal_pagination_id(value: Option<&str>, field: &str) -> Result<Option<i64>> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => value
            .parse::<i64>()
            .map(Some)
            .map_err(|_| Error::RustError(format!("{field} must be an integer cursor id"))),
    }
}

fn build_internal_cursor_link_header(
    req: &Request,
    limit: u32,
    first_cursor: Option<i64>,
    last_cursor: Option<i64>,
    has_next: bool,
    has_prev: bool,
) -> Result<Option<String>> {
    let mut links = Vec::new();

    if has_next && let Some(cursor) = last_cursor {
        links.push(build_internal_cursor_link(
            req,
            limit,
            Some(cursor),
            None,
            "next",
        )?);
    }

    if has_prev && let Some(cursor) = first_cursor {
        links.push(build_internal_cursor_link(
            req,
            limit,
            None,
            Some(cursor),
            "prev",
        )?);
    }

    if links.is_empty() {
        return Ok(None);
    }

    Ok(Some(links.join(", ")))
}

fn build_internal_cursor_link(
    req: &Request,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    rel: &str,
) -> Result<String> {
    build_internal_cursor_link_for_url(&req.url()?, limit, max_id, since_id, rel)
}

fn build_internal_cursor_link_for_url(
    url: &Url,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
    rel: &str,
) -> Result<String> {
    let mut url = url.clone();
    let pairs = url
        .query_pairs()
        .filter(|(key, _)| {
            key != "max_id" && key != "since_id" && key != "min_id" && key != "limit"
        })
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (key, value) in pairs {
            query.append_pair(&key, &value);
        }
        query.append_pair("limit", &limit.to_string());
        if let Some(value) = max_id {
            query.append_pair("max_id", &value.to_string());
        }
        if let Some(value) = since_id {
            query.append_pair("since_id", &value.to_string());
        }
    }

    Ok(format!("<{}>; rel=\"{}\"", url, rel))
}

fn parse_optional_bool(value: Option<&str>) -> std::result::Result<Option<bool>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    match value.to_ascii_lowercase().as_str() {
        "true" | "1" | "on" => Ok(Some(true)),
        "false" | "0" | "off" => Ok(Some(false)),
        _ => Err(format!("invalid boolean value: {value}")),
    }
}

fn search_category_flags(search_type: Option<&str>) -> SearchCategoryFlags {
    match search_type.map(str::trim).filter(|value| !value.is_empty()) {
        None => SearchCategoryFlags {
            accounts: true,
            statuses: true,
            hashtags: true,
        },
        Some("accounts") => SearchCategoryFlags {
            accounts: true,
            statuses: false,
            hashtags: false,
        },
        Some("statuses") => SearchCategoryFlags {
            accounts: false,
            statuses: true,
            hashtags: false,
        },
        Some("hashtags") => SearchCategoryFlags {
            accounts: false,
            statuses: false,
            hashtags: true,
        },
        Some(_) => SearchCategoryFlags::default(),
    }
}

fn search_v2_requires_auth(query: &SearchV2Query) -> bool {
    query.resolve.unwrap_or(false)
        || query.following.unwrap_or(false)
        || query.offset.unwrap_or(0) > 0
}

fn search_v2_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(20).clamp(1, 40)
}

fn search_text_match_rank(query: &str, candidate: &str) -> u8 {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return 3;
    }

    let candidate = candidate.trim().to_ascii_lowercase();
    if candidate == query {
        0
    } else if candidate.starts_with(&query) {
        1
    } else if candidate.contains(&query) {
        2
    } else {
        3
    }
}

fn account_search_rank(
    query: &str,
    username: &str,
    acct: &str,
    display_name: &str,
) -> (u8, u8, String) {
    let candidates = [
        (search_text_match_rank(query, username), 0u8),
        (search_text_match_rank(query, acct), 1u8),
        (search_text_match_rank(query, display_name), 2u8),
    ];
    let (match_rank, field_rank) = candidates.into_iter().min().unwrap_or((3, 3));
    (match_rank, field_rank, acct.to_ascii_lowercase())
}

fn tag_search_rank(query: &str, tag: &str) -> (u8, String) {
    (search_text_match_rank(query, tag), normalize_hashtag(tag))
}

fn normalize_hashtag(value: &str) -> String {
    value.trim().trim_start_matches('#').to_ascii_lowercase()
}

fn include_local_source(local: Option<bool>, remote: Option<bool>) -> bool {
    local.unwrap_or(false) || !remote.unwrap_or(false)
}

fn include_remote_source(local: Option<bool>, remote: Option<bool>) -> bool {
    remote.unwrap_or(false) || !local.unwrap_or(false)
}

fn status_id_from_context(ctx: &RouteContext<()>) -> Result<String> {
    ctx.param("id")
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("missing status id route parameter".to_owned()))
}

fn status_is_visible_to_requester(
    status: &StatusRow,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> bool {
    is_public_activitypub_visibility(&status.visibility)
        || viewer.map(|viewer| viewer.id == owner.id).unwrap_or(false)
}

async fn can_view_local_status(
    db: &D1Database,
    status: &StatusRow,
    viewer: Option<&LocalAccount>,
    owner: &LocalAccount,
) -> Result<bool> {
    if status_is_visible_to_requester(status, viewer, owner) {
        return Ok(true);
    }
    if status.visibility != "private" {
        return Ok(false);
    }

    let Some(viewer) = viewer else {
        return Ok(false);
    };
    is_local_follower_authorized(db, &viewer.id, &owner.id).await
}

fn status_contains_tag(status: &StatusRow, tag: &str) -> bool {
    let normalized_tag = tag.trim().trim_start_matches('#').to_ascii_lowercase();
    if normalized_tag.is_empty() {
        return true;
    }

    let needle = format!("#{normalized_tag}");
    status._text_content.to_ascii_lowercase().contains(&needle)
}

fn extract_hashtags_from_text(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut seen = HashSet::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] != '#' {
            index += 1;
            continue;
        }
        if index > 0 {
            let previous = chars[index - 1];
            if previous.is_ascii_alphanumeric() || previous == '_' {
                index += 1;
                continue;
            }
        }

        let mut end = index + 1;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        if end == index + 1 {
            index += 1;
            continue;
        }

        let tag = chars[index + 1..end]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        if seen.insert(tag.clone()) {
            tags.push(tag);
        }
        index = end;
    }

    tags
}

fn extract_mentions_from_text(text: &str, config: &AppConfig) -> Vec<AccountHandle> {
    extract_account_handles_from_text(text, config)
        .into_iter()
        .filter(|handle| handle.is_local_to(&config.instance_domain))
        .collect()
}

fn extract_account_handles_from_text(text: &str, config: &AppConfig) -> Vec<AccountHandle> {
    let mut mentions = Vec::new();
    let mut seen = HashSet::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;

    while index < chars.len() {
        if chars[index] != '@' {
            index += 1;
            continue;
        }
        if index > 0 {
            let previous = chars[index - 1];
            if previous.is_ascii_alphanumeric() || previous == '_' {
                index += 1;
                continue;
            }
        }

        let mut end = index + 1;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        if end == index + 1 {
            index += 1;
            continue;
        }

        let username = chars[index + 1..end]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        let mut domain = None;
        let mut next_index = end;
        if next_index < chars.len() && chars[next_index] == '@' {
            let mut domain_end = next_index + 1;
            while domain_end < chars.len()
                && (chars[domain_end].is_ascii_alphanumeric()
                    || chars[domain_end] == '-'
                    || chars[domain_end] == '.')
            {
                domain_end += 1;
            }
            if domain_end > next_index + 1 {
                domain = Some(
                    chars[next_index + 1..domain_end]
                        .iter()
                        .collect::<String>()
                        .to_ascii_lowercase(),
                );
                next_index = domain_end;
            }
        }

        let handle = AccountHandle {
            username,
            domain: domain.or_else(|| Some(instance_host(config))),
        };
        let key = format!(
            "{}@{}",
            handle.username,
            handle
                .domain
                .clone()
                .unwrap_or_else(|| instance_host(config))
        );
        if seen.insert(key) {
            mentions.push(handle);
        }
        index = next_index;
    }

    mentions
}

fn strip_html_tags(html: &str) -> String {
    let mut output = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn extract_hashtags_from_html(html: &str) -> Vec<String> {
    extract_hashtags_from_text(&strip_html_tags(html))
}

fn matches_tag_timeline_filters(
    tags: &[String],
    primary_tag: &str,
    query: &TagTimelineQuery,
) -> bool {
    let tag_set = tags.iter().map(|tag| tag.as_str()).collect::<HashSet<_>>();
    if !tag_set.contains(primary_tag) {
        return false;
    }

    if let Some(any_tags) = query.any.as_ref() {
        let normalized = any_tags
            .iter()
            .map(|tag| normalize_hashtag(tag))
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        if !normalized.is_empty() && !normalized.iter().any(|tag| tag_set.contains(tag.as_str())) {
            return false;
        }
    }

    if let Some(all_tags) = query.all.as_ref()
        && !all_tags
            .iter()
            .map(|tag| normalize_hashtag(tag))
            .filter(|tag| !tag.is_empty())
            .all(|tag| tag_set.contains(tag.as_str()))
    {
        return false;
    }

    if let Some(none_tags) = query.none.as_ref()
        && none_tags
            .iter()
            .map(|tag| normalize_hashtag(tag))
            .filter(|tag| !tag.is_empty())
            .any(|tag| tag_set.contains(tag.as_str()))
    {
        return false;
    }

    true
}

fn tag_rest_id(name: &str) -> String {
    let checksum = name.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(16777619).wrapping_add(byte as u32)
    });
    checksum.to_string()
}

fn tag_url(config: &AppConfig, name: &str) -> String {
    format!("{}/tags/{}", instance_base_url(config), name)
}

fn tag_history_stub() -> Vec<MastodonTagHistoryEntry> {
    Vec::new()
}

fn build_ordered_collection_document(
    collection_id: &str,
    ordered_items: &[String],
    query: &CollectionPagingQuery,
) -> serde_json::Value {
    let total_items = ordered_items.len();
    let limit = query.limit.unwrap_or(50).clamp(1, 80) as usize;
    let offset = query.offset.unwrap_or(0) as usize;

    if query.page.unwrap_or(false) || query.offset.unwrap_or(0) > 0 {
        let page_items = ordered_items
            .iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_offset = offset.saturating_add(page_items.len());
        let next = if next_offset < total_items {
            Some(format!(
                "{collection_id}?page=true&offset={next_offset}&limit={limit}"
            ))
        } else {
            None
        };

        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollectionPage",
            "id": format!("{collection_id}?page=true&offset={offset}&limit={limit}"),
            "partOf": collection_id,
            "next": next,
            "orderedItems": page_items,
        })
    } else {
        serde_json::json!({
            "@context": "https://www.w3.org/ns/activitystreams",
            "type": "OrderedCollection",
            "id": collection_id,
            "totalItems": total_items,
            "first": format!("{collection_id}?page=true&offset=0&limit={limit}"),
        })
    }
}

async fn store_media_attachment(
    db: &D1Database,
    bucket: &Bucket,
    account: &LocalAccount,
    draft: &MediaUploadDraft,
) -> Result<MediaAttachmentRow> {
    let media_id = generate_entity_id(16)?;
    let object_key = format!(
        "media/{}/{}/{}",
        account.id,
        media_kind_label(draft.kind),
        media_id
    );

    let upload = bucket
        .put(&object_key, draft.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(draft.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await?;
    if upload.is_none() {
        return Err(Error::RustError(
            "failed to persist media object to R2".to_owned(),
        ));
    }

    let bindings = [
        D1Type::Text(media_id.as_str()),
        D1Type::Text(account.id.as_str()),
        D1Type::Text(object_key.as_str()),
        D1Type::Text(draft.content_type.as_str()),
        D1Type::Text(draft.description.as_str()),
    ];

    let insert_result = db
        .prepare(
            "INSERT INTO media_attachments (
                id,
                account_id,
                status_id,
                object_key,
                content_type,
                description,
                created_at
            ) VALUES (
                ?1,
                ?2,
                NULL,
                ?3,
                ?4,
                ?5,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await;

    if let Err(error) = insert_result {
        let _ = bucket.delete(&object_key).await;
        return Err(error);
    }

    require_media_attachment_by_id(db, &media_id).await
}

async fn require_media_attachment_by_id(
    db: &D1Database,
    media_id: &str,
) -> Result<MediaAttachmentRow> {
    find_media_attachment_by_id(db, media_id)
        .await?
        .ok_or_else(|| Error::RustError("media attachment not found".to_owned()))
}

async fn find_media_attachment_by_id(
    db: &D1Database,
    media_id: &str,
) -> Result<Option<MediaAttachmentRow>> {
    let media_id = D1Type::Text(media_id);
    db.prepare(
        "SELECT id, account_id, status_id, object_key, content_type, description, focus_x, focus_y, created_at
         FROM media_attachments
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&media_id)?
    .first::<MediaAttachmentRow>(None)
    .await
}

async fn apply_media_update(
    db: &D1Database,
    media: &MediaAttachmentRow,
    update: UpdateMediaRequest,
) -> Result<MediaAttachmentRow> {
    let description = update
        .description
        .as_deref()
        .map(str::trim)
        .unwrap_or(&media.description)
        .to_owned();
    let focus = parse_media_focus(update.focus.as_deref()).map_err(Error::RustError)?;
    let focus_x = focus.map(|(x, _)| x).or(media.focus_x);
    let focus_y = focus.map(|(_, y)| y).or(media.focus_y);

    let bindings = [
        D1Type::Text(description.as_str()),
        match focus_x {
            Some(value) => D1Type::Real(value),
            None => D1Type::Null,
        },
        match focus_y {
            Some(value) => D1Type::Real(value),
            None => D1Type::Null,
        },
        D1Type::Text(media.id.as_str()),
    ];
    db.prepare(
        "UPDATE media_attachments
         SET description = ?1,
             focus_x = ?2,
             focus_y = ?3
         WHERE id = ?4",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    require_media_attachment_by_id(db, &media.id).await
}

async fn find_media_attachments_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Vec<MediaAttachmentRow>> {
    let status_id = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT id, account_id, status_id, object_key, content_type, description, focus_x, focus_y, created_at
             FROM media_attachments
             WHERE status_id = ?1
             ORDER BY created_at ASC",
        )
        .bind_refs(&status_id)?
        .all()
        .await?;

    result.results::<MediaAttachmentRow>()
}

async fn list_orphan_media(
    db: &D1Database,
    older_than_hours: u32,
    limit: u32,
) -> Result<Vec<OrphanMediaRow>> {
    let older_than_modifier = format!("-{} hours", older_than_hours);
    let older_than = D1Type::Text(older_than_modifier.as_str());
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, object_key
             FROM media_attachments
             WHERE status_id IS NULL
               AND created_at <= datetime(CURRENT_TIMESTAMP, ?1)
             ORDER BY created_at ASC
             LIMIT ?2",
        )
        .bind_refs(&[older_than, limit])?
        .all()
        .await?;

    result.results::<OrphanMediaRow>()
}

async fn delete_media_attachments(
    db: &D1Database,
    bucket: &Bucket,
    attachments: &[MediaAttachmentRow],
) -> Result<()> {
    for attachment in attachments {
        bucket.delete(&attachment.object_key).await?;
        delete_media_attachment_row(db, &attachment.id).await?;
    }

    Ok(())
}

async fn delete_orphan_media(
    db: &D1Database,
    bucket: &Bucket,
    orphans: &[OrphanMediaRow],
) -> Result<u32> {
    let mut deleted = 0;

    for orphan in orphans {
        bucket.delete(&orphan.object_key).await?;
        delete_media_attachment_row(db, &orphan.id).await?;
        deleted += 1;
    }

    Ok(deleted)
}

async fn delete_media_attachment_row(db: &D1Database, media_id: &str) -> Result<()> {
    let media_id = D1Type::Text(media_id);
    db.prepare(
        "DELETE FROM media_attachments
         WHERE id = ?1",
    )
    .bind_refs(&media_id)?
    .run()
    .await?;

    Ok(())
}

async fn resolve_attachable_media(
    db: &D1Database,
    account: &LocalAccount,
    media_ids: &[String],
) -> std::result::Result<Vec<MediaAttachmentRow>, String> {
    let mut media = Vec::with_capacity(media_ids.len());

    for media_id in media_ids {
        let row = find_media_attachment_by_id(db, media_id)
            .await
            .map_err(|error| format!("failed to load media attachment {media_id}: {error}"))?
            .ok_or_else(|| format!("media attachment {media_id} was not found"))?;

        if row.account_id != account.id {
            return Err(format!(
                "media attachment {media_id} does not belong to the authenticated account"
            ));
        }
        if row.status_id.is_some() {
            return Err(format!("media attachment {media_id} is already attached"));
        }

        media.push(row);
    }

    Ok(media)
}

async fn attach_media_to_status(
    db: &D1Database,
    status_id: &str,
    media: &[MediaAttachmentRow],
) -> Result<()> {
    for attachment in media {
        let bindings = [
            D1Type::Text(status_id),
            D1Type::Text(attachment.id.as_str()),
        ];
        db.prepare(
            "UPDATE media_attachments
             SET status_id = ?1
             WHERE id = ?2 AND status_id IS NULL",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn insert_status(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    draft: &StatusDraft,
) -> Result<StatusRow> {
    let status_id = generate_entity_id(16)?;
    let ap_id = format!(
        "{}/statuses/{}",
        actor_url(config, &account.username),
        status_id
    );
    let content_html = render_status_html(&draft.text);
    let created_at = now_iso_string()?;

    let id = D1Type::Text(status_id.as_str());
    let account_id = D1Type::Text(account.id.as_str());
    let ap_id_binding = D1Type::Text(ap_id.as_str());
    let in_reply_to_id = match draft.in_reply_to_id.as_deref() {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    };
    let content_html_binding = D1Type::Text(content_html.as_str());
    let text_content = D1Type::Text(draft.text.as_str());
    let visibility = D1Type::Text(draft.visibility.as_str());
    let sensitive = D1Type::Integer(if draft.sensitive { 1 } else { 0 });
    let created_at_binding = D1Type::Text(created_at.as_str());
    let spoiler_text = D1Type::Text(draft.spoiler_text.as_str());
    let language = match draft.language.as_deref() {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    };

    let bindings = [
        id,
        account_id,
        ap_id_binding,
        in_reply_to_id,
        content_html_binding,
        text_content,
        spoiler_text,
        visibility,
        sensitive,
        language,
        created_at_binding,
    ];

    db.prepare(
        "INSERT INTO statuses (
            id,
            account_id,
            ap_id,
            in_reply_to_id,
            content_html,
            text_content,
            spoiler_text,
            visibility,
            sensitive,
            language,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?7,
            ?8,
            ?9,
            ?10,
            ?11,
            ?11
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    if let Some(poll) = draft.poll.as_ref() {
        insert_status_poll(db, &status_id, poll, &created_at).await?;
    }

    require_status_by_id(db, &status_id).await
}

async fn insert_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &PollDraft,
    created_at: &str,
) -> Result<()> {
    let poll_id = generate_entity_id(16)?;
    let expires_at = add_seconds_to_iso_string(created_at, poll.expires_in_seconds)?;
    let bindings = [
        D1Type::Text(poll_id.as_str()),
        D1Type::Text(status_id),
        D1Type::Integer(if poll.multiple { 1 } else { 0 }),
        D1Type::Integer(if poll.hide_totals { 1 } else { 0 }),
        D1Type::Text(expires_at.as_str()),
        D1Type::Text(created_at),
    ];
    db.prepare(
        "INSERT INTO status_polls (
            id,
            status_id,
            multiple,
            hide_totals,
            expires_at,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            ?6
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    for (position, option) in poll.options.iter().enumerate() {
        let option_id = generate_entity_id(16)?;
        let bindings = [
            D1Type::Text(option_id.as_str()),
            D1Type::Text(poll_id.as_str()),
            D1Type::Text(option.as_str()),
            D1Type::Integer(position as i32),
        ];
        db.prepare(
            "INSERT INTO status_poll_options (
                id,
                poll_id,
                title,
                position,
                votes_count,
                created_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                0,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn require_status_by_id(db: &D1Database, status_id: &str) -> Result<StatusRow> {
    find_status_by_id(db, status_id)
        .await?
        .ok_or_else(|| Error::RustError("status not found".to_owned()))
}

async fn find_status_poll_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<StatusPollRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, status_id, multiple, hide_totals, expires_at
         FROM status_polls
         WHERE status_id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<StatusPollRow>(None)
    .await
}

async fn find_status_poll_by_id(db: &D1Database, poll_id: &str) -> Result<Option<StatusPollRow>> {
    let poll_id = D1Type::Text(poll_id);
    db.prepare(
        "SELECT id, status_id, multiple, hide_totals, expires_at
         FROM status_polls
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&poll_id)?
    .first::<StatusPollRow>(None)
    .await
}

async fn find_report_by_id(db: &D1Database, report_id: &str) -> Result<Option<ReportRow>> {
    let report_id = D1Type::Text(report_id);
    db.prepare(
        "SELECT id, account_id, target_account_id, target_remote_actor_uri, comment, category, forward, created_at
         FROM reports
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&report_id)?
    .first::<ReportRow>(None)
    .await
}

async fn list_report_status_ids(db: &D1Database, report_id: &str) -> Result<Vec<String>> {
    let bindings = [D1Type::Text(report_id)];
    let result = db
        .prepare(
            "SELECT status_id
             FROM report_statuses
             WHERE report_id = ?1
             ORDER BY status_id ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<serde_json::Value>()?
        .into_iter()
        .filter_map(|value| {
            value
                .get("status_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect())
}

async fn list_reports(db: &D1Database, limit: u32) -> Result<Vec<ReportRow>> {
    let bindings = [D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT id, account_id, target_account_id, target_remote_actor_uri, comment, category, forward, created_at
             FROM reports
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<ReportRow>()
}

async fn list_admin_report_notifications(db: &D1Database, limit: u32) -> Result<Vec<ReportRow>> {
    list_reports(db, limit).await
}

async fn find_remote_status_poll_by_id(
    db: &D1Database,
    poll_id: &str,
) -> Result<Option<RemoteStatusPollRow>> {
    let poll_id = D1Type::Text(poll_id);
    db.prepare(
        "SELECT id, status_id, multiple, expires_at, voters_count, votes_count, expired
         FROM remote_status_polls
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&poll_id)?
    .first::<RemoteStatusPollRow>(None)
    .await
}

async fn find_remote_status_poll_by_status_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<RemoteStatusPollRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, status_id, multiple, expires_at, voters_count, votes_count, expired
         FROM remote_status_polls
         WHERE status_id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<RemoteStatusPollRow>(None)
    .await
}

async fn list_remote_status_poll_options(
    db: &D1Database,
    poll_id: &str,
) -> Result<Vec<RemoteStatusPollOptionRow>> {
    let bindings = [D1Type::Text(poll_id)];
    let result = db
        .prepare(
            "SELECT title, votes_count
             FROM remote_status_poll_options
             WHERE poll_id = ?1
             ORDER BY position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusPollOptionRow>()
}

fn resolve_remote_poll_vote_position(
    options: &[RemoteStatusPollOptionRow],
    option_position: i64,
    option_title: Option<&str>,
) -> Option<u32> {
    let stored_position = u32::try_from(option_position).ok();
    if let Some(title) = option_title {
        if let Some(position) = stored_position
            .filter(|position| (*position as usize) < options.len())
            .filter(|position| options[*position as usize].title == title)
        {
            return Some(position);
        }

        if let Some(position) = options
            .iter()
            .position(|option| option.title == title)
            .and_then(|position| u32::try_from(position).ok())
        {
            return Some(position);
        }
    }

    stored_position.filter(|position| (*position as usize) < options.len())
}

fn remap_remote_poll_vote_positions(
    options: &[RemoteStatusPollOptionRow],
    votes: &[RemoteStatusPollVoteRow],
) -> Vec<u32> {
    votes
        .iter()
        .filter_map(|vote| {
            resolve_remote_poll_vote_position(
                options,
                vote.option_position,
                vote.option_title.as_deref(),
            )
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn list_remote_poll_votes_for_account(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
) -> Result<Vec<RemoteStatusPollVoteRow>> {
    let bindings = [D1Type::Text(poll_id), D1Type::Text(account_id)];
    let result = db
        .prepare(
            "SELECT option_position, option_title
             FROM remote_status_poll_votes
             WHERE poll_id = ?1
               AND account_id = ?2
             ORDER BY option_position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusPollVoteRow>()
}

async fn list_remote_poll_votes_by_poll(
    db: &D1Database,
    poll_id: &str,
) -> Result<Vec<RemoteStatusPollVoteWithIdRow>> {
    let bindings = [D1Type::Text(poll_id)];
    let result = db
        .prepare(
            "SELECT id, option_position, option_title
             FROM remote_status_poll_votes
             WHERE poll_id = ?1",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusPollVoteWithIdRow>()
}

async fn prune_remote_poll_vote_rows(
    db: &D1Database,
    poll_id: &str,
    options: &[RemoteStatusPollOptionRow],
) -> Result<()> {
    for vote in list_remote_poll_votes_by_poll(db, poll_id).await? {
        if resolve_remote_poll_vote_position(
            options,
            vote.option_position,
            vote.option_title.as_deref(),
        )
        .is_some()
        {
            continue;
        }

        let bindings = [D1Type::Text(vote.id.as_str())];
        db.prepare(
            "DELETE FROM remote_status_poll_votes
             WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn load_remote_mastodon_poll_response(
    db: &D1Database,
    status: &RemoteStatusRow,
    viewer: Option<&LocalAccount>,
) -> Result<Option<serde_json::Value>> {
    let Some(poll) = find_remote_status_poll_by_status_id(db, &status.id).await? else {
        return Ok(None);
    };

    Ok(build_remote_mastodon_poll_response(db, &poll, viewer)
        .await?
        .map(|value| serde_json::to_value(value).unwrap_or(serde_json::Value::Null)))
}

async fn build_remote_mastodon_poll_response(
    db: &D1Database,
    poll: &RemoteStatusPollRow,
    viewer: Option<&LocalAccount>,
) -> Result<Option<MastodonPollResponse>> {
    let options = list_remote_status_poll_options(db, &poll.id).await?;
    if options.is_empty() {
        return Ok(None);
    }
    let own_votes = match viewer {
        Some(viewer) => remap_remote_poll_vote_positions(
            &options,
            &list_remote_poll_votes_for_account(db, &poll.id, &viewer.id).await?,
        ),
        None => Vec::new(),
    };

    Ok(Some(MastodonPollResponse {
        id: poll.id.clone(),
        expires_at: poll.expires_at.clone().unwrap_or_default(),
        expired: poll.expired != 0
            || poll
                .expires_at
                .as_deref()
                .map(|value| is_iso_timestamp_in_past(value).unwrap_or(false))
                .unwrap_or(false),
        multiple: poll.multiple != 0,
        votes_count: poll.votes_count.max(0) as u64,
        voters_count: if poll.multiple != 0 {
            poll.voters_count.map(|value| value.max(0) as u64)
        } else {
            None
        },
        voted: !own_votes.is_empty(),
        own_votes,
        options: options
            .into_iter()
            .map(|option| MastodonPollOptionResponse {
                title: option.title,
                votes_count: Some(option.votes_count.max(0) as u64),
            })
            .collect(),
        emojis: Vec::new(),
    }))
}

async fn list_status_poll_options(
    db: &D1Database,
    poll_id: &str,
) -> Result<Vec<StatusPollOptionRow>> {
    let bindings = [D1Type::Text(poll_id)];
    let result = db
        .prepare(
            "SELECT title, votes_count
             FROM status_poll_options
             WHERE poll_id = ?1
             ORDER BY position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusPollOptionRow>()
}

async fn list_poll_vote_positions_for_account(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
) -> Result<Vec<u32>> {
    let bindings = [D1Type::Text(poll_id), D1Type::Text(account_id)];
    let result = db
        .prepare(
            "SELECT option_position
             FROM status_poll_votes
             WHERE poll_id = ?1
               AND account_id = ?2
             ORDER BY option_position ASC",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<serde_json::Value>()?
        .into_iter()
        .filter_map(|value| {
            value
                .get("option_position")
                .and_then(serde_json::Value::as_u64)
        })
        .filter_map(|value| u32::try_from(value).ok())
        .collect())
}

async fn find_status_poll_vote_by_activity_uri(
    db: &D1Database,
    activity_uri: &str,
) -> Result<Option<PollVoteTargetRow>> {
    let bindings = [D1Type::Text(activity_uri)];
    db.prepare(
        "SELECT v.poll_id,
                p.status_id,
                s.account_id AS status_account_id,
                v.option_position
         FROM status_poll_votes v
         JOIN status_polls p
           ON p.id = v.poll_id
         JOIN statuses s
           ON s.id = p.status_id
         WHERE v.activity_uri = ?1
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<PollVoteTargetRow>(None)
    .await
}

async fn find_status_poll_vote_for_remote_actor_by_activity_uri(
    db: &D1Database,
    account_id: &str,
    activity_uri: &str,
) -> Result<Option<PollVoteTargetRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(activity_uri)];
    db.prepare(
        "SELECT v.poll_id,
                p.status_id,
                s.account_id AS status_account_id,
                v.option_position
         FROM status_poll_votes v
         JOIN status_polls p
           ON p.id = v.poll_id
         JOIN statuses s
           ON s.id = p.status_id
         WHERE v.account_id = ?1
           AND v.activity_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<PollVoteTargetRow>(None)
    .await
}

async fn find_status_poll_vote_id_by_position(
    db: &D1Database,
    poll_id: &str,
    account_id: &str,
    option_position: u32,
) -> Result<Option<PollVoteIdRow>> {
    let bindings = [
        D1Type::Text(poll_id),
        D1Type::Text(account_id),
        D1Type::Integer(option_position as i32),
    ];
    db.prepare(
        "SELECT id
         FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2
           AND option_position = ?3
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<PollVoteIdRow>(None)
    .await
}

async fn count_poll_voters(db: &D1Database, poll_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(DISTINCT account_id) AS count
         FROM status_poll_votes
         WHERE poll_id = ?1",
        poll_id,
    )
    .await
}

async fn parse_poll_vote_request(req: &mut Request) -> std::result::Result<Vec<u32>, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let choices = if content_type.contains("application/json") {
        req.json::<PollVoteRequest>()
            .await
            .map_err(|error| format!("invalid JSON poll vote payload: {error}"))?
            .choices
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form poll vote payload: {error}"))?;
        form.get_all("choices[]")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| match entry {
                FormEntry::Field(value) => value.trim().parse::<u32>().ok(),
                FormEntry::File(_) => None,
            })
            .collect()
    };

    if choices.is_empty() {
        return Err("choices must not be empty".to_owned());
    }
    let unique = choices.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != choices.len() {
        return Err("duplicate poll choices are not allowed".to_owned());
    }

    Ok(choices)
}

async fn apply_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    choices: &[u32],
) -> Result<()> {
    let options = list_status_poll_options(db, &poll.id).await?;
    let max_index = options.len();
    if choices.iter().any(|choice| (*choice as usize) >= max_index) {
        return Err(Error::RustError(
            "poll choice index is out of range".to_owned(),
        ));
    }
    if poll.multiple == 0 && choices.len() != 1 {
        return Err(Error::RustError(
            "poll does not allow multiple choices".to_owned(),
        ));
    }

    let existing = list_poll_vote_positions_for_account(db, &poll.id, account_id).await?;
    for choice in existing {
        let bindings = [
            D1Type::Text(poll.id.as_str()),
            D1Type::Integer(choice as i32),
        ];
        db.prepare(
            "UPDATE status_poll_options
             SET votes_count = CASE
                 WHEN votes_count > 0 THEN votes_count - 1
                 ELSE 0
             END
             WHERE poll_id = ?1
               AND position = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }
    let bindings = [D1Type::Text(poll.id.as_str()), D1Type::Text(account_id)];
    db.prepare(
        "DELETE FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    for choice in choices {
        let vote_id = generate_entity_id(16)?;
        let bindings = [
            D1Type::Text(vote_id.as_str()),
            D1Type::Text(poll.id.as_str()),
            D1Type::Text(account_id),
            D1Type::Integer(*choice as i32),
        ];
        db.prepare(
            "INSERT INTO status_poll_votes (
                id,
                poll_id,
                account_id,
                option_position,
                created_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;

        let bindings = [
            D1Type::Text(poll.id.as_str()),
            D1Type::Integer(*choice as i32),
        ];
        db.prepare(
            "UPDATE status_poll_options
             SET votes_count = votes_count + 1
             WHERE poll_id = ?1
               AND position = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn apply_incoming_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    choice: u32,
    activity_uri: Option<&str>,
) -> Result<()> {
    let options = list_status_poll_options(db, &poll.id).await?;
    if choice as usize >= options.len() {
        return Ok(());
    }
    let existing = list_poll_vote_positions_for_account(db, &poll.id, account_id).await?;
    if poll.multiple == 0 && !existing.is_empty() {
        return Ok(());
    }
    if existing.iter().any(|position| *position == choice) {
        return Ok(());
    }

    let vote_id = generate_entity_id(16)?;
    let bindings = [
        D1Type::Text(vote_id.as_str()),
        D1Type::Text(poll.id.as_str()),
        D1Type::Text(account_id),
        D1Type::Integer(choice as i32),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT OR IGNORE INTO status_poll_votes (
            id,
            poll_id,
            account_id,
            option_position,
            activity_uri,
            created_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_incoming_poll_vote(
    db: &D1Database,
    poll: &StatusPollRow,
    account_id: &str,
    activity_uri: Option<&str>,
    choice_name: Option<&str>,
) -> Result<bool> {
    let target = if let Some(activity_uri) = activity_uri {
        find_status_poll_vote_for_remote_actor_by_activity_uri(db, account_id, activity_uri).await?
    } else if let Some(choice_name) = choice_name {
        let options = list_status_poll_options(db, &poll.id).await?;
        let Some(position) = options
            .iter()
            .position(|option| option.title == choice_name)
            .and_then(|position| u32::try_from(position).ok())
        else {
            return Ok(false);
        };
        let Some(vote_id) =
            find_status_poll_vote_id_by_position(db, &poll.id, account_id, position).await?
        else {
            return Ok(false);
        };
        let bindings = [D1Type::Text(vote_id.id.as_str())];
        db.prepare(
            "DELETE FROM status_poll_votes
             WHERE id = ?1",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
        let bindings = [
            D1Type::Text(poll.id.as_str()),
            D1Type::Integer(position as i32),
        ];
        db.prepare(
            "UPDATE status_poll_options
             SET votes_count = CASE
                 WHEN votes_count > 0 THEN votes_count - 1
                 ELSE 0
             END
             WHERE poll_id = ?1
               AND position = ?2",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
        return Ok(true);
    } else {
        None
    };

    let Some(target) = target else {
        return Ok(false);
    };
    let bindings = [
        D1Type::Text(target.poll_id.as_str()),
        D1Type::Text(account_id),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "DELETE FROM status_poll_votes
         WHERE poll_id = ?1
           AND account_id = ?2
           AND activity_uri = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    let bindings = [
        D1Type::Text(target.poll_id.as_str()),
        D1Type::Integer(target.option_position as i32),
    ];
    db.prepare(
        "UPDATE status_poll_options
         SET votes_count = CASE
             WHEN votes_count > 0 THEN votes_count - 1
             ELSE 0
         END
         WHERE poll_id = ?1
           AND position = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(true)
}

async fn apply_remote_poll_vote(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    actor: &RemoteActorRow,
    status: &RemoteStatusRow,
    poll: &RemoteStatusPollRow,
    choices: &[u32],
) -> Result<Vec<u32>> {
    let options = list_remote_status_poll_options(db, &poll.id).await?;
    if options.is_empty() {
        return Err(Error::RustError("poll not found".to_owned()));
    }
    let existing_votes = list_remote_poll_votes_for_account(db, &poll.id, &viewer.id).await?;
    let existing = remap_remote_poll_vote_positions(&options, &existing_votes);
    if poll.multiple == 0 && !existing.is_empty() {
        return Err(Error::RustError(
            "you have already voted in this poll".to_owned(),
        ));
    }
    if poll.multiple == 0 && choices.len() > 1 {
        return Err(Error::RustError(
            "single-choice polls accept exactly one choice".to_owned(),
        ));
    }

    let mut new_choices = Vec::new();
    for choice in choices {
        let position = *choice as usize;
        if position >= options.len() {
            return Err(Error::RustError(
                "choices contains an out-of-range option".to_owned(),
            ));
        }
        if existing.iter().any(|value| value == choice)
            || new_choices.iter().any(|value| value == choice)
        {
            continue;
        }
        new_choices.push(*choice);
    }
    if new_choices.is_empty() {
        return Err(Error::RustError(
            "you have already voted in this poll".to_owned(),
        ));
    }

    for choice in &new_choices {
        let option = &options[*choice as usize];
        let (activity_id, payload_json) = build_poll_vote_activity(
            config,
            viewer,
            &actor.actor_uri,
            &status.object_uri,
            &option.title,
        )?;
        queue_remote_actor_activity_required(db, &viewer.id, &actor.actor_uri, &payload_json)
            .await?;

        let vote_id = generate_entity_id(16)?;
        let bindings = [
            D1Type::Text(vote_id.as_str()),
            D1Type::Text(poll.id.as_str()),
            D1Type::Text(viewer.id.as_str()),
            D1Type::Integer(*choice as i32),
            D1Type::Text(option.title.as_str()),
            D1Type::Text(activity_id.as_str()),
        ];
        db.prepare(
            "INSERT OR IGNORE INTO remote_status_poll_votes (
                id,
                poll_id,
                account_id,
                option_position,
                option_title,
                activity_id,
                created_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    let mut own_votes = existing;
    own_votes.extend(new_choices);
    own_votes.sort_unstable();
    own_votes.dedup();
    Ok(own_votes)
}

async fn load_mastodon_poll_response(
    db: &D1Database,
    status_id: &str,
    viewer: Option<&LocalAccount>,
) -> Result<Option<serde_json::Value>> {
    let Some(poll) = find_status_poll_by_status_id(db, status_id).await? else {
        return Ok(None);
    };
    build_mastodon_poll_response(db, &poll, viewer)
        .await
        .map(|value| {
            value.map(|poll| serde_json::to_value(poll).unwrap_or(serde_json::Value::Null))
        })
}

async fn build_mastodon_poll_response(
    db: &D1Database,
    poll: &StatusPollRow,
    viewer: Option<&LocalAccount>,
) -> Result<Option<MastodonPollResponse>> {
    let options = list_status_poll_options(db, &poll.id).await?;
    if options.is_empty() {
        return Ok(None);
    }
    let votes_count = options
        .iter()
        .map(|option| option.votes_count.max(0) as u64)
        .sum();
    let expired = is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false);
    let reveal_totals = expired || poll.hide_totals == 0;
    let own_votes = match viewer {
        Some(viewer) => list_poll_vote_positions_for_account(db, &poll.id, &viewer.id).await?,
        None => Vec::new(),
    };
    let voters_count = if poll.multiple != 0 {
        if reveal_totals {
            Some(count_poll_voters(db, &poll.id).await?)
        } else {
            None
        }
    } else if reveal_totals {
        Some(votes_count)
    } else {
        None
    };

    Ok(Some(MastodonPollResponse {
        id: poll.id.clone(),
        expires_at: poll.expires_at.clone(),
        expired,
        multiple: poll.multiple != 0,
        votes_count: if reveal_totals { votes_count } else { 0 },
        voters_count,
        voted: !own_votes.is_empty(),
        own_votes,
        options: options
            .into_iter()
            .map(|option| MastodonPollOptionResponse {
                title: option.title,
                votes_count: reveal_totals.then_some(option.votes_count.max(0) as u64),
            })
            .collect(),
        emojis: Vec::new(),
    }))
}

async fn delete_status_by_id(db: &D1Database, status_id: &str) -> Result<()> {
    let status_binding = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM status_poll_options
         WHERE poll_id IN (
             SELECT id
             FROM status_polls
             WHERE status_id = ?1
         )",
    )
    .bind_refs(&status_binding)?
    .run()
    .await?;

    let status_binding = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(&status_binding)?
    .run()
    .await?;

    let status_id = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM statuses
         WHERE id = ?1",
    )
    .bind_refs(&status_id)?
    .run()
    .await?;

    Ok(())
}

async fn delete_remote_status_by_id(db: &D1Database, status_id: &str) -> Result<()> {
    let bindings = [D1Type::Text(status_id)];
    db.prepare(
        "DELETE FROM remote_status_poll_options
         WHERE poll_id IN (
            SELECT id
            FROM remote_status_polls
            WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "DELETE FROM remote_status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let status_id = D1Type::Text(status_id);
    db.prepare(
        "DELETE FROM remote_statuses
         WHERE id = ?1",
    )
    .bind_refs(&status_id)?
    .run()
    .await?;

    Ok(())
}

async fn begin_inbox_activity_processing(
    db: &D1Database,
    actor_uri: &str,
    activity_id: &str,
    activity_type: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(actor_uri),
        D1Type::Text(activity_id),
        D1Type::Text(activity_type),
    ];
    let row = db
        .prepare(
            "INSERT OR IGNORE INTO inbox_activities (
                actor_uri,
                activity_id,
                activity_type,
                created_at,
                processed_at
            ) VALUES (
                ?1,
                ?2,
                ?3,
                CURRENT_TIMESTAMP,
                NULL
            )
            RETURNING activity_id",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.is_some())
}

async fn mark_inbox_activity_processed(
    db: &D1Database,
    actor_uri: &str,
    activity_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Text(activity_id)];
    db.prepare(
        "UPDATE inbox_activities
         SET processed_at = CURRENT_TIMESTAMP
         WHERE actor_uri = ?1
           AND activity_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn release_inbox_activity_processing(
    db: &D1Database,
    actor_uri: &str,
    activity_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Text(activity_id)];
    db.prepare(
        "DELETE FROM inbox_activities
         WHERE actor_uri = ?1
           AND activity_id = ?2
           AND processed_at IS NULL",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn find_status_by_id(db: &D1Database, status_id: &str) -> Result<Option<StatusRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
         FROM statuses
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<StatusRow>(None)
    .await
}

async fn find_status_by_ap_id(db: &D1Database, ap_id: &str) -> Result<Option<StatusRow>> {
    let ap_id = D1Type::Text(ap_id);
    db.prepare(
        "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
         FROM statuses
         WHERE ap_id = ?1
         LIMIT 1",
    )
    .bind_refs(&ap_id)?
    .first::<StatusRow>(None)
    .await
}

async fn load_in_reply_to_account_id(
    db: &D1Database,
    status: &StatusRow,
) -> Result<Option<String>> {
    match status.in_reply_to_id.as_deref() {
        Some(reply_id) => Ok(find_status_by_id(db, reply_id)
            .await?
            .map(|reply| reply.account_id)),
        None => Ok(None),
    }
}

async fn status_is_reply_to_other_account(
    db: &D1Database,
    status: &StatusRow,
    account_id: &str,
) -> Result<bool> {
    let Some(reply_id) = status.in_reply_to_id.as_deref() else {
        return Ok(false);
    };

    Ok(find_status_by_id(db, reply_id)
        .await?
        .map(|reply| reply.account_id != account_id)
        .unwrap_or(false))
}

async fn list_public_outbox_statuses(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let account_id = D1Type::Text(account_id);
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id = ?1
               AND visibility IN ('public', 'unlisted')
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(&[account_id, limit])?
        .all()
        .await?;

    result.results::<StatusRow>()
}

async fn list_account_statuses(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let account_id = D1Type::Text(account_id);
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(&[account_id, limit])?
        .all()
        .await?;

    result.results::<StatusRow>()
}

async fn list_direct_local_replies(db: &D1Database, status_id: &str) -> Result<Vec<StatusRow>> {
    let status_id = D1Type::Text(status_id);
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE in_reply_to_id = ?1
             ORDER BY created_at ASC",
        )
        .bind_refs(&status_id)?
        .all()
        .await?;

    result.results::<StatusRow>()
}

async fn list_local_home_timeline_statuses(
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

async fn build_outbox_activities(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    statuses: &[StatusRow],
) -> Result<Vec<serde_json::Value>> {
    let mut items = Vec::with_capacity(statuses.len());

    for status in statuses {
        let note = build_activitypub_note(db, config, account, status, false).await?;
        let note_id = note
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let published = note
            .get("published")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::String(status.created_at.clone()));
        let to = note
            .get("to")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let cc = note
            .get("cc")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));

        items.push(serde_json::json!({
            "type": "Create",
            "id": format!("{note_id}/activity"),
            "actor": actor_url(config, &account.username),
            "published": published,
            "to": to,
            "cc": cc,
            "object": note,
        }));
    }

    Ok(items)
}

async fn enqueue_outbox_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(&status.visibility) {
        return Ok(());
    }

    let note = build_activitypub_note(db, config, account, status, false).await?;
    let note_id = note
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub note id missing".to_owned()))?;
    let activity_id = format!("{note_id}/activity");
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "type": "Create",
        "id": activity_id,
        "actor": actor_url(config, &account.username),
        "published": status.created_at.clone(),
        "to": note.get("to").cloned().unwrap_or_else(|| serde_json::json!([])),
        "cc": note.get("cc").cloned().unwrap_or_else(|| serde_json::json!([])),
        "object": note,
    });
    let payload_json = serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!("failed to serialize queued activity: {error}"))
    })?;

    let bindings = [
        D1Type::Text(account.id.as_str()),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(activity_id.as_str()),
        D1Type::Text(payload_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO outbox_deliveries (
            id,
            account_id,
            status_id,
            activity_id,
            activity_type,
            target_inbox,
            payload_json,
            state,
            attempt_count,
            last_attempt_at,
            next_attempt_at,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            'Create',
            NULL,
            ?4,
            'queued',
            0,
            NULL,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn enqueue_outbox_delete(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(&status.visibility) {
        return Ok(());
    }

    let activity = build_activitypub_delete(config, account, status)?;
    let activity_id = activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub delete id missing".to_owned()))?;
    let payload_json = serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!("failed to serialize delete activity: {error}"))
    })?;

    let bindings = [
        D1Type::Text(account.id.as_str()),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(activity_id),
        D1Type::Text(payload_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO outbox_deliveries (
            id,
            account_id,
            status_id,
            activity_id,
            activity_type,
            target_inbox,
            payload_json,
            state,
            attempt_count,
            last_attempt_at,
            next_attempt_at,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            'Delete',
            NULL,
            ?4,
            'queued',
            0,
            NULL,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn list_pending_generic_outbox_deliveries(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<OutboxDeliveryRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, status_id, activity_id, activity_type, target_inbox, payload_json, attempt_count
             FROM outbox_deliveries
             WHERE state = 'queued'
               AND target_inbox IS NULL
               AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
             ORDER BY created_at ASC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<OutboxDeliveryRow>()
}

async fn list_pending_target_outbox_deliveries(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<OutboxDeliveryRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, status_id, activity_id, activity_type, target_inbox, payload_json, attempt_count
             FROM outbox_deliveries
             WHERE state = 'queued'
               AND target_inbox IS NOT NULL
               AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
             ORDER BY created_at ASC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<OutboxDeliveryRow>()
}

async fn list_follower_delivery_targets(db: &D1Database, account_id: &str) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT DISTINCT COALESCE(NULLIF(shared_inbox_uri, ''), inbox_uri) AS target_inbox
             FROM followers
             WHERE account_id = ?1
             ORDER BY target_inbox ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<FollowerTargetRow>()?
        .into_iter()
        .map(|row| row.target_inbox)
        .filter(|value| !value.trim().is_empty())
        .collect())
}

async fn list_follower_actor_uris(db: &D1Database, account_id: &str) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT actor_uri AS target_inbox
             FROM followers
             WHERE account_id = ?1
             ORDER BY created_at ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<FollowerTargetRow>()?
        .into_iter()
        .map(|row| row.target_inbox)
        .filter(|value| !value.trim().is_empty())
        .collect())
}

async fn list_local_follower_usernames(db: &D1Database, account_id: &str) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT a.username
             FROM follows f
             JOIN accounts a ON a.id = f.follower_account_id
             WHERE f.target_account_id = ?1
               AND f.state = 'accepted'
             ORDER BY f.created_at ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<UsernameRow>()?
        .into_iter()
        .map(|row| row.username)
        .collect())
}

async fn list_following_actor_uris(db: &D1Database, account_id: &str) -> Result<Vec<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT target_actor_uri AS target_inbox
             FROM follows
             WHERE follower_account_id = ?1
               AND state = 'accepted'
             ORDER BY created_at ASC",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<FollowerTargetRow>()?
        .into_iter()
        .map(|row| row.target_inbox)
        .filter(|value| !value.trim().is_empty())
        .collect())
}

async fn count_accepted_following(db: &D1Database, account_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM follows
         WHERE follower_account_id = ?1
           AND state = 'accepted'",
        account_id,
    )
    .await
}

async fn count_local_followers(db: &D1Database, account_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM follows
         WHERE target_account_id = ?1
           AND state = 'accepted'",
        account_id,
    )
    .await
}

async fn count_remote_followers(db: &D1Database, account_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM followers
         WHERE account_id = ?1",
        account_id,
    )
    .await
}

async fn list_local_public_timeline_statuses(
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

async fn list_local_public_statuses_by_tag(
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

async fn list_remote_public_timeline_statuses(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE rs.visibility = 'public'
             ORDER BY rs.published_at DESC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;
    let values = result.results::<serde_json::Value>()?;
    let mut rows = Vec::with_capacity(values.len());

    for value in values {
        rows.push((
            RemoteStatusRow {
                id: value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                actor_uri: value
                    .get("actor_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                object_uri: value
                    .get("object_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: value
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                in_reply_to_uri: value
                    .get("in_reply_to_uri")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                content_html: value
                    .get("content_html")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                spoiler_text: value
                    .get("spoiler_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                visibility: value
                    .get("visibility")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                sensitive: value
                    .get("sensitive")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_default() as i32,
                language: value
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                published_at: value
                    .get("published_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
            RemoteActorRow::from_value(&value),
        ));
    }

    Ok(rows)
}

async fn list_remote_home_timeline_statuses(
    db: &D1Database,
    viewer_account_id: &str,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let bindings = [
        D1Type::Text(viewer_account_id),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             JOIN follows f
               ON f.target_actor_uri = rs.actor_uri
              AND f.follower_account_id = ?1
              AND f.state = 'accepted'
             WHERE rs.visibility IN ('public', 'unlisted', 'private')
             ORDER BY rs.published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    let values = result.results::<serde_json::Value>()?;
    let mut rows = Vec::with_capacity(values.len());

    for value in values {
        rows.push((
            RemoteStatusRow {
                id: value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                actor_uri: value
                    .get("actor_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                object_uri: value
                    .get("object_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: value
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                in_reply_to_uri: value
                    .get("in_reply_to_uri")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                content_html: value
                    .get("content_html")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                spoiler_text: value
                    .get("spoiler_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                visibility: value
                    .get("visibility")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                sensitive: value
                    .get("sensitive")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_default() as i32,
                language: value
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                published_at: value
                    .get("published_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
            RemoteActorRow::from_value(&value),
        ));
    }

    Ok(rows)
}

async fn list_remote_public_statuses_by_tag(
    db: &D1Database,
    tag: &str,
    limit: u32,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let pattern = format!("%#{}%", normalize_hashtag(tag));
    let bindings = [
        D1Type::Text(pattern.as_str()),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE rs.visibility = 'public'
               AND lower(rs.content_html) LIKE ?1
             ORDER BY rs.published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    let values = result.results::<serde_json::Value>()?;
    let mut rows = Vec::with_capacity(values.len());

    for value in values {
        rows.push((
            RemoteStatusRow {
                id: value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                actor_uri: value
                    .get("actor_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                object_uri: value
                    .get("object_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: value
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                in_reply_to_uri: value
                    .get("in_reply_to_uri")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                content_html: value
                    .get("content_html")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                spoiler_text: value
                    .get("spoiler_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                visibility: value
                    .get("visibility")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                sensitive: value
                    .get("sensitive")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_default() as i32,
                language: value
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                published_at: value
                    .get("published_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
            RemoteActorRow::from_value(&value),
        ));
    }

    Ok(rows)
}

async fn list_remote_statuses_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusRow>> {
    let bindings = [D1Type::Text(actor_uri), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
             FROM remote_statuses
             WHERE actor_uri = ?1
             ORDER BY published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusRow>()
}

async fn find_remote_status_by_id(
    db: &D1Database,
    status_id: &str,
) -> Result<Option<RemoteStatusRow>> {
    let status_id = D1Type::Text(status_id);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
         FROM remote_statuses
         WHERE id = ?1
         LIMIT 1",
    )
    .bind_refs(&status_id)?
    .first::<RemoteStatusRow>(None)
    .await
}

async fn find_remote_status_by_object_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Option<RemoteStatusRow>> {
    let object_uri = D1Type::Text(object_uri);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
         FROM remote_statuses
         WHERE object_uri = ?1
         LIMIT 1",
    )
    .bind_refs(&object_uri)?
    .first::<RemoteStatusRow>(None)
    .await
}

async fn find_remote_status_by_url_or_object_uri(
    db: &D1Database,
    value: &str,
) -> Result<Option<RemoteStatusRow>> {
    let value = D1Type::Text(value);
    db.prepare(
        "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
         FROM remote_statuses
         WHERE object_uri = ?1
            OR url = ?1
         LIMIT 1",
    )
    .bind_refs(&value)?
    .first::<RemoteStatusRow>(None)
    .await
}

async fn list_direct_remote_replies_by_uri(
    db: &D1Database,
    object_uri: &str,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let object_uri = D1Type::Text(object_uri);
    let result = db
        .prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE rs.in_reply_to_uri = ?1
             ORDER BY rs.published_at ASC",
        )
        .bind_refs(&object_uri)?
        .all()
        .await?;
    let values = result.results::<serde_json::Value>()?;
    let mut rows = Vec::with_capacity(values.len());
    for value in values {
        rows.push((
            RemoteStatusRow {
                id: value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                actor_uri: value
                    .get("actor_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                object_uri: value
                    .get("object_uri")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: value
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                in_reply_to_uri: value
                    .get("in_reply_to_uri")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                content_html: value
                    .get("content_html")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                spoiler_text: value
                    .get("spoiler_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                visibility: value
                    .get("visibility")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                sensitive: value
                    .get("sensitive")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_default() as i32,
                language: value
                    .get("language")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                published_at: value
                    .get("published_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
            RemoteActorRow::from_value(&value),
        ));
    }

    Ok(rows)
}

async fn find_remote_actor_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<RemoteActorRow>> {
    let actor_uri = D1Type::Text(actor_uri);
    db.prepare(
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE actor_uri = ?1
         LIMIT 1",
    )
    .bind_refs(&actor_uri)?
    .first::<RemoteActorRow>(None)
    .await
}

async fn find_remote_actor_by_profile_url_or_actor_uri(
    db: &D1Database,
    value: &str,
) -> Result<Option<RemoteActorRow>> {
    let value = D1Type::Text(value);
    db.prepare(
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE actor_uri = ?1
            OR profile_url = ?1
         LIMIT 1",
    )
    .bind_refs(&value)?
    .first::<RemoteActorRow>(None)
    .await
}

async fn find_cached_remote_actor_profile_by_actor_uri(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<RemoteActorProfile>> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT actor_uri, username, domain, inbox_uri, shared_inbox_uri, public_key_id, public_key_pem, display_name, summary_html, profile_url, avatar_url, header_url
             FROM remote_actors
             WHERE actor_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&actor_uri)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row.map(|value| RemoteActorProfile {
        actor_uri: value
            .get("actor_uri")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        username: value
            .get("username")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        domain: value
            .get("domain")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        inbox_uri: value
            .get("inbox_uri")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        shared_inbox_uri: value
            .get("shared_inbox_uri")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        public_key_id: value
            .get("public_key_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        public_key_pem: value
            .get("public_key_pem")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        display_name: value
            .get("display_name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        summary_html: value
            .get("summary_html")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        profile_url: value
            .get("profile_url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        avatar_url: value
            .get("avatar_url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        header_url: value
            .get("header_url")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    }))
}

async fn find_remote_actor_by_username_domain(
    db: &D1Database,
    username: &str,
    domain: &str,
) -> Result<Option<RemoteActorRow>> {
    let username = username.to_ascii_lowercase();
    let domain = domain.to_ascii_lowercase();
    let bindings = [D1Type::Text(&username), D1Type::Text(&domain)];
    db.prepare(
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE lower(username) = ?1
           AND lower(domain) = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<RemoteActorRow>(None)
    .await
}

async fn search_local_accounts(
    db: &D1Database,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
    viewer_account_id: Option<&str>,
) -> Result<Vec<LocalAccount>> {
    if following_only && viewer_account_id.is_none() {
        return Ok(Vec::new());
    }

    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let sql = if following_only {
        "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
         FROM accounts a
         JOIN follows f
           ON f.target_account_id = a.id
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE lower(a.username) LIKE ?2
            OR lower(a.display_name) LIKE ?2
         ORDER BY a.username ASC
         LIMIT ?3
         OFFSET ?4"
    } else {
        "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
         FROM accounts
         WHERE lower(username) LIKE ?1
            OR lower(display_name) LIKE ?1
         ORDER BY username ASC
         LIMIT ?2
         OFFSET ?3"
    };

    let result = if following_only {
        let bindings = [
            D1Type::Text(
                viewer_account_id
                    .ok_or_else(|| Error::RustError("missing viewer account id".to_owned()))?,
            ),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    };

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

fn directory_order(value: Option<&str>) -> DirectoryOrder {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("new") => DirectoryOrder::New,
        _ => DirectoryOrder::Active,
    }
}

async fn list_discoverable_accounts(
    db: &D1Database,
    limit: u32,
    offset: u32,
    order: DirectoryOrder,
) -> Result<Vec<LocalAccount>> {
    let sql = match order {
        DirectoryOrder::Active => {
            "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
             FROM accounts a
             LEFT JOIN statuses s
               ON s.account_id = a.id
             WHERE a.discoverable = 1
             GROUP BY a.id
             ORDER BY COALESCE(MAX(s.created_at), a.created_at) DESC, a.username ASC
             LIMIT ?1
             OFFSET ?2"
        }
        DirectoryOrder::New => {
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE discoverable = 1
             ORDER BY created_at DESC, username ASC
             LIMIT ?1
             OFFSET ?2"
        }
    };

    let bindings = [
        D1Type::Integer(limit as i32),
        D1Type::Integer(offset as i32),
    ];
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

async fn search_remote_accounts(
    db: &D1Database,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
    viewer_account_id: Option<&str>,
) -> Result<Vec<RemoteActorRow>> {
    if following_only && viewer_account_id.is_none() {
        return Ok(Vec::new());
    }

    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let sql = if following_only {
        "SELECT ra.actor_uri, ra.username, ra.domain, ra.display_name, ra.summary_html, ra.profile_url, ra.avatar_url, ra.header_url
         FROM remote_actors ra
         JOIN follows f
           ON f.target_actor_uri = ra.actor_uri
          AND f.follower_account_id = ?1
          AND f.state = 'accepted'
         WHERE lower(ra.username) LIKE ?2
            OR lower(ra.display_name) LIKE ?2
            OR lower(ra.domain) LIKE ?2
         ORDER BY ra.username ASC, ra.domain ASC
         LIMIT ?3
         OFFSET ?4"
    } else {
        "SELECT actor_uri, username, domain, display_name, summary_html, profile_url, avatar_url, header_url
         FROM remote_actors
         WHERE lower(username) LIKE ?1
            OR lower(display_name) LIKE ?1
            OR lower(domain) LIKE ?1
         ORDER BY username ASC, domain ASC
         LIMIT ?2
         OFFSET ?3"
    };

    let result = if following_only {
        let bindings = [
            D1Type::Text(
                viewer_account_id
                    .ok_or_else(|| Error::RustError("missing viewer account id".to_owned()))?,
            ),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
            D1Type::Integer(offset as i32),
        ];
        db.prepare(sql).bind_refs(bindings.iter())?.all().await?
    };

    result.results::<RemoteActorRow>()
}

async fn search_cached_accounts(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    query: &str,
    limit: u32,
    offset: u32,
    following_only: bool,
) -> Result<Vec<MastodonAccountResponse>> {
    let mut accounts = Vec::new();
    let viewer_account_id = viewer.map(|account| account.id.as_str());
    let query_limit = limit.saturating_add(offset).clamp(limit, 200);

    for account in
        search_local_accounts(db, query, query_limit, 0, following_only, viewer_account_id).await?
    {
        let stats = load_account_stats(db, &account.id).await?;
        accounts.push(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }
    for actor in
        search_remote_accounts(db, query, query_limit, 0, following_only, viewer_account_id).await?
    {
        accounts.push(MastodonAccountResponse::from_remote_actor(&actor));
    }

    accounts.sort_by_key(|account| {
        account_search_rank(
            query,
            &account.username,
            &account.acct,
            &account.display_name,
        )
    });
    Ok(accounts
        .into_iter()
        .skip(offset as usize)
        .take(limit as usize)
        .collect())
}

async fn search_statuses_for_v2(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &str,
    limit: u32,
    offset: u32,
    account_id: Option<&str>,
) -> Result<Vec<MastodonStatusResponse>> {
    let account_reference = match account_id {
        Some(account_id) => resolve_account_reference(db, account_id).await?,
        None => None,
    };
    if account_id.is_some() && account_reference.is_none() {
        return Ok(Vec::new());
    }
    let query_limit = limit.saturating_add(offset).clamp(limit, 80);
    let mut entries = Vec::new();

    if !matches!(
        account_reference.as_ref(),
        Some(AccountReference::Remote(_))
    ) {
        let local_account_filter = match account_reference.as_ref() {
            Some(AccountReference::Local(account)) => Some(account.id.as_str()),
            _ => None,
        };
        for status in search_local_status_rows(db, query, query_limit, local_account_filter).await?
        {
            let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if !can_view_local_status(db, &status, Some(viewer), &owner).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
            entries.push((
                status.created_at.clone(),
                build_local_status_response(
                    db,
                    config,
                    Some(viewer),
                    &status,
                    &owner,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            ));
        }
    }

    if !matches!(account_reference.as_ref(), Some(AccountReference::Local(_))) {
        let remote_actor_filter = match account_reference.as_ref() {
            Some(AccountReference::Remote(actor)) => Some(actor.actor_uri.as_str()),
            _ => None,
        };
        for (status, actor) in
            search_remote_status_rows(db, query, query_limit, remote_actor_filter).await?
        {
            if !is_public_activitypub_visibility(&status.visibility) {
                continue;
            }
            entries.push((
                status.published_at.clone(),
                build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
            ));
        }
    }

    entries.sort_by(|left, right| right.0.cmp(&left.0));
    Ok(entries
        .into_iter()
        .skip(offset as usize)
        .map(|(_, value)| value)
        .take(limit as usize)
        .collect())
}

async fn search_local_status_rows(
    db: &D1Database,
    query: &str,
    limit: u32,
    account_id: Option<&str>,
) -> Result<Vec<StatusRow>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let result = if let Some(account_id) = account_id {
        let bindings = [
            D1Type::Text(account_id),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id = ?1
               AND (lower(text_content) LIKE ?2 OR lower(spoiler_text) LIKE ?2)
             ORDER BY created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE lower(text_content) LIKE ?1
                OR lower(spoiler_text) LIKE ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    result.results::<StatusRow>()
}

async fn search_remote_status_rows(
    db: &D1Database,
    query: &str,
    limit: u32,
    actor_uri: Option<&str>,
) -> Result<Vec<(RemoteStatusRow, RemoteActorRow)>> {
    let pattern = format!("%{}%", query.trim().to_ascii_lowercase());
    let result = if let Some(actor_uri) = actor_uri {
        let bindings = [
            D1Type::Text(actor_uri),
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE rs.actor_uri = ?1
               AND (lower(rs.content_html) LIKE ?2 OR lower(rs.spoiler_text) LIKE ?2)
             ORDER BY rs.published_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    } else {
        let bindings = [
            D1Type::Text(pattern.as_str()),
            D1Type::Integer(limit as i32),
        ];
        db.prepare(
            "SELECT
                rs.id,
                rs.actor_uri,
                rs.object_uri,
                rs.url,
                rs.in_reply_to_uri,
                rs.content_html,
                rs.spoiler_text,
                rs.visibility,
                rs.sensitive,
                rs.language,
                rs.published_at,
                ra.username,
                ra.domain,
                ra.display_name,
                ra.summary_html,
                ra.profile_url,
                ra.avatar_url,
                ra.header_url
             FROM remote_statuses rs
             JOIN remote_actors ra ON ra.actor_uri = rs.actor_uri
             WHERE lower(rs.content_html) LIKE ?1
                OR lower(rs.spoiler_text) LIKE ?1
             ORDER BY rs.published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?
    };

    let values = result.results::<serde_json::Value>()?;
    let mut rows = Vec::with_capacity(values.len());
    for value in values {
        rows.push((
            RemoteStatusRow {
                id: value
                    .get("id")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                actor_uri: value
                    .get("actor_uri")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                object_uri: value
                    .get("object_uri")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                url: value
                    .get("url")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                in_reply_to_uri: value
                    .get("in_reply_to_uri")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                content_html: value
                    .get("content_html")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                spoiler_text: value
                    .get("spoiler_text")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                visibility: value
                    .get("visibility")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                sensitive: value
                    .get("sensitive")
                    .and_then(|field| field.as_i64())
                    .unwrap_or_default() as i32,
                language: value
                    .get("language")
                    .and_then(|field| field.as_str())
                    .map(ToOwned::to_owned),
                published_at: value
                    .get("published_at")
                    .and_then(|field| field.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
            RemoteActorRow::from_value(&value),
        ));
    }

    Ok(rows)
}

async fn search_tags_for_v2(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
    limit: u32,
) -> Result<Vec<MastodonTagResponse>> {
    let needle = normalize_hashtag(query);
    if needle.is_empty() {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    let mut seen = HashSet::new();

    for status in list_local_public_timeline_statuses(db, 200).await? {
        for tag in extract_hashtags_from_text(&status._text_content) {
            if tag.contains(&needle) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    for (status, _) in list_remote_public_timeline_statuses(db, 200).await? {
        for tag in extract_hashtags_from_html(&status.content_html) {
            if tag.contains(&needle) && seen.insert(tag.clone()) {
                matches.push(tag);
            }
        }
    }

    matches.sort_by_key(|tag| tag_search_rank(&needle, tag));
    matches.truncate(limit as usize);

    let mut responses = Vec::with_capacity(matches.len());
    for tag in matches {
        responses.push(build_tag_response(db, config, &tag).await?);
    }

    Ok(responses)
}

async fn build_tag_response(
    db: &D1Database,
    config: &AppConfig,
    tag: &str,
) -> Result<MastodonTagResponse> {
    let tag = normalize_hashtag(tag);
    let local_count = count_local_public_statuses_by_tag(db, &tag).await?;
    let remote_count = count_remote_public_statuses_by_tag(db, &tag).await?;
    let total_uses = local_count + remote_count;
    let accounts = count_local_accounts_for_tag(db, &tag).await?
        + count_remote_accounts_for_tag(db, &tag).await?;

    Ok(MastodonTagResponse {
        id: tag_rest_id(&tag),
        name: tag.clone(),
        url: tag_url(config, &tag),
        history: if total_uses == 0 {
            tag_history_stub()
        } else {
            vec![MastodonTagHistoryEntry {
                day: js_sys::Date::new_0()
                    .to_iso_string()
                    .as_string()
                    .unwrap_or_default()
                    .chars()
                    .take(10)
                    .collect(),
                uses: total_uses.to_string(),
                accounts: accounts.to_string(),
            }]
        },
        following: false,
        featured: false,
    })
}

async fn count_local_public_statuses_by_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(*) AS count
         FROM statuses
         WHERE visibility = 'public'
           AND lower(text_content) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}

async fn count_remote_public_statuses_by_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(*) AS count
         FROM remote_statuses
         WHERE visibility = 'public'
           AND lower(content_html) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}

async fn count_local_accounts_for_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(DISTINCT account_id) AS count
         FROM statuses
         WHERE visibility = 'public'
           AND lower(text_content) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}

async fn count_remote_accounts_for_tag(db: &D1Database, tag: &str) -> Result<u64> {
    count_rows_like(
        db,
        "SELECT COUNT(DISTINCT actor_uri) AS count
         FROM remote_statuses
         WHERE visibility = 'public'
           AND lower(content_html) LIKE ?1",
        &format!("%#{}%", normalize_hashtag(tag)),
    )
    .await
}

async fn build_local_status_context(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &StatusRow,
    root_owner: &LocalAccount,
) -> Result<MastodonContextResponse> {
    let mut ancestors = Vec::new();
    let mut current = root.in_reply_to_id.clone();
    let mut seen_local_ids = HashSet::new();

    while let Some(status_id) = current {
        if !seen_local_ids.insert(status_id.clone()) {
            break;
        }
        let Some(status) = find_status_by_id(db, &status_id).await? else {
            break;
        };
        let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
            break;
        };
        if !can_view_local_status(db, &status, viewer, &owner).await? {
            break;
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
        ancestors.push(
            build_local_status_response(
                db,
                config,
                viewer,
                &status,
                &owner,
                in_reply_to_account_id,
                media,
            )
            .await?,
        );
        current = status.in_reply_to_id.clone();
    }
    ancestors.reverse();

    let root_uri = root.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, &root_owner.username),
            root.id
        )
    });
    let descendants =
        collect_descendants_for_local_root(db, config, viewer, root, &root_uri).await?;

    Ok(MastodonContextResponse {
        ancestors,
        descendants,
    })
}

async fn collect_descendants_for_local_root(
    db: &D1Database,
    config: &AppConfig,
    viewer: Option<&LocalAccount>,
    root: &StatusRow,
    root_uri: &str,
) -> Result<Vec<MastodonStatusResponse>> {
    let mut descendants = Vec::new();
    let mut queued_local_ids = vec![root.id.clone()];
    let mut queued_uris = vec![root_uri.to_owned()];
    let mut seen_local_ids = HashSet::new();
    let mut seen_remote_ids = HashSet::new();

    while let Some(status_id) = queued_local_ids.pop() {
        if !seen_local_ids.insert(status_id.clone()) {
            continue;
        }
        for status in list_direct_local_replies(db, &status_id).await? {
            let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if !can_view_local_status(db, &status, viewer, &owner).await? {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
            descendants.push((
                status.created_at.clone(),
                build_local_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    &owner,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            ));
            queued_local_ids.push(status.id.clone());
        }
    }

    while let Some(object_uri) = queued_uris.pop() {
        for (status, actor) in list_direct_remote_replies_by_uri(db, &object_uri).await? {
            if !seen_remote_ids.insert(status.id.clone()) {
                continue;
            }
            if !is_public_activitypub_visibility(&status.visibility) {
                continue;
            }
            descendants.push((
                status.published_at.clone(),
                build_remote_status_response(db, config, viewer, &status, &actor).await?,
            ));
            queued_uris.push(status.object_uri.clone());
        }
    }

    descendants.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(descendants.into_iter().map(|(_, status)| status).collect())
}

async fn build_remote_status_context(
    db: &D1Database,
    config: &AppConfig,
    root: &RemoteStatusRow,
    root_actor: &RemoteActorRow,
) -> Result<MastodonContextResponse> {
    let mut ancestors = Vec::new();
    let mut current = root.in_reply_to_uri.clone();
    let mut seen_remote_ids = HashSet::new();

    while let Some(object_uri) = current {
        if let Some(local_status) = find_status_by_ap_id(db, &object_uri).await? {
            let Some(owner) = find_account_by_id(db, &local_status.account_id).await? else {
                break;
            };
            if !is_public_activitypub_visibility(&local_status.visibility) {
                break;
            }
            let media = find_media_attachments_by_status_id(db, &local_status.id).await?;
            let in_reply_to_account_id = load_in_reply_to_account_id(db, &local_status).await?;
            ancestors.push(
                build_local_status_response(
                    db,
                    config,
                    None,
                    &local_status,
                    &owner,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            );
            break;
        }

        let Some(status) = find_remote_status_by_object_uri(db, &object_uri).await? else {
            break;
        };
        if !seen_remote_ids.insert(status.id.clone()) {
            break;
        }
        if !is_public_activitypub_visibility(&status.visibility) {
            break;
        }
        let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
            break;
        };
        ancestors.push(build_remote_status_response(db, config, None, &status, &actor).await?);
        current = status.in_reply_to_uri.clone();
    }
    ancestors.reverse();

    let descendants = collect_descendants_for_remote_root(db, config, root, root_actor).await?;
    Ok(MastodonContextResponse {
        ancestors,
        descendants,
    })
}

async fn collect_descendants_for_remote_root(
    db: &D1Database,
    config: &AppConfig,
    root: &RemoteStatusRow,
    _root_actor: &RemoteActorRow,
) -> Result<Vec<MastodonStatusResponse>> {
    let mut descendants = Vec::new();
    let mut queued_uris = vec![root.object_uri.clone()];
    let mut seen_remote_ids = HashSet::from([root.id.clone()]);

    while let Some(object_uri) = queued_uris.pop() {
        for (status, actor) in list_direct_remote_replies_by_uri(db, &object_uri).await? {
            if !seen_remote_ids.insert(status.id.clone()) {
                continue;
            }
            if !is_public_activitypub_visibility(&status.visibility) {
                continue;
            }
            descendants.push((
                status.published_at.clone(),
                build_remote_status_response(db, config, None, &status, &actor).await?,
            ));
            queued_uris.push(status.object_uri.clone());
        }
    }

    descendants.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(descendants.into_iter().map(|(_, status)| status).collect())
}

async fn upsert_remote_actor(db: &D1Database, actor: &RemoteActorProfile) -> Result<()> {
    let bindings = [
        D1Type::Text(actor.actor_uri.as_str()),
        D1Type::Text(actor.username.as_str()),
        D1Type::Text(actor.domain.as_str()),
        D1Type::Text(actor.inbox_uri.as_str()),
        match actor.shared_inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(actor.public_key_id.as_str()),
        D1Type::Text(actor.public_key_pem.as_str()),
        D1Type::Text(actor.display_name.as_str()),
        D1Type::Text(actor.summary_html.as_str()),
        match actor.profile_url.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match actor.avatar_url.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match actor.header_url.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO remote_actors (
            actor_uri,
            username,
            domain,
            inbox_uri,
            shared_inbox_uri,
            public_key_id,
            public_key_pem,
            display_name,
            summary_html,
            profile_url,
            avatar_url,
            header_url,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(actor_uri) DO UPDATE SET
            username = excluded.username,
            domain = excluded.domain,
            inbox_uri = excluded.inbox_uri,
            shared_inbox_uri = excluded.shared_inbox_uri,
            public_key_id = excluded.public_key_id,
            public_key_pem = excluded.public_key_pem,
            display_name = excluded.display_name,
            summary_html = excluded.summary_html,
            profile_url = excluded.profile_url,
            avatar_url = excluded.avatar_url,
            header_url = excluded.header_url,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn resolve_account_reference(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<AccountReference>> {
    if let Some(account) = find_account_by_id(db, account_id).await? {
        return Ok(Some(AccountReference::Local(account)));
    }
    let Some(actor_uri) = remote_actor_uri_from_rest_id(account_id) else {
        return Ok(None);
    };
    Ok(find_remote_actor_by_actor_uri(db, &actor_uri)
        .await?
        .map(AccountReference::Remote))
}

async fn resolve_lookup_account(
    db: &D1Database,
    config: &AppConfig,
    acct: &str,
) -> Result<MastodonAccountResponse> {
    let handle = parse_lookup_handle(acct, config)?;
    if handle.is_local_to(&config.instance_domain) {
        let Some(account) = find_account_by_username(db, &handle.username).await? else {
            return Err(Error::RustError("account not found".to_owned()));
        };
        let stats = load_account_stats(db, &account.id).await?;
        return Ok(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        ));
    }

    let profile = fetch_remote_account_profile_by_handle(&handle).await?;
    upsert_remote_actor(db, &profile).await?;
    let actor = find_remote_actor_by_actor_uri(db, &profile.actor_uri)
        .await?
        .ok_or_else(|| Error::RustError("remote account could not be cached".to_owned()))?;
    Ok(MastodonAccountResponse::from_remote_actor(&actor))
}

async fn resolve_search_account(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
) -> Result<Option<MastodonAccountResponse>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }

    if query.contains('@')
        && let Ok(account) = resolve_lookup_account(db, config, query).await
    {
        return Ok(Some(account));
    }

    if parse_remote_http_url(query).is_err() {
        return Ok(None);
    }

    if let Some(username) = local_username_from_actor_uri(config, query)
        && let Some(account) = find_account_by_username(db, &username).await?
    {
        let stats = load_account_stats(db, &account.id).await?;
        return Ok(Some(MastodonAccountResponse::from_account_with_stats(
            &account, config, &stats,
        )));
    }

    if let Some(actor) = find_remote_actor_by_profile_url_or_actor_uri(db, query).await? {
        return Ok(Some(MastodonAccountResponse::from_remote_actor(&actor)));
    }

    let profile = match fetch_remote_actor_profile(query).await {
        Ok(profile) => profile,
        Err(_) => return Ok(None),
    };
    upsert_remote_actor(db, &profile).await?;
    Ok(find_remote_actor_by_actor_uri(db, &profile.actor_uri)
        .await?
        .map(|actor| MastodonAccountResponse::from_remote_actor(&actor)))
}

async fn resolve_search_tag(
    db: &D1Database,
    config: &AppConfig,
    query: &str,
) -> Result<Option<MastodonTagResponse>> {
    let Some(tag) = resolve_search_tag_name(query) else {
        return Ok(None);
    };

    Ok(Some(build_tag_response(db, config, &tag).await?))
}

fn resolve_search_tag_name(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }

    if query.starts_with('#') {
        let tag = normalize_hashtag(query);
        return (!tag.is_empty()).then_some(tag);
    }

    if let Ok(url) = Url::parse(query) {
        return search_tag_name_from_path(url.path());
    }

    if query.starts_with('/') {
        return search_tag_name_from_path(query);
    }

    None
}

fn search_tag_name_from_path(path: &str) -> Option<String> {
    let segments = path
        .split('?')
        .next()
        .unwrap_or(path)
        .split('#')
        .next()
        .unwrap_or(path)
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let tag = match segments.as_slice() {
        ["tags", tag] => *tag,
        ["explore", "tags", tag] => *tag,
        _ => return None,
    };
    let normalized = normalize_hashtag(tag);
    (!normalized.is_empty()).then_some(normalized)
}

async fn resolve_remote_status_by_url(
    db: &D1Database,
    _config: &AppConfig,
    url: &str,
) -> Result<Option<(RemoteStatusRow, RemoteActorRow)>> {
    if let Some(status) = find_remote_status_by_url_or_object_uri(db, url).await? {
        let Some(actor) = find_remote_actor_by_actor_uri(db, &status.actor_uri).await? else {
            return Ok(None);
        };
        return Ok(Some((status, actor)));
    }

    let document = match fetch_remote_activitypub_document(url).await {
        Ok(document) => document,
        Err(_) => return Ok(None),
    };
    let Some(object) = extract_remote_note_object(&document) else {
        return Ok(None);
    };
    if !is_public_activitypub_visibility(&visibility_from_activitypub_object(object)) {
        return Ok(None);
    }

    let actor_uri = object
        .get("attributedTo")
        .and_then(serde_json::Value::as_str)
        .or_else(|| document.get("actor").and_then(serde_json::Value::as_str))
        .ok_or_else(|| {
            Error::RustError("remote status object is missing attributedTo".to_owned())
        })?;
    let actor = fetch_remote_actor_profile(actor_uri).await?;
    upsert_remote_actor(db, &actor).await?;
    upsert_remote_status(db, &actor, object).await?;
    let object_uri = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote status object is missing id".to_owned()))?;
    let Some(status) = find_remote_status_by_object_uri(db, object_uri).await? else {
        return Ok(None);
    };
    let Some(actor_row) = find_remote_actor_by_actor_uri(db, &actor.actor_uri).await? else {
        return Ok(None);
    };
    Ok(Some((status, actor_row)))
}

async fn resolve_search_status(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &str,
) -> Result<Option<MastodonStatusResponse>> {
    let query = query.trim();
    if parse_remote_http_url(query).is_err() {
        return Ok(None);
    }

    if let Some(status) = find_local_status_by_object_uri(db, config, query).await? {
        let Some(account) = find_account_by_id(db, &status.account_id).await? else {
            return Ok(None);
        };
        if !can_view_local_status(db, &status, Some(viewer), &account).await? {
            return Ok(None);
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        return Ok(Some(
            build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &account,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?,
        ));
    }

    if let Some((status, actor)) = resolve_remote_status_by_url(db, config, query).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Ok(None);
        }
        return Ok(Some(
            build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
        ));
    }

    Ok(None)
}

async fn upsert_remote_status(
    db: &D1Database,
    actor: &RemoteActorProfile,
    object: &serde_json::Value,
) -> Result<()> {
    let object_uri = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote status object is missing id".to_owned()))?
        .to_owned();
    let raw_object_json = serde_json::to_string(object).map_err(|error| {
        Error::RustError(format!("failed to serialize remote status object: {error}"))
    })?;
    let visibility = visibility_from_activitypub_object(object);
    let content_html = object
        .get("content")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            object
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(render_status_html)
        })
        .unwrap_or_default();
    let spoiler_text = object
        .get("summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let sensitive = object
        .get("sensitive")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let published_at = object
        .get("published")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| {
            object
                .get("updated")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        })
        .to_owned();
    let language = object
        .get("contentMap")
        .and_then(serde_json::Value::as_object)
        .and_then(|map| map.keys().next().cloned());
    let status_id = generate_entity_id(16)?;

    let bindings = [
        D1Type::Text(status_id.as_str()),
        D1Type::Text(actor.actor_uri.as_str()),
        D1Type::Text(object_uri.as_str()),
        match object.get("url").and_then(serde_json::Value::as_str) {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match object.get("inReplyTo").and_then(serde_json::Value::as_str) {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(content_html.as_str()),
        D1Type::Text(spoiler_text.as_str()),
        D1Type::Text(visibility.as_str()),
        D1Type::Integer(if sensitive { 1 } else { 0 }),
        match language.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(published_at.as_str()),
        D1Type::Text(raw_object_json.as_str()),
    ];
    db.prepare(
        "INSERT INTO remote_statuses (
            id,
            actor_uri,
            object_uri,
            url,
            in_reply_to_uri,
            content_html,
            spoiler_text,
            visibility,
            sensitive,
            language,
            published_at,
            raw_object_json,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(object_uri) DO UPDATE SET
            actor_uri = excluded.actor_uri,
            url = excluded.url,
            in_reply_to_uri = excluded.in_reply_to_uri,
            content_html = excluded.content_html,
            spoiler_text = excluded.spoiler_text,
            visibility = excluded.visibility,
            sensitive = excluded.sensitive,
            language = excluded.language,
            published_at = excluded.published_at,
            raw_object_json = excluded.raw_object_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let status = find_remote_status_by_object_uri(db, &object_uri)
        .await?
        .ok_or_else(|| Error::RustError("cached remote status could not be reloaded".to_owned()))?;
    if let Some(poll) = extract_remote_poll_draft(object) {
        upsert_remote_status_poll(db, &status.id, &poll).await?;
    } else {
        delete_remote_status_poll_by_status_id(db, &status.id).await?;
    }

    Ok(())
}

async fn upsert_remote_status_poll(
    db: &D1Database,
    status_id: &str,
    poll: &RemotePollDraft,
) -> Result<()> {
    let poll_id = format!("remote-{status_id}");
    let bindings = [
        D1Type::Text(poll_id.as_str()),
        D1Type::Text(status_id),
        D1Type::Integer(if poll.multiple { 1 } else { 0 }),
        match poll.expires_at.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match poll.voters_count {
            Some(value) => D1Type::Integer(value as i32),
            None => D1Type::Null,
        },
        D1Type::Integer(poll.votes_count.min(i32::MAX as u64) as i32),
        D1Type::Integer(if poll.expired { 1 } else { 0 }),
    ];
    db.prepare(
        "INSERT INTO remote_status_polls (
            id,
            status_id,
            multiple,
            expires_at,
            voters_count,
            votes_count,
            expired,
            created_at,
            updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(status_id) DO UPDATE SET
            id = excluded.id,
            multiple = excluded.multiple,
            expires_at = excluded.expires_at,
            voters_count = excluded.voters_count,
            votes_count = excluded.votes_count,
            expired = excluded.expired,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let delete_bindings = [D1Type::Text(poll_id.as_str())];
    db.prepare(
        "DELETE FROM remote_status_poll_options
         WHERE poll_id = ?1",
    )
    .bind_refs(delete_bindings.iter())?
    .run()
    .await?;

    for (position, option) in poll.options.iter().enumerate() {
        let bindings = [
            D1Type::Text(poll_id.as_str()),
            D1Type::Integer(position as i32),
            D1Type::Text(option.title.as_str()),
            D1Type::Integer(option.votes_count.min(i32::MAX as u64) as i32),
        ];
        db.prepare(
            "INSERT INTO remote_status_poll_options (
                poll_id,
                position,
                title,
                votes_count
            ) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    let current_options = list_remote_status_poll_options(db, &poll_id).await?;
    prune_remote_poll_vote_rows(db, &poll_id, &current_options).await?;

    Ok(())
}

async fn delete_remote_status_poll_by_status_id(db: &D1Database, status_id: &str) -> Result<()> {
    let bindings = [D1Type::Text(status_id)];
    db.prepare(
        "DELETE FROM remote_status_poll_votes
         WHERE poll_id IN (
            SELECT id
            FROM remote_status_polls
            WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "DELETE FROM remote_status_poll_options
         WHERE poll_id IN (
            SELECT id
            FROM remote_status_polls
            WHERE status_id = ?1
         )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "DELETE FROM remote_status_polls
         WHERE status_id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn upsert_follower(
    db: &D1Database,
    account_id: &str,
    remote_actor: &RemoteActorProfile,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(remote_actor.actor_uri.as_str()),
        D1Type::Text(remote_actor.inbox_uri.as_str()),
        match remote_actor.shared_inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO followers (
            id,
            account_id,
            actor_uri,
            inbox_uri,
            shared_inbox_uri,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, actor_uri) DO UPDATE SET
            inbox_uri = excluded.inbox_uri,
            shared_inbox_uri = excluded.shared_inbox_uri,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_follower_by_actor(
    db: &D1Database,
    account_id: &str,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(actor_uri),
        D1Type::Text(canonical_actor_uri),
    ];
    db.prepare(
        "DELETE FROM followers
         WHERE account_id = ?1
           AND (actor_uri = ?2 OR actor_uri = ?3)",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn expand_outbox_delivery_targets(
    db: &D1Database,
    delivery: &OutboxDeliveryRow,
    targets: &[String],
) -> Result<usize> {
    let mut seen = HashSet::new();
    let mut inserted = 0usize;

    for target in targets {
        if !seen.insert(target.clone()) {
            continue;
        }

        let bindings = [
            D1Type::Text(delivery.account_id.as_str()),
            D1Type::Text(delivery.status_id.as_str()),
            D1Type::Text(delivery.activity_id.as_str()),
            D1Type::Text(delivery.activity_type.as_str()),
            D1Type::Text(target.as_str()),
            D1Type::Text(delivery.payload_json.as_str()),
        ];
        db.prepare(
            "INSERT OR IGNORE INTO outbox_deliveries (
                id,
                account_id,
                status_id,
                activity_id,
                activity_type,
                target_inbox,
                payload_json,
                state,
                attempt_count,
                last_attempt_at,
                next_attempt_at,
                created_at,
                updated_at
            ) VALUES (
                lower(hex(randomblob(16))),
                ?1,
                ?2,
                ?3,
                ?4,
                ?5,
                ?6,
                'queued',
                0,
                NULL,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP,
                CURRENT_TIMESTAMP
            )",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
        inserted += 1;
    }

    Ok(inserted)
}

async fn mark_outbox_delivery_expanded(db: &D1Database, delivery_id: &str) -> Result<()> {
    let bindings = [D1Type::Text("expanded"), D1Type::Text(delivery_id)];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn mark_outbox_delivery_completed_without_targets(
    db: &D1Database,
    delivery_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text("delivered"), D1Type::Text(delivery_id)];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn mark_outbox_delivery_delivered(db: &D1Database, delivery_id: &str) -> Result<()> {
    let bindings = [D1Type::Text("delivered"), D1Type::Text(delivery_id)];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn mark_outbox_delivery_terminal_failure(
    db: &D1Database,
    delivery_id: &str,
    next_attempt: u32,
) -> Result<()> {
    let bindings = [
        D1Type::Text("failed"),
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delivery_id),
    ];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = ?1,
             attempt_count = ?2,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn reschedule_outbox_delivery(
    db: &D1Database,
    delivery_id: &str,
    next_attempt: u32,
) -> Result<()> {
    let delay = delivery_retry_delay_modifier(next_attempt);
    let bindings = [
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delay),
        D1Type::Text(delivery_id),
    ];
    db.prepare(
        "UPDATE outbox_deliveries
         SET state = 'queued',
             attempt_count = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = datetime(CURRENT_TIMESTAMP, ?2),
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn enqueue_outbound_activity(
    db: &D1Database,
    account_id: &str,
    activity_id: &str,
    activity_type: &str,
    target_actor_uri: Option<&str>,
    target_inbox: &str,
    payload_json: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(activity_id),
        D1Type::Text(activity_type),
        match target_actor_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(target_inbox),
        D1Type::Text(payload_json),
    ];
    db.prepare(
        "INSERT INTO outbound_activities (
            id,
            account_id,
            activity_id,
            activity_type,
            target_actor_uri,
            target_inbox,
            payload_json,
            state,
            attempt_count,
            last_attempt_at,
            next_attempt_at,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            ?6,
            'queued',
            0,
            NULL,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(activity_id) DO UPDATE SET
            activity_type = excluded.activity_type,
            target_actor_uri = excluded.target_actor_uri,
            target_inbox = excluded.target_inbox,
            payload_json = excluded.payload_json,
            state = 'queued',
            attempt_count = 0,
            last_attempt_at = NULL,
            next_attempt_at = CURRENT_TIMESTAMP,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

fn describe_outbound_activity(payload_json: &str) -> Result<OutboundActivityDescriptor> {
    let payload = serde_json::from_str::<serde_json::Value>(payload_json)
        .map_err(|error| Error::RustError(format!("failed to parse outbound activity: {error}")))?;
    let activity_id = payload
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("outbound activity is missing id".to_owned()))?
        .to_owned();
    let activity_type = payload
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::RustError("outbound activity is missing type".to_owned()))?
        .to_owned();

    Ok(OutboundActivityDescriptor {
        activity_id,
        activity_type,
    })
}

async fn queue_remote_actor_activity(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
    payload_json: &str,
) -> Result<Option<String>> {
    let Some(target_inbox) = load_remote_actor_delivery_inbox(db, target_actor_uri).await? else {
        return Ok(None);
    };
    let descriptor = describe_outbound_activity(payload_json)?;
    enqueue_outbound_activity(
        db,
        account_id,
        &descriptor.activity_id,
        &descriptor.activity_type,
        Some(target_actor_uri),
        &target_inbox,
        payload_json,
    )
    .await?;
    Ok(Some(descriptor.activity_id))
}

async fn queue_remote_actor_activity_required(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
    payload_json: &str,
) -> Result<String> {
    queue_remote_actor_activity(db, account_id, target_actor_uri, payload_json)
        .await?
        .ok_or_else(|| Error::RustError("remote account is missing a delivery inbox".to_owned()))
}

async fn enqueue_profile_update_activities(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
) -> Result<()> {
    for target_actor_uri in list_follower_actor_uris(db, &account.id).await? {
        let payload_json = build_update_person_activity(config, account)?;
        queue_remote_actor_activity(db, &account.id, &target_actor_uri, &payload_json).await?;
    }

    Ok(())
}

async fn enqueue_status_update_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<()> {
    if !is_public_activitypub_visibility(&status.visibility) {
        return Ok(());
    }

    let payload_json = build_status_update_activity(db, config, account, status).await?;
    for target_actor_uri in list_follower_actor_uris(db, &account.id).await? {
        queue_remote_actor_activity(db, &account.id, &target_actor_uri, &payload_json).await?;
    }

    Ok(())
}

async fn list_pending_outbound_activities(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<OutboundActivityRow>> {
    let limit = D1Type::Integer(limit as i32);
    let result = db
        .prepare(
            "SELECT id, account_id, activity_id, activity_type, target_actor_uri, target_inbox, payload_json, attempt_count
             FROM outbound_activities
             WHERE state = 'queued'
               AND (next_attempt_at IS NULL OR next_attempt_at <= CURRENT_TIMESTAMP)
             ORDER BY created_at ASC
             LIMIT ?1",
        )
        .bind_refs(&limit)?
        .all()
        .await?;

    result.results::<OutboundActivityRow>()
}

async fn mark_outbound_activity_delivered(db: &D1Database, activity_id: &str) -> Result<()> {
    let bindings = [D1Type::Text("delivered"), D1Type::Text(activity_id)];
    db.prepare(
        "UPDATE outbound_activities
         SET state = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn mark_outbound_activity_terminal_failure(
    db: &D1Database,
    activity_id: &str,
    next_attempt: u32,
) -> Result<()> {
    let bindings = [
        D1Type::Text("failed"),
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(activity_id),
    ];
    db.prepare(
        "UPDATE outbound_activities
         SET state = ?1,
             attempt_count = ?2,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

fn outbound_terminal_failure_follow_state(activity_type: &str) -> Option<&'static str> {
    match activity_type {
        "Follow" => Some("failed"),
        _ => None,
    }
}

async fn reconcile_outbound_activity_terminal_failure(
    db: &D1Database,
    delivery: &OutboundActivityRow,
    next_attempt: u32,
) -> Result<()> {
    mark_outbound_activity_terminal_failure(db, &delivery.id, next_attempt).await?;

    if let Some(state) = outbound_terminal_failure_follow_state(&delivery.activity_type)
        && let Some(target_actor_uri) = delivery.target_actor_uri.as_deref()
    {
        let bindings = [
            D1Type::Text(state),
            D1Type::Text(delivery.account_id.as_str()),
            D1Type::Text(target_actor_uri),
            D1Type::Text(delivery.activity_id.as_str()),
        ];
        db.prepare(
            "UPDATE follows
             SET state = ?1,
                 updated_at = CURRENT_TIMESTAMP
             WHERE follower_account_id = ?2
               AND target_actor_uri = ?3
               AND follow_activity_id = ?4
               AND state = 'pending'",
        )
        .bind_refs(bindings.iter())?
        .run()
        .await?;
    }

    Ok(())
}

async fn reschedule_outbound_activity(
    db: &D1Database,
    activity_id: &str,
    next_attempt: u32,
) -> Result<()> {
    let delay = delivery_retry_delay_modifier(next_attempt);
    let bindings = [
        D1Type::Integer(next_attempt as i32),
        D1Type::Text(delay),
        D1Type::Text(activity_id),
    ];
    db.prepare(
        "UPDATE outbound_activities
         SET state = 'queued',
             attempt_count = ?1,
             last_attempt_at = CURRENT_TIMESTAMP,
             next_attempt_at = datetime(CURRENT_TIMESTAMP, ?2),
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn build_activitypub_note(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    include_context: bool,
) -> Result<serde_json::Value> {
    let actor = actor_url(config, &account.username);
    let note_id = local_status_ap_id(config, account, status);
    let audiences = activitypub_audiences(config, &account.username, &status.visibility);
    let poll = find_status_poll_by_status_id(db, &status.id).await?;
    let reply_uri = match status.in_reply_to_id.as_deref() {
        Some(reply_id) => find_status_by_id(db, reply_id)
            .await?
            .and_then(|reply| reply.ap_id),
        None => None,
    };
    let attachments = find_media_attachments_by_status_id(db, &status.id).await?;

    let mut note = serde_json::json!({
        "type": "Note",
        "id": note_id.clone(),
        "url": note_id.clone(),
        "attributedTo": actor,
        "content": status.content_html,
        "published": status.created_at,
        "to": audiences.0,
        "cc": audiences.1,
        "attachment": attachments
            .iter()
            .map(|attachment| {
                serde_json::json!({
                    "type": "Document",
                    "mediaType": attachment.content_type,
                    "url": media_object_url(config, &attachment.object_key),
                    "name": if attachment.description.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(attachment.description.clone())
                    },
                })
            })
            .collect::<Vec<_>>(),
    });

    if include_context {
        note["@context"] = serde_json::json!("https://www.w3.org/ns/activitystreams");
    }
    if !status.spoiler_text.is_empty() {
        note["summary"] = serde_json::json!(status.spoiler_text.clone());
        note["sensitive"] = serde_json::json!(true);
    } else {
        note["sensitive"] = serde_json::json!(status.sensitive != 0);
    }
    if let Some(language) = &status.language {
        let mut content_map = serde_json::Map::new();
        content_map.insert(
            language.clone(),
            serde_json::Value::String(status.content_html.clone()),
        );
        note["contentMap"] = serde_json::Value::Object(content_map);
    }
    if let Some(reply_uri) = reply_uri {
        note["inReplyTo"] = serde_json::json!(reply_uri);
    }
    if let Some(poll) = poll {
        let options = list_status_poll_options(db, &poll.id).await?;
        let voters_count = count_poll_voters(db, &poll.id).await?;
        let expired = is_iso_timestamp_in_past(&poll.expires_at).unwrap_or(false);
        apply_activitypub_poll_fields(&mut note, &poll, &options, voters_count, expired);
        if include_context {
            note["@context"] = serde_json::json!([
                "https://www.w3.org/ns/activitystreams",
                {
                    "votersCount": "http://joinmastodon.org/ns#votersCount"
                }
            ]);
        }
    }

    Ok(note)
}

fn apply_activitypub_poll_fields(
    object: &mut serde_json::Value,
    poll: &StatusPollRow,
    options: &[StatusPollOptionRow],
    voters_count: u64,
    expired: bool,
) {
    if options.is_empty() {
        return;
    }

    object["type"] = serde_json::json!("Question");
    object["endTime"] = serde_json::json!(poll.expires_at.clone());
    object["votersCount"] = serde_json::json!(voters_count);
    if expired {
        object["closed"] = serde_json::json!(poll.expires_at.clone());
    }

    let rendered_options = options
        .iter()
        .map(|option| {
            serde_json::json!({
                "type": "Note",
                "name": option.title,
                "replies": {
                    "type": "Collection",
                    "totalItems": option.votes_count.max(0) as u64,
                }
            })
        })
        .collect::<Vec<_>>();

    if poll.multiple != 0 {
        object["anyOf"] = serde_json::Value::Array(rendered_options);
    } else {
        object["oneOf"] = serde_json::Value::Array(rendered_options);
    }
}

fn build_activitypub_delete(
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<serde_json::Value> {
    build_activitypub_delete_with_published_at(config, account, status, &now_iso_string()?)
}

fn build_activitypub_delete_with_published_at(
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
    published_at: &str,
) -> Result<serde_json::Value> {
    let note_id = local_status_ap_id(config, account, status);
    let audiences = activitypub_audiences(config, &account.username, &status.visibility);
    Ok(serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "type": "Delete",
        "id": format!("{note_id}#delete"),
        "actor": actor_url(config, &account.username),
        "published": published_at,
        "to": audiences.0,
        "cc": audiences.1,
        "object": note_id,
    }))
}

fn local_status_ap_id(config: &AppConfig, account: &LocalAccount, status: &StatusRow) -> String {
    status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, &account.username),
            status.id
        )
    })
}

fn activitypub_audiences(
    config: &AppConfig,
    username: &str,
    visibility: &str,
) -> (serde_json::Value, serde_json::Value) {
    let public = serde_json::json!(["https://www.w3.org/ns/activitystreams#Public"]);
    let followers = serde_json::json!([format!("{}/followers", actor_url(config, username))]);

    match visibility {
        "unlisted" => (followers, public),
        _ => (public, followers),
    }
}

fn is_public_activitypub_visibility(visibility: &str) -> bool {
    matches!(visibility, "public" | "unlisted")
}

fn generate_entity_id(byte_len: usize) -> Result<String> {
    let global = js_sys::global()
        .dyn_into::<WorkerGlobalScope>()
        .map_err(|_| Error::RustError("failed to access WorkerGlobalScope".to_owned()))?;
    let mut bytes = vec![0u8; byte_len];
    global
        .crypto()
        .map_err(Error::from)?
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(Error::from)?;

    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

async fn send_signed_activity(
    config: &AppConfig,
    account: &LocalAccount,
    inbox_url: &str,
    payload_json: &str,
) -> Result<()> {
    let (host, path_and_query) = parse_http_url_parts(inbox_url)?;
    let date = now_http_date_string()?;
    let digest = sha256_http_digest(payload_json.as_bytes()).await?;
    let signing_string = format!(
        "(request-target): post {path_and_query}\nhost: {host}\ndate: {date}\ndigest: {digest}"
    );
    let signature =
        sign_http_signature(&account.private_key_jwk, signing_string.as_bytes()).await?;

    let headers = Headers::new();
    headers.set("Accept", "application/activity+json")?;
    headers.set("Content-Type", "application/activity+json")?;
    headers.set("Date", &date)?;
    headers.set("Digest", &digest)?;
    headers.set(
        "Signature",
        &format!(
            "keyId=\"{}\",algorithm=\"rsa-sha256\",headers=\"(request-target) host date digest\",signature=\"{}\"",
            public_key_id(config, &account.username),
            signature
        ),
    )?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(JsValue::from_str(payload_json)));

    let request = Request::new_with_init(inbox_url, &init)?;
    let response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 == 2 {
        Ok(())
    } else {
        Err(Error::RustError(format!(
            "remote inbox rejected activity with HTTP {}",
            response.status_code()
        )))
    }
}

async fn verify_incoming_activitypub_request(
    req: &Request,
    db: &D1Database,
    body: &[u8],
    activity: &serde_json::Value,
) -> Result<RemoteActorProfile> {
    let actor_uri = extract_activity_actor_uri(activity)?;
    let signature_header = req
        .headers()
        .get("Signature")?
        .ok_or_else(|| Error::RustError("missing Signature header".to_owned()))?;
    let parsed_signature = parse_signature_header(&signature_header)?;
    let signing_string = build_signature_signing_string(req, req.headers(), &parsed_signature)?;

    validate_request_date(req.headers())?;
    validate_request_digest(req.headers(), body).await?;

    if let Some(remote_actor) =
        find_cached_remote_actor_profile_by_actor_uri(db, &actor_uri).await?
        && cached_remote_actor_matches_key(&remote_actor, &parsed_signature.key_id, &actor_uri)
        && verify_http_signature_bytes(
            &remote_actor.public_key_pem,
            signing_string.as_bytes(),
            &parsed_signature.signature,
        )
        .await
        .is_ok()
    {
        return Ok(remote_actor);
    }

    let remote_actor = fetch_remote_actor_profile(&actor_uri).await?;
    if !cached_remote_actor_matches_key(&remote_actor, &parsed_signature.key_id, &actor_uri) {
        return Err(Error::RustError(
            "Signature keyId did not match activity actor".to_owned(),
        ));
    }
    verify_http_signature_bytes(
        &remote_actor.public_key_pem,
        signing_string.as_bytes(),
        &parsed_signature.signature,
    )
    .await?;
    upsert_remote_actor(db, &remote_actor).await?;

    Ok(remote_actor)
}

fn cached_remote_actor_matches_key(
    remote_actor: &RemoteActorProfile,
    key_id: &str,
    actor_uri: &str,
) -> bool {
    if !key_id_matches_actor(key_id, actor_uri, &remote_actor.actor_uri) {
        return false;
    }
    remote_actor.public_key_id.is_empty() || key_id == remote_actor.public_key_id
}

fn extract_activity_actor_uri(activity: &serde_json::Value) -> Result<String> {
    activity
        .get("actor")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| Error::RustError("activity is missing actor".to_owned()))
}

fn inbox_activity_id(activity: &serde_json::Value) -> Option<String> {
    activity
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_signature_header(header: &str) -> Result<ParsedSignatureHeader> {
    let mut key_id = None;
    let mut headers = None;
    let mut signature = None;

    for part in header.split(',') {
        let mut segments = part.trim().splitn(2, '=');
        let Some(name) = segments.next() else {
            continue;
        };
        let Some(raw_value) = segments.next() else {
            continue;
        };
        let value = raw_value.trim().trim_matches('"');

        match name.trim() {
            "keyId" => key_id = Some(value.to_owned()),
            "headers" => {
                headers = Some(
                    value
                        .split_whitespace()
                        .map(|entry| entry.to_ascii_lowercase())
                        .collect::<Vec<_>>(),
                )
            }
            "signature" => {
                signature = Some(STANDARD.decode(value).map_err(|error| {
                    Error::RustError(format!("invalid Signature header encoding: {error}"))
                })?)
            }
            _ => {}
        }
    }

    Ok(ParsedSignatureHeader {
        key_id: key_id.ok_or_else(|| Error::RustError("Signature keyId missing".to_owned()))?,
        headers: headers.ok_or_else(|| Error::RustError("Signature headers missing".to_owned()))?,
        signature: signature
            .ok_or_else(|| Error::RustError("Signature value missing".to_owned()))?,
    })
}

fn key_id_matches_actor(key_id: &str, raw_actor_uri: &str, canonical_actor_uri: &str) -> bool {
    let Ok(key_url) = parse_remote_http_url(key_id) else {
        return false;
    };
    let mut key_actor = key_url.clone();
    key_actor.set_fragment(None);

    key_actor.as_str() == raw_actor_uri || key_actor.as_str() == canonical_actor_uri
}

fn validate_request_date(headers: &Headers) -> Result<()> {
    let date = headers
        .get("Date")?
        .ok_or_else(|| Error::RustError("missing Date header".to_owned()))?;
    let parsed = js_sys::Date::parse(&date);
    if parsed.is_nan() {
        return Err(Error::RustError("invalid Date header".to_owned()));
    }

    let skew_ms = (js_sys::Date::now() - parsed).abs();
    if skew_ms > 12.0 * 60.0 * 60.0 * 1000.0 {
        return Err(Error::RustError(
            "Date header outside allowed skew".to_owned(),
        ));
    }

    Ok(())
}

async fn validate_request_digest(headers: &Headers, body: &[u8]) -> Result<()> {
    let digest = headers
        .get("Digest")?
        .ok_or_else(|| Error::RustError("missing Digest header".to_owned()))?;
    let (algorithm, value) = digest
        .split_once('=')
        .ok_or_else(|| Error::RustError("invalid Digest header".to_owned()))?;
    if !algorithm.eq_ignore_ascii_case("sha-256") {
        return Err(Error::RustError("unsupported Digest algorithm".to_owned()));
    }

    let expected = sha256_http_digest(body).await?;
    let expected_value = expected
        .split_once('=')
        .map(|(_, value)| value)
        .unwrap_or_default();
    if value != expected_value {
        return Err(Error::RustError("Digest header mismatch".to_owned()));
    }

    Ok(())
}

fn build_signature_signing_string(
    req: &Request,
    headers: &Headers,
    signature: &ParsedSignatureHeader,
) -> Result<String> {
    let url = parse_remote_http_url(req.url()?.as_str())?;
    let path_and_query = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let mut lines = Vec::with_capacity(signature.headers.len());

    for header_name in &signature.headers {
        let line = if header_name == "(request-target)" {
            format!(
                "(request-target): {} {}",
                req.method().as_ref().to_ascii_lowercase(),
                path_and_query
            )
        } else {
            let value = headers
                .get(header_name)?
                .ok_or_else(|| Error::RustError(format!("missing signed header {header_name}")))?;
            format!("{header_name}: {value}")
        };
        lines.push(line);
    }

    Ok(lines.join("\n"))
}

async fn verify_http_signature_bytes(
    public_key_pem: &str,
    data: &[u8],
    signature: &[u8],
) -> Result<()> {
    let subtle = subtle_crypto()?;
    let public_key = import_public_verification_key(&subtle, public_key_pem).await?;
    let verify_algorithm = Algorithm::new("RSASSA-PKCS1-v1_5");
    let verify_algorithm: Object = verify_algorithm.into();
    let signature = Uint8Array::from(signature);
    let data = Uint8Array::from(data);

    let verified = JsFuture::from(
        subtle.verify_with_object_and_buffer_source_and_buffer_source(
            &verify_algorithm,
            &public_key,
            signature.as_ref(),
            data.as_ref(),
        )?,
    )
    .await?
    .as_bool()
    .unwrap_or(false);

    if verified {
        Ok(())
    } else {
        Err(Error::RustError(
            "ActivityPub signature verification failed".to_owned(),
        ))
    }
}

async fn import_public_verification_key(
    subtle: &web_sys::SubtleCrypto,
    public_key_pem: &str,
) -> Result<CryptoKey> {
    let public_key_der = decode_public_key_pem(public_key_pem)?;
    let import_params = RsaHashedImportParams::new_with_str("SHA-256");
    let import_algorithm: Object = import_params.into();
    Reflect::set(
        &import_algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )
    .map_err(Error::from)?;

    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("verify"));

    JsFuture::from(subtle.import_key_with_object(
        "spki",
        Uint8Array::from(public_key_der.as_slice()).as_ref(),
        &import_algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<CryptoKey>()
    .map_err(|_| Error::RustError("failed to import actor public key".to_owned()))
}

fn decode_public_key_pem(public_key_pem: &str) -> Result<Vec<u8>> {
    let encoded = public_key_pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>();
    STANDARD
        .decode(encoded)
        .map_err(|error| Error::RustError(format!("invalid public key PEM: {error}")))
}

fn build_accept_activity(
    config: &AppConfig,
    account: &LocalAccount,
    follow_activity: &serde_json::Value,
    remote_actor_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/accepts/{}", generate_entity_id(12)?),
        "type": "Accept",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": follow_activity,
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Accept activity: {error}")))
}

fn build_activitypub_actor_document(
    config: &AppConfig,
    account: &LocalAccount,
) -> ActivityPubActorResponse {
    let actor_url = actor_url(config, &account.username);
    let public_key_id = public_key_id(config, &account.username);
    let icon = account
        .avatar_object_key
        .as_ref()
        .zip(account.avatar_content_type.as_ref())
        .map(|(object_key, content_type)| ActivityPubImage {
            image_type: "Image",
            media_type: content_type.clone(),
            url: media_object_url(config, object_key),
        });
    let image = account
        .header_object_key
        .as_ref()
        .zip(account.header_content_type.as_ref())
        .map(|(object_key, content_type)| ActivityPubImage {
            image_type: "Image",
            media_type: content_type.clone(),
            url: media_object_url(config, object_key),
        });

    ActivityPubActorResponse {
        context: vec![
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ],
        id: actor_url.clone(),
        actor_type: "Person",
        preferred_username: account.username.clone(),
        name: account.display_name.clone(),
        summary: account.bio_html.clone(),
        inbox: format!("{actor_url}/inbox"),
        outbox: format!("{actor_url}/outbox"),
        followers: format!("{actor_url}/followers"),
        following: format!("{actor_url}/following"),
        url: actor_url.clone(),
        endpoints: ActivityPubActorEndpoints {
            shared_inbox: shared_inbox_url(config),
        },
        icon,
        image,
        attachment: activitypub_profile_attachments(&account.fields),
        public_key: ActivityPubPublicKey {
            id: public_key_id,
            owner: actor_url.clone(),
            public_key_pem: account.public_key_pem.clone(),
        },
        manually_approves_followers: false,
        discoverable: account.discoverable,
        published: account.created_at.clone(),
    }
}

fn build_update_person_activity_with_id(
    config: &AppConfig,
    account: &LocalAccount,
    activity_id: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Update",
        "actor": actor,
        "to": ["https://www.w3.org/ns/activitystreams#Public"],
        "cc": [format!("{}/followers", actor_url(config, &account.username))],
        "object": build_activitypub_actor_document(config, account),
    });

    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Update activity: {error}")))
}

fn build_update_person_activity(config: &AppConfig, account: &LocalAccount) -> Result<String> {
    let actor = actor_url(config, &account.username);
    build_update_person_activity_with_id(
        config,
        account,
        &format!("{actor}/updates/{}", generate_entity_id(12)?),
    )
}

fn build_status_update_activity_with_id(
    config: &AppConfig,
    account: &LocalAccount,
    object: serde_json::Value,
    activity_id: &str,
    published_at: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let to = object
        .get("to")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let cc = object
        .get("cc")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Update",
        "actor": actor,
        "published": published_at,
        "to": to,
        "cc": cc,
        "object": object,
    });

    serde_json::to_string(&activity).map_err(|error| {
        Error::RustError(format!(
            "failed to serialize status Update activity: {error}"
        ))
    })
}

async fn build_status_update_activity(
    db: &D1Database,
    config: &AppConfig,
    account: &LocalAccount,
    status: &StatusRow,
) -> Result<String> {
    let object = build_activitypub_note(db, config, account, status, false).await?;
    let object_id = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("activitypub status object id missing".to_owned()))?
        .to_owned();
    build_status_update_activity_with_id(
        config,
        account,
        object,
        &format!("{object_id}/updates/{}", generate_entity_id(12)?),
        &now_iso_string()?,
    )
}

fn build_follow_activity(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let follow_activity_id = format!("{actor}/follows/{}", generate_entity_id(12)?);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": follow_activity_id,
        "type": "Follow",
        "actor": actor,
        "object": remote_actor_uri,
        "to": [remote_actor_uri],
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Follow activity: {error}"))
        })?,
    ))
}

fn build_undo_follow_activity(
    config: &AppConfig,
    account: &LocalAccount,
    follow_activity_id: &str,
    remote_actor_uri: &str,
) -> Result<String> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/undo/{}", generate_entity_id(12)?),
        "type": "Undo",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": {
            "id": follow_activity_id,
            "type": "Follow",
            "actor": actor,
            "object": remote_actor_uri,
        }
    });
    serde_json::to_string(&activity)
        .map_err(|error| Error::RustError(format!("failed to serialize Undo activity: {error}")))
}

fn build_like_activity(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
    object_uri: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let activity_id = format!("{actor}/likes/{}", generate_entity_id(12)?);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Like",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": object_uri,
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Like activity: {error}"))
        })?,
    ))
}

fn build_undo_like_activity(
    config: &AppConfig,
    account: &LocalAccount,
    like_activity_id: &str,
    remote_actor_uri: &str,
    object_uri: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/undo/{}", generate_entity_id(12)?),
        "type": "Undo",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": {
            "id": like_activity_id,
            "type": "Like",
            "actor": actor,
            "object": object_uri,
        }
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Undo Like activity: {error}"))
        })?,
    ))
}

fn build_announce_activity(
    config: &AppConfig,
    account: &LocalAccount,
    object_uri: &str,
    visibility: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let audiences = activitypub_audiences(config, &account.username, visibility);
    let activity_id = format!("{actor}/announces/{}", generate_entity_id(12)?);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Announce",
        "actor": actor,
        "published": now_iso_string()?,
        "to": audiences.0,
        "cc": audiences.1,
        "object": object_uri,
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize Announce activity: {error}"))
        })?,
    ))
}

fn build_poll_vote_activity(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
    question_uri: &str,
    option_title: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let vote_id = format!("{actor}/votes/{}", generate_entity_id(12)?);
    let activity_id = format!("{vote_id}/activity");
    build_poll_vote_activity_with_ids(
        config,
        account,
        remote_actor_uri,
        question_uri,
        option_title,
        &vote_id,
        &activity_id,
    )
}

fn build_poll_vote_activity_with_ids(
    config: &AppConfig,
    account: &LocalAccount,
    remote_actor_uri: &str,
    question_uri: &str,
    option_title: &str,
    vote_id: &str,
    activity_id: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": activity_id,
        "type": "Create",
        "to": [remote_actor_uri],
        "actor": actor,
        "object": {
            "id": vote_id,
            "type": "Note",
            "name": option_title,
            "attributedTo": actor_url(config, &account.username),
            "to": [remote_actor_uri],
            "inReplyTo": question_uri,
        }
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!("failed to serialize poll vote activity: {error}"))
        })?,
    ))
}

fn build_undo_announce_activity(
    config: &AppConfig,
    account: &LocalAccount,
    announce_activity_id: &str,
    remote_actor_uri: &str,
    object_uri: &str,
    visibility: &str,
) -> Result<(String, String)> {
    let actor = actor_url(config, &account.username);
    let audiences = activitypub_audiences(config, &account.username, visibility);
    let activity = serde_json::json!({
        "@context": "https://www.w3.org/ns/activitystreams",
        "id": format!("{actor}/undo/{}", generate_entity_id(12)?),
        "type": "Undo",
        "actor": actor,
        "to": [remote_actor_uri],
        "object": {
            "id": announce_activity_id,
            "type": "Announce",
            "actor": actor,
            "to": audiences.0,
            "cc": audiences.1,
            "object": object_uri,
        }
    });
    Ok((
        activity["id"].as_str().unwrap_or_default().to_owned(),
        serde_json::to_string(&activity).map_err(|error| {
            Error::RustError(format!(
                "failed to serialize Undo Announce activity: {error}"
            ))
        })?,
    ))
}

async fn fetch_remote_account_profile_by_handle(
    handle: &AccountHandle,
) -> Result<RemoteActorProfile> {
    let domain = handle
        .domain
        .as_deref()
        .ok_or_else(|| Error::RustError("remote handle is missing domain".to_owned()))?;
    let resource = format!("acct:{}@{}", handle.username, domain);
    let encoded_resource =
        url::form_urlencoded::byte_serialize(resource.as_bytes()).collect::<String>();
    let webfinger_url = format!(
        "https://{}/.well-known/webfinger?resource={}",
        domain, encoded_resource
    );
    let webfinger_url = parse_remote_http_url(&webfinger_url)?;
    validate_remote_fetch_url(&webfinger_url).await?;

    let request = Request::new(webfinger_url.as_str(), Method::Get)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(Error::RustError(format!(
            "failed to resolve remote account {}@{}: HTTP {}",
            handle.username,
            domain,
            response.status_code()
        )));
    }

    let webfinger: serde_json::Value = response.json().await?;
    let actor_uri = webfinger
        .get("links")
        .and_then(serde_json::Value::as_array)
        .and_then(|links| {
            links.iter().find_map(|link| {
                let rel = link.get("rel").and_then(serde_json::Value::as_str)?;
                let href = link.get("href").and_then(serde_json::Value::as_str)?;
                (rel == "self").then_some(href)
            })
        })
        .ok_or_else(|| {
            Error::RustError("webfinger response did not include a self link".to_owned())
        })?;

    fetch_remote_actor_profile(actor_uri).await
}

async fn fetch_remote_actor_profile(actor_uri: &str) -> Result<RemoteActorProfile> {
    let actor_url = parse_remote_http_url(actor_uri)?;
    let actor = fetch_remote_activitypub_document(actor_url.as_str()).await?;
    let profile = parse_remote_actor_profile_document(&actor, actor_uri)?;
    validate_remote_actor_profile_urls(&profile).await?;
    Ok(profile)
}

fn parse_remote_actor_profile_document(
    actor: &serde_json::Value,
    fallback_actor_uri: &str,
) -> Result<RemoteActorProfile> {
    let canonical_actor_uri = actor
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(fallback_actor_uri)
        .to_owned();
    let actor_url = parse_remote_http_url(&canonical_actor_uri)?;
    let inbox_uri = actor
        .get("inbox")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::RustError("remote actor document is missing inbox".to_owned()))?
        .to_owned();
    let shared_inbox_uri = actor
        .get("endpoints")
        .and_then(|value| value.get("sharedInbox"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned);
    let username = actor
        .get("preferredUsername")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| {
            actor_url
                .path_segments()
                .and_then(|segments| segments.last())
                .unwrap_or("remote")
        })
        .to_ascii_lowercase();
    let domain = actor_url
        .host_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let public_key_id = actor
        .get("publicKey")
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::RustError("remote actor document is missing publicKey.id".to_owned())
        })?
        .to_owned();
    let public_key_pem = actor
        .get("publicKey")
        .and_then(|value| value.get("publicKeyPem"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::RustError("remote actor document is missing publicKey.publicKeyPem".to_owned())
        })?
        .to_owned();

    Ok(RemoteActorProfile {
        actor_uri: canonical_actor_uri,
        username,
        domain,
        inbox_uri,
        shared_inbox_uri,
        public_key_id,
        public_key_pem,
        display_name: actor
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        summary_html: actor
            .get("summary")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        profile_url: actor.get("url").and_then(extract_remote_profile_url),
        avatar_url: extract_remote_profile_media_url(actor.get("icon")),
        header_url: extract_remote_profile_media_url(actor.get("image")),
    })
}

async fn validate_remote_actor_profile_urls(profile: &RemoteActorProfile) -> Result<()> {
    validate_remote_fetch_url(&parse_remote_http_url(&profile.actor_uri)?).await?;
    validate_remote_fetch_url(&parse_remote_http_url(&profile.inbox_uri)?).await?;
    if let Some(shared_inbox_uri) = profile.shared_inbox_uri.as_deref() {
        validate_remote_fetch_url(&parse_remote_http_url(shared_inbox_uri)?).await?;
    }
    validate_remote_fetch_url(&parse_remote_http_url(&profile.public_key_id)?).await?;
    Ok(())
}

async fn fetch_remote_activitypub_document(url: &str) -> Result<serde_json::Value> {
    let parsed = parse_remote_http_url(url)?;
    validate_remote_fetch_url(&parsed).await?;

    let headers = Headers::new();
    headers.set(
        "Accept",
        "application/activity+json, application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"",
    )?;

    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let request = Request::new_with_init(parsed.as_str(), &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(Error::RustError(format!(
            "failed to fetch remote activitypub document {}: HTTP {}",
            url,
            response.status_code()
        )));
    }

    response.json().await
}

fn is_supported_remote_status_object_type(value: Option<&str>) -> bool {
    matches!(value, Some("Note" | "Question"))
}

fn extract_remote_note_object(document: &serde_json::Value) -> Option<&serde_json::Value> {
    if is_supported_remote_status_object_type(
        document.get("type").and_then(serde_json::Value::as_str),
    ) {
        return Some(document);
    }

    let object = document.get("object")?;
    if is_supported_remote_status_object_type(
        object.get("type").and_then(serde_json::Value::as_str),
    ) {
        Some(object)
    } else {
        None
    }
}

fn extract_remote_poll_draft(object: &serde_json::Value) -> Option<RemotePollDraft> {
    let (multiple, entries) =
        if let Some(entries) = object.get("anyOf").and_then(serde_json::Value::as_array) {
            (true, entries)
        } else if let Some(entries) = object.get("oneOf").and_then(serde_json::Value::as_array) {
            (false, entries)
        } else {
            return None;
        };

    let options = entries
        .iter()
        .filter_map(extract_remote_poll_option_draft)
        .collect::<Vec<_>>();
    if options.len() < 2 {
        return None;
    }

    let votes_count = options.iter().map(|option| option.votes_count).sum::<u64>();
    let expires_at = object
        .get("closed")
        .and_then(serde_json::Value::as_str)
        .or_else(|| object.get("endTime").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned)?;

    Some(RemotePollDraft {
        multiple,
        expires_at: Some(expires_at.clone()),
        voters_count: object
            .get("votersCount")
            .and_then(serde_json::Value::as_u64),
        votes_count,
        expired: object.get("closed").is_some(),
        options,
    })
}

fn extract_remote_poll_option_draft(value: &serde_json::Value) -> Option<RemotePollOptionDraft> {
    let title = value
        .get("name")
        .and_then(serde_json::Value::as_str)?
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }

    Some(RemotePollOptionDraft {
        title,
        votes_count: value
            .pointer("/replies/totalItems")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    })
}

fn extract_remote_profile_url(value: &serde_json::Value) -> Option<String> {
    extract_remote_profile_media_url(Some(value))
}

fn extract_remote_profile_media_url(value: Option<&serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::String(url) => normalize_remote_media_url(url),
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|entry| extract_remote_profile_media_url(Some(entry))),
        serde_json::Value::Object(map) => map
            .get("url")
            .and_then(|entry| extract_remote_profile_media_url(Some(entry)))
            .or_else(|| {
                map.get("href")
                    .and_then(|entry| extract_remote_profile_media_url(Some(entry)))
            }),
        _ => None,
    }
}

fn normalize_remote_media_url(url: &str) -> Option<String> {
    parse_remote_http_url(url).ok().map(Into::into)
}

fn parse_remote_http_url(url: &str) -> Result<Url> {
    let parsed = Url::parse(url.trim())
        .map_err(|error| Error::RustError(format!("invalid remote URL {url}: {error}")))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        scheme => Err(Error::RustError(format!(
            "unsupported remote URL scheme {scheme}"
        ))),
    }
}

async fn validate_remote_fetch_url(url: &Url) -> Result<()> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::RustError(
            "remote URL must not include user info".to_owned(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::RustError("remote URL must include host".to_owned()))?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(Error::RustError("localhost is not allowed".to_owned()));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_blocked_ip_address(ip)
    {
        return Err(Error::RustError(
            "private or loopback IPs are not allowed".to_owned(),
        ));
    }
    if host.parse::<IpAddr>().is_err() {
        validate_remote_hostname_resolution(&host).await?;
    }

    Ok(())
}

async fn validate_remote_hostname_resolution(host: &str) -> Result<()> {
    let mut resolved = Vec::new();
    resolved.extend(resolve_dns_json_ips(host, "A").await?);
    resolved.extend(resolve_dns_json_ips(host, "AAAA").await?);
    if resolved.is_empty() {
        return Err(Error::RustError(format!(
            "remote host {host} did not resolve to any public A/AAAA records"
        )));
    }
    if resolved.iter().any(|ip| is_blocked_ip_address(*ip)) {
        return Err(Error::RustError(format!(
            "remote host {host} resolved to a blocked IP range"
        )));
    }

    Ok(())
}

async fn resolve_dns_json_ips(host: &str, record_type: &str) -> Result<Vec<IpAddr>> {
    let headers = Headers::new();
    headers.set("Accept", "application/dns-json")?;
    let mut init = RequestInit::new();
    init.with_method(Method::Get).with_headers(headers);
    let encoded_host = url::form_urlencoded::byte_serialize(host.as_bytes()).collect::<String>();
    let url =
        format!("https://cloudflare-dns.com/dns-query?name={encoded_host}&type={record_type}");
    let request = Request::new_with_init(&url, &init)?;
    let mut response = Fetch::Request(request).send().await?;
    if response.status_code() / 100 != 2 {
        return Err(Error::RustError(format!(
            "DNS resolution failed for {host}: HTTP {}",
            response.status_code()
        )));
    }

    let body: DnsJsonResponse = response.json().await?;
    if body.status != 0 {
        return Err(Error::RustError(format!(
            "DNS resolution failed for {host}: response status {}",
            body.status
        )));
    }

    Ok(body
        .answer
        .unwrap_or_default()
        .into_iter()
        .filter_map(|answer| answer.data.parse::<IpAddr>().ok())
        .collect())
}

fn is_blocked_ip_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_unspecified()
                || v6.is_multicast()
        }
    }
}

fn extract_inbox_target_username(
    config: &AppConfig,
    activity: &serde_json::Value,
) -> Option<String> {
    match activity.get("type").and_then(serde_json::Value::as_str) {
        Some("Follow") => activity_object_id(activity.get("object"))
            .and_then(|actor_uri| local_username_from_actor_uri(config, actor_uri)),
        Some("Accept") | Some("Reject") => activity
            .get("object")
            .and_then(|object| object.get("actor"))
            .and_then(serde_json::Value::as_str)
            .and_then(|actor_uri| local_username_from_actor_uri(config, actor_uri)),
        Some("Undo") => activity
            .get("object")
            .and_then(|object| object.get("object"))
            .and_then(|object| {
                activity_object_id(Some(object))
                    .and_then(|uri| {
                        local_username_from_actor_uri(config, uri)
                            .or_else(|| local_username_from_status_uri(config, uri))
                    })
                    .or_else(|| {
                        object
                            .get("inReplyTo")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|uri| local_username_from_status_uri(config, uri))
                    })
            }),
        Some("Like") | Some("Announce") => activity
            .get("object")
            .and_then(|object| activity_object_id(Some(object)))
            .and_then(|uri| local_username_from_status_uri(config, uri)),
        Some("Create") | Some("Update") => first_local_audience_username(config, activity),
        _ => None,
    }
}

fn activity_object_id(value: Option<&serde_json::Value>) -> Option<&str> {
    match value {
        Some(serde_json::Value::String(value)) => Some(value.as_str()),
        Some(serde_json::Value::Object(map)) => map.get("id").and_then(serde_json::Value::as_str),
        _ => None,
    }
}

fn first_local_audience_username(
    config: &AppConfig,
    activity: &serde_json::Value,
) -> Option<String> {
    for audience in activity_audience_uris(activity) {
        if let Some(username) = local_username_from_audience_uri(config, &audience) {
            return Some(username);
        }
    }

    None
}

fn activity_audience_uris(activity: &serde_json::Value) -> Vec<String> {
    let mut audiences = Vec::new();
    for key in ["to", "cc"] {
        collect_activitypub_uris(activity.get(key), &mut audiences);
        collect_activitypub_uris(
            activity.get("object").and_then(|object| object.get(key)),
            &mut audiences,
        );
    }
    audiences
}

fn collect_activitypub_uris(value: Option<&serde_json::Value>, audiences: &mut Vec<String>) {
    match value {
        Some(serde_json::Value::String(uri)) => audiences.push(uri.clone()),
        Some(serde_json::Value::Array(values)) => {
            for entry in values {
                collect_activitypub_uris(Some(entry), audiences);
            }
        }
        Some(serde_json::Value::Object(map)) => {
            if let Some(uri) = map.get("id").and_then(serde_json::Value::as_str) {
                audiences.push(uri.to_owned());
            }
        }
        _ => {}
    }
}

fn local_username_from_audience_uri(config: &AppConfig, uri: &str) -> Option<String> {
    if let Some(stripped) = uri.strip_suffix("/followers") {
        return local_username_from_actor_uri(config, stripped);
    }

    local_username_from_actor_uri(config, uri)
}

fn note_targets_account_or_followers(
    object: &serde_json::Value,
    account: &LocalAccount,
    config: &AppConfig,
) -> bool {
    let actor = actor_url(config, &account.username);
    let followers = format!("{actor}/followers");
    activity_audience_uris(&serde_json::json!({ "object": object }))
        .into_iter()
        .any(|audience| audience == actor || audience == followers)
}

fn visibility_from_activitypub_object(object: &serde_json::Value) -> String {
    let to = object.get("to");
    let cc = object.get("cc");

    if contains_public_audience(to) {
        "public".to_owned()
    } else if contains_public_audience(cc) {
        "unlisted".to_owned()
    } else {
        "private".to_owned()
    }
}

fn contains_public_audience(value: Option<&serde_json::Value>) -> bool {
    match value {
        Some(serde_json::Value::String(value)) => {
            value == "https://www.w3.org/ns/activitystreams#Public"
        }
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .any(|value| contains_public_audience(Some(value))),
        Some(serde_json::Value::Object(map)) => map
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(|value| value == "https://www.w3.org/ns/activitystreams#Public")
            .unwrap_or(false),
        _ => false,
    }
}

fn local_username_from_actor_uri(config: &AppConfig, actor_uri: &str) -> Option<String> {
    let parsed = Url::parse(actor_uri).ok()?;
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host != instance_host(config) {
        return None;
    }

    let mut segments = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty());
    match (segments.next(), segments.next(), segments.next()) {
        (Some("users"), Some(username), None) => Some(username.to_ascii_lowercase()),
        _ => None,
    }
}

fn follow_targets_local_actor(object: Option<&serde_json::Value>, local_actor_uri: &str) -> bool {
    match object {
        Some(serde_json::Value::String(value)) => value == local_actor_uri,
        Some(serde_json::Value::Object(map)) => map
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(|value| value == local_actor_uri)
            .unwrap_or(false),
        _ => false,
    }
}

fn is_follow_undo(
    object: Option<&serde_json::Value>,
    actor_uri: &str,
    canonical_actor_uri: &str,
) -> bool {
    match object {
        Some(serde_json::Value::String(_)) => true,
        Some(serde_json::Value::Object(map)) => {
            let is_follow = map
                .get("type")
                .and_then(serde_json::Value::as_str)
                .map(|value| value == "Follow")
                .unwrap_or(false);
            let object_actor = map
                .get("actor")
                .and_then(serde_json::Value::as_str)
                .map(|value| value == actor_uri || value == canonical_actor_uri)
                .unwrap_or(false);
            is_follow && object_actor
        }
        _ => false,
    }
}

fn parse_http_url_parts(url: &str) -> Result<(String, String)> {
    let scheme_separator = url
        .find("://")
        .ok_or_else(|| Error::RustError("delivery target URL must include a scheme".to_owned()))?;
    let rest = &url[(scheme_separator + 3)..];
    let separator = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..separator];
    if authority.is_empty() {
        return Err(Error::RustError(
            "delivery target URL must include a host".to_owned(),
        ));
    }

    let path_and_query = match rest.get(separator..) {
        Some(fragment) if fragment.starts_with('/') => fragment
            .split('#')
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or("/"),
        Some(fragment) if fragment.starts_with('?') => {
            return Ok((authority.to_owned(), format!("/{fragment}")));
        }
        _ => "/",
    };

    Ok((authority.to_owned(), path_and_query.to_owned()))
}

fn now_http_date_string() -> Result<String> {
    js_sys::Date::new_0()
        .to_utc_string()
        .as_string()
        .ok_or_else(|| Error::RustError("failed to build HTTP date".to_owned()))
}

async fn sha256_http_digest(payload: &[u8]) -> Result<String> {
    let subtle = subtle_crypto()?;
    let digest = JsFuture::from(subtle.digest_with_str_and_u8_array("SHA-256", payload)?).await?;
    let digest = Uint8Array::new(&digest).to_vec();
    Ok(format!("sha-256={}", STANDARD.encode(digest)))
}

async fn sign_http_signature(private_key_jwk: &str, payload: &[u8]) -> Result<String> {
    let subtle = subtle_crypto()?;
    let key = import_private_signing_key(&subtle, private_key_jwk).await?;
    let signature =
        JsFuture::from(subtle.sign_with_str_and_u8_array("RSASSA-PKCS1-v1_5", &key, payload)?)
            .await?;
    Ok(STANDARD.encode(Uint8Array::new(&signature).to_vec()))
}

async fn import_private_signing_key(
    subtle: &web_sys::SubtleCrypto,
    private_key_jwk: &str,
) -> Result<CryptoKey> {
    let jwk = js_sys::JSON::parse(private_key_jwk).map_err(Error::from)?;
    let jwk = jwk
        .dyn_into::<Object>()
        .map_err(|_| Error::RustError("failed to parse account private JWK".to_owned()))?;

    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("sign"));
    let algorithm = rsa_signing_algorithm(2048)?;

    JsFuture::from(subtle.import_key_with_object(
        "jwk",
        &jwk,
        &algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<CryptoKey>()
    .map_err(|_| Error::RustError("failed to import account private signing key".to_owned()))
}

fn delivery_retry_delay_modifier(attempt: u32) -> &'static str {
    match attempt {
        1 => "+1 minute",
        2 => "+5 minutes",
        3 => "+15 minutes",
        _ => "+60 minutes",
    }
}

fn now_iso_string() -> Result<String> {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .ok_or_else(|| Error::RustError("failed to build ISO timestamp".to_owned()))
}

fn add_seconds_to_iso_string(value: &str, seconds: u64) -> Result<String> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| Error::RustError(format!("invalid ISO timestamp {value}: {error}")))?;
    let seconds = i64::try_from(seconds)
        .map_err(|_| Error::RustError("poll expiration is too large".to_owned()))?;
    (timestamp + Duration::seconds(seconds))
        .format(&Rfc3339)
        .map_err(|error| Error::RustError(format!("failed to format ISO timestamp: {error}")))
}

fn is_iso_timestamp_in_past(value: &str) -> Result<bool> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| Error::RustError(format!("invalid ISO timestamp {value}: {error}")))?;
    let now = OffsetDateTime::parse(&now_iso_string()?, &Rfc3339)
        .map_err(|error| Error::RustError(format!("invalid current ISO timestamp: {error}")))?;
    Ok(timestamp <= now)
}

fn render_status_html(text: &str) -> String {
    let escaped = escape_html(text.trim());
    let paragraphs = escaped
        .split("\n\n")
        .map(|paragraph| paragraph.replace('\n', "<br />"))
        .map(|paragraph| format!("<p>{paragraph}</p>"))
        .collect::<Vec<_>>();

    if paragraphs.is_empty() {
        "<p></p>".to_owned()
    } else {
        paragraphs.join("")
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

async fn extract_authenticated_user(
    req: &Request,
    config: &AppConfig,
) -> Result<Option<AuthenticatedUser>> {
    let token = match req.headers().get(&config.access_jwt_header)? {
        Some(value) if !value.trim().is_empty() => value.trim().to_owned(),
        _ => return Ok(None),
    };

    if config.access_team_domain.is_empty() || config.access_audience.is_empty() {
        return Err(Error::RustError(
            "missing Cloudflare Access configuration: ACCESS_TEAM_DOMAIN and ACCESS_AUD are required"
                .to_owned(),
        ));
    }

    let claims = verify_access_jwt(&token, config).await?;
    let header_email = req
        .headers()
        .get(&config.access_email_header)?
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    let email = claims
        .email
        .map(|value| value.trim().to_ascii_lowercase())
        .or(header_email.clone())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::RustError("validated Access JWT did not include an email".to_owned())
        })?;

    if let Some(header_email) = header_email
        && header_email != email
    {
        return Err(Error::RustError(
            "Cloudflare Access email header did not match JWT email claim".to_owned(),
        ));
    }

    Ok(Some(AuthenticatedUser::cloudflare_access(email, true)))
}

async fn resolve_local_account(db: &D1Database, user: &AuthenticatedUser) -> Result<LocalAccount> {
    if let Some(account) = find_account_by_email(db, &user.email).await? {
        return ensure_account_keys(db, account).await;
    }

    let base_username = username_from_email(&user.email);
    let candidate = match find_account_by_username(db, &base_username).await? {
        Some(_) => format!("{}-{}", base_username, short_email_suffix(&user.email)),
        None => base_username,
    };

    let display_name = candidate.clone();
    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(candidate.as_str()),
        D1Type::Text(user.email.as_str()),
        D1Type::Text(display_name.as_str()),
        D1Type::Text(key_material.private_key_jwk.as_str()),
        D1Type::Text(key_material.public_key_pem.as_str()),
    ];

    db.prepare(
        "INSERT INTO accounts (
            id,
            username,
            access_email,
            display_name,
            fields_json,
            discoverable,
            private_key_jwk,
            public_key_pem,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            '[]',
            0,
            ?4,
            ?5,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    find_account_by_email(db, &user.email)
        .await?
        .ok_or_else(|| Error::RustError("failed to load provisioned account".to_owned()))
}

async fn find_authenticated_local_account(
    req: &Request,
    db: &D1Database,
    config: &AppConfig,
) -> Result<Option<LocalAccount>> {
    let Some(user) = extract_authenticated_user(req, config).await? else {
        return Ok(None);
    };

    find_account_by_email(db, &user.email).await
}

async fn ensure_account_keys(db: &D1Database, account: LocalAccount) -> Result<LocalAccount> {
    if !account.private_key_jwk.is_empty() && !account.public_key_pem.is_empty() {
        return Ok(account);
    }

    let key_material = generate_account_key_material().await?;
    let bindings = [
        D1Type::Text(key_material.private_key_jwk.as_str()),
        D1Type::Text(key_material.public_key_pem.as_str()),
        D1Type::Text(account.id.as_str()),
    ];

    db.prepare(
        "UPDATE accounts
         SET private_key_jwk = ?1,
             public_key_pem = ?2,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?3",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    find_account_by_id(db, &account.id)
        .await?
        .ok_or_else(|| Error::RustError("failed to reload account key material".to_owned()))
}

async fn find_account_by_email(db: &D1Database, email: &str) -> Result<Option<LocalAccount>> {
    let email = D1Type::Text(email);

    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE access_email = ?1
             LIMIT 1",
        )
        .bind_refs(&email)?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from))
}

async fn find_account_by_id(db: &D1Database, id: &str) -> Result<Option<LocalAccount>> {
    let id = D1Type::Text(id);

    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE id = ?1
             LIMIT 1",
        )
        .bind_refs(&id)?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from))
}

async fn find_account_by_username(db: &D1Database, username: &str) -> Result<Option<LocalAccount>> {
    let username = username.trim().to_ascii_lowercase();
    let username = D1Type::Text(username.as_str());

    let row = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE username = ?1
             LIMIT 1",
        )
        .bind_refs(&username)?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from))
}

async fn load_account_stats(db: &D1Database, account_id: &str) -> Result<AccountStats> {
    Ok(AccountStats {
        followers_count: count_remote_followers(db, account_id).await?
            + count_local_followers(db, account_id).await?,
        following_count: count_accepted_following(db, account_id).await?,
        statuses_count: count_rows(
            db,
            "SELECT COUNT(*) AS count FROM statuses WHERE account_id = ?1",
            account_id,
        )
        .await?,
    })
}

async fn apply_account_credentials_update(
    db: &D1Database,
    bucket: &Bucket,
    config: &AppConfig,
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> Result<LocalAccount> {
    let display_name = update
        .display_name
        .as_deref()
        .unwrap_or(account.display_name.as_str())
        .to_owned();
    let bio_text = update
        .note
        .as_deref()
        .unwrap_or(account.bio_text.as_str())
        .to_owned();
    let bio_html = render_status_html(&bio_text);
    let fields = update
        .fields_attributes
        .as_ref()
        .map(|fields| {
            fields
                .iter()
                .filter_map(profile_field_from_update)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| account.fields.clone());
    let fields_json = serde_json::to_string(&fields).map_err(|error| {
        Error::RustError(format!("failed to serialize account fields: {error}"))
    })?;
    let discoverable = update.discoverable.unwrap_or(account.discoverable);
    let default_post_visibility = update
        .source
        .as_ref()
        .and_then(|source| source.privacy.as_deref())
        .unwrap_or(account.default_post_visibility.as_str())
        .to_owned();
    let default_sensitive = update
        .source
        .as_ref()
        .and_then(|source| source.sensitive)
        .unwrap_or(account.default_sensitive);
    let default_language = update
        .source
        .as_ref()
        .and_then(|source| source.language.clone())
        .or_else(|| account.default_language.clone());
    let avatar_profile = match update.avatar.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };
    let header_profile = match update.header.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };
    if let Some(previous) = account.avatar_object_key.as_deref()
        && avatar_profile.is_some()
        && avatar_profile
            .as_ref()
            .map(|profile| profile.0.as_str() != previous)
            .unwrap_or(false)
    {
        bucket.delete(previous).await?;
    }
    if let Some(previous) = account.header_object_key.as_deref()
        && header_profile.is_some()
        && header_profile
            .as_ref()
            .map(|profile| profile.0.as_str() != previous)
            .unwrap_or(false)
    {
        bucket.delete(previous).await?;
    }

    let bindings = [
        D1Type::Text(display_name.as_str()),
        D1Type::Text(bio_html.as_str()),
        D1Type::Text(bio_text.as_str()),
        D1Type::Text(fields_json.as_str()),
        D1Type::Integer(if discoverable { 1 } else { 0 }),
        D1Type::Text(default_post_visibility.as_str()),
        D1Type::Integer(if default_sensitive { 1 } else { 0 }),
        match default_language.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match avatar_profile.as_ref().map(|value| value.0.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.avatar_object_key.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        match avatar_profile.as_ref().map(|value| value.1.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.avatar_content_type.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        match header_profile.as_ref().map(|value| value.0.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.header_object_key.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        match header_profile.as_ref().map(|value| value.1.as_str()) {
            Some(value) => D1Type::Text(value),
            None => match account.header_content_type.as_deref() {
                Some(value) => D1Type::Text(value),
                None => D1Type::Null,
            },
        },
        D1Type::Text(account.id.as_str()),
    ];

    db.prepare(
        "UPDATE accounts
         SET display_name = ?1,
             bio_html = ?2,
             bio_text = ?3,
             fields_json = ?4,
             discoverable = ?5,
             default_post_visibility = ?6,
             default_sensitive = ?7,
             default_language = ?8,
             avatar_object_key = ?9,
             avatar_content_type = ?10,
             header_object_key = ?11,
             header_content_type = ?12,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?13",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let updated = find_account_by_id(db, &account.id)
        .await?
        .ok_or_else(|| Error::RustError("failed to reload updated account".to_owned()))?;
    enqueue_profile_update_activities(db, config, &updated).await?;
    Ok(updated)
}

async fn store_profile_media(
    bucket: &Bucket,
    account: &LocalAccount,
    upload: &ProfileMediaUpload,
) -> Result<(String, String)> {
    let media_id = generate_entity_id(16)?;
    let object_key = format!(
        "profiles/{}/{}/{}",
        account.id, upload.object_kind, media_id
    );
    let result = bucket
        .put(&object_key, upload.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(upload.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await?;
    if result.is_none() {
        return Err(Error::RustError(format!(
            "failed to persist {} object to R2",
            upload.object_kind
        )));
    }
    Ok((object_key, upload.content_type.clone()))
}

fn account_avatar_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .avatar_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

fn account_header_url(config: &AppConfig, account: &LocalAccount) -> String {
    account
        .header_object_key
        .as_deref()
        .map(|object_key| media_object_url(config, object_key))
        .unwrap_or_default()
}

fn local_status_target_uri(status: &StatusRow) -> String {
    status
        .ap_id
        .clone()
        .unwrap_or_else(|| format!("local:{}", status.id))
}

fn local_status_identity_from_uri(config: &AppConfig, uri: &str) -> Option<(String, String)> {
    let base = instance_base_url(config);
    let expected_prefix = format!("{base}/users/");
    let canonical = uri.trim_end_matches('/');
    if !canonical.starts_with(&expected_prefix) {
        return None;
    }
    let remainder = &canonical[expected_prefix.len()..];
    let mut segments = remainder.split('/');
    let username = segments.next()?.trim();
    let statuses = segments.next()?;
    let status_id = segments.next()?.trim();
    if statuses != "statuses"
        || username.is_empty()
        || status_id.is_empty()
        || segments.next().is_some()
    {
        return None;
    }
    Some((username.to_ascii_lowercase(), status_id.to_owned()))
}

fn local_username_from_status_uri(config: &AppConfig, uri: &str) -> Option<String> {
    local_status_identity_from_uri(config, uri).map(|(username, _)| username)
}

async fn find_local_status_by_object_uri(
    db: &D1Database,
    config: &AppConfig,
    object_uri: &str,
) -> Result<Option<StatusRow>> {
    if let Some(status) = find_status_by_ap_id(db, object_uri).await? {
        return Ok(Some(status));
    }
    let Some((username, status_id)) = local_status_identity_from_uri(config, object_uri) else {
        return Ok(None);
    };
    let Some(status) = find_status_by_id(db, &status_id).await? else {
        return Ok(None);
    };
    let Some(owner) = find_account_by_id(db, &status.account_id).await? else {
        return Ok(None);
    };
    if owner.username.eq_ignore_ascii_case(&username) {
        Ok(Some(status))
    } else {
        Ok(None)
    }
}

async fn upsert_favourite_local_status(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<()> {
    let target_uri = local_status_target_uri(status);
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(target_uri.as_str()),
    ];

    db.prepare(
        "INSERT INTO favourites (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            ap_activity_id,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            NULL,
            ?3,
            NULL,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            remote_status_id = NULL,
            ap_activity_id = NULL,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn upsert_favourite_remote_status(
    db: &D1Database,
    account_id: &str,
    status: &RemoteStatusRow,
    ap_activity_id: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(status.object_uri.as_str()),
        match ap_activity_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];

    db.prepare(
        "INSERT INTO favourites (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            ap_activity_id,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            NULL,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = NULL,
            remote_status_id = excluded.remote_status_id,
            ap_activity_id = excluded.ap_activity_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_favourite_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "DELETE FROM favourites
         WHERE account_id = ?1
           AND target_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn count_local_status_favourites(db: &D1Database, status_id: &str) -> Result<u64> {
    Ok(count_rows(
        db,
        "SELECT COUNT(*) AS count FROM favourites WHERE status_id = ?1",
        status_id,
    )
    .await?
        + count_rows(
            db,
            "SELECT COUNT(*) AS count FROM remote_favourites WHERE status_id = ?1",
            status_id,
        )
        .await?)
}

async fn count_remote_status_favourites(db: &D1Database, remote_status_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count FROM favourites WHERE remote_status_id = ?1",
        remote_status_id,
    )
    .await
}

async fn is_local_status_favourited_by(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<bool> {
    is_favourite_target_for_account(db, account_id, &local_status_target_uri(status)).await
}

async fn is_remote_status_favourited_by(
    db: &D1Database,
    account_id: &str,
    remote_status_id: &str,
) -> Result<bool> {
    let remote_status_id = D1Type::Text(remote_status_id);
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM favourites
             WHERE account_id = ?1
               AND remote_status_id = ?2",
        )
        .bind_refs(&[account_id, remote_status_id])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

async fn is_favourite_target_for_account(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM favourites
             WHERE account_id = ?1
               AND target_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

async fn list_favourites_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<FavouriteEntryRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT status_id, remote_status_id, created_at
             FROM favourites
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<FavouriteEntryRow>()
}

async fn find_favourite_activity_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<Option<InteractionActivityRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "SELECT ap_activity_id
         FROM favourites
         WHERE account_id = ?1
           AND target_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<InteractionActivityRow>(None)
    .await
}

async fn upsert_bookmark_local_status(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<()> {
    let target_uri = local_status_target_uri(status);
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(target_uri.as_str()),
    ];

    db.prepare(
        "INSERT INTO bookmarks (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            NULL,
            ?3,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            remote_status_id = NULL,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn upsert_bookmark_remote_status(
    db: &D1Database,
    account_id: &str,
    status: &RemoteStatusRow,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(status.object_uri.as_str()),
    ];

    db.prepare(
        "INSERT INTO bookmarks (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            NULL,
            ?2,
            ?3,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = NULL,
            remote_status_id = excluded.remote_status_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_bookmark_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "DELETE FROM bookmarks
         WHERE account_id = ?1
           AND target_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn is_local_status_bookmarked_by(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<bool> {
    is_bookmark_target_for_account(db, account_id, &local_status_target_uri(status)).await
}

async fn is_remote_status_bookmarked_by(
    db: &D1Database,
    account_id: &str,
    remote_status_id: &str,
) -> Result<bool> {
    let remote_status_id = D1Type::Text(remote_status_id);
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM bookmarks
             WHERE account_id = ?1
               AND remote_status_id = ?2",
        )
        .bind_refs(&[account_id, remote_status_id])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

async fn is_bookmark_target_for_account(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM bookmarks
             WHERE account_id = ?1
               AND target_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

async fn list_bookmarks_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<FavouriteEntryRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT status_id, remote_status_id, created_at
             FROM bookmarks
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<FavouriteEntryRow>()
}

async fn upsert_reblog_local_status(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
    visibility: &str,
) -> Result<()> {
    let target_uri = local_status_target_uri(status);
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(target_uri.as_str()),
        D1Type::Text(visibility),
    ];

    db.prepare(
        "INSERT INTO reblogs (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            visibility,
            ap_activity_id,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            NULL,
            ?3,
            ?4,
            NULL,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            remote_status_id = NULL,
            visibility = excluded.visibility,
            ap_activity_id = NULL,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn upsert_reblog_remote_status(
    db: &D1Database,
    account_id: &str,
    status: &RemoteStatusRow,
    visibility: &str,
    ap_activity_id: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        D1Type::Text(status.id.as_str()),
        D1Type::Text(status.object_uri.as_str()),
        D1Type::Text(visibility),
        match ap_activity_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];

    db.prepare(
        "INSERT INTO reblogs (
            account_id,
            status_id,
            remote_status_id,
            target_uri,
            visibility,
            ap_activity_id,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            NULL,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_uri) DO UPDATE SET
            status_id = NULL,
            remote_status_id = excluded.remote_status_id,
            visibility = excluded.visibility,
            ap_activity_id = excluded.ap_activity_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_reblog_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "DELETE FROM reblogs
         WHERE account_id = ?1
           AND target_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn find_reblog_activity_by_target_uri(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<Option<ReblogActivityRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    db.prepare(
        "SELECT ap_activity_id, visibility
         FROM reblogs
         WHERE account_id = ?1
           AND target_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<ReblogActivityRow>(None)
    .await
}

async fn upsert_remote_favourite(
    db: &D1Database,
    remote_actor_uri: &str,
    status_id: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(remote_actor_uri),
        D1Type::Text(status_id),
        D1Type::Text(target_uri),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO remote_favourites (
            remote_actor_uri,
            status_id,
            target_uri,
            activity_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(remote_actor_uri, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            activity_uri = excluded.activity_uri,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_remote_favourite(
    db: &D1Database,
    remote_actor_uri: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    match activity_uri {
        Some(activity_uri) => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(activity_uri)];
            db.prepare(
                "DELETE FROM remote_favourites
                 WHERE remote_actor_uri = ?1
                   AND activity_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
        None => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(target_uri)];
            db.prepare(
                "DELETE FROM remote_favourites
                 WHERE remote_actor_uri = ?1
                   AND target_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
    }

    Ok(())
}

async fn upsert_remote_reblog(
    db: &D1Database,
    remote_actor_uri: &str,
    status_id: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(remote_actor_uri),
        D1Type::Text(status_id),
        D1Type::Text(target_uri),
        match activity_uri {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO remote_reblogs (
            remote_actor_uri,
            status_id,
            target_uri,
            activity_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(remote_actor_uri, target_uri) DO UPDATE SET
            status_id = excluded.status_id,
            activity_uri = excluded.activity_uri,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_remote_reblog(
    db: &D1Database,
    remote_actor_uri: &str,
    target_uri: &str,
    activity_uri: Option<&str>,
) -> Result<()> {
    match activity_uri {
        Some(activity_uri) => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(activity_uri)];
            db.prepare(
                "DELETE FROM remote_reblogs
                 WHERE remote_actor_uri = ?1
                   AND activity_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
        None => {
            let bindings = [D1Type::Text(remote_actor_uri), D1Type::Text(target_uri)];
            db.prepare(
                "DELETE FROM remote_reblogs
                 WHERE remote_actor_uri = ?1
                   AND target_uri = ?2",
            )
            .bind_refs(bindings.iter())?
            .run()
            .await?;
        }
    }

    Ok(())
}

async fn count_local_status_reblogs(db: &D1Database, status_id: &str) -> Result<u64> {
    Ok(count_rows(
        db,
        "SELECT COUNT(*) AS count FROM reblogs WHERE status_id = ?1",
        status_id,
    )
    .await?
        + count_rows(
            db,
            "SELECT COUNT(*) AS count FROM remote_reblogs WHERE status_id = ?1",
            status_id,
        )
        .await?)
}

async fn count_remote_status_reblogs(db: &D1Database, remote_status_id: &str) -> Result<u64> {
    count_rows(
        db,
        "SELECT COUNT(*) AS count FROM reblogs WHERE remote_status_id = ?1",
        remote_status_id,
    )
    .await
}

async fn is_local_status_reblogged_by(
    db: &D1Database,
    account_id: &str,
    status: &StatusRow,
) -> Result<bool> {
    is_reblog_target_for_account(db, account_id, &local_status_target_uri(status)).await
}

async fn is_remote_status_reblogged_by(
    db: &D1Database,
    account_id: &str,
    remote_status_id: &str,
) -> Result<bool> {
    let remote_status_id = D1Type::Text(remote_status_id);
    let account_id = D1Type::Text(account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM reblogs
             WHERE account_id = ?1
               AND remote_status_id = ?2",
        )
        .bind_refs(&[account_id, remote_status_id])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

async fn is_reblog_target_for_account(
    db: &D1Database,
    account_id: &str,
    target_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM reblogs
             WHERE account_id = ?1
               AND target_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

fn normalize_notification_types(values: Option<&Vec<String>>) -> Vec<String> {
    let mut values = values
        .into_iter()
        .flat_map(|entries| entries.iter())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn notification_timestamp_sort_token(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut digits = value
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.len() < 14 {
        return None;
    }
    while digits.len() < 17 {
        digits.push('0');
    }
    digits.truncate(17);
    Some(digits)
}

fn notification_sort_key(value: &str) -> String {
    notification_timestamp_sort_token(value).unwrap_or_default()
}

fn notification_type_allowed(query: &NotificationsQuery, notification_type: &str) -> bool {
    let include = normalize_notification_types(query.types.as_ref());
    let exclude = normalize_notification_types(query.exclude_types.as_ref());
    if !include.is_empty() && !include.iter().any(|value| value == notification_type) {
        return false;
    }
    !exclude.iter().any(|value| value == notification_type)
}

async fn load_dismissed_notification_ids(
    db: &D1Database,
    account_id: &str,
) -> Result<HashSet<String>> {
    let account_id = D1Type::Text(account_id);
    let result = db
        .prepare(
            "SELECT notification_id
             FROM notification_dismissals
             WHERE account_id = ?1",
        )
        .bind_refs(&account_id)?
        .all()
        .await?;

    Ok(result
        .results::<NotificationDismissalRow>()?
        .into_iter()
        .map(|row| row.notification_id)
        .collect())
}

async fn load_notification_clear_marker(
    db: &D1Database,
    account_id: &str,
) -> Result<Option<String>> {
    let account_id = D1Type::Text(account_id);
    Ok(db
        .prepare(
            "SELECT cleared_at
             FROM notification_clear_markers
             WHERE account_id = ?1
             LIMIT 1",
        )
        .bind_refs(&account_id)?
        .first::<NotificationClearMarkerRow>(None)
        .await?
        .map(|row| row.cleared_at))
}

async fn dismiss_notification_for_account(
    db: &D1Database,
    account_id: &str,
    notification_id: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(notification_id)];
    db.prepare(
        "INSERT INTO notification_dismissals (account_id, notification_id)
         VALUES (?1, ?2)
         ON CONFLICT(account_id, notification_id) DO NOTHING",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

async fn clear_notifications_for_account(db: &D1Database, account_id: &str) -> Result<()> {
    let cleared_at = now_iso_string()?;
    let bindings = [D1Type::Text(account_id), D1Type::Text(cleared_at.as_str())];
    db.prepare(
        "INSERT INTO notification_clear_markers (account_id, cleared_at)
         VALUES (?1, ?2)
         ON CONFLICT(account_id) DO UPDATE SET
             cleared_at = excluded.cleared_at",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;
    Ok(())
}

fn notification_account_matches_filter(
    filter_account_id: Option<&str>,
    local_account_id: &str,
    remote_actor_uri: Option<&str>,
) -> bool {
    match filter_account_id {
        None => true,
        Some(filter) if filter == local_account_id => true,
        Some(filter) => remote_actor_uri
            .map(remote_account_rest_id)
            .map(|value| value == filter)
            .unwrap_or(false),
    }
}

fn is_admin_account(config: &AppConfig, account: &LocalAccount) -> bool {
    config
        .admin_emails
        .iter()
        .any(|email| email == &account.access_email.to_ascii_lowercase())
}

async fn list_local_follow_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<LocalFollowNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT follower_account_id, created_at
             FROM follows
             WHERE target_account_id = ?1
               AND state = 'accepted'
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<LocalFollowNotificationRow>()
}

async fn list_admin_sign_up_notifications(
    db: &D1Database,
    admin_account_id: &str,
    limit: u32,
) -> Result<Vec<LocalAccount>> {
    let bindings = [
        D1Type::Text(admin_account_id),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE id != ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

async fn list_remote_follow_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteFollowNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT actor_uri, created_at
             FROM followers
             WHERE account_id = ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteFollowNotificationRow>()
}

async fn list_favourite_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<FavouriteNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT f.account_id, f.status_id, f.created_at
             FROM favourites f
             JOIN statuses s
               ON s.id = f.status_id
             WHERE s.account_id = ?1
               AND f.account_id != ?1
               AND f.status_id IS NOT NULL
             ORDER BY f.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<FavouriteNotificationRow>()
}

async fn list_remote_favourite_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusInteractionRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rf.remote_actor_uri, rf.status_id, rf.created_at
             FROM remote_favourites rf
             JOIN statuses s
               ON s.id = rf.status_id
             WHERE s.account_id = ?1
             ORDER BY rf.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusInteractionRow>()
}

async fn list_local_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    config: &AppConfig,
    limit: u32,
) -> Result<Vec<MentionNotificationRow>> {
    let pattern = format!("%@{}%", viewer.username.to_ascii_lowercase());
    let bindings = [
        D1Type::Text(viewer.id.as_str()),
        D1Type::Text(pattern.as_str()),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, account_id, ap_id, in_reply_to_id, content_html, text_content, spoiler_text, visibility, sensitive, language, created_at
             FROM statuses
             WHERE account_id != ?1
               AND lower(text_content) LIKE ?2
             ORDER BY created_at DESC
             LIMIT ?3",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    let mut rows = Vec::new();
    for row in result.results::<MentionNotificationRow>()? {
        if extract_mentions_from_text(&row.text_content, config)
            .into_iter()
            .any(|handle| handle.username == viewer.username)
        {
            rows.push(row);
        }
    }

    Ok(rows)
}

async fn list_remote_mention_notifications_for_account(
    db: &D1Database,
    viewer: &LocalAccount,
    config: &AppConfig,
    limit: u32,
) -> Result<Vec<RemoteMentionNotificationRow>> {
    let pattern = format!(
        "%@{}@{}%",
        viewer.username.to_ascii_lowercase(),
        instance_host(config)
    );
    let bindings = [
        D1Type::Text(pattern.as_str()),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT id, actor_uri, object_uri, url, in_reply_to_uri, content_html, spoiler_text, visibility, sensitive, language, published_at
             FROM remote_statuses
             WHERE lower(content_html) LIKE ?1
                OR lower(spoiler_text) LIKE ?1
             ORDER BY published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    let mut rows = Vec::new();
    for row in result.results::<RemoteMentionNotificationRow>()? {
        let text_content = strip_html_tags(&row.content_html);
        if extract_mentions_from_text(&text_content, config)
            .into_iter()
            .any(|handle| handle.username == viewer.username)
        {
            rows.push(row);
        }
    }

    Ok(rows)
}

async fn list_reblog_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<ReblogNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT r.account_id, r.status_id, r.created_at
             FROM reblogs r
             JOIN statuses s
               ON s.id = r.status_id
             WHERE s.account_id = ?1
               AND r.account_id != ?1
               AND r.status_id IS NOT NULL
             ORDER BY r.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<ReblogNotificationRow>()
}

async fn list_local_status_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<StatusRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT s.id, s.account_id, s.ap_id, s.in_reply_to_id, s.content_html, s.text_content, s.spoiler_text, s.visibility, s.sensitive, s.language, s.created_at
             FROM statuses s
             JOIN follows f
               ON f.target_account_id = s.account_id
              AND f.follower_account_id = ?1
              AND f.state = 'accepted'
              AND f.notify = 1
             WHERE s.account_id != ?1
               AND s.created_at >= f.updated_at
             ORDER BY s.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<StatusRow>()
}

async fn list_remote_status_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rs.id, rs.actor_uri, rs.object_uri, rs.url, rs.in_reply_to_uri, rs.content_html, rs.spoiler_text, rs.visibility, rs.sensitive, rs.language, rs.published_at
             FROM remote_statuses rs
             JOIN follows f
               ON f.target_actor_uri = rs.actor_uri
              AND f.follower_account_id = ?1
              AND f.state = 'accepted'
              AND f.notify = 1
             WHERE rs.published_at >= f.updated_at
             ORDER BY rs.published_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusNotificationRow>()
}

async fn list_poll_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<PollNotificationRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT p.id AS poll_id,
                    p.status_id,
                    s.account_id,
                    p.expires_at
             FROM status_polls p
             JOIN statuses s
               ON s.id = p.status_id
             LEFT JOIN status_poll_votes v
               ON v.poll_id = p.id
              AND v.account_id = ?1
             WHERE datetime(replace(replace(p.expires_at, 'T', ' '), 'Z', '')) <= CURRENT_TIMESTAMP
               AND (s.account_id = ?1 OR v.account_id = ?1)
             GROUP BY p.id, p.status_id, s.account_id, p.expires_at
             ORDER BY p.expires_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<PollNotificationRow>()
}

async fn list_expired_polls_requiring_federation_close(
    db: &D1Database,
    limit: u32,
) -> Result<Vec<ExpiredPollQueueRow>> {
    let bindings = [D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT p.id AS poll_id,
                    p.status_id,
                    s.account_id
             FROM status_polls p
             JOIN statuses s
               ON s.id = p.status_id
             WHERE p.federated_closed_at IS NULL
               AND s.visibility IN ('public', 'unlisted')
               AND datetime(replace(replace(p.expires_at, 'T', ' '), 'Z', '')) <= CURRENT_TIMESTAMP
             ORDER BY p.expires_at ASC
             LIMIT ?1",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<ExpiredPollQueueRow>()
}

async fn mark_status_poll_federated_closed(db: &D1Database, poll_id: &str) -> Result<()> {
    let bindings = [D1Type::Text(poll_id)];
    db.prepare(
        "UPDATE status_polls
         SET federated_closed_at = CURRENT_TIMESTAMP,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?1",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn list_remote_reblog_notifications_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
) -> Result<Vec<RemoteStatusInteractionRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Integer(limit as i32)];
    let result = db
        .prepare(
            "SELECT rr.remote_actor_uri, rr.status_id, rr.created_at
             FROM remote_reblogs rr
             JOIN statuses s
               ON s.id = rr.status_id
             WHERE s.account_id = ?1
             ORDER BY rr.created_at DESC
             LIMIT ?2",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<RemoteStatusInteractionRow>()
}

async fn list_mutes_for_account(
    db: &D1Database,
    account_id: &str,
    limit: u32,
    max_id: Option<i64>,
    since_id: Option<i64>,
) -> Result<Vec<MuteEntryRow>> {
    let account_id_binding = D1Type::Text(account_id);
    db.prepare(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND expires_at IS NOT NULL
           AND expires_at <= CURRENT_TIMESTAMP",
    )
    .bind_refs(&account_id_binding)?
    .run()
    .await?;

    let bindings = [
        D1Type::Text(account_id),
        max_id
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
        since_id
            .map(|value| D1Type::Integer(value as i32))
            .unwrap_or(D1Type::Null),
        D1Type::Integer(limit as i32),
    ];
    let result = db
        .prepare(
            "SELECT rowid AS cursor_id, target_account_id, target_actor_uri
             FROM mutes
             WHERE account_id = ?1
               AND (?2 IS NULL OR rowid < ?2)
               AND (?3 IS NULL OR rowid > ?3)
             ORDER BY rowid DESC
             LIMIT ?4",
        )
        .bind_refs(bindings.iter())?
        .all()
        .await?;

    result.results::<MuteEntryRow>()
}

async fn count_rows(db: &D1Database, sql: &str, value: &str) -> Result<u64> {
    let value = D1Type::Text(value);
    let row = db
        .prepare(sql)
        .bind_refs(&value)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|row| row.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0))
}

async fn count_rows_like(db: &D1Database, sql: &str, pattern: &str) -> Result<u64> {
    count_rows(db, sql, pattern).await
}

async fn upsert_local_follow(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    target: &LocalAccount,
    request: &FollowAccountRequest,
) -> Result<()> {
    let target_actor_uri = actor_url(config, &target.username);
    let languages_json = request
        .languages
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            Error::RustError(format!("failed to serialize follow languages: {error}"))
        })?;
    let bindings = [
        D1Type::Text(follower.id.as_str()),
        D1Type::Text(target.id.as_str()),
        D1Type::Text(target_actor_uri.as_str()),
        D1Type::Integer(if request.reblogs.unwrap_or(true) {
            1
        } else {
            0
        }),
        D1Type::Integer(if request.notify.unwrap_or(false) {
            1
        } else {
            0
        }),
        match languages_json.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO follows (
            id,
            follower_account_id,
            target_account_id,
            target_actor_uri,
            target_inbox_uri,
            target_shared_inbox_uri,
            follow_activity_id,
            state,
            show_reblogs,
            notify,
            languages_json,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            ?2,
            ?3,
            NULL,
            NULL,
            NULL,
            'accepted',
            ?4,
            ?5,
            ?6,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(follower_account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            state = 'accepted',
            show_reblogs = excluded.show_reblogs,
            notify = excluded.notify,
            languages_json = excluded.languages_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_follow_by_target(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    db.prepare(
        "DELETE FROM follows
         WHERE follower_account_id = ?1
           AND target_actor_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn find_follow_by_target(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<Option<FollowRow>> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    db.prepare(
        "SELECT follower_account_id, target_account_id, target_actor_uri, follow_activity_id, state, show_reblogs, notify, languages_json
         FROM follows
         WHERE follower_account_id = ?1
           AND target_actor_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<FollowRow>(None)
    .await
}

async fn find_follow_by_activity_id(
    db: &D1Database,
    follow_activity_id: &str,
) -> Result<Option<FollowRow>> {
    let follow_activity_id = D1Type::Text(follow_activity_id);
    db.prepare(
        "SELECT follower_account_id, target_account_id, target_actor_uri, follow_activity_id, state, show_reblogs, notify, languages_json
         FROM follows
         WHERE follow_activity_id = ?1
         LIMIT 1",
    )
    .bind_refs(&follow_activity_id)?
    .first::<FollowRow>(None)
    .await
}

async fn build_relationship_for_target(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    target_id: &str,
    target_actor_uri: &str,
) -> Result<RelationshipResponse> {
    let follow = find_follow_by_target(db, &viewer.id, target_actor_uri).await?;
    let reciprocal =
        find_follow_by_target(db, target_id, &actor_url(config, &viewer.username)).await?;
    let languages = follow
        .as_ref()
        .and_then(|row| row.languages_json.as_deref())
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok());
    let state = follow
        .as_ref()
        .map(|row| row.state.as_str())
        .unwrap_or("none");
    let followed_by_remote = count_followers_by_actor(db, &viewer.id, target_actor_uri).await? > 0;
    let blocking = is_blocking_actor(db, &viewer.id, target_actor_uri).await?;
    let blocked_by = if target_id.starts_with("r_") {
        false
    } else {
        is_blocking_actor(db, target_id, &actor_url(config, &viewer.username)).await?
    };
    let mute = find_active_mute(db, &viewer.id, target_actor_uri).await?;

    Ok(RelationshipResponse {
        id: target_id.to_owned(),
        following: state == "accepted",
        showing_reblogs: follow
            .as_ref()
            .map(|row| row.show_reblogs != 0)
            .unwrap_or(false),
        notifying: follow.as_ref().map(|row| row.notify != 0).unwrap_or(false),
        languages,
        followed_by: reciprocal
            .as_ref()
            .map(|row| row.state == "accepted")
            .unwrap_or(false)
            || followed_by_remote,
        blocking,
        blocked_by,
        muting: mute.is_some(),
        muting_notifications: mute
            .as_ref()
            .map(|row| row.notifications != 0)
            .unwrap_or(false),
        muting_expires_at: mute.and_then(|row| row.expires_at),
        requested: state == "pending",
        requested_by: false,
        domain_blocking: false,
        endorsed: false,
        note: String::new(),
    })
}

async fn upsert_block(
    db: &D1Database,
    blocker_account_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(blocker_account_id),
        match target_account_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(target_actor_uri),
    ];

    db.prepare(
        "INSERT INTO blocks (
            blocker_account_id,
            target_account_id,
            target_actor_uri,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(blocker_account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_block_by_target(
    db: &D1Database,
    blocker_account_id: &str,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [
        D1Type::Text(blocker_account_id),
        D1Type::Text(target_actor_uri),
    ];

    db.prepare(
        "DELETE FROM blocks
         WHERE blocker_account_id = ?1
           AND target_actor_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn is_blocking_actor(
    db: &D1Database,
    blocker_account_id: &str,
    target_actor_uri: &str,
) -> Result<bool> {
    let bindings = [
        D1Type::Text(blocker_account_id),
        D1Type::Text(target_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM blocks
             WHERE blocker_account_id = ?1
               AND target_actor_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

fn expiry_from_duration_seconds(duration: u32) -> Result<String> {
    let now = js_sys::Date::new_0();
    now.set_time(now.get_time() + (duration as f64 * 1000.0));
    now.to_iso_string()
        .as_string()
        .ok_or_else(|| Error::RustError("failed to compute mute expiry timestamp".to_owned()))
}

async fn upsert_mute(
    db: &D1Database,
    account_id: &str,
    target_account_id: Option<&str>,
    target_actor_uri: &str,
    notifications: bool,
    expires_at: Option<&str>,
) -> Result<()> {
    let bindings = [
        D1Type::Text(account_id),
        match target_account_id {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(target_actor_uri),
        D1Type::Integer(if notifications { 1 } else { 0 }),
        match expires_at {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];

    db.prepare(
        "INSERT INTO mutes (
            account_id,
            target_account_id,
            target_actor_uri,
            notifications,
            expires_at,
            created_at,
            updated_at
        ) VALUES (
            ?1,
            ?2,
            ?3,
            ?4,
            ?5,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(account_id, target_actor_uri) DO UPDATE SET
            target_account_id = excluded.target_account_id,
            notifications = excluded.notifications,
            expires_at = excluded.expires_at,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn delete_mute_by_target(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<()> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_actor_uri)];
    db.prepare(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn find_active_mute(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<Option<MuteRow>> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(target_actor_uri)];
    db.prepare(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri = ?2
           AND expires_at IS NOT NULL
           AND expires_at <= CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    db.prepare(
        "SELECT notifications, expires_at
         FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri = ?2
         LIMIT 1",
    )
    .bind_refs(bindings.iter())?
    .first::<MuteRow>(None)
    .await
}

async fn is_muted_actor(db: &D1Database, account_id: &str, target_actor_uri: &str) -> Result<bool> {
    Ok(find_active_mute(db, account_id, target_actor_uri)
        .await?
        .is_some())
}

async fn muted_notifications_for_actor(
    db: &D1Database,
    account_id: &str,
    target_actor_uri: &str,
) -> Result<bool> {
    Ok(find_active_mute(db, account_id, target_actor_uri)
        .await?
        .map(|row| row.notifications != 0)
        .unwrap_or(false))
}

async fn count_followers_by_actor(
    db: &D1Database,
    account_id: &str,
    actor_uri: &str,
) -> Result<u64> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(actor_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM followers
             WHERE account_id = ?1
               AND actor_uri = ?2",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0))
}

async fn has_any_local_followers_for_remote_actor(
    db: &D1Database,
    actor_uri: &str,
) -> Result<bool> {
    Ok(count_rows(
        db,
        "SELECT COUNT(*) AS count
         FROM follows
         WHERE target_actor_uri = ?1
           AND state = 'accepted'",
        actor_uri,
    )
    .await?
        > 0)
}

async fn is_local_account_following_remote_actor(
    db: &D1Database,
    account_id: &str,
    actor_uri: &str,
) -> Result<bool> {
    let bindings = [D1Type::Text(account_id), D1Type::Text(actor_uri)];
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
               AND state = 'accepted'",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

async fn first_local_follower_for_remote_actor(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<LocalAccount>> {
    let bindings = [D1Type::Text(actor_uri)];
    let row = db
        .prepare(
            "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
             FROM follows f
             JOIN accounts a
               ON a.id = f.follower_account_id
             WHERE f.target_actor_uri = ?1
               AND f.state = 'accepted'
             ORDER BY f.created_at ASC
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<AccountRow>(None)
        .await?;

    Ok(row.map(LocalAccount::from))
}

async fn is_local_follower_authorized(
    db: &D1Database,
    viewer_account_id: &str,
    owner_account_id: &str,
) -> Result<bool> {
    let owner = D1Type::Text(owner_account_id);
    let viewer = D1Type::Text(viewer_account_id);
    let row = db
        .prepare(
            "SELECT COUNT(*) AS count
             FROM follows
             WHERE follower_account_id = ?2
               AND target_account_id = ?1
               AND state = 'accepted'",
        )
        .bind_refs(&[owner, viewer])?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        > 0)
}

async fn update_follow_state_from_response(
    db: &D1Database,
    activity: &serde_json::Value,
    remote_actor: &RemoteActorProfile,
    state: &str,
) -> Result<()> {
    let Some(follow_activity_id) = activity
        .get("object")
        .and_then(|object| object.get("id"))
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    let Some(follow) = find_follow_by_activity_id(db, follow_activity_id).await? else {
        return Ok(());
    };
    if follow.target_actor_uri != remote_actor.actor_uri {
        return Ok(());
    }

    let bindings = [D1Type::Text(state), D1Type::Text(follow_activity_id)];
    db.prepare(
        "UPDATE follows
         SET state = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE follow_activity_id = ?2",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn follow_remote_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
    request: &FollowAccountRequest,
) -> Result<RelationshipResponse> {
    let (_, payload) = build_follow_activity(config, follower, &actor.actor_uri)?;
    let follow_activity_id =
        queue_remote_actor_activity_required(db, &follower.id, &actor.actor_uri, &payload).await?;
    upsert_remote_follow(db, follower, actor, request, &follow_activity_id).await?;
    build_relationship_for_target(
        db,
        config,
        follower,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}

async fn unfollow_remote_account(
    db: &D1Database,
    config: &AppConfig,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
) -> Result<RelationshipResponse> {
    if let Some(follow_activity_id) =
        load_follow_activity_id(db, &follower.id, &actor.actor_uri).await?
    {
        let payload =
            build_undo_follow_activity(config, follower, &follow_activity_id, &actor.actor_uri)?;
        let _ = queue_remote_actor_activity(db, &follower.id, &actor.actor_uri, &payload).await?;
    }

    delete_follow_by_target(db, &follower.id, &actor.actor_uri).await?;
    build_relationship_for_target(
        db,
        config,
        follower,
        &remote_account_rest_id(&actor.actor_uri),
        &actor.actor_uri,
    )
    .await
}

async fn upsert_remote_follow(
    db: &D1Database,
    follower: &LocalAccount,
    actor: &RemoteActorRow,
    request: &FollowAccountRequest,
    follow_activity_id: &str,
) -> Result<()> {
    let (inbox_uri, shared_inbox_uri) = load_remote_actor_inbox_uris(db, &actor.actor_uri).await?;
    let languages_json = request
        .languages
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| {
            Error::RustError(format!("failed to serialize follow languages: {error}"))
        })?;
    let bindings = [
        D1Type::Text(follower.id.as_str()),
        D1Type::Text(actor.actor_uri.as_str()),
        match inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        match shared_inbox_uri.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        D1Type::Text(follow_activity_id),
        D1Type::Integer(if request.reblogs.unwrap_or(true) {
            1
        } else {
            0
        }),
        D1Type::Integer(if request.notify.unwrap_or(false) {
            1
        } else {
            0
        }),
        match languages_json.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
    ];
    db.prepare(
        "INSERT INTO follows (
            id,
            follower_account_id,
            target_account_id,
            target_actor_uri,
            target_inbox_uri,
            target_shared_inbox_uri,
            follow_activity_id,
            state,
            show_reblogs,
            notify,
            languages_json,
            created_at,
            updated_at
        ) VALUES (
            lower(hex(randomblob(16))),
            ?1,
            NULL,
            ?2,
            ?3,
            ?4,
            ?5,
            'pending',
            ?6,
            ?7,
            ?8,
            CURRENT_TIMESTAMP,
            CURRENT_TIMESTAMP
        )
        ON CONFLICT(follower_account_id, target_actor_uri) DO UPDATE SET
            target_inbox_uri = excluded.target_inbox_uri,
            target_shared_inbox_uri = excluded.target_shared_inbox_uri,
            follow_activity_id = excluded.follow_activity_id,
            state = 'pending',
            show_reblogs = excluded.show_reblogs,
            notify = excluded.notify,
            languages_json = excluded.languages_json,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    Ok(())
}

async fn load_remote_actor_inbox_uris(
    db: &D1Database,
    actor_uri: &str,
) -> Result<(Option<String>, Option<String>)> {
    let actor_uri = D1Type::Text(actor_uri);
    let row = db
        .prepare(
            "SELECT inbox_uri, shared_inbox_uri
             FROM remote_actors
             WHERE actor_uri = ?1
             LIMIT 1",
        )
        .bind_refs(&actor_uri)?
        .first::<serde_json::Value>(None)
        .await?;

    Ok((
        row.as_ref()
            .and_then(|value| value.get("inbox_uri"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        row.as_ref()
            .and_then(|value| value.get("shared_inbox_uri"))
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
    ))
}

async fn load_remote_actor_delivery_inbox(
    db: &D1Database,
    actor_uri: &str,
) -> Result<Option<String>> {
    let (inbox_uri, shared_inbox_uri) = load_remote_actor_inbox_uris(db, actor_uri).await?;
    Ok(shared_inbox_uri.or(inbox_uri))
}

async fn load_follow_activity_id(
    db: &D1Database,
    follower_account_id: &str,
    target_actor_uri: &str,
) -> Result<Option<String>> {
    let bindings = [
        D1Type::Text(follower_account_id),
        D1Type::Text(target_actor_uri),
    ];
    let row = db
        .prepare(
            "SELECT follow_activity_id
             FROM follows
             WHERE follower_account_id = ?1
               AND target_actor_uri = ?2
             LIMIT 1",
        )
        .bind_refs(bindings.iter())?
        .first::<serde_json::Value>(None)
        .await?;

    Ok(row
        .as_ref()
        .and_then(|value| value.get("follow_activity_id"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned))
}

async fn resolve_inbox_target_account(
    db: &D1Database,
    config: &AppConfig,
    username: Option<&str>,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    let account = match username {
        Some(username) => find_account_by_username(db, username).await?,
        None => match extract_inbox_target_username(config, activity) {
            Some(target_username) => find_account_by_username(db, &target_username).await?,
            None => resolve_follow_response_target_account(db, activity)
                .await?
                .or(resolve_poll_vote_target_account(db, activity).await?)
                .or(resolve_remote_actor_update_target_account(db, activity).await?),
        },
    };

    match account {
        Some(account) => ensure_account_keys(db, account).await.map(Some),
        None => Ok(None),
    }
}

async fn resolve_remote_actor_update_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if activity.get("type").and_then(serde_json::Value::as_str) != Some("Update") {
        return Ok(None);
    }
    let Some(object) = activity.get("object").filter(|value| value.is_object()) else {
        return Ok(None);
    };
    if !is_activitypub_actor_type(object.get("type").and_then(serde_json::Value::as_str)) {
        return Ok(None);
    }
    let Some(actor_uri) = activity_object_id(Some(object))
        .or_else(|| activity.get("actor").and_then(serde_json::Value::as_str))
    else {
        return Ok(None);
    };

    first_local_follower_for_remote_actor(db, actor_uri).await
}

async fn resolve_follow_response_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    let Some(follow_activity_id) = activity
        .get("object")
        .and_then(|object| activity_object_id(Some(object)))
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    let Some(follow) = find_follow_by_activity_id(db, &follow_activity_id).await? else {
        return Ok(None);
    };

    find_account_by_id(db, &follow.follower_account_id).await
}

async fn resolve_poll_vote_target_account(
    db: &D1Database,
    activity: &serde_json::Value,
) -> Result<Option<LocalAccount>> {
    if activity.get("type").and_then(serde_json::Value::as_str) != Some("Undo") {
        return Ok(None);
    }
    let Some(activity_uri) = activity
        .get("object")
        .and_then(|object| activity_object_id(Some(object)))
    else {
        return Ok(None);
    };
    let Some(vote) = find_status_poll_vote_by_activity_uri(db, activity_uri).await? else {
        return Ok(None);
    };

    find_account_by_id(db, &vote.status_account_id).await
}

fn username_from_email(email: &str) -> String {
    let local = email.split('@').next().unwrap_or("user");
    let sanitized: String = local
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '-')
        .collect();

    if sanitized.is_empty() {
        "user".to_owned()
    } else {
        sanitized
    }
}

fn short_email_suffix(email: &str) -> String {
    let checksum = email.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(16777619).wrapping_add(byte as u32)
    });

    format!("{:06x}", checksum & 0x00ff_ffff)
}

async fn verify_access_jwt(token: &str, config: &AppConfig) -> Result<AccessJwtClaims> {
    let (header_segment, payload_segment, signature_segment) = split_jwt(token)
        .ok_or_else(|| Error::RustError("malformed Cloudflare Access JWT".to_owned()))?;

    let header: AccessJwtHeader = decode_jwt_segment(header_segment)?;
    if header.alg != "RS256" {
        return Err(Error::RustError(format!(
            "unsupported Cloudflare Access JWT algorithm: {}",
            header.alg
        )));
    }

    let claims: AccessJwtClaims = decode_jwt_segment(payload_segment)?;
    let expected_issuer = config.access_team_domain.trim_end_matches('/').to_owned();
    if claims.iss != expected_issuer {
        return Err(Error::RustError(
            "Cloudflare Access JWT issuer mismatch".to_owned(),
        ));
    }
    if !claims.aud.contains(&config.access_audience) {
        return Err(Error::RustError(
            "Cloudflare Access JWT audience mismatch".to_owned(),
        ));
    }

    let now = current_unix_timestamp();
    if let Some(exp) = claims.exp
        && exp < now
    {
        return Err(Error::RustError(
            "Cloudflare Access JWT has expired".to_owned(),
        ));
    }
    if let Some(nbf) = claims.nbf
        && nbf > now
    {
        return Err(Error::RustError(
            "Cloudflare Access JWT is not yet valid".to_owned(),
        ));
    }

    let jwk = fetch_access_jwk(config, &header.kid).await?;
    verify_rs256_signature(
        &jwk,
        format!("{header_segment}.{payload_segment}").as_bytes(),
        &decode_base64url(signature_segment)?,
    )
    .await?;

    Ok(claims)
}

fn split_jwt(token: &str) -> Option<(&str, &str, &str)> {
    let mut parts = token.split('.');
    let header = parts.next()?;
    let payload = parts.next()?;
    let signature = parts.next()?;

    if parts.next().is_some() {
        return None;
    }

    Some((header, payload, signature))
}

fn decode_jwt_segment<T>(segment: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = decode_base64url(segment)?;
    serde_json::from_slice(&bytes).map_err(|error| {
        Error::RustError(format!("invalid Cloudflare Access JWT payload: {error}"))
    })
}

fn decode_base64url(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|error| Error::RustError(format!("invalid base64url data: {error}")))
}

async fn fetch_access_jwk(config: &AppConfig, expected_kid: &str) -> Result<AccessJwk> {
    let certs_url = format!(
        "{}/cdn-cgi/access/certs",
        config.access_team_domain.trim_end_matches('/')
    );
    let request = Request::new(&certs_url, Method::Get)?;
    let mut response = Fetch::Request(request).send().await?;
    let certs: AccessCertsResponse = response.json().await?;

    certs
        .keys
        .into_iter()
        .find(|jwk| jwk.kid == expected_kid)
        .ok_or_else(|| {
            Error::RustError("matching Cloudflare Access signing key was not found".to_owned())
        })
}

async fn verify_rs256_signature(jwk: &AccessJwk, data: &[u8], signature: &[u8]) -> Result<()> {
    let subtle = subtle_crypto()?;

    let jwk_value = worker::d1::serde_wasm_bindgen::to_value(jwk)
        .map_err(|error| Error::RustError(format!("failed to serialize JWK: {error}")))?;
    let jwk_object = jwk_value
        .dyn_into::<Object>()
        .map_err(|_| Error::RustError("failed to convert JWK to object".to_owned()))?;

    let import_params = RsaHashedImportParams::new_with_str("SHA-256");
    let import_algorithm: Object = import_params.into();
    Reflect::set(
        &import_algorithm,
        &JsValue::from_str("name"),
        &JsValue::from_str("RSASSA-PKCS1-v1_5"),
    )
    .map_err(Error::from)?;

    let key_usages = Array::new();
    key_usages.push(&JsValue::from_str("verify"));

    let crypto_key = JsFuture::from(subtle.import_key_with_object(
        "jwk",
        &jwk_object,
        &import_algorithm,
        false,
        &key_usages.into(),
    )?)
    .await?
    .dyn_into::<web_sys::CryptoKey>()
    .map_err(|_| Error::RustError("failed to import Cloudflare Access public key".to_owned()))?;

    let verify_algorithm = Algorithm::new("RSASSA-PKCS1-v1_5");
    let verify_algorithm: Object = verify_algorithm.into();
    let signature = Uint8Array::from(signature);
    let data = Uint8Array::from(data);

    let verified = JsFuture::from(
        subtle.verify_with_object_and_buffer_source_and_buffer_source(
            &verify_algorithm,
            &crypto_key,
            signature.as_ref(),
            data.as_ref(),
        )?,
    )
    .await?
    .as_bool()
    .unwrap_or(false);

    if verified {
        Ok(())
    } else {
        Err(Error::RustError(
            "Cloudflare Access JWT signature verification failed".to_owned(),
        ))
    }
}

fn current_unix_timestamp() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

const fn build_metadata() -> BuildMetadata {
    BuildMetadata::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        "cloudflare-workers",
    )
}

impl From<AccountRow> for LocalAccount {
    fn from(value: AccountRow) -> Self {
        Self {
            id: value.id,
            username: value.username,
            access_email: value.access_email,
            display_name: value.display_name,
            bio_html: value.bio_html,
            bio_text: value.bio_text,
            fields: parse_profile_fields_json(&value.fields_json),
            discoverable: value.discoverable != 0,
            default_post_visibility: value.default_post_visibility,
            default_sensitive: value.default_sensitive != 0,
            default_language: value.default_language,
            avatar_object_key: value.avatar_object_key,
            avatar_content_type: value.avatar_content_type,
            header_object_key: value.header_object_key,
            header_content_type: value.header_content_type,
            private_key_jwk: value.private_key_jwk,
            public_key_pem: value.public_key_pem,
            created_at: value.created_at,
        }
    }
}

impl MastodonAccountResponse {
    fn from_account(account: &LocalAccount, config: &AppConfig) -> Self {
        Self::from_account_with_stats(account, config, &AccountStats::default())
    }

    fn from_account_with_stats(
        account: &LocalAccount,
        config: &AppConfig,
        stats: &AccountStats,
    ) -> Self {
        let profile_url = actor_url(config, &account.username);

        Self {
            id: account.id.clone(),
            username: account.username.clone(),
            acct: account.acct().to_owned(),
            display_name: account.display_name.clone(),
            locked: false,
            bot: false,
            created_at: account.created_at.clone(),
            note: account.bio_html.clone(),
            url: profile_url,
            avatar: account_avatar_url(config, account),
            avatar_static: account_avatar_url(config, account),
            header: account_header_url(config, account),
            header_static: account_header_url(config, account),
            fields: mastodon_account_fields(&account.fields),
            followers_count: stats.followers_count,
            following_count: stats.following_count,
            statuses_count: stats.statuses_count,
            source: None,
        }
    }

    fn from_credentials_account(
        account: &LocalAccount,
        config: &AppConfig,
        stats: &AccountStats,
    ) -> Self {
        let mut value = Self::from_account_with_stats(account, config, stats);
        value.source = Some(MastodonAccountSource {
            note: account.bio_text.clone(),
            fields: mastodon_account_fields(&account.fields),
            privacy: account.default_post_visibility.clone(),
            sensitive: account.default_sensitive,
            language: account.default_language.clone().unwrap_or_default(),
            follow_requests_count: 0,
            hide_collections: None,
            discoverable: Some(account.discoverable),
        });
        value
    }

    fn from_remote_actor(actor: &RemoteActorRow) -> Self {
        let profile_url = actor
            .profile_url
            .clone()
            .unwrap_or_else(|| actor.actor_uri.clone());
        let avatar_url = actor.avatar_url.clone().unwrap_or_default();
        let header_url = actor.header_url.clone().unwrap_or_default();

        Self {
            id: remote_account_rest_id(&actor.actor_uri),
            username: actor.username.clone(),
            acct: format!("{}@{}", actor.username, actor.domain),
            display_name: actor.display_name.clone(),
            locked: false,
            bot: false,
            created_at: String::new(),
            note: actor.summary_html.clone(),
            url: profile_url,
            avatar: avatar_url.clone(),
            avatar_static: avatar_url,
            header: header_url.clone(),
            header_static: header_url,
            fields: Vec::new(),
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            source: None,
        }
    }
}

impl MastodonStatusResponse {
    fn from_row(
        row: &StatusRow,
        account: &LocalAccount,
        config: &AppConfig,
        in_reply_to_account_id: Option<String>,
        media_attachments: Vec<MediaAttachmentRow>,
    ) -> Self {
        let uri = row.ap_id.clone().unwrap_or_else(|| {
            format!(
                "{}/statuses/{}",
                actor_url(config, &account.username),
                row.id
            )
        });

        Self {
            id: row.id.clone(),
            created_at: row.created_at.clone(),
            in_reply_to_id: row.in_reply_to_id.clone(),
            in_reply_to_account_id,
            sensitive: row.sensitive != 0,
            spoiler_text: row.spoiler_text.clone(),
            visibility: row.visibility.clone(),
            language: row.language.clone(),
            uri: uri.clone(),
            url: uri,
            replies_count: 0,
            reblogs_count: 0,
            favourites_count: 0,
            favourited: false,
            reblogged: false,
            muted: false,
            bookmarked: false,
            pinned: false,
            content: row.content_html.clone(),
            text: None,
            reblog: None,
            application: None,
            account: MastodonAccountResponse::from_account(account, config),
            media_attachments: media_attachments
                .iter()
                .map(|media| {
                    serde_json::to_value(MastodonMediaAttachmentResponse::from_row(media, config))
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
            mentions: Vec::new(),
            tags: extract_hashtags_from_text(&row._text_content)
                .into_iter()
                .map(|tag| {
                    serde_json::to_value(MastodonTagResponse {
                        id: tag_rest_id(&tag),
                        name: tag.clone(),
                        url: tag_url(config, &tag),
                        history: tag_history_stub(),
                        following: false,
                        featured: false,
                    })
                    .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
            emojis: Vec::new(),
            card: None,
            poll: None,
        }
    }

    fn from_deleted_row(
        row: &StatusRow,
        account: &LocalAccount,
        config: &AppConfig,
        in_reply_to_account_id: Option<String>,
        media_attachments: Vec<MediaAttachmentRow>,
    ) -> Self {
        let mut response = Self::from_row(
            row,
            account,
            config,
            in_reply_to_account_id,
            media_attachments,
        );
        response.text = Some(row._text_content.clone());
        response
    }

    fn from_remote_row(row: &RemoteStatusRow, actor: &RemoteActorRow, config: &AppConfig) -> Self {
        let uri = row.object_uri.clone();
        let url = row.url.clone().unwrap_or_else(|| uri.clone());

        Self {
            id: row.id.clone(),
            created_at: row.published_at.clone(),
            in_reply_to_id: row.in_reply_to_uri.clone(),
            in_reply_to_account_id: None,
            sensitive: row.sensitive != 0,
            spoiler_text: row.spoiler_text.clone(),
            visibility: row.visibility.clone(),
            language: row.language.clone(),
            uri,
            url,
            replies_count: 0,
            reblogs_count: 0,
            favourites_count: 0,
            favourited: false,
            reblogged: false,
            muted: false,
            bookmarked: false,
            pinned: false,
            content: row.content_html.clone(),
            text: None,
            reblog: None,
            application: None,
            account: MastodonAccountResponse::from_remote_actor(actor),
            media_attachments: Vec::new(),
            mentions: Vec::new(),
            tags: extract_hashtags_from_html(&row.content_html)
                .into_iter()
                .map(|tag| {
                    serde_json::to_value(MastodonTagResponse {
                        id: tag_rest_id(&tag),
                        name: tag.clone(),
                        url: tag_url(config, &tag),
                        history: tag_history_stub(),
                        following: false,
                        featured: false,
                    })
                    .unwrap_or(serde_json::Value::Null)
                })
                .collect(),
            emojis: Vec::new(),
            card: None,
            poll: None,
        }
    }
}

impl MastodonMediaAttachmentResponse {
    fn from_row(row: &MediaAttachmentRow, config: &AppConfig) -> Self {
        let url = media_object_url(config, &row.object_key);
        let fallback_url = media_fallback_url(config, &row.id);
        let focus = row
            .focus_x
            .zip(row.focus_y)
            .map(|(x, y)| MastodonMediaFocus { x, y });

        Self {
            id: row.id.clone(),
            media_type: media_kind_label(
                classify_media_kind(&row.content_type).unwrap_or(MediaKind::Image),
            ),
            url: url.clone(),
            preview_url: url,
            remote_url: None,
            text_url: Some(fallback_url),
            meta: MastodonMediaMeta {
                original: None,
                small: None,
                focus,
            },
            description: if row.description.is_empty() {
                None
            } else {
                Some(row.description.clone())
            },
            blurhash: None,
        }
    }
}

#[cfg(test)]
mod compat_tests;

#[cfg(test)]
mod tests {
    use super::{
        CreateStatusPollRequest, MastodonAccountResponse, MastodonReportResponse, RemoteActorRow,
        RemoteStatusPollOptionRow, RemoteStatusPollVoteRow, SearchCategoryFlags, SearchV2Query,
        StatusPollOptionRow, StatusPollRow, StatusRow, TagTimelineQuery,
        activitypub_profile_attachments, apply_activitypub_poll_fields,
        build_activitypub_delete_with_published_at, build_instance_v1_document,
        build_instance_v2_document, build_internal_cursor_link_for_url, build_nodeinfo_document,
        build_nodeinfo_links_document, build_poll_vote_activity_with_ids,
        build_status_update_activity_with_id, build_update_person_activity_with_id,
        classify_media_kind, configured_html_document, delivery_retry_delay_modifier,
        describe_outbound_activity, directory_order, extract_account_handles_from_text,
        extract_hashtags_from_html, extract_hashtags_from_text, extract_inbox_target_username,
        extract_mentions_from_text, extract_remote_note_object, extract_remote_poll_draft,
        extract_remote_profile_media_url, follow_targets_local_actor, include_local_source,
        include_remote_source, instance_base_url, is_activitypub_actor_type, is_admin_account,
        is_follow_undo, local_username_from_actor_uri, local_username_from_status_uri,
        mastodon_account_fields, matches_tag_timeline_filters, media_fallback_url,
        media_kind_label, media_object_url, nodeinfo_url, normalize_status_poll,
        notification_sort_key, notification_timestamp_sort_token,
        outbound_terminal_failure_follow_state, parse_csv_list, parse_http_url_parts,
        parse_internal_pagination_id, parse_lookup_handle, parse_media_focus,
        parse_remote_actor_profile_document, parse_webfinger_resource, peer_authority_from_uri,
        remap_remote_poll_vote_positions, remote_account_rest_id, remote_actor_uri_from_rest_id,
        resolve_search_tag_name, search_category_flags, search_text_match_rank, search_v2_limit,
        search_v2_requires_auth, tag_search_rank, visibility_from_activitypub_object,
    };
    use cfwdon_core::AppConfig;
    use cfwdon_domain::{
        InstanceCapabilities, InstanceSummary, LocalAccount, ProfileField, SoftwareInfo,
    };
    use url::Url;

    #[test]
    fn parse_webfinger_resource_extracts_local_handle() {
        let handle = parse_webfinger_resource("acct:alice@example.com").unwrap();
        assert_eq!(handle.username, "alice");
        assert_eq!(handle.domain.as_deref(), Some("example.com"));
    }

    #[test]
    fn parse_webfinger_resource_rejects_non_acct_scheme() {
        let error = parse_webfinger_resource("https://example.com/users/alice").unwrap_err();
        assert!(error.to_string().contains("acct"));
    }

    #[test]
    fn parse_internal_pagination_id_accepts_integer_cursor() {
        assert_eq!(
            parse_internal_pagination_id(Some("42"), "max_id").unwrap(),
            Some(42)
        );
        assert_eq!(
            parse_internal_pagination_id(Some(""), "max_id").unwrap(),
            None
        );
        assert_eq!(parse_internal_pagination_id(None, "max_id").unwrap(), None);
    }

    #[test]
    fn parse_internal_pagination_id_rejects_invalid_cursor() {
        let error = parse_internal_pagination_id(Some("abc"), "since_id").unwrap_err();
        assert!(error.to_string().contains("since_id"));
    }

    #[test]
    fn internal_cursor_link_header_preserves_other_query_params() {
        let url = Url::parse("https://social.example/api/v1/mutes?foo=bar&limit=20").unwrap();
        let next = build_internal_cursor_link_for_url(&url, 10, Some(150), None, "next").unwrap();
        let prev = build_internal_cursor_link_for_url(&url, 10, None, Some(200), "prev").unwrap();

        assert!(next.contains("foo=bar"));
        assert!(next.contains("limit=10"));
        assert!(next.contains("max_id=150"));
        assert!(next.contains("rel=\"next\""));
        assert!(prev.contains("foo=bar"));
        assert!(prev.contains("limit=10"));
        assert!(prev.contains("since_id=200"));
        assert!(prev.contains("rel=\"prev\""));
    }

    #[test]
    fn describe_outbound_activity_extracts_id_and_type() {
        let descriptor = describe_outbound_activity(
            r#"{"id":"https://social.example/users/alice/likes/123","type":"Like"}"#,
        )
        .unwrap();

        assert_eq!(
            descriptor.activity_id,
            "https://social.example/users/alice/likes/123"
        );
        assert_eq!(descriptor.activity_type, "Like");
    }

    #[test]
    fn describe_outbound_activity_rejects_missing_fields() {
        assert!(describe_outbound_activity(r#"{"type":"Like"}"#).is_err());
        assert!(describe_outbound_activity(r#"{"id":"abc"}"#).is_err());
    }

    #[test]
    fn extract_remote_note_object_supports_note_question_and_create_wrappers() {
        let note = serde_json::json!({"type":"Note","id":"https://remote.example/notes/1"});
        assert_eq!(
            extract_remote_note_object(&note)
                .and_then(|value| value.get("id"))
                .and_then(serde_json::Value::as_str),
            Some("https://remote.example/notes/1")
        );

        let question = serde_json::json!({
            "type":"Question",
            "id":"https://remote.example/notes/3",
            "oneOf":[
                {"type":"Note","name":"yes","replies":{"totalItems":2}},
                {"type":"Note","name":"no","replies":{"totalItems":1}}
            ]
        });
        assert_eq!(
            extract_remote_note_object(&question)
                .and_then(|value| value.get("id"))
                .and_then(serde_json::Value::as_str),
            Some("https://remote.example/notes/3")
        );

        let create = serde_json::json!({
            "type":"Create",
            "object":{"type":"Question","id":"https://remote.example/notes/2"}
        });
        assert_eq!(
            extract_remote_note_object(&create)
                .and_then(|value| value.get("id"))
                .and_then(serde_json::Value::as_str),
            Some("https://remote.example/notes/2")
        );
    }

    #[test]
    fn extract_remote_note_object_rejects_non_note_documents() {
        let actor = serde_json::json!({"type":"Person","id":"https://remote.example/users/alice"});
        assert!(extract_remote_note_object(&actor).is_none());
    }

    #[test]
    fn extract_remote_poll_draft_reads_question_options_and_counts() {
        let question = serde_json::json!({
            "type":"Question",
            "endTime":"2026-03-01T00:00:00Z",
            "votersCount": 2,
            "anyOf":[
                {"type":"Note","name":"rust","replies":{"totalItems":2}},
                {"type":"Note","name":"workers","replies":{"totalItems":1}}
            ]
        });

        let poll = extract_remote_poll_draft(&question).unwrap();
        assert!(poll.multiple);
        assert_eq!(poll.expires_at.as_deref(), Some("2026-03-01T00:00:00Z"));
        assert_eq!(poll.voters_count, Some(2));
        assert_eq!(poll.votes_count, 3);
        assert_eq!(poll.options.len(), 2);
        assert_eq!(poll.options[0].title, "rust");
        assert_eq!(poll.options[1].votes_count, 1);
    }

    #[test]
    fn remap_remote_poll_vote_positions_prefers_matching_title_after_reorder() {
        let options = vec![
            RemoteStatusPollOptionRow {
                title: "green".to_owned(),
                votes_count: 5,
            },
            RemoteStatusPollOptionRow {
                title: "orange".to_owned(),
                votes_count: 3,
            },
            RemoteStatusPollOptionRow {
                title: "blue".to_owned(),
                votes_count: 1,
            },
        ];
        let votes = vec![RemoteStatusPollVoteRow {
            option_position: 0,
            option_title: Some("orange".to_owned()),
        }];

        assert_eq!(remap_remote_poll_vote_positions(&options, &votes), vec![1]);
    }

    #[test]
    fn remap_remote_poll_vote_positions_falls_back_to_stored_position_for_legacy_rows() {
        let options = vec![
            RemoteStatusPollOptionRow {
                title: "yes".to_owned(),
                votes_count: 2,
            },
            RemoteStatusPollOptionRow {
                title: "no".to_owned(),
                votes_count: 1,
            },
        ];
        let votes = vec![RemoteStatusPollVoteRow {
            option_position: 1,
            option_title: None,
        }];

        assert_eq!(remap_remote_poll_vote_positions(&options, &votes), vec![1]);
    }

    #[test]
    fn remap_remote_poll_vote_positions_drops_unresolvable_stale_votes() {
        let options = vec![RemoteStatusPollOptionRow {
            title: "green".to_owned(),
            votes_count: 2,
        }];
        let votes = vec![RemoteStatusPollVoteRow {
            option_position: 4,
            option_title: Some("orange".to_owned()),
        }];

        assert!(remap_remote_poll_vote_positions(&options, &votes).is_empty());
    }

    #[test]
    fn build_poll_vote_activity_uses_question_reply_shape() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let account = LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            discoverable: false,
            default_post_visibility: "public".to_owned(),
            default_sensitive: false,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };

        let (activity_id, payload) = build_poll_vote_activity_with_ids(
            &config,
            &account,
            "https://remote.example/users/bob",
            "https://remote.example/questions/1",
            "orange",
            "https://social.example/users/alice/votes/test-vote",
            "https://social.example/users/alice/votes/test-vote/activity",
        )
        .unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
        assert_eq!(value["id"], serde_json::json!(activity_id));
        assert_eq!(value["type"], serde_json::json!("Create"));
        assert_eq!(
            value["to"],
            serde_json::json!(["https://remote.example/users/bob"])
        );
        assert_eq!(
            value["object"]["inReplyTo"],
            serde_json::json!("https://remote.example/questions/1")
        );
        assert_eq!(value["object"]["name"], serde_json::json!("orange"));
    }

    #[test]
    fn build_status_update_activity_wraps_question_object() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let account = LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            discoverable: false,
            default_post_visibility: "public".to_owned(),
            default_sensitive: false,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };
        let object = serde_json::json!({
            "id": "https://social.example/users/alice/statuses/status-1",
            "type": "Question",
            "to": ["https://www.w3.org/ns/activitystreams#Public"],
            "cc": ["https://social.example/users/alice/followers"],
        });

        let payload = build_status_update_activity_with_id(
            &config,
            &account,
            object,
            "https://social.example/users/alice/statuses/status-1/updates/test",
            "2026-02-01T00:00:00Z",
        )
        .unwrap();
        let value = serde_json::from_str::<serde_json::Value>(&payload).unwrap();
        assert_eq!(value["type"], serde_json::json!("Update"));
        assert_eq!(
            value["id"],
            serde_json::json!("https://social.example/users/alice/statuses/status-1/updates/test")
        );
        assert_eq!(value["object"]["type"], serde_json::json!("Question"));
        assert_eq!(
            value["to"],
            serde_json::json!(["https://www.w3.org/ns/activitystreams#Public"])
        );
    }

    #[test]
    fn apply_activitypub_poll_fields_uses_question_shape_for_single_choice() {
        let poll = StatusPollRow {
            id: "poll-1".to_owned(),
            status_id: "status-1".to_owned(),
            multiple: 0,
            hide_totals: 0,
            expires_at: "2026-02-01T00:00:00Z".to_owned(),
        };
        let options = vec![
            StatusPollOptionRow {
                title: "yes".to_owned(),
                votes_count: 2,
            },
            StatusPollOptionRow {
                title: "no".to_owned(),
                votes_count: 1,
            },
        ];
        let mut object = serde_json::json!({
            "type": "Note",
            "id": "https://social.example/users/alice/statuses/status-1",
        });

        apply_activitypub_poll_fields(&mut object, &poll, &options, 3, true);
        assert_eq!(object["type"], serde_json::json!("Question"));
        assert_eq!(object["endTime"], serde_json::json!("2026-02-01T00:00:00Z"));
        assert_eq!(object["closed"], serde_json::json!("2026-02-01T00:00:00Z"));
        assert_eq!(object["votersCount"], serde_json::json!(3));
        assert!(object.get("anyOf").is_none());
        assert_eq!(object["oneOf"][0]["name"], serde_json::json!("yes"));
        assert_eq!(
            object["oneOf"][1]["replies"]["totalItems"],
            serde_json::json!(1)
        );
    }

    #[test]
    fn apply_activitypub_poll_fields_uses_any_of_for_multiple_choice() {
        let poll = StatusPollRow {
            id: "poll-1".to_owned(),
            status_id: "status-1".to_owned(),
            multiple: 1,
            hide_totals: 0,
            expires_at: "2026-02-01T00:00:00Z".to_owned(),
        };
        let options = vec![
            StatusPollOptionRow {
                title: "rust".to_owned(),
                votes_count: 2,
            },
            StatusPollOptionRow {
                title: "workers".to_owned(),
                votes_count: 3,
            },
        ];
        let mut object = serde_json::json!({
            "type": "Note",
            "id": "https://social.example/users/alice/statuses/status-1",
        });

        apply_activitypub_poll_fields(&mut object, &poll, &options, 4, false);
        assert_eq!(object["type"], serde_json::json!("Question"));
        assert!(object.get("oneOf").is_none());
        assert_eq!(object["anyOf"][0]["name"], serde_json::json!("rust"));
        assert_eq!(
            object["anyOf"][1]["replies"]["totalItems"],
            serde_json::json!(3)
        );
        assert!(object.get("closed").is_none());
    }

    #[test]
    fn outbound_terminal_failure_marks_follow_as_failed_only_for_follow() {
        assert_eq!(
            outbound_terminal_failure_follow_state("Follow"),
            Some("failed")
        );
        assert_eq!(outbound_terminal_failure_follow_state("Undo"), None);
        assert_eq!(outbound_terminal_failure_follow_state("Like"), None);
    }

    #[test]
    fn instance_base_url_normalizes_bare_domain() {
        let config = AppConfig::new("example.com", "cfwdon", "test instance");
        assert_eq!(instance_base_url(&config), "https://example.com");
    }

    #[test]
    fn instance_base_url_preserves_explicit_scheme() {
        let config = AppConfig::new("https://social.example.com", "cfwdon", "test instance");
        assert_eq!(instance_base_url(&config), "https://social.example.com");
    }

    #[test]
    fn classify_media_kind_detects_supported_types() {
        assert_eq!(
            classify_media_kind("image/png").map(media_kind_label),
            Some("image")
        );
        assert_eq!(
            classify_media_kind("video/mp4").map(media_kind_label),
            Some("video")
        );
        assert_eq!(
            classify_media_kind("audio/ogg").map(media_kind_label),
            Some("audio")
        );
        assert_eq!(classify_media_kind("application/pdf"), None);
    }

    #[test]
    fn parse_http_url_parts_keeps_path_and_query() {
        let (host, path) =
            parse_http_url_parts("https://remote.example/inbox/shared?foo=bar#ignored").unwrap();
        assert_eq!(host, "remote.example");
        assert_eq!(path, "/inbox/shared?foo=bar");
    }

    #[test]
    fn parse_http_url_parts_adds_root_for_bare_query() {
        let (host, path) = parse_http_url_parts("https://remote.example?foo=bar").unwrap();
        assert_eq!(host, "remote.example");
        assert_eq!(path, "/?foo=bar");
    }

    #[test]
    fn delivery_retry_delay_backoff_steps_up() {
        assert_eq!(delivery_retry_delay_modifier(1), "+1 minute");
        assert_eq!(delivery_retry_delay_modifier(2), "+5 minutes");
        assert_eq!(delivery_retry_delay_modifier(3), "+15 minutes");
        assert_eq!(delivery_retry_delay_modifier(4), "+60 minutes");
        assert_eq!(delivery_retry_delay_modifier(8), "+60 minutes");
    }

    #[test]
    fn follow_targets_local_actor_accepts_string_and_object_forms() {
        assert!(follow_targets_local_actor(
            Some(&serde_json::json!("https://example.com/users/alice")),
            "https://example.com/users/alice",
        ));
        assert!(follow_targets_local_actor(
            Some(&serde_json::json!({"id": "https://example.com/users/alice"})),
            "https://example.com/users/alice",
        ));
        assert!(!follow_targets_local_actor(
            Some(&serde_json::json!("https://example.com/users/bob")),
            "https://example.com/users/alice",
        ));
    }

    #[test]
    fn is_follow_undo_accepts_follow_object_for_same_actor() {
        assert!(is_follow_undo(
            Some(&serde_json::json!({
                "type": "Follow",
                "actor": "https://remote.example/users/bob",
            })),
            "https://remote.example/users/bob",
            "https://remote.example/@bob",
        ));
        assert!(!is_follow_undo(
            Some(&serde_json::json!({
                "type": "Like",
                "actor": "https://remote.example/users/bob",
            })),
            "https://remote.example/users/bob",
            "https://remote.example/@bob",
        ));
    }

    #[test]
    fn extract_inbox_target_username_supports_follow_undo_accept_reject_and_create() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Follow",
                    "object": "https://social.example/users/alice",
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Accept",
                    "object": {
                        "type": "Follow",
                        "actor": "https://social.example/users/alice",
                        "object": "https://remote.example/users/bob"
                    }
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Reject",
                    "object": {
                        "type": "Follow",
                        "actor": "https://social.example/users/alice",
                        "object": "https://remote.example/users/bob"
                    }
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Undo",
                    "object": {
                        "type": "Follow",
                        "object": "https://social.example/users/alice",
                    }
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Create",
                    "object": {
                        "to": ["https://social.example/users/alice"]
                    }
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Create",
                    "object": {
                        "to": ["https://www.w3.org/ns/activitystreams#Public"],
                        "cc": ["https://social.example/users/alice/followers"]
                    }
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Update",
                    "object": {
                        "to": ["https://www.w3.org/ns/activitystreams#Public"],
                        "cc": ["https://social.example/users/alice/followers"]
                    }
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Like",
                    "object": "https://social.example/users/alice/statuses/status-1"
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Undo",
                    "object": {
                        "type": "Create",
                        "object": {
                            "type": "Note",
                            "inReplyTo": "https://social.example/users/alice/statuses/status-1"
                        }
                    }
                })
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            extract_inbox_target_username(
                &config,
                &serde_json::json!({
                    "type": "Undo",
                    "object": {
                        "type": "Announce",
                        "object": "https://social.example/users/alice/statuses/status-1"
                    }
                })
            ),
            Some("alice".to_owned())
        );
    }

    #[test]
    fn local_username_from_actor_uri_matches_local_users_only() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        assert_eq!(
            local_username_from_actor_uri(&config, "https://social.example/users/alice"),
            Some("alice".to_owned())
        );
        assert_eq!(
            local_username_from_actor_uri(&config, "https://remote.example/users/alice"),
            None
        );
        assert_eq!(
            local_username_from_actor_uri(&config, "https://social.example/@alice"),
            None
        );
    }

    #[test]
    fn local_username_from_status_uri_matches_local_statuses_only() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        assert_eq!(
            local_username_from_status_uri(
                &config,
                "https://social.example/users/alice/statuses/status-1"
            ),
            Some("alice".to_owned())
        );
        assert_eq!(
            local_username_from_status_uri(
                &config,
                "https://remote.example/users/alice/statuses/status-1"
            ),
            None
        );
    }

    #[test]
    fn visibility_from_activitypub_object_detects_public_and_unlisted() {
        assert_eq!(
            visibility_from_activitypub_object(&serde_json::json!({
                "to": ["https://www.w3.org/ns/activitystreams#Public"]
            })),
            "public"
        );
        assert_eq!(
            visibility_from_activitypub_object(&serde_json::json!({
                "cc": ["https://www.w3.org/ns/activitystreams#Public"]
            })),
            "unlisted"
        );
        assert_eq!(
            visibility_from_activitypub_object(&serde_json::json!({
                "to": ["https://social.example/users/alice/followers"]
            })),
            "private"
        );
    }

    #[test]
    fn remote_account_rest_id_round_trips_actor_uri() {
        let actor_uri = "https://remote.example/users/alice";
        let id = remote_account_rest_id(actor_uri);
        assert_eq!(
            remote_actor_uri_from_rest_id(&id).as_deref(),
            Some(actor_uri)
        );
    }

    #[test]
    fn parse_lookup_handle_defaults_bare_username_to_local_domain() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let handle = parse_lookup_handle("alice", &config).unwrap();
        assert_eq!(handle.username, "alice");
        assert_eq!(handle.domain.as_deref(), Some("social.example"));
    }

    #[test]
    fn search_category_flags_defaults_to_all_categories() {
        assert_eq!(
            search_category_flags(None),
            SearchCategoryFlags {
                accounts: true,
                statuses: true,
                hashtags: true,
            }
        );
    }

    #[test]
    fn search_category_flags_respects_explicit_type() {
        assert_eq!(
            search_category_flags(Some("accounts")),
            SearchCategoryFlags {
                accounts: true,
                statuses: false,
                hashtags: false,
            }
        );
        assert_eq!(
            search_category_flags(Some("statuses")),
            SearchCategoryFlags {
                accounts: false,
                statuses: true,
                hashtags: false,
            }
        );
        assert_eq!(
            search_category_flags(Some("hashtags")),
            SearchCategoryFlags {
                accounts: false,
                statuses: false,
                hashtags: true,
            }
        );
    }

    #[test]
    fn search_v2_requires_auth_for_resolve_following_and_offset() {
        assert!(search_v2_requires_auth(&SearchV2Query {
            resolve: Some(true),
            ..SearchV2Query::default()
        }));
        assert!(search_v2_requires_auth(&SearchV2Query {
            following: Some(true),
            ..SearchV2Query::default()
        }));
        assert!(search_v2_requires_auth(&SearchV2Query {
            offset: Some(1),
            ..SearchV2Query::default()
        }));
        assert!(!search_v2_requires_auth(&SearchV2Query::default()));
    }

    #[test]
    fn search_v2_limit_matches_mastodon_bounds() {
        assert_eq!(search_v2_limit(None), 20);
        assert_eq!(search_v2_limit(Some(0)), 1);
        assert_eq!(search_v2_limit(Some(5)), 5);
        assert_eq!(search_v2_limit(Some(80)), 40);
    }

    #[test]
    fn search_text_match_rank_prefers_exact_then_prefix_then_contains() {
        assert_eq!(search_text_match_rank("alice", "alice"), 0);
        assert_eq!(search_text_match_rank("ali", "alice"), 1);
        assert_eq!(search_text_match_rank("lic", "alice"), 2);
        assert_eq!(search_text_match_rank("bob", "alice"), 3);
    }

    #[test]
    fn tag_search_rank_prefers_exact_matches() {
        assert!(tag_search_rank("rust", "rust") < tag_search_rank("rust", "rustlang"));
        assert!(tag_search_rank("rust", "rustlang") < tag_search_rank("rust", "fedirust"));
    }

    #[test]
    fn resolve_search_tag_name_supports_hash_and_tag_urls() {
        assert_eq!(resolve_search_tag_name("#Rust"), Some("rust".to_owned()));
        assert_eq!(
            resolve_search_tag_name("https://social.example/tags/Rust"),
            Some("rust".to_owned())
        );
        assert_eq!(
            resolve_search_tag_name("https://social.example/explore/tags/Workers"),
            Some("workers".to_owned())
        );
        assert_eq!(
            resolve_search_tag_name("/tags/fediverse_test"),
            Some("fediverse_test".to_owned())
        );
    }

    #[test]
    fn resolve_search_tag_name_rejects_non_tag_queries() {
        assert_eq!(resolve_search_tag_name("rust"), None);
        assert_eq!(
            resolve_search_tag_name("https://social.example/@alice"),
            None
        );
        assert_eq!(resolve_search_tag_name(""), None);
    }

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
        let mentions =
            extract_account_handles_from_text("@alice @bob@remote.example @alice", &config);
        assert_eq!(mentions.len(), 2);
        assert_eq!(mentions[0].username, "alice");
        assert_eq!(mentions[0].domain.as_deref(), Some("social.example"));
        assert_eq!(mentions[1].username, "bob");
        assert_eq!(mentions[1].domain.as_deref(), Some("remote.example"));
    }

    #[test]
    fn build_activitypub_delete_uses_status_audience_and_object_id() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test");
        let account = LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            discoverable: false,
            default_post_visibility: "public".to_owned(),
            default_sensitive: false,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };
        let status = StatusRow {
            id: "status-1".to_owned(),
            account_id: account.id.clone(),
            ap_id: None,
            in_reply_to_id: None,
            content_html: "<p>hello</p>".to_owned(),
            _text_content: "hello".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 0,
            language: Some("en".to_owned()),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };

        let activity = build_activitypub_delete_with_published_at(
            &config,
            &account,
            &status,
            "2026-01-02T00:00:00.000Z",
        )
        .unwrap();
        assert_eq!(activity.get("type"), Some(&serde_json::json!("Delete")));
        assert_eq!(
            activity.get("object"),
            Some(&serde_json::json!(
                "https://social.example/users/alice/statuses/status-1"
            ))
        );
        assert_eq!(
            activity.get("published"),
            Some(&serde_json::json!("2026-01-02T00:00:00.000Z"))
        );
        assert_eq!(
            activity.get("to"),
            Some(&serde_json::json!([
                "https://www.w3.org/ns/activitystreams#Public"
            ]))
        );
        assert_eq!(
            activity.pointer("/cc/0"),
            Some(&serde_json::json!(
                "https://social.example/users/alice/followers"
            ))
        );
    }

    #[test]
    fn matches_tag_timeline_filters_applies_any_all_none() {
        let tags = vec![
            "rust".to_owned(),
            "workers".to_owned(),
            "activitypub".to_owned(),
        ];
        assert!(matches_tag_timeline_filters(
            &tags,
            "rust",
            &TagTimelineQuery::default()
        ));
        assert!(matches_tag_timeline_filters(
            &tags,
            "rust",
            &TagTimelineQuery {
                any: Some(vec!["workers".to_owned(), "d1".to_owned()]),
                all: Some(vec!["activitypub".to_owned()]),
                ..TagTimelineQuery::default()
            }
        ));
        assert!(!matches_tag_timeline_filters(
            &tags,
            "rust",
            &TagTimelineQuery {
                none: Some(vec!["workers".to_owned()]),
                ..TagTimelineQuery::default()
            }
        ));
    }

    #[test]
    fn tag_timeline_source_flags_default_to_both_sources() {
        assert!(include_local_source(None, None));
        assert!(include_remote_source(None, None));
        assert!(include_local_source(Some(true), Some(false)));
        assert!(!include_remote_source(Some(true), Some(false)));
        assert!(!include_local_source(Some(false), Some(true)));
        assert!(include_remote_source(Some(false), Some(true)));
    }

    #[test]
    fn parse_media_focus_accepts_valid_coordinates() {
        assert_eq!(
            parse_media_focus(Some("0.25,-0.5")).unwrap(),
            Some((0.25, -0.5))
        );
        assert_eq!(parse_media_focus(Some("")).unwrap(), None);
        assert_eq!(parse_media_focus(None).unwrap(), None);
    }

    #[test]
    fn parse_media_focus_rejects_invalid_coordinates() {
        assert!(parse_media_focus(Some("1.5,0")).is_err());
        assert!(parse_media_focus(Some("abc,0")).is_err());
        assert!(parse_media_focus(Some("0")).is_err());
    }

    #[test]
    fn media_urls_prefer_custom_domain_and_keep_worker_fallback() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.media_public_base_url = Some("https://media.example.com".to_owned());
        assert_eq!(
            media_object_url(&config, "media/account/image/abc"),
            "https://media.example.com/media/account/image/abc"
        );
        assert_eq!(
            media_fallback_url(&config, "abc"),
            "https://social.example/media/abc"
        );
    }

    #[test]
    fn mastodon_report_response_serializes_forwarded_and_nullable_status_ids() {
        let target_account = MastodonAccountResponse {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            acct: "alice".to_owned(),
            display_name: "Alice".to_owned(),
            locked: false,
            bot: false,
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
            note: String::new(),
            url: "https://social.example/@alice".to_owned(),
            avatar: String::new(),
            avatar_static: String::new(),
            header: String::new(),
            header_static: String::new(),
            fields: Vec::new(),
            followers_count: 0,
            following_count: 0,
            statuses_count: 0,
            source: None,
        };
        let response = MastodonReportResponse {
            id: "report-1".to_owned(),
            action_taken: false,
            action_taken_at: None,
            category: "other".to_owned(),
            comment: "context".to_owned(),
            forwarded: false,
            created_at: "2026-01-02T00:00:00.000Z".to_owned(),
            status_ids: None,
            target_account,
            rule_ids: None,
        };

        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["forwarded"], serde_json::json!(false));
        assert!(value.get("forward").is_none());
        assert_eq!(value["status_ids"], serde_json::Value::Null);
        assert_eq!(value["rule_ids"], serde_json::Value::Null);
    }

    #[test]
    fn extract_remote_profile_media_url_supports_string_object_and_array_shapes() {
        assert_eq!(
            extract_remote_profile_media_url(Some(&serde_json::json!(
                "https://cdn.example/avatar.png"
            ))),
            Some("https://cdn.example/avatar.png".to_owned())
        );
        assert_eq!(
            extract_remote_profile_media_url(Some(&serde_json::json!({
                "type": "Image",
                "url": {
                    "type": "Link",
                    "href": "https://cdn.example/header.webp"
                }
            }))),
            Some("https://cdn.example/header.webp".to_owned())
        );
        assert_eq!(
            extract_remote_profile_media_url(Some(&serde_json::json!([
                {"type": "Image", "url": "https://cdn.example/first.png"},
                {"type": "Image", "url": "https://cdn.example/second.png"}
            ]))),
            Some("https://cdn.example/first.png".to_owned())
        );
        assert_eq!(
            extract_remote_profile_media_url(Some(&serde_json::json!("javascript:alert(1)"))),
            None
        );
    }

    #[test]
    fn remote_account_response_uses_cached_profile_media() {
        let actor = RemoteActorRow {
            actor_uri: "https://remote.example/users/alice".to_owned(),
            username: "alice".to_owned(),
            domain: "remote.example".to_owned(),
            display_name: "Alice".to_owned(),
            summary_html: "<p>hello</p>".to_owned(),
            profile_url: Some("https://remote.example/@alice".to_owned()),
            avatar_url: Some("https://cdn.remote.example/avatar.png".to_owned()),
            header_url: Some("https://cdn.remote.example/header.png".to_owned()),
        };

        let response = MastodonAccountResponse::from_remote_actor(&actor);
        assert_eq!(response.avatar, "https://cdn.remote.example/avatar.png");
        assert_eq!(response.header, "https://cdn.remote.example/header.png");
        assert_eq!(response.url, "https://remote.example/@alice");
    }

    #[test]
    fn mastodon_account_fields_render_urls_as_links() {
        let fields = vec![ProfileField {
            name: "Website".to_owned(),
            value: "https://example.com".to_owned(),
        }];
        let rendered = mastodon_account_fields(&fields);
        assert_eq!(rendered[0]["name"], serde_json::json!("Website"));
        assert!(
            rendered[0]["value"]
                .as_str()
                .unwrap_or_default()
                .contains("<a href=\"https://example.com\"")
        );
    }

    #[test]
    fn activitypub_profile_attachments_use_property_value_shape() {
        let fields = vec![ProfileField {
            name: "Pronouns".to_owned(),
            value: "they/them".to_owned(),
        }];
        let rendered = activitypub_profile_attachments(&fields);
        assert_eq!(rendered[0]["type"], serde_json::json!("PropertyValue"));
        assert_eq!(rendered[0]["name"], serde_json::json!("Pronouns"));
        assert_eq!(rendered[0]["value"], serde_json::json!("they/them"));
    }

    #[test]
    fn build_update_person_activity_wraps_actor_document() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let account = LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: "<p>hello</p>".to_owned(),
            bio_text: "hello".to_owned(),
            fields: vec![ProfileField {
                name: "Website".to_owned(),
                value: "https://example.com".to_owned(),
            }],
            discoverable: true,
            default_post_visibility: "public".to_owned(),
            default_sensitive: false,
            default_language: Some("en".to_owned()),
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };

        let activity = serde_json::from_str::<serde_json::Value>(
            &build_update_person_activity_with_id(
                &config,
                &account,
                "https://social.example/users/alice/updates/test-update",
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(activity["type"], serde_json::json!("Update"));
        assert_eq!(
            activity["id"],
            serde_json::json!("https://social.example/users/alice/updates/test-update")
        );
        assert_eq!(
            activity["object"]["id"],
            serde_json::json!("https://social.example/users/alice")
        );
        assert_eq!(activity["object"]["discoverable"], serde_json::json!(true));
        assert_eq!(
            activity["object"]["attachment"][0]["name"],
            serde_json::json!("Website")
        );
    }

    #[test]
    fn parse_remote_actor_profile_document_extracts_profile_fields() {
        let actor = serde_json::json!({
            "id": "https://remote.example/users/alice",
            "type": "Person",
            "preferredUsername": "Alice",
            "name": "Alice Example",
            "summary": "<p>remote bio</p>",
            "inbox": "https://remote.example/users/alice/inbox",
            "endpoints": {
                "sharedInbox": "https://remote.example/inbox"
            },
            "publicKey": {
                "id": "https://remote.example/users/alice#main-key",
                "publicKeyPem": "pem"
            },
            "url": "https://remote.example/@alice",
            "icon": {
                "type": "Image",
                "url": "https://cdn.remote.example/avatar.png"
            },
            "image": {
                "type": "Image",
                "url": "https://cdn.remote.example/header.png"
            }
        });

        let profile =
            parse_remote_actor_profile_document(&actor, "https://remote.example/users/fallback")
                .unwrap();
        assert_eq!(profile.actor_uri, "https://remote.example/users/alice");
        assert_eq!(profile.username, "alice");
        assert_eq!(profile.domain, "remote.example");
        assert_eq!(
            profile.inbox_uri,
            "https://remote.example/users/alice/inbox"
        );
        assert_eq!(
            profile.shared_inbox_uri.as_deref(),
            Some("https://remote.example/inbox")
        );
        assert_eq!(
            profile.public_key_id,
            "https://remote.example/users/alice#main-key"
        );
        assert_eq!(profile.display_name, "Alice Example");
        assert_eq!(profile.summary_html, "<p>remote bio</p>");
        assert_eq!(
            profile.profile_url.as_deref(),
            Some("https://remote.example/@alice")
        );
        assert_eq!(
            profile.avatar_url.as_deref(),
            Some("https://cdn.remote.example/avatar.png")
        );
        assert_eq!(
            profile.header_url.as_deref(),
            Some("https://cdn.remote.example/header.png")
        );
    }

    #[test]
    fn activitypub_actor_type_detection_matches_supported_profile_types() {
        assert!(is_activitypub_actor_type(Some("Person")));
        assert!(is_activitypub_actor_type(Some("Application")));
        assert!(is_activitypub_actor_type(Some("Group")));
        assert!(!is_activitypub_actor_type(Some("Note")));
        assert!(!is_activitypub_actor_type(None));
    }

    #[test]
    fn normalize_status_poll_accepts_minimal_valid_poll() {
        let poll = normalize_status_poll(Some(CreateStatusPollRequest {
            options: Some(vec![" One ".to_owned(), "Two".to_owned(), String::new()]),
            expires_in: Some(600),
            multiple: Some(true),
            hide_totals: Some(true),
        }))
        .unwrap()
        .unwrap();

        assert_eq!(poll.options, vec!["One".to_owned(), "Two".to_owned()]);
        assert_eq!(poll.expires_in_seconds, 600);
        assert!(poll.multiple);
        assert!(poll.hide_totals);
    }

    #[test]
    fn normalize_status_poll_rejects_invalid_shapes() {
        assert!(
            normalize_status_poll(Some(CreateStatusPollRequest {
                options: Some(vec!["Only one".to_owned()]),
                expires_in: Some(600),
                multiple: None,
                hide_totals: None,
            }))
            .is_err()
        );
        assert!(
            normalize_status_poll(Some(CreateStatusPollRequest {
                options: Some(vec!["One".to_owned(), "Two".to_owned()]),
                expires_in: Some(60),
                multiple: None,
                hide_totals: None,
            }))
            .is_err()
        );
    }

    #[test]
    fn is_admin_account_matches_configured_emails() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.admin_emails = vec!["admin@example.com".to_owned()];
        let mut account = LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "admin@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: Vec::new(),
            discoverable: false,
            default_post_visibility: "public".to_owned(),
            default_sensitive: false,
            default_language: None,
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2026-01-01T00:00:00.000Z".to_owned(),
        };
        assert!(is_admin_account(&config, &account));

        account.access_email = "user@example.com".to_owned();
        assert!(!is_admin_account(&config, &account));
    }

    #[test]
    fn directory_order_defaults_to_active_and_accepts_new() {
        assert_eq!(directory_order(None), super::DirectoryOrder::Active);
        assert_eq!(
            directory_order(Some("active")),
            super::DirectoryOrder::Active
        );
        assert_eq!(directory_order(Some("new")), super::DirectoryOrder::New);
        assert_eq!(directory_order(Some("NEW")), super::DirectoryOrder::New);
        assert_eq!(
            directory_order(Some("unexpected")),
            super::DirectoryOrder::Active
        );
    }

    #[test]
    fn parse_csv_list_normalizes_and_deduplicates() {
        assert_eq!(
            parse_csv_list("Ja, en,ja ,, EN"),
            vec!["en".to_owned(), "ja".to_owned()]
        );
    }

    #[test]
    fn notification_timestamp_sort_token_supports_sqlite_and_iso_shapes() {
        assert!(notification_timestamp_sort_token("2026-04-14 12:34:56").is_some());
        assert!(notification_timestamp_sort_token("2026-04-14T12:34:56.000Z").is_some());
        assert!(notification_timestamp_sort_token("not-a-date").is_none());
    }

    #[test]
    fn notification_sort_key_orders_newer_timestamps_higher() {
        assert!(
            notification_sort_key("2026-04-14T12:34:56.000Z")
                > notification_sort_key("2026-04-14 12:33:56")
        );
    }

    #[test]
    fn instance_v2_document_uses_conservative_defaults() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.source_url = Some("https://codeberg.example/cfwdon".to_owned());
        config.instance_languages = vec!["ja".to_owned(), "en".to_owned()];
        config.contact_email = Some("admin@example.com".to_owned());
        config.instance_thumbnail_url = Some("https://media.example.com/site.png".to_owned());

        let document = build_instance_v2_document(
            &InstanceSummary {
                domain: "social.example".to_owned(),
                title: "cfwdon".to_owned(),
                description: "test instance".to_owned(),
                software: SoftwareInfo {
                    name: "cfwdon".to_owned(),
                    version: "0.1.0".to_owned(),
                },
                capabilities: InstanceCapabilities {
                    federation: true,
                    local_timeline: true,
                    media_uploads: true,
                },
            },
            &config,
            3,
        );

        assert_eq!(
            document.get("domain"),
            Some(&serde_json::json!("social.example"))
        );
        assert_eq!(
            document.get("source_url"),
            Some(&serde_json::json!("https://codeberg.example/cfwdon"))
        );
        assert_eq!(
            document.pointer("/usage/users/active_month"),
            Some(&serde_json::json!(3))
        );
        assert_eq!(
            document.pointer("/api_versions/mastodon"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            document.pointer("/configuration/polls/max_options"),
            Some(&serde_json::json!(0))
        );
        assert_eq!(
            document.pointer("/registrations/enabled"),
            Some(&serde_json::json!(false))
        );
        assert_eq!(
            document.pointer("/contact/email"),
            Some(&serde_json::json!("admin@example.com"))
        );
    }

    #[test]
    fn instance_v2_document_advertises_configured_policy_urls() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.instance_extended_description_html = Some("<p>About</p>".to_owned());
        config.privacy_policy_html = Some("<p>Privacy</p>".to_owned());
        config.terms_of_service_html = Some("<p>Terms</p>".to_owned());

        let document = build_instance_v2_document(
            &InstanceSummary {
                domain: "social.example".to_owned(),
                title: "cfwdon".to_owned(),
                description: "test instance".to_owned(),
                software: SoftwareInfo {
                    name: "cfwdon".to_owned(),
                    version: "0.1.0".to_owned(),
                },
                capabilities: InstanceCapabilities {
                    federation: true,
                    local_timeline: true,
                    media_uploads: true,
                },
            },
            &config,
            3,
        );

        assert_eq!(
            document.pointer("/configuration/urls/about"),
            Some(&serde_json::json!(
                "https://social.example/api/v1/instance/extended_description"
            ))
        );
        assert_eq!(
            document.pointer("/configuration/urls/privacy_policy"),
            Some(&serde_json::json!(
                "https://social.example/api/v1/instance/privacy_policy"
            ))
        );
        assert_eq!(
            document.pointer("/configuration/urls/terms_of_service"),
            Some(&serde_json::json!(
                "https://social.example/api/v1/instance/terms_of_service"
            ))
        );
    }

    #[test]
    fn instance_v1_document_reports_mastodon_compatible_shape() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.contact_email = Some("admin@example.com".to_owned());
        config.instance_thumbnail_url = Some("https://media.example.com/site.png".to_owned());

        let document = build_instance_v1_document(
            &InstanceSummary {
                domain: "social.example".to_owned(),
                title: "cfwdon".to_owned(),
                description: "test instance".to_owned(),
                software: SoftwareInfo {
                    name: "cfwdon".to_owned(),
                    version: "0.1.0".to_owned(),
                },
                capabilities: InstanceCapabilities {
                    federation: true,
                    local_timeline: true,
                    media_uploads: true,
                },
            },
            &config,
            2,
            5,
            9,
            4,
        );

        assert_eq!(
            document.get("uri"),
            Some(&serde_json::json!("social.example"))
        );
        assert_eq!(
            document.pointer("/stats/user_count"),
            Some(&serde_json::json!(5))
        );
        assert_eq!(
            document.pointer("/stats/status_count"),
            Some(&serde_json::json!(9))
        );
        assert_eq!(
            document.pointer("/stats/domain_count"),
            Some(&serde_json::json!(4))
        );
        assert_eq!(
            document.pointer("/contact_account"),
            Some(&serde_json::Value::Null)
        );
    }

    #[test]
    fn build_nodeinfo_documents_expose_expected_urls_and_counts() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let summary = InstanceSummary {
            domain: "social.example".to_owned(),
            title: "cfwdon".to_owned(),
            description: "test instance".to_owned(),
            software: SoftwareInfo {
                name: "cfwdon".to_owned(),
                version: "0.1.0".to_owned(),
            },
            capabilities: InstanceCapabilities {
                federation: true,
                local_timeline: true,
                media_uploads: true,
            },
        };

        let links = build_nodeinfo_links_document(&config);
        assert_eq!(
            links["links"][0]["href"],
            serde_json::json!(nodeinfo_url(&config))
        );

        let document = build_nodeinfo_document(&summary, &config, 5, 3, 8);
        assert_eq!(document["protocols"][0], serde_json::json!("activitypub"));
        assert_eq!(document["usage"]["users"]["total"], serde_json::json!(5));
        assert_eq!(
            document["usage"]["users"]["activeMonth"],
            serde_json::json!(3)
        );
        assert_eq!(document["usage"]["localPosts"], serde_json::json!(8));
    }

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

    #[test]
    fn peer_authority_from_uri_normalizes_default_and_custom_ports() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test");
        assert_eq!(
            peer_authority_from_uri(&config, "https://remote.example/users/alice"),
            Some("remote.example".to_owned())
        );
        assert_eq!(
            peer_authority_from_uri(&config, "https://remote.example:8443/users/alice"),
            Some("remote.example:8443".to_owned())
        );
        assert_eq!(
            peer_authority_from_uri(&config, "https://social.example/users/alice"),
            None
        );
    }
}
