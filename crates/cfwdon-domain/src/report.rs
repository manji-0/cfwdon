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

#[cfg(test)]
mod tests {
    use super::*;

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
