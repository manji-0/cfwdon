use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SoftwareInfo {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceCapabilities {
    pub federation: bool,
    pub local_timeline: bool,
    pub media_uploads: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceSummary {
    pub domain: String,
    pub title: String,
    pub description: String,
    pub software: SoftwareInfo,
    pub capabilities: InstanceCapabilities,
}
