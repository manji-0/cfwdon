use super::{AppConfig, InstanceSummary, nodeinfo_url};

pub(crate) fn build_nodeinfo_links_document(config: &AppConfig) -> serde_json::Value {
    serde_json::json!({
        "links": [
            {
                "rel": "http://nodeinfo.diaspora.software/ns/schema/2.0",
                "href": nodeinfo_url(config),
            }
        ]
    })
}

pub(crate) fn build_nodeinfo_document(
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
