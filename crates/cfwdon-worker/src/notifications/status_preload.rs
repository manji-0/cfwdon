use super::{
    AppConfig, BoostTargetPreload, LocalStatusViewerStatePreload, MastodonPollResponsePreload,
    MediaAttachmentRow, MentionAccountsPreload, RemoteActorRow, RemoteMastodonPollResponsePreload,
    RemoteStatusAttachmentRow, RemoteStatusEditUpdatedAtPreload,
    RemoteStatusFederatedEmojisPreload, RemoteStatusRow, RemoteStatusViewerStatePreload,
    StatusApplicationPreload, StatusCountsPreload, StatusQuoteCountsPreload, StatusRow, actor_url,
    config_with_resolved_custom_emojis, find_accounts_by_ids, find_media_attachments_by_status_ids,
    find_remote_actors_by_actor_uris, find_remote_status_attachments_by_status_ids,
    load_in_reply_to_account_ids, preload_boost_targets, preload_local_status_viewer_state,
    preload_mastodon_poll_responses, preload_mention_accounts_from_texts,
    preload_remote_mastodon_poll_responses, preload_remote_status_edit_updated_at,
    preload_remote_status_federated_emojis, preload_remote_status_viewer_state,
    preload_status_applications, preload_status_counts, preload_status_quote_counts,
};
use cfwdon_domain::LocalAccount;
use std::collections::{HashMap, HashSet};
use worker::{Result, d1::D1Type};

