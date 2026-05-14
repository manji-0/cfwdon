use super::{
    AppConfig, LocalAccount, activitypub_profile_attachments, actor_url, media_object_url,
    public_key_id, shared_inbox_url,
};

#[derive(Debug, serde::Serialize)]
pub(crate) struct ActivityPubActorResponse {
    #[serde(rename = "@context")]
    pub(crate) context: Vec<&'static str>,
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) actor_type: &'static str,
    #[serde(rename = "preferredUsername")]
    pub(crate) preferred_username: String,
    pub(crate) name: String,
    pub(crate) summary: String,
    pub(crate) inbox: String,
    pub(crate) outbox: String,
    pub(crate) followers: String,
    pub(crate) following: String,
    pub(crate) featured: String,
    #[serde(rename = "featuredTags")]
    pub(crate) featured_tags: String,
    pub(crate) url: String,
    pub(crate) endpoints: ActivityPubActorEndpoints,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) icon: Option<ActivityPubImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) image: Option<ActivityPubImage>,
    pub(crate) attachment: Vec<serde_json::Value>,
    #[serde(rename = "publicKey")]
    pub(crate) public_key: ActivityPubPublicKey,
    #[serde(rename = "manuallyApprovesFollowers")]
    pub(crate) manually_approves_followers: bool,
    pub(crate) discoverable: bool,
    pub(crate) published: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ActivityPubActorEndpoints {
    #[serde(rename = "sharedInbox")]
    pub(crate) shared_inbox: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ActivityPubPublicKey {
    pub(crate) id: String,
    pub(crate) owner: String,
    #[serde(rename = "publicKeyPem")]
    pub(crate) public_key_pem: String,
}

#[derive(Debug, serde::Serialize)]
pub(crate) struct ActivityPubImage {
    #[serde(rename = "type")]
    pub(crate) image_type: &'static str,
    #[serde(rename = "mediaType")]
    pub(crate) media_type: String,
    pub(crate) url: String,
}

pub(crate) fn build_activitypub_actor_document(
    config: &AppConfig,
    account: &LocalAccount,
) -> ActivityPubActorResponse {
    let actor_url = actor_url(config, &account.username);
    let public_key_id = public_key_id(config, &account.username);
    let icon = account
        .avatar_object_key
        .as_ref()
        .zip(account.avatar_content_type.as_ref())
        .map(|(object_key, content_type)| ActivityPubImage {
            image_type: "Image",
            media_type: content_type.clone(),
            url: media_object_url(config, object_key),
        });
    let image = account
        .header_object_key
        .as_ref()
        .zip(account.header_content_type.as_ref())
        .map(|(object_key, content_type)| ActivityPubImage {
            image_type: "Image",
            media_type: content_type.clone(),
            url: media_object_url(config, object_key),
        });

    ActivityPubActorResponse {
        context: vec![
            "https://www.w3.org/ns/activitystreams",
            "https://w3id.org/security/v1",
        ],
        id: actor_url.clone(),
        actor_type: if account.bot { "Service" } else { "Person" },
        preferred_username: account.username.clone(),
        name: account.display_name.clone(),
        summary: account.bio_html.clone(),
        inbox: format!("{actor_url}/inbox"),
        outbox: format!("{actor_url}/outbox"),
        followers: format!("{actor_url}/followers"),
        following: format!("{actor_url}/following"),
        featured: format!("{actor_url}/collections/featured"),
        featured_tags: format!("{actor_url}/collections/tags"),
        url: actor_url.clone(),
        endpoints: ActivityPubActorEndpoints {
            shared_inbox: shared_inbox_url(config),
        },
        icon,
        image,
        attachment: activitypub_profile_attachments(&account.fields),
        public_key: ActivityPubPublicKey {
            id: public_key_id,
            owner: actor_url.clone(),
            public_key_pem: account.public_key_pem.clone(),
        },
        manually_approves_followers: account.locked,
        discoverable: account.discoverable,
        published: account.created_at.clone(),
    }
}
