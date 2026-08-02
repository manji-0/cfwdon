use super::notifications::{
    MastodonNotificationResponse, NotificationEntry, notification_account_matches_filter,
    notification_type_allowed, push_notification_entry,
};
use super::{
    AppConfig, BoostTargetPreload, LocalStatusViewerStatePreload, MastodonAccountResponse,
    MastodonPollResponsePreload, MastodonStatusResponse, MediaAttachmentRow,
    MentionAccountsPreload, NotificationsQuery, RemoteActorRow, RemoteMastodonPollResponsePreload,
    RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload,
    RemoteStatusFederatedEmojisPreload, RemoteStatusNotificationRow, RemoteStatusRecord,
    RemoteStatusRow, RemoteStatusViewerStatePreload, StatusApplicationPreload, StatusCountsPreload,
    StatusQuoteCountsPreload, StatusRow, actor_url,
    build_local_status_response_with_timeline_preloads,
    build_remote_status_response_with_timeline_preloads, can_view_local_status,
    config_with_resolved_custom_emojis, find_accounts_by_ids, find_media_attachments_by_status_ids,
    find_remote_actors_by_actor_uris, find_remote_status_attachments_by_status_ids,
    list_local_status_notifications_for_account, list_remote_status_notifications_for_account,
    load_in_reply_to_account_ids, preload_boost_targets, preload_local_status_viewer_state,
    preload_mastodon_poll_responses, preload_mention_accounts_from_texts,
    preload_remote_mastodon_poll_responses, preload_remote_status_edit_updated_at,
    preload_remote_status_federated_emojis, preload_remote_status_viewer_state,
    preload_status_applications, preload_status_counts, preload_status_quote_counts,
    remote_account_rest_id, remote_status_from_record, strip_html_tags,
};
use cfwdon_domain::LocalAccount;
use std::collections::{HashMap, HashSet};
use worker::{Result, d1::D1Type};

use crate::D1Database;
fn remote_status_notification_row(status: &RemoteStatusNotificationRow) -> Option<RemoteStatusRow> {
    remote_status_from_record(RemoteStatusRecord {
        id: status.id.clone(),
        actor_uri: status.actor_uri.clone(),
        object_uri: status.object_uri.clone(),
        url: status.url.clone(),
        in_reply_to_uri: status.in_reply_to_uri.clone(),
        boost_of_uri: status.boost_of_uri.clone(),
        quote_of_uri: status.quote_of_uri.clone(),
        content_html: status.content_html.clone(),
        spoiler_text: status.spoiler_text.clone(),
        visibility: status.visibility.clone(),
        sensitive: status.sensitive,
        language: status.language.clone(),
        quote_state: status.quote_state.clone(),
        published_at: status.published_at.clone(),
    })
    .ok()
}

pub(crate) struct NotificationStatusPreloads {
    pub(crate) local_accounts_by_id: HashMap<String, LocalAccount>,
    pub(crate) local_media_by_status_id: HashMap<String, Vec<MediaAttachmentRow>>,
    pub(crate) remote_actors_by_uri: HashMap<String, RemoteActorRow>,
    pub(crate) remote_attachments_by_status_id: HashMap<String, Vec<RemoteStatusAttachmentRow>>,
    pub(crate) in_reply_to_account_ids: HashMap<String, String>,
    pub(crate) resolved_config: AppConfig,
    pub(crate) counts: StatusCountsPreload,
    pub(crate) quote_counts: StatusQuoteCountsPreload,
    pub(crate) local_polls: MastodonPollResponsePreload,
    pub(crate) local_viewer_state: LocalStatusViewerStatePreload,
    pub(crate) remote_viewer_state: RemoteStatusViewerStatePreload,
    pub(crate) remote_polls: RemoteMastodonPollResponsePreload,
    pub(crate) remote_edit_updated_at: RemoteStatusEditUpdatedAtPreload,
    pub(crate) remote_federated_emojis: RemoteStatusFederatedEmojisPreload,
    pub(crate) applications: StatusApplicationPreload,
    pub(crate) mentions: MentionAccountsPreload,
    pub(crate) boost_targets: BoostTargetPreload,
    pub(crate) muted_notification_actor_uris: HashSet<String>,
}

