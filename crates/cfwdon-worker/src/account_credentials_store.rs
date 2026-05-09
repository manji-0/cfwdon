use super::{
    ProfileMediaUpload, UpdateCredentialsRequest, enqueue_profile_update_activities,
    find_account_by_id, generate_entity_id, profile_field_from_update, render_status_html,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, ProfileField};
use worker::d1::D1Type;
use worker::{Bucket, D1Database, Error, HttpMetadata, Result};

struct AccountSourceDefaults {
    post_visibility: String,
    quote_policy: String,
    sensitive: bool,
    language: Option<String>,
}

fn account_source_defaults(
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> AccountSourceDefaults {
    AccountSourceDefaults {
        post_visibility: update
            .source
            .as_ref()
            .and_then(|source| source.privacy.as_deref())
            .unwrap_or(account.default_post_visibility.as_str())
            .to_owned(),
        quote_policy: update
            .source
            .as_ref()
            .and_then(|source| source.quote_policy.as_deref())
            .unwrap_or(account.default_quote_policy.as_str())
            .to_owned(),
        sensitive: update
            .source
            .as_ref()
            .and_then(|source| source.sensitive)
            .unwrap_or(account.default_sensitive),
        language: update
            .source
            .as_ref()
            .and_then(|source| source.language.clone())
            .or_else(|| account.default_language.clone()),
    }
}

fn profile_media_was_replaced(previous: Option<&str>, next: Option<&(String, String)>) -> bool {
    match (previous, next) {
        (Some(previous), Some(next)) => next.0.as_str() != previous,
        _ => false,
    }
}

fn account_profile_fields(
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> Vec<ProfileField> {
    update
        .fields_attributes
        .as_ref()
        .map(|fields| {
            fields
                .iter()
                .filter_map(profile_field_from_update)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| account.fields.clone())
}

fn profile_media_value<'a>(
    next: Option<&'a (String, String)>,
    current: Option<&'a str>,
    field: fn(&(String, String)) -> &String,
) -> D1Type<'a> {
    match next.map(field).map(String::as_str).or(current) {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    }
}

fn bool_binding(value: bool) -> D1Type<'static> {
    D1Type::Integer(if value { 1 } else { 0 })
}

pub(crate) async fn apply_account_credentials_update(
    db: &D1Database,
    bucket: &Bucket,
    config: &AppConfig,
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> Result<LocalAccount> {
    let display_name = update
        .display_name
        .as_deref()
        .unwrap_or(account.display_name.as_str())
        .to_owned();
    let bio_text = update
        .note
        .as_deref()
        .unwrap_or(account.bio_text.as_str())
        .to_owned();
    let bio_html = render_status_html(&bio_text);
    let fields = account_profile_fields(account, update);
    let fields_json = serde_json::to_string(&fields).map_err(|error| {
        Error::RustError(format!("failed to serialize account fields: {error}"))
    })?;
    let locked = update.locked.unwrap_or(account.locked);
    let bot = update.bot.unwrap_or(account.bot);
    let discoverable = update.discoverable.unwrap_or(account.discoverable);
    let source_defaults = account_source_defaults(account, update);
    let avatar_profile = match update.avatar.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };
    let header_profile = match update.header.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };
    if let Some(previous) = account.avatar_object_key.as_deref()
        && profile_media_was_replaced(Some(previous), avatar_profile.as_ref())
    {
        bucket.delete(previous).await?;
    }
    if let Some(previous) = account.header_object_key.as_deref()
        && profile_media_was_replaced(Some(previous), header_profile.as_ref())
    {
        bucket.delete(previous).await?;
    }

    let bindings = [
        D1Type::Text(display_name.as_str()),
        D1Type::Text(bio_html.as_str()),
        D1Type::Text(bio_text.as_str()),
        D1Type::Text(fields_json.as_str()),
        bool_binding(locked),
        bool_binding(bot),
        bool_binding(discoverable),
        D1Type::Text(source_defaults.post_visibility.as_str()),
        D1Type::Text(source_defaults.quote_policy.as_str()),
        bool_binding(source_defaults.sensitive),
        match source_defaults.language.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        profile_media_value(
            avatar_profile.as_ref(),
            account.avatar_object_key.as_deref(),
            |value| &value.0,
        ),
        profile_media_value(
            avatar_profile.as_ref(),
            account.avatar_content_type.as_deref(),
            |value| &value.1,
        ),
        profile_media_value(
            header_profile.as_ref(),
            account.header_object_key.as_deref(),
            |value| &value.0,
        ),
        profile_media_value(
            header_profile.as_ref(),
            account.header_content_type.as_deref(),
            |value| &value.1,
        ),
        D1Type::Text(account.id.as_str()),
    ];

    db.prepare(
        "UPDATE accounts
         SET display_name = ?1,
             bio_html = ?2,
             bio_text = ?3,
             fields_json = ?4,
             locked = ?5,
             bot = ?6,
             discoverable = ?7,
             default_post_visibility = ?8,
             default_quote_policy = ?9,
             default_sensitive = ?10,
             default_language = ?11,
             avatar_object_key = ?12,
             avatar_content_type = ?13,
             header_object_key = ?14,
             header_content_type = ?15,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?16",
    )
    .bind_refs(bindings.iter())?
    .run()
    .await?;

    let updated = find_account_by_id(db, &account.id)
        .await?
        .ok_or_else(|| Error::RustError("failed to reload updated account".to_owned()))?;
    enqueue_profile_update_activities(db, config, &updated).await?;
    Ok(updated)
}

