//! Bulk resolution of boost (`Announce`) targets.
//!
//! Rendering a boost needs the status it points at. Resolving that one URI at a
//! time costs up to four round trips per boost -- an `ap_id` lookup, a local
//! id/owner check, then a remote `object_uri`/`url` lookup -- which a
//! boost-heavy timeline page multiplies by its item count. This module resolves
//! every URI on a page in a fixed number of batched queries instead.

use super::{LocalStatusViewerStatePreload, RemoteStatusViewerStatePreload, StoredMentionsPreload};
use super::{find_local_status_by_object_uri, find_statuses_by_ap_ids, find_statuses_by_ids};
use crate::accounts::find_accounts_by_ids;
use crate::activitypub::local_status_identity_from_uri;
use crate::custom_emojis::RemoteStatusFederatedEmojisPreload;
use crate::media::RemoteStatusAttachmentRow;
use crate::remote::RemoteMastodonPollResponsePreload;
use crate::remote::{
    find_remote_status_by_url_or_object_uri, find_remote_statuses_by_url_or_object_uris,
};
use crate::store::remote::RemoteActorRow;
use crate::tracked_d1::D1Database;
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalStatus, RemoteStatus};
use std::collections::{HashMap, HashSet};
use worker::Result;

/// The status a boost points at, once resolved.
#[derive(Debug, Clone)]
pub(crate) enum BoostTarget {
    Local(LocalStatus),
    Remote(RemoteStatus),
}

/// Boost targets resolved for a batch of URIs.
///
/// Distinguishes "resolved to nothing" from "not looked up": only the former
/// lets a caller skip its own lookup.
#[derive(Debug, Default)]
pub(crate) struct BoostTargetPreload {
    by_uri: HashMap<String, BoostTarget>,
    resolved_uris: HashSet<String>,
    embed: Option<QuoteEmbedPreloads>,
}

/// Per-target data the timeline preloads for resolved targets, so a quote
/// embed renders from memory instead of issuing its own lookups.
#[derive(Debug, Default)]
pub(crate) struct QuoteEmbedPreloads {
    pub(crate) remote_actors: HashMap<String, RemoteActorRow>,
    pub(crate) remote_attachments: HashMap<String, Vec<RemoteStatusAttachmentRow>>,
    pub(crate) federated_emojis: RemoteStatusFederatedEmojisPreload,
    pub(crate) remote_polls: RemoteMastodonPollResponsePreload,
    pub(crate) local_viewer_state: Option<LocalStatusViewerStatePreload>,
    pub(crate) remote_viewer_state: Option<RemoteStatusViewerStatePreload>,
    pub(crate) local_mentions: StoredMentionsPreload,
    pub(crate) remote_mentions: StoredMentionsPreload,
    pub(crate) remote_in_reply_to: HashMap<String, Option<String>>,
}

impl BoostTargetPreload {
    pub(crate) fn set_embed_preloads(&mut self, embed: QuoteEmbedPreloads) {
        self.embed = Some(embed);
    }

    pub(crate) fn embed_preloads(&self) -> Option<&QuoteEmbedPreloads> {
        self.embed.as_ref()
    }

    /// `None` when `uri` was never looked up, `Some(None)` when it was looked
    /// up and matched nothing.
    pub(crate) fn target(&self, uri: &str) -> Option<Option<&BoostTarget>> {
        self.resolved_uris
            .contains(uri)
            .then(|| self.by_uri.get(uri))
    }

    pub(crate) fn resolved_targets(&self) -> impl Iterator<Item = &BoostTarget> {
        self.by_uri.values()
    }
}

/// Resolves a single boost target, for callers rendering one status.
///
/// [`preload_boost_targets`] batches this same precedence across a page.
pub(crate) async fn resolve_boost_target(
    db: &D1Database,
    config: &AppConfig,
    uri: &str,
) -> Result<Option<BoostTarget>> {
    if let Some(status) = find_local_status_by_object_uri(db, config, uri).await? {
        return Ok(Some(BoostTarget::Local(status)));
    }
    Ok(find_remote_status_by_url_or_object_uri(db, uri)
        .await?
        .map(BoostTarget::Remote))
}