impl NotificationStatusPreloads {
    pub(crate) fn is_notification_muted(&self, actor_uri: &str) -> bool {
        self.muted_notification_actor_uris.contains(actor_uri)
    }

    pub(crate) fn local_media(&self, status_id: &str) -> Vec<MediaAttachmentRow> {
        self.local_media_by_status_id
            .get(status_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn remote_media(&self, status_id: &str) -> Vec<RemoteStatusAttachmentRow> {
        self.remote_attachments_by_status_id
            .get(status_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) async fn build_local_status_response(
        &self,
        db: &D1Database,
        config: &AppConfig,
        viewer: &LocalAccount,
        status: &StatusRow,
        account: &LocalAccount,
        media: Vec<MediaAttachmentRow>,
    ) -> Result<MastodonStatusResponse> {
        build_local_status_response_with_timeline_preloads(
            db,
            config,
            Some(&self.resolved_config),
            Some(viewer),
            status,
            account,
            self.in_reply_to_account_ids.get(&status.id).cloned(),
            media,
            None,
            Some(&self.counts),
            Some(&self.quote_counts),
            Some(&self.local_polls),
            Some(&self.local_viewer_state),
            Some(&self.applications),
            Some(&self.mentions),
            Some(&self.boost_targets),
        )
        .await
    }

    pub(crate) async fn build_remote_status_response(
        &self,
        db: &D1Database,
        config: &AppConfig,
        viewer: &LocalAccount,
        status: &RemoteStatusRow,
        actor: &RemoteActorRow,
        media: Vec<RemoteStatusAttachmentRow>,
    ) -> Result<MastodonStatusResponse> {
        build_remote_status_response_with_timeline_preloads(
            db,
            config,
            Some(viewer),
            status,
            actor,
            None,
            Some(&self.counts),
            Some(&self.quote_counts),
            Some(&self.remote_viewer_state),
            Some(&self.remote_polls),
            Some(&self.remote_edit_updated_at),
            Some(&self.remote_federated_emojis),
            media,
            Some(&self.mentions),
            Some(&self.boost_targets),
            None,
            None,
            None,
        )
        .await
    }
}

pub(crate) fn build_status_notification_entry(
    id: String,
    notification_type: &str,
    created_at: String,
    account: MastodonAccountResponse,
    status: MastodonStatusResponse,
) -> NotificationEntry {
    let mut entries = Vec::new();
    push_notification_entry(
        &mut entries,
        MastodonNotificationResponse {
            group_key: id.clone(),
            id,
            notification_type: notification_type.to_owned(),
            created_at,
            account,
            status: Some(status),
            report: None,
        },
    );
    entries
        .pop()
        .expect("status notification entry is pushed before it is returned")
}

pub(crate) async fn preload_notification_statuses(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    local_statuses: &[StatusRow],
    remote_statuses: &[RemoteStatusRow],
    additional_local_account_ids: &[String],
    additional_remote_actor_uris: &[String],
) -> Result<NotificationStatusPreloads> {
    if local_statuses.is_empty() && remote_statuses.is_empty() {
        return Ok(NotificationStatusPreloads {
            local_accounts_by_id: HashMap::new(),
            local_media_by_status_id: HashMap::new(),
            remote_actors_by_uri: HashMap::new(),
            remote_attachments_by_status_id: HashMap::new(),
            in_reply_to_account_ids: HashMap::new(),
            resolved_config: config.clone(),
            counts: StatusCountsPreload::default(),
            quote_counts: StatusQuoteCountsPreload::default(),
            local_polls: MastodonPollResponsePreload::default(),
            local_viewer_state: LocalStatusViewerStatePreload::default(),
            remote_viewer_state: RemoteStatusViewerStatePreload::default(),
            remote_polls: RemoteMastodonPollResponsePreload::default(),
            remote_edit_updated_at: RemoteStatusEditUpdatedAtPreload::default(),
            remote_federated_emojis: RemoteStatusFederatedEmojisPreload::default(),
            applications: StatusApplicationPreload::default(),
            mentions: MentionAccountsPreload::default(),
            boost_targets: BoostTargetPreload::default(),
            muted_notification_actor_uris: HashSet::new(),
        });
    }

    let local_status_refs = local_statuses.iter().collect::<Vec<_>>();
    let local_status_ids = local_statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();
    let remote_status_ids = remote_statuses
        .iter()
        .map(|status| status.id.clone())
        .collect::<Vec<_>>();

    let mut local_account_ids = local_statuses
        .iter()
        .map(|status| status.account_id.clone())
        .collect::<Vec<_>>();
    local_account_ids.extend(additional_local_account_ids.iter().cloned());
    let mut remote_actor_uris = remote_statuses
        .iter()
        .map(|status| status.actor_uri.clone())
        .collect::<Vec<_>>();
    remote_actor_uris.extend(additional_remote_actor_uris.iter().cloned());

    let mut mention_texts_owned = local_statuses
        .iter()
        .map(|status| status.text.clone())
        .collect::<Vec<_>>();
    mention_texts_owned.extend(
        remote_statuses
            .iter()
            .map(|status| strip_html_tags(&status.content_html)),
    );
    let mention_texts = mention_texts_owned
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();

    let mut boost_uris = Vec::new();
    let mut seen_boost_uris = HashSet::new();
    for uri in local_statuses
        .iter()
        .filter_map(|status| status.boost_of_uri.as_ref())
        .chain(
            remote_statuses
                .iter()
                .filter_map(|status| status.boost_of_uri.as_ref()),
        )
    {
        if seen_boost_uris.insert(uri.as_str()) {
            boost_uris.push(uri.clone());
        }
    }

    let (
        local_accounts_by_id,
        local_media_by_status_id,
        remote_actors_by_uri,
        remote_attachments_by_status_id,
        in_reply_to_account_ids,
        counts,
        local_polls,
        local_viewer_state,
        remote_polls,
        remote_edit_updated_at,
        remote_federated_emojis,
        applications,
        mentions,
        resolved_config,
        boost_targets,
    ) = futures_util::try_join!(
        find_accounts_by_ids(db, &local_account_ids),
        find_media_attachments_by_status_ids(db, &local_status_ids),
        find_remote_actors_by_actor_uris(db, &remote_actor_uris),
        find_remote_status_attachments_by_status_ids(db, &remote_status_ids),
        load_in_reply_to_account_ids(db, local_statuses),
        preload_status_counts(db, &local_status_ids, &remote_status_ids),
        preload_mastodon_poll_responses(db, &local_status_ids, Some(viewer)),
        preload_local_status_viewer_state(db, viewer.id(), &local_status_refs, None),
        preload_remote_mastodon_poll_responses(db, &remote_status_ids, Some(viewer)),
        preload_remote_status_edit_updated_at(db, &remote_status_ids),
        preload_remote_status_federated_emojis(db, &remote_status_ids),
        preload_status_applications(db, config, &local_status_refs),
        preload_mention_accounts_from_texts(db, config, &mention_texts),
        config_with_resolved_custom_emojis(db, config),
        preload_boost_targets(db, config, &boost_uris),
    )?;

    let remote_status_refs = remote_statuses
        .iter()
        .filter_map(|status| {
            remote_actors_by_uri
                .get(&status.actor_uri)
                .map(|actor| (status, actor))
        })
        .collect::<Vec<_>>();
    let mut quote_uris = remote_statuses
        .iter()
        .map(|status| status.object_uri.clone())
        .collect::<Vec<_>>();
    quote_uris.extend(local_statuses.iter().filter_map(|status| {
        local_accounts_by_id
            .get(&status.account_id)
            .map(|account| local_status_quote_count_uri(config, status, account))
    }));

    let mut notification_actor_uris = Vec::new();
    let mut seen_notification_actor_uris = HashSet::new();
    for account_id in local_statuses
        .iter()
        .map(|status| status.account_id.as_str())
        .chain(additional_local_account_ids.iter().map(String::as_str))
    {
        if let Some(account) = local_accounts_by_id.get(account_id) {
            let uri = actor_url(config, account.username());
            if seen_notification_actor_uris.insert(uri.clone()) {
                notification_actor_uris.push(uri);
            }
        }
    }
    for actor_uri in remote_statuses
        .iter()
        .map(|status| status.actor_uri.as_str())
        .chain(additional_remote_actor_uris.iter().map(String::as_str))
    {
        if seen_notification_actor_uris.insert(actor_uri.to_owned()) {
            notification_actor_uris.push(actor_uri.to_owned());
        }
    }

    let (quote_counts, remote_viewer_state, muted_notification_actor_uris) = futures_util::try_join!(
        preload_status_quote_counts(db, &quote_uris),
        preload_remote_status_viewer_state(db, viewer.id(), &remote_status_refs),
        preload_notification_mutes(db, viewer.id(), &notification_actor_uris),
    )?;

    Ok(NotificationStatusPreloads {
        local_accounts_by_id,
        local_media_by_status_id,
        remote_actors_by_uri,
        remote_attachments_by_status_id,
        in_reply_to_account_ids,
        resolved_config,
        counts,
        quote_counts,
        local_polls,
        local_viewer_state,
        remote_viewer_state,
        remote_polls,
        remote_edit_updated_at,
        remote_federated_emojis,
        applications,
        mentions,
        boost_targets,
        muted_notification_actor_uris,
    })
}

fn local_status_quote_count_uri(
    config: &AppConfig,
    status: &StatusRow,
    account: &LocalAccount,
) -> String {
    status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, account.username()),
            status.id
        )
    })
}