async fn store_profile_media(
    bucket: &Bucket,
    account: &LocalAccount,
    upload: &ProfileMediaUpload,
) -> Result<(String, String)> {
    let media_id = generate_entity_id(16)?;
    let object_key = format!(
        "profiles/{}/{}/{}",
        account.id, upload.object_kind, media_id
    );
    bucket
        .put(&object_key, upload.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(upload.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await?;
    Ok((object_key, upload.content_type.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{UpdateCredentialsField, UpdateCredentialsSource};

    fn test_account() -> LocalAccount {
        LocalAccount {
            id: "acct-1".to_owned(),
            username: "alice".to_owned(),
            access_email: "alice@example.com".to_owned(),
            display_name: "Alice".to_owned(),
            bio_html: String::new(),
            bio_text: String::new(),
            fields: vec![ProfileField {
                name: "Website".to_owned(),
                value: "https://example.com".to_owned(),
            }],
            locked: false,
            bot: false,
            discoverable: true,
            default_post_visibility: "public".to_owned(),
            default_quote_policy: "public".to_owned(),
            default_sensitive: false,
            default_language: None,
            avatar_object_key: None,
            avatar_content_type: None,
            header_object_key: None,
            header_content_type: None,
            private_key_jwk: "{}".to_owned(),
            public_key_pem: "pem".to_owned(),
            created_at: "2025-01-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn profile_media_was_replaced_only_when_new_key_differs() {
        let unchanged = (
            "profiles/account/avatar/same".to_owned(),
            "image/png".to_owned(),
        );
        let changed = (
            "profiles/account/avatar/new".to_owned(),
            "image/png".to_owned(),
        );

        assert!(!profile_media_was_replaced(None, Some(&changed)));
        assert!(!profile_media_was_replaced(
            Some("profiles/account/avatar/same"),
            None
        ));
        assert!(!profile_media_was_replaced(
            Some("profiles/account/avatar/same"),
            Some(&unchanged)
        ));
        assert!(profile_media_was_replaced(
            Some("profiles/account/avatar/old"),
            Some(&changed)
        ));
    }

    #[test]
    fn account_profile_fields_keeps_existing_when_update_omits_fields() {
        let account = test_account();
        let update = UpdateCredentialsRequest::default();

        assert_eq!(account_profile_fields(&account, &update), account.fields);
    }

    #[test]
    fn account_profile_fields_uses_complete_update_fields() {
        let account = test_account();
        let update = UpdateCredentialsRequest {
            fields_attributes: Some(vec![
                UpdateCredentialsField {
                    name: Some("Git".to_owned()),
                    value: Some("https://example.com/git".to_owned()),
                },
                UpdateCredentialsField {
                    name: Some("Ignored".to_owned()),
                    value: None,
                },
            ]),
            ..UpdateCredentialsRequest::default()
        };

        assert_eq!(
            account_profile_fields(&account, &update),
            vec![ProfileField {
                name: "Git".to_owned(),
                value: "https://example.com/git".to_owned(),
            }]
        );
    }

    #[test]
    fn account_source_defaults_prefers_update_source_values() {
        let account = test_account();
        let update = UpdateCredentialsRequest {
            source: Some(UpdateCredentialsSource {
                privacy: Some("private".to_owned()),
                quote_policy: Some("nobody".to_owned()),
                sensitive: Some(true),
                language: Some("ja".to_owned()),
            }),
            ..UpdateCredentialsRequest::default()
        };

        let defaults = account_source_defaults(&account, &update);

        assert_eq!(defaults.post_visibility, "private");
        assert_eq!(defaults.quote_policy, "nobody");
        assert!(defaults.sensitive);
        assert_eq!(defaults.language.as_deref(), Some("ja"));
    }
}
