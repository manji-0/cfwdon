/// Persistence-ready moderation report before D1 insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredReportIntent {
    pub report_id: String,
    pub target_account_id: String,
    pub target_remote_actor_uri: Option<String>,
    pub comment: String,
    pub category: String,
    pub forward: bool,
}

impl StoredReportIntent {
    pub fn new(
        report_id: impl Into<String>,
        target_account_id: impl Into<String>,
        target_remote_actor_uri: Option<String>,
        comment: impl Into<String>,
        category: impl Into<String>,
        forward: bool,
    ) -> Self {
        Self {
            report_id: report_id.into(),
            target_account_id: target_account_id.into(),
            target_remote_actor_uri,
            comment: comment.into(),
            category: category.into(),
            forward,
        }
    }
}

/// Most characters of a report comment cfwdon keeps: the local report API's
/// limit, applied by truncation to comments from other servers.
pub const REPORT_COMMENT_MAX_CHARS: usize = 1000;

/// Most `object` entries of one inbound `Flag` that are looked up, so one
/// activity cannot fan out into unbounded D1 reads.
pub const INBOUND_FLAG_MAX_OBJECTS: usize = 100;

/// A local resource one `object` URI of an inbound `Flag` names. URIs that name
/// nothing local are dropped before this point, as Mastodon drops them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlaggedObject {
    LocalAccount {
        account_id: String,
    },
    LocalStatus {
        status_id: String,
        account_id: String,
    },
}

/// One report an inbound `Flag` files on a local account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboundFlagReport {
    pub target_account_id: String,
    /// Flagged statuses authored by the target, in `object` order.
    pub status_ids: Vec<String>,
}

/// Mastodon's `ActivityPub::Activity::Flag`: one report per local account the
/// `Flag` names, carrying only the named statuses that account wrote. A status
/// alone does not make its author a target.
pub fn inbound_flag_reports(objects: &[FlaggedObject]) -> Vec<InboundFlagReport> {
    let mut reports: Vec<InboundFlagReport> = Vec::new();
    for object in objects {
        if let FlaggedObject::LocalAccount { account_id } = object
            && !reports
                .iter()
                .any(|report| &report.target_account_id == account_id)
        {
            reports.push(InboundFlagReport {
                target_account_id: account_id.clone(),
                status_ids: Vec::new(),
            });
        }
    }
    for object in objects {
        let FlaggedObject::LocalStatus {
            status_id,
            account_id,
        } = object
        else {
            continue;
        };
        if let Some(report) = reports
            .iter_mut()
            .find(|report| &report.target_account_id == account_id)
            && !report.status_ids.contains(status_id)
        {
            report.status_ids.push(status_id.clone());
        }
    }
    reports
}

/// The `Flag` id a report is keyed on for redelivery. Like Mastodon's
/// `report_uri`, an id on another host than the sending actor is not trusted,
/// so a sender cannot claim another server's report id.
pub fn inbound_flag_report_uri<'a>(
    activity_id: Option<&'a str>,
    actor_uri: &str,
) -> Option<&'a str> {
    activity_id.map(str::trim).filter(|id| {
        !id.is_empty() && crate::federation::remote_http_authorities_match(id, actor_uri)
    })
}

/// A remote report's comment: the `Flag` `content`, trimmed and truncated to
/// [`REPORT_COMMENT_MAX_CHARS`].
pub fn inbound_flag_comment(content: Option<&str>) -> String {
    content
        .unwrap_or_default()
        .trim()
        .chars()
        .take(REPORT_COMMENT_MAX_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str) -> FlaggedObject {
        FlaggedObject::LocalAccount {
            account_id: id.to_owned(),
        }
    }

    fn status(id: &str, author: &str) -> FlaggedObject {
        FlaggedObject::LocalStatus {
            status_id: id.to_owned(),
            account_id: author.to_owned(),
        }
    }

    #[test]
    fn inbound_flag_files_one_report_per_named_account_with_its_own_statuses() {
        let reports = inbound_flag_reports(&[
            status("s-1", "alice"),
            account("alice"),
            status("s-2", "carol"),
            account("bob"),
            status("s-3", "bob"),
            account("alice"),
            status("s-1", "alice"),
        ]);

        assert_eq!(
            reports,
            vec![
                InboundFlagReport {
                    target_account_id: "alice".to_owned(),
                    status_ids: vec!["s-1".to_owned()],
                },
                InboundFlagReport {
                    target_account_id: "bob".to_owned(),
                    status_ids: vec!["s-3".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn inbound_flag_naming_only_statuses_files_nothing() {
        assert!(inbound_flag_reports(&[status("s-1", "alice")]).is_empty());
        assert!(inbound_flag_reports(&[]).is_empty());
    }

    #[test]
    fn inbound_flag_report_uri_requires_the_actor_host() {
        let actor = "https://remote.example/actor";
        assert_eq!(
            inbound_flag_report_uri(Some(" https://remote.example/flags/1 "), actor),
            Some("https://remote.example/flags/1")
        );
        assert_eq!(
            inbound_flag_report_uri(Some("https://other.example/flags/1"), actor),
            None
        );
        assert_eq!(inbound_flag_report_uri(Some(""), actor), None);
        assert_eq!(inbound_flag_report_uri(None, actor), None);
    }

    #[test]
    fn inbound_flag_comment_is_trimmed_and_bounded() {
        assert_eq!(inbound_flag_comment(None), "");
        assert_eq!(inbound_flag_comment(Some("  spam \n")), "spam");
        let long = "あ".repeat(REPORT_COMMENT_MAX_CHARS + 5);
        assert_eq!(
            inbound_flag_comment(Some(&long)).chars().count(),
            REPORT_COMMENT_MAX_CHARS
        );
    }

    #[test]
    fn stored_report_intent_preserves_fields() {
        let intent = StoredReportIntent::new(
            "report-1",
            "acct-target",
            Some("https://remote.example/users/bob".to_owned()),
            "spam",
            "other",
            true,
        );

        assert_eq!(intent.report_id, "report-1");
        assert_eq!(intent.target_account_id, "acct-target");
        assert!(intent.forward);
    }
}