async fn preload_notification_mutes(
    db: &D1Database,
    account_id: &str,
    actor_uris: &[String],
) -> Result<HashSet<String>> {
    let mut seen = HashSet::new();
    let actor_uris = actor_uris
        .iter()
        .filter(|uri| seen.insert(uri.as_str()))
        .collect::<Vec<_>>();
    if actor_uris.is_empty() {
        return Ok(HashSet::new());
    }

    let placeholders = (2..=(actor_uris.len() + 1))
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut bindings = Vec::with_capacity(actor_uris.len() + 1);
    bindings.push(D1Type::Text(account_id));
    bindings.extend(actor_uris.iter().map(|uri| D1Type::Text(uri.as_str())));

    let delete_sql = format!(
        "DELETE FROM mutes
         WHERE account_id = ?1
           AND target_actor_uri IN ({placeholders})
           AND expires_at IS NOT NULL
           AND expires_at <= CURRENT_TIMESTAMP"
    );
    db.prepare(&delete_sql)
        .bind_refs(bindings.iter())?
        .run()
        .await?;

    #[derive(Debug, serde::Deserialize)]
    struct NotificationMuteActorRow {
        target_actor_uri: String,
    }

    let select_sql = format!(
        "SELECT target_actor_uri
         FROM mutes
         WHERE account_id = ?1
           AND notifications != 0
           AND target_actor_uri IN ({placeholders})"
    );
    let result = db
        .prepare(&select_sql)
        .bind_refs(bindings.iter())?
        .all()
        .await?;
    Ok(result
        .results::<NotificationMuteActorRow>()?
        .into_iter()
        .map(|row| row.target_actor_uri)
        .collect())
}