/// Resolves `uris` the same way [`super::find_local_status_by_object_uri`] and
/// [`super::find_remote_status_by_url_or_object_uri`] would, preserving their
/// precedence: local `ap_id`, then local instance URL with a matching owner,
/// then remote `object_uri`/`url`.
pub(crate) async fn preload_boost_targets(
    db: &D1Database,
    config: &AppConfig,
    uris: &[String],
) -> Result<BoostTargetPreload> {
    let mut resolved_uris = uris.iter().cloned().collect::<HashSet<_>>();
    if resolved_uris.is_empty() {
        return Ok(BoostTargetPreload::default());
    }
    let uris = resolved_uris.iter().cloned().collect::<Vec<_>>();

    // Local instance URLs are parsed without touching the database, so the
    // `ap_id` lookup and the parsed-id lookup can share one wave.
    let local_identities = uris
        .iter()
        .filter_map(|uri| {
            local_status_identity_from_uri(config, uri)
                .map(|(username, status_id)| (uri.clone(), username, status_id))
        })
        .collect::<Vec<_>>();
    let identity_status_ids = local_identities
        .iter()
        .map(|(_, _, status_id)| status_id.clone())
        .collect::<Vec<_>>();

    // Remote rows are looked up for every URI in the same wave; local matches
    // still win because remote rows only fill keys left unresolved below.
    let (by_ap_id, by_identity_id, remote_statuses) = futures_util::try_join!(
        find_statuses_by_ap_ids(db, &uris),
        find_statuses_by_ids(db, &identity_status_ids),
        find_remote_statuses_by_url_or_object_uris(db, &uris),
    )?;
    let by_ap_id = by_ap_id
        .into_iter()
        .filter_map(|status| status.ap_id.clone().map(|ap_id| (ap_id, status)))
        .collect::<HashMap<_, _>>();
    let by_identity_id = by_identity_id
        .into_iter()
        .map(|status| (status.id.clone(), status))
        .collect::<HashMap<_, _>>();

    let mut by_uri = HashMap::new();
    for uri in &uris {
        if let Some(status) = by_ap_id.get(uri) {
            by_uri.insert(uri.clone(), BoostTarget::Local(status.clone()));
        }
    }

    // The instance-URL form only counts when the URL's username is the status
    // owner, so the owning accounts have to be checked.
    let pending_identities = local_identities
        .iter()
        .filter(|(uri, _, _)| !by_uri.contains_key(uri))
        .filter_map(|(uri, username, status_id)| {
            by_identity_id
                .get(status_id)
                .map(|status| (uri, username, status))
        })
        .collect::<Vec<_>>();
    let identity_account_ids = pending_identities
        .iter()
        .map(|(_, _, status)| status.account_id.clone())
        .collect::<Vec<_>>();
    let identity_accounts = find_accounts_by_ids(db, &identity_account_ids).await?;
    for (uri, username, status) in pending_identities {
        let owner_matches = identity_accounts
            .get(&status.account_id)
            .is_some_and(|owner| owner.username().eq_ignore_ascii_case(username));
        if owner_matches {
            by_uri.insert(uri.clone(), BoostTarget::Local((*status).clone()));
        }
    }

    for status in remote_statuses {
        for key in [Some(status.object_uri.as_str()), status.url.as_deref()]
            .into_iter()
            .flatten()
        {
            if resolved_uris.contains(key) && !by_uri.contains_key(key) {
                by_uri.insert(key.to_owned(), BoostTarget::Remote(status.clone()));
            }
        }
    }

    resolved_uris.extend(by_uri.keys().cloned());
    Ok(BoostTargetPreload {
        by_uri,
        resolved_uris,
        embed: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boost_target_preload_distinguishes_absent_from_unknown() {
        let preload = BoostTargetPreload {
            by_uri: HashMap::new(),
            resolved_uris: HashSet::from(["https://example.test/statuses/1".to_owned()]),
            embed: None,
        };

        // Looked up, matched nothing: the caller must not retry.
        assert!(
            preload
                .target("https://example.test/statuses/1")
                .is_some_and(|target| target.is_none())
        );
        // Never looked up: the caller falls back to its own lookup.
        assert!(preload.target("https://example.test/statuses/2").is_none());
    }
}
