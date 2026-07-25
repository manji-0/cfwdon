use super::{
    AppConfig, InstanceSummary, instance_base_url, instance_open_registrations, nodeinfo_21_url,
    nodeinfo_url,
};

pub(crate) fn build_nodeinfo_links_document(config: &AppConfig) -> serde_json::Value {
    serde_json::json!({
        "links": [
            {
                "rel": "http://nodeinfo.diaspora.software/ns/schema/2.0",
                "href": nodeinfo_url(config),
            },
            {
                "rel": "http://nodeinfo.diaspora.software/ns/schema/2.1",
                "href": nodeinfo_21_url(config),
            }
        ]
    })
}

pub(crate) fn build_nodeinfo_document_with_halfyear(
    summary: &InstanceSummary,
    config: &AppConfig,
    user_count: u64,
    active_month: u64,
    active_halfyear: u64,
    status_count: u64,
) -> serde_json::Value {
    build_nodeinfo_schema_document(
        summary,
        config,
        "2.0",
        user_count,
        active_month,
        active_halfyear,
        status_count,
    )
}

pub(crate) fn build_nodeinfo_21_document(
    summary: &InstanceSummary,
    config: &AppConfig,
    user_count: u64,
    active_month: u64,
    active_halfyear: u64,
    status_count: u64,
) -> serde_json::Value {
    build_nodeinfo_schema_document(
        summary,
        config,
        "2.1",
        user_count,
        active_month,
        active_halfyear,
        status_count,
    )
}

fn build_nodeinfo_schema_document(
    summary: &InstanceSummary,
    config: &AppConfig,
    version: &str,
    user_count: u64,
    active_month: u64,
    active_halfyear: u64,
    status_count: u64,
) -> serde_json::Value {
    let mut software = serde_json::Map::new();
    software.insert("name".to_owned(), serde_json::json!(summary.software.name));
    software.insert(
        "version".to_owned(),
        serde_json::json!(summary.software.version),
    );
    if version == "2.1" {
        if let Some(source_url) = config.source_url.as_deref() {
            software.insert("repository".to_owned(), serde_json::json!(source_url));
            software.insert("homepage".to_owned(), serde_json::json!(source_url));
        } else {
            software.insert(
                "homepage".to_owned(),
                serde_json::json!(instance_base_url(config)),
            );
        }
    }

    serde_json::json!({
        "version": version,
        "software": software,
        "protocols": ["activitypub"],
        "services": {
            "inbound": [],
            "outbound": [],
        },
        "openRegistrations": instance_open_registrations(),
        "usage": {
            "users": {
                "total": user_count,
                "activeHalfyear": active_halfyear,
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

#[cfg(test)]
mod tests {
    use super::{
        build_nodeinfo_21_document, build_nodeinfo_document_with_halfyear,
        build_nodeinfo_links_document,
    };
    use crate::{
        AppConfig, InstanceCapabilities, InstanceSummary, SoftwareInfo,
        instance_open_registrations, nodeinfo_21_url, nodeinfo_url,
    };

    fn sample_summary() -> InstanceSummary {
        InstanceSummary {
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
        }
    }

    #[test]
    fn nodeinfo_document_includes_active_halfyear_as_number() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let document =
            build_nodeinfo_document_with_halfyear(&sample_summary(), &config, 5, 3, 4, 8);
        assert_eq!(
            document["usage"]["users"]["activeHalfyear"],
            serde_json::json!(4)
        );
        assert!(document["usage"]["users"]["activeHalfyear"].is_number());
    }

    #[test]
    fn nodeinfo_open_registrations_matches_instance_helper() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let document =
            build_nodeinfo_document_with_halfyear(&sample_summary(), &config, 1, 1, 1, 1);
        assert_eq!(
            document["openRegistrations"],
            serde_json::json!(instance_open_registrations())
        );
        assert!(!instance_open_registrations());
    }

    #[test]
    fn nodeinfo_links_advertise_schema_20_and_21() {
        let config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        let links = build_nodeinfo_links_document(&config);
        let link_array = links["links"].as_array().expect("links array");
        assert_eq!(link_array.len(), 2);
        assert_eq!(
            link_array[0]["rel"],
            serde_json::json!("http://nodeinfo.diaspora.software/ns/schema/2.0")
        );
        assert_eq!(
            link_array[0]["href"],
            serde_json::json!(nodeinfo_url(&config))
        );
        assert_eq!(
            link_array[1]["rel"],
            serde_json::json!("http://nodeinfo.diaspora.software/ns/schema/2.1")
        );
        assert_eq!(
            link_array[1]["href"],
            serde_json::json!(nodeinfo_21_url(&config))
        );
    }

    #[test]
    fn nodeinfo_21_document_includes_software_homepage_and_optional_repository() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.source_url = Some("https://github.com/example/cfwdon".to_owned());
        let document = build_nodeinfo_21_document(&sample_summary(), &config, 5, 3, 4, 8);
        assert_eq!(document["version"], serde_json::json!("2.1"));
        assert_eq!(
            document["software"]["repository"],
            serde_json::json!("https://github.com/example/cfwdon")
        );
        assert_eq!(
            document["software"]["homepage"],
            serde_json::json!("https://github.com/example/cfwdon")
        );
        assert_eq!(
            document["usage"]["users"]["activeHalfyear"],
            serde_json::json!(4)
        );
    }
}
