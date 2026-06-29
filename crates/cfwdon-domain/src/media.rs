use crate::error::IdError;
use crate::ids::{MediaId, StatusId};
use serde::{Deserialize, Serialize};

/// Media stored for an account but not yet attached to a status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadedMedia {
    id: MediaId,
    object_key: String,
    content_type: String,
}

impl UploadedMedia {
    pub fn new(
        id: impl Into<String>,
        object_key: impl Into<String>,
        content_type: impl Into<String>,
    ) -> Result<Self, IdError> {
        Ok(Self {
            id: MediaId::new(id)?,
            object_key: object_key.into(),
            content_type: content_type.into(),
        })
    }

    pub fn id(&self) -> &MediaId {
        &self.id
    }

    pub fn object_key(&self) -> &str {
        &self.object_key
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub fn attach_to_status(self, status_id: StatusId) -> StatusBoundMedia {
        StatusBoundMedia {
            id: self.id,
            status_id,
            object_key: self.object_key,
            content_type: self.content_type,
        }
    }
}

/// Media attached to a published or publishing status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusBoundMedia {
    id: MediaId,
    status_id: StatusId,
    object_key: String,
    content_type: String,
}

impl StatusBoundMedia {
    pub fn id(&self) -> &MediaId {
        &self.id
    }

    pub fn status_id(&self) -> &StatusId {
        &self.status_id
    }

    pub fn object_key(&self) -> &str {
        &self.object_key
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }
}

/// Persistence-facing media attachment snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MediaAttachment {
    pub id: String,
    pub object_key: String,
    pub content_type: String,
}

impl MediaAttachment {
    pub fn from_uploaded(uploaded: &UploadedMedia) -> Self {
        Self {
            id: uploaded.id().as_str().to_owned(),
            object_key: uploaded.object_key().to_owned(),
            content_type: uploaded.content_type().to_owned(),
        }
    }
}

/// Persistence-ready media attachment row before D1 insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredMediaAttachmentIntent {
    pub media_id: String,
    pub account_id: String,
    pub object_key: String,
    pub content_type: String,
    pub description: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl StoredMediaAttachmentIntent {
    pub fn new(
        media_id: impl Into<String>,
        account_id: impl Into<String>,
        object_key: impl Into<String>,
        content_type: impl Into<String>,
        description: impl Into<String>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Self {
        Self {
            media_id: media_id.into(),
            account_id: account_id.into(),
            object_key: object_key.into(),
            content_type: content_type.into(),
            description: description.into(),
            width,
            height,
        }
    }
}