use crate::D1Database;

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
    ) -> Result<crate::MastodonStatusResponse> {
        crate::build_local_status_response_with_timeline_preloads(
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
    ) -> Result<crate::MastodonStatusResponse> {
        crate::build_remote_status_response_with_timeline_preloads(
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

struct NotificationPreloadPlan {
    local_status_ids: Vec<String>,
    remote_status_ids: Vec<String>,
    local_account_ids: Vec<String>,
    remote_actor_uris: Vec<String>,
    mention_texts: Vec<String>,
    boost_uris: Vec<String>,
}

fn empty_notification_status_preloads(config: &AppConfig) -> NotificationStatusPreloads {
    NotificationStatusPreloads {
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
    }
}

fn plan_notification_status_preloads(
    local_statuses: &[StatusRow],
    remote_statuses: &[RemoteStatusRow],
    additional_local_account_ids: &[String],
    additional_remote_actor_uris: &[String],
) -> NotificationPreloadPlan {
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

    let mut mention_texts = local_statuses
        .iter()
        .map(|status| status.text.clone())
        .collect::<Vec<_>>();
    mention_texts.extend(remote_statuses.iter().map(|status| status.plain_text()));

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

    NotificationPreloadPlan {
        local_status_ids,
        remote_status_ids,
        local_account_ids,
        remote_actor_uris,
        mention_texts,
        boost_uris,
    }
}

struct NotificationEntityPreloads {
    local_accounts_by_id: HashMap<String, LocalAccount>,
    local_media_by_status_id: HashMap<String, Vec<MediaAttachmentRow>>,
    remote_actors_by_uri: HashMap<String, RemoteActorRow>,
    remote_attachments_by_status_id: HashMap<String, Vec<RemoteStatusAttachmentRow>>,
    in_reply_to_account_ids: HashMap<String, String>,
    counts: StatusCountsPreload,
    local_polls: MastodonPollResponsePreload,
    local_viewer_state: LocalStatusViewerStatePreload,
    remote_polls: RemoteMastodonPollResponsePreload,
    remote_edit_updated_at: RemoteStatusEditUpdatedAtPreload,
    remote_federated_emojis: RemoteStatusFederatedEmojisPreload,
    applications: StatusApplicationPreload,
    mentions: MentionAccountsPreload,
    resolved_config: AppConfig,
    boost_targets: BoostTargetPreload,
}

async fn load_notification_entity_preloads(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    local_statuses: &[StatusRow],
    plan: &NotificationPreloadPlan,
) -> Result<NotificationEntityPreloads> {
    let local_status_refs = local_statuses.iter().collect::<Vec<_>>();
    let mention_text_refs = plan
        .mention_texts
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
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
        find_accounts_by_ids(db, &plan.local_account_ids),
        find_media_attachments_by_status_ids(db, &plan.local_status_ids),
        find_remote_actors_by_actor_uris(db, &plan.remote_actor_uris),
        find_remote_status_attachments_by_status_ids(db, &plan.remote_status_ids),
        load_in_reply_to_account_ids(db, local_statuses),
        preload_status_counts(db, &plan.local_status_ids, &plan.remote_status_ids),
        preload_mastodon_poll_responses(db, &plan.local_status_ids, Some(viewer)),
        preload_local_status_viewer_state(db, viewer.id(), &local_status_refs, None),
        preload_remote_mastodon_poll_responses(db, &plan.remote_status_ids, Some(viewer)),
        preload_remote_status_edit_updated_at(db, &plan.remote_status_ids),
        preload_remote_status_federated_emojis(db, &plan.remote_status_ids),
        preload_status_applications(db, config, &local_status_refs),
        preload_mention_accounts_from_texts(db, config, &mention_text_refs),
        config_with_resolved_custom_emojis(db, config),
        preload_boost_targets(db, config, &plan.boost_uris),
    )?;

    Ok(NotificationEntityPreloads {
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
    })
}

fn collect_notification_quote_uris(
    config: &AppConfig,
    local_statuses: &[StatusRow],
    remote_statuses: &[RemoteStatusRow],
    local_accounts_by_id: &HashMap<String, LocalAccount>,
) -> Vec<String> {
    let mut quote_uris = remote_statuses
        .iter()
        .map(|status| status.object_uri.clone())
        .collect::<Vec<_>>();
    quote_uris.extend(local_statuses.iter().filter_map(|status| {
        local_accounts_by_id
            .get(&status.account_id)
            .map(|account| local_status_quote_count_uri(config, status, account))
    }));
    quote_uris
}

fn collect_notification_actor_uris(
    config: &AppConfig,
    local_statuses: &[StatusRow],
    additional_local_account_ids: &[String],
    remote_statuses: &[RemoteStatusRow],
    additional_remote_actor_uris: &[String],
    local_accounts_by_id: &HashMap<String, LocalAccount>,
) -> Vec<String> {
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
    notification_actor_uris
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
        return Ok(empty_notification_status_preloads(config));
    }

    let plan = plan_notification_status_preloads(
        local_statuses,
        remote_statuses,
        additional_local_account_ids,
        additional_remote_actor_uris,
    );
    let entity_preloads =
        load_notification_entity_preloads(db, config, viewer, local_statuses, &plan).await?;

    let remote_status_refs = remote_statuses
        .iter()
        .filter_map(|status| {
            entity_preloads
                .remote_actors_by_uri
                .get(&status.actor_uri)
                .map(|actor| (status, actor))
        })
        .collect::<Vec<_>>();
    let quote_uris = collect_notification_quote_uris(
        config,
        local_statuses,
        remote_statuses,
        &entity_preloads.local_accounts_by_id,
    );
    let notification_actor_uris = collect_notification_actor_uris(
        config,
        local_statuses,
        additional_local_account_ids,
        remote_statuses,
        additional_remote_actor_uris,
        &entity_preloads.local_accounts_by_id,
    );

    let (quote_counts, remote_viewer_state, muted_notification_actor_uris) = futures_util::try_join!(
        preload_status_quote_counts(db, &quote_uris),
        preload_remote_status_viewer_state(db, viewer.id(), &remote_status_refs),
        preload_notification_mutes(db, viewer.id(), &notification_actor_uris),
    )?;

    Ok(NotificationStatusPreloads {
        local_accounts_by_id: entity_preloads.local_accounts_by_id,
        local_media_by_status_id: entity_preloads.local_media_by_status_id,
        remote_actors_by_uri: entity_preloads.remote_actors_by_uri,
        remote_attachments_by_status_id: entity_preloads.remote_attachments_by_status_id,
        in_reply_to_account_ids: entity_preloads.in_reply_to_account_ids,
        resolved_config: entity_preloads.resolved_config,
        counts: entity_preloads.counts,
        quote_counts,
        local_polls: entity_preloads.local_polls,
        local_viewer_state: entity_preloads.local_viewer_state,
        remote_viewer_state,
        remote_polls: entity_preloads.remote_polls,
        remote_edit_updated_at: entity_preloads.remote_edit_updated_at,
        remote_federated_emojis: entity_preloads.remote_federated_emojis,
        applications: entity_preloads.applications,
        mentions: entity_preloads.mentions,
        boost_targets: entity_preloads.boost_targets,
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
    Ok(crate::d1_results::<NotificationMuteActorRow>(&result)?
        .into_iter()
        .map(|row| row.target_actor_uri)
        .collect())
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
            in_reply_to_account_id: None,
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
            card_json: None,
            created_at: "2026-01-02T00:00:00.000Z".to_owned(),
            updated_at: None,
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