pub(crate) async fn collect_status_notification_entries(
    entries: &mut Vec<NotificationEntry>,
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &NotificationsQuery,
    per_type_limit: u32,
) -> Result<()> {
    if !notification_type_allowed(query, "status") {
        return Ok(());
    }

    let (local_statuses, remote_status_rows) = futures_util::try_join!(
        list_local_status_notifications_for_account(db, viewer.id(), per_type_limit),
        list_remote_status_notifications_for_account(db, viewer.id(), per_type_limit),
    )?;
    let remote_statuses = remote_status_rows
        .iter()
        .filter_map(remote_status_notification_row)
        .collect::<Vec<_>>();
    let preloads = preload_notification_statuses(
        db,
        config,
        viewer,
        &local_statuses,
        &remote_statuses,
        &[],
        &[],
    )
    .await?;
    let preloads_ref = &preloads;

    let mut local_candidates = Vec::new();
    for status in local_statuses {
        let Some(actor) = preloads.local_accounts_by_id.get(&status.account_id) else {
            continue;
        };
        if !can_view_local_status(db, &status, Some(viewer), actor).await?
            || preloads.is_notification_muted(&actor_url(config, actor.username()))
            || !notification_account_matches_filter(query.account_id.as_deref(), actor.id(), None)
        {
            continue;
        }
        local_candidates.push((status, actor.clone()));
    }

    let local_entries = futures_util::future::try_join_all(local_candidates.into_iter().map(
        |(status, actor)| async move {
            let status_response = preloads_ref
                .build_local_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    &actor,
                    preloads_ref.local_media(&status.id),
                )
                .await?;
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("status-local-{}-{}", actor.id(), status.id),
                "status",
                status.created_at,
                MastodonAccountResponse::from_account(&actor, config),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(local_entries);

    let mut remote_candidates = Vec::new();
    for raw_status in remote_status_rows {
        if raw_status.visibility == "direct" {
            continue;
        }
        let Some(status) = remote_status_notification_row(&raw_status) else {
            continue;
        };
        let Some(actor) = preloads.remote_actors_by_uri.get(&status.actor_uri) else {
            continue;
        };
        if preloads.is_notification_muted(&actor.actor_uri) {
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
        remote_candidates.push((raw_status.published_at, status, actor, remote_id));
    }

    let remote_entries = futures_util::future::try_join_all(remote_candidates.into_iter().map(
        |(created_at, status, actor, remote_id)| async move {
            let status_response = preloads_ref
                .build_remote_status_response(
                    db,
                    config,
                    viewer,
                    &status,
                    actor,
                    preloads_ref.remote_media(&status.id),
                )
                .await?;
            Ok::<NotificationEntry, worker::Error>(build_status_notification_entry(
                format!("status-remote-{}-{}", remote_id, status.id),
                "status",
                created_at,
                MastodonAccountResponse::from_remote_actor(actor),
                status_response,
            ))
        },
    ))
    .await?;
    entries.extend(remote_entries);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_domain::{LocalAccountRecord, QuoteState, Visibility};

    fn test_config() -> AppConfig {
        AppConfig::new("https://social.example", "cfwdon", "test instance")
    }

    fn test_local_account(username: &str) -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", username))
    }

    fn test_local_status(ap_id: Option<String>) -> StatusRow {
        StatusRow {
            id: "status-1".to_owned(),
            account_id: "acct-1".to_owned(),
            ap_id,
            in_reply_to_id: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hello</p>".to_owned(),
            text: "Hello".to_owned(),
            spoiler_text: String::new(),
            visibility: Visibility::Public,
            sensitive: false,
            language: Some("ja".to_owned()),
            quote_approval_policy: None,
            quote_state: QuoteState::Accepted,
            application_id: None,
            created_at: "2026-01-02T00:00:00.000Z".to_owned(),
            updated_at: None,
        }
    }

    fn test_remote_notification_row() -> RemoteStatusNotificationRow {
        RemoteStatusNotificationRow {
            id: "remote-status-1".to_owned(),
            actor_uri: "https://remote.example/users/bob".to_owned(),
            object_uri: "https://remote.example/users/bob/statuses/1".to_owned(),
            url: Some("https://remote.example/@bob/1".to_owned()),
            in_reply_to_uri: None,
            boost_of_uri: None,
            quote_of_uri: None,
            content_html: "<p>Hi</p>".to_owned(),
            spoiler_text: String::new(),
            visibility: "public".to_owned(),
            sensitive: 1,
            language: Some("en".to_owned()),
            quote_state: "accepted".to_owned(),
            published_at: "2026-01-02T00:00:00Z".to_owned(),
        }
    }

    fn empty_preloads() -> NotificationStatusPreloads {
        NotificationStatusPreloads {
            local_accounts_by_id: HashMap::new(),
            local_media_by_status_id: HashMap::new(),
            remote_actors_by_uri: HashMap::new(),
            remote_attachments_by_status_id: HashMap::new(),
            in_reply_to_account_ids: HashMap::new(),
            resolved_config: test_config(),
            counts: StatusCountsPreload::default(),
            quote_counts: StatusQuoteCountsPreload::default(),
            local_polls: MastodonPollResponsePreload::default(),
            local_viewer_state: LocalStatusViewerStatePreload::default(),
            remote_viewer_state: RemoteStatusViewerStatePreload::default(),
            remote_polls: RemoteMastodonPollResponsePreload::default(),
            remote_edit_updated_at: RemoteStatusEditUpdatedAtPreload::default(),
            remote_federated_emojis: RemoteStatusFederatedEmojisPreload::default(),
            applications: StatusApplicationPreload::default(),
            mentions: MentionAccountsPreload::default(),
            boost_targets: BoostTargetPreload::default(),
            muted_notification_actor_uris: HashSet::new(),
        }
    }

    #[test]
    fn local_status_quote_count_uri_uses_ap_id_when_present() {
        let config = test_config();
        let account = test_local_account("alice");
        let status = test_local_status(Some(
            "https://social.example/users/alice/statuses/status-1".to_owned(),
        ));

        assert_eq!(
            local_status_quote_count_uri(&config, &status, &account),
            "https://social.example/users/alice/statuses/status-1"
        );
    }

    #[test]
    fn local_status_quote_count_uri_falls_back_to_actor_status_url() {
        let config = test_config();
        let account = test_local_account("alice");
        let status = test_local_status(None);

        assert_eq!(
            local_status_quote_count_uri(&config, &status, &account),
            "https://social.example/users/alice/statuses/status-1"
        );
    }

    #[test]
    fn remote_status_notification_row_maps_valid_row() {
        let status = remote_status_notification_row(&test_remote_notification_row())
            .expect("valid row maps to a remote status");

        assert_eq!(status.id, "remote-status-1");
        assert_eq!(status.actor_uri, "https://remote.example/users/bob");
        assert_eq!(
            status.object_uri,
            "https://remote.example/users/bob/statuses/1"
        );
        assert_eq!(status.url.as_deref(), Some("https://remote.example/@bob/1"));
        assert_eq!(status.content_html, "<p>Hi</p>");
        assert_eq!(status.visibility, Visibility::Public);
        assert!(status.sensitive);
        assert_eq!(status.language.as_deref(), Some("en"));
        assert_eq!(status.quote_state, QuoteState::Accepted);
        assert_eq!(status.published_at, "2026-01-02T00:00:00Z");
    }

    #[test]
    fn remote_status_notification_row_rejects_invalid_visibility() {
        let mut row = test_remote_notification_row();
        row.visibility = "invalid".to_owned();

        assert!(remote_status_notification_row(&row).is_none());
    }

    #[test]
    fn notification_preloads_default_mute_and_media_accessors() {
        let preloads = empty_preloads();

        assert!(!preloads.is_notification_muted("https://remote.example/users/bob"));
        assert!(preloads.local_media("status-1").is_empty());
        assert!(preloads.remote_media("status-1").is_empty());
    }

    #[test]
    fn notification_preloads_accessors_return_stored_values() {
        let mut preloads = empty_preloads();
        preloads
            .muted_notification_actor_uris
            .insert("https://remote.example/users/bob".to_owned());
        preloads.local_media_by_status_id.insert(
            "status-1".to_owned(),
            vec![MediaAttachmentRow {
                id: "media-1".to_owned(),
                account_id: "acct-1".to_owned(),
                status_id: Some("status-1".to_owned()),
                object_key: "media/status-1/1".to_owned(),
                content_type: "image/png".to_owned(),
                description: String::new(),
                focus_x: None,
                focus_y: None,
                width: Some(640),
                height: Some(480),
                _created_at: "2026-01-02T00:00:00Z".to_owned(),
            }],
        );

        assert!(preloads.is_notification_muted("https://remote.example/users/bob"));
        assert!(!preloads.is_notification_muted("https://remote.example/users/carol"));
        assert_eq!(preloads.local_media("status-1").len(), 1);
        assert_eq!(preloads.local_media("status-1")[0].id, "media-1");
        assert!(preloads.local_media("status-2").is_empty());
    }
}
