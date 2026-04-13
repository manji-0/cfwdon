use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaAttachment {
    pub id: String,
    pub object_key: String,
    pub content_type: String,
}
