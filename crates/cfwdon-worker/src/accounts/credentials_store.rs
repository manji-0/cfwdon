use crate::auth::find_account_by_id;
use crate::delivery::enqueue_profile_update_activities;
use crate::id_utils::generate_entity_id;
use crate::media::{delete_r2_object, log_r2_operation};
use crate::observability::observability_started_at_ms;
use crate::profile::{ProfileMediaUpload, UpdateCredentialsRequest, profile_field_from_update};
use crate::time_html::render_status_html;
use cfwdon_core::AppConfig;
use cfwdon_domain::{LocalAccount, ProfileField};
use worker::d1::D1Type;
use worker::{Bucket, Error, HttpMetadata, Result};

use crate::D1Database;
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
            .unwrap_or(account.default_visibility().as_str())
            .to_owned(),
        quote_policy: update
            .source
            .as_ref()
            .and_then(|source| source.quote_policy.as_deref())
            .unwrap_or(account.default_quote_policy().as_str())
            .to_owned(),
        sensitive: update
            .source
            .as_ref()
            .and_then(|source| source.sensitive)
            .unwrap_or(account.default_sensitive()),
        language: update
            .source
            .as_ref()
            .and_then(|source| source.language.clone())
            .or_else(|| account.default_language().map(str::to_owned)),
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
    match &update.fields_attributes {
        crate::FieldsAttributesUpdate::Set(fields) => fields
            .iter()
            .filter_map(profile_field_from_update)
            .collect(),
        crate::FieldsAttributesUpdate::Omitted => account.fields().to_vec(),
    }
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

struct AccountCredentialsUpdateDraft {
    display_name: String,
    bio_text: String,
    bio_html: String,
    fields_json: String,
    locked: bool,
    bot: bool,
    discoverable: bool,
    source_defaults: AccountSourceDefaults,
}

fn account_credentials_update_draft(
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> Result<AccountCredentialsUpdateDraft> {
    let display_name = update
        .display_name
        .as_deref()
        .unwrap_or(account.display_name())
        .to_owned();
    let bio_text = update
        .note
        .as_deref()
        .unwrap_or(account.bio_text())
        .to_owned();
    let bio_html = render_status_html(&bio_text);
    let fields = account_profile_fields(account, update);
    let fields_json = serde_json::to_string(&fields).map_err(|error| {
        Error::RustError(format!("failed to serialize account fields: {error}"))
    })?;

    Ok(AccountCredentialsUpdateDraft {
        display_name,
        bio_text,
        bio_html,
        fields_json,
        locked: update.locked.unwrap_or(account.is_locked()),
        bot: update.bot.unwrap_or(account.is_bot()),
        discoverable: update.discoverable.unwrap_or(account.is_discoverable()),
        source_defaults: account_source_defaults(account, update),
    })
}

struct StoredProfileMedia {
    avatar: Option<(String, String)>,
    header: Option<(String, String)>,
}

async fn store_account_profile_media(
    bucket: &Bucket,
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> Result<StoredProfileMedia> {
    let avatar = match update.avatar.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };
    let header = match update.header.as_ref() {
        Some(upload) => Some(store_profile_media(bucket, account, upload).await?),
        None => None,
    };

    Ok(StoredProfileMedia { avatar, header })
}

async fn delete_replaced_profile_media(
    bucket: &Bucket,
    account: &LocalAccount,
    media: &StoredProfileMedia,
) -> Result<()> {
    if let Some(previous) = account.avatar_object_key()
        && profile_media_was_replaced(Some(previous), media.avatar.as_ref())
    {
        delete_r2_object(bucket, previous, "delete_profile_previous").await?;
    }
    if let Some(previous) = account.header_object_key()
        && profile_media_was_replaced(Some(previous), media.header.as_ref())
    {
        delete_r2_object(bucket, previous, "delete_profile_previous").await?;
    }

    Ok(())
}

async fn update_account_credentials_row(
    db: &D1Database,
    account: &LocalAccount,
    draft: &AccountCredentialsUpdateDraft,
    media: &StoredProfileMedia,
) -> Result<()> {
    let bindings = [
        D1Type::Text(draft.display_name.as_str()),
        D1Type::Text(draft.bio_html.as_str()),
        D1Type::Text(draft.bio_text.as_str()),
        D1Type::Text(draft.fields_json.as_str()),
        bool_binding(draft.locked),
        bool_binding(draft.bot),
        bool_binding(draft.discoverable),
        D1Type::Text(draft.source_defaults.post_visibility.as_str()),
        D1Type::Text(draft.source_defaults.quote_policy.as_str()),
        bool_binding(draft.source_defaults.sensitive),
        match draft.source_defaults.language.as_deref() {
            Some(value) => D1Type::Text(value),
            None => D1Type::Null,
        },
        profile_media_value(
            media.avatar.as_ref(),
            account.avatar_object_key(),
            |value| &value.0,
        ),
        profile_media_value(
            media.avatar.as_ref(),
            account.avatar_content_type(),
            |value| &value.1,
        ),
        profile_media_value(
            media.header.as_ref(),
            account.header_object_key(),
            |value| &value.0,
        ),
        profile_media_value(
            media.header.as_ref(),
            account.header_content_type(),
            |value| &value.1,
        ),
        D1Type::Text(account.id()),
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

    Ok(())
}

pub(crate) async fn apply_account_credentials_update(
    db: &D1Database,
    bucket: &Bucket,
    config: &AppConfig,
    account: &LocalAccount,
    update: &UpdateCredentialsRequest,
) -> Result<LocalAccount> {
    let draft = account_credentials_update_draft(account, update)?;
    let media = store_account_profile_media(bucket, account, update).await?;
    delete_replaced_profile_media(bucket, account, &media).await?;
    update_account_credentials_row(db, account, &draft, &media).await?;

    let updated = find_account_by_id(db, account.id())
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
        account.id(),
        upload.object_kind,
        media_id
    );
    let started_at_ms = observability_started_at_ms();
    let result = bucket
        .put(&object_key, upload.bytes.clone())
        .http_metadata(HttpMetadata {
            content_type: Some(upload.content_type.clone()),
            content_disposition: Some("inline".to_owned()),
            ..Default::default()
        })
        .execute()
        .await;
    let outcome = if result.is_ok() { "ok" } else { "error" };
    log_r2_operation(
        "put_profile",
        outcome,
        started_at_ms,
        &object_key,
        Some(upload.bytes.len()),
    );
    result?;
    Ok((object_key, upload.content_type.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{FieldsAttributesUpdate, UpdateCredentialsField, UpdateCredentialsSource};
    use cfwdon_domain::LocalAccountRecord;

    fn test_account() -> LocalAccount {
        LocalAccount::from_record(LocalAccountRecord::test_fixture("acct-1", "alice"))
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

        assert_eq!(account_profile_fields(&account, &update), account.fields());
    }

    #[test]
    fn account_profile_fields_clears_when_update_sets_empty_list() {
        let account = test_account();
        let update = UpdateCredentialsRequest {
            fields_attributes: FieldsAttributesUpdate::Set(Vec::new()),
            ..UpdateCredentialsRequest::default()
        };

        assert!(account_profile_fields(&account, &update).is_empty());
    }

    #[test]
    fn account_profile_fields_uses_complete_update_fields() {
        let account = test_account();
        let update = UpdateCredentialsRequest {
            fields_attributes: FieldsAttributesUpdate::Set(vec![
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
    fn account_credentials_update_draft_applies_request_fields() {
        let account = test_account();
        let update = UpdateCredentialsRequest {
            display_name: Some("Alice Updated".to_owned()),
            note: Some("hello **world**".to_owned()),
            locked: Some(true),
            bot: Some(true),
            discoverable: Some(false),
            fields_attributes: FieldsAttributesUpdate::Set(vec![UpdateCredentialsField {
                name: Some("Site".to_owned()),
                value: Some("https://example.com".to_owned()),
            }]),
            source: Some(UpdateCredentialsSource {
                privacy: Some("private".to_owned()),
                quote_policy: Some("followers".to_owned()),
                sensitive: Some(true),
                language: Some("ja".to_owned()),
            }),
            ..UpdateCredentialsRequest::default()
        };

        let draft = account_credentials_update_draft(&account, &update).unwrap();

        assert_eq!(draft.display_name, "Alice Updated");
        assert_eq!(draft.bio_text, "hello **world**");
        assert_eq!(draft.bio_html, render_status_html("hello **world**"));
        assert!(draft.fields_json.contains("\"Site\""));
        assert!(draft.locked);
        assert!(draft.bot);
        assert!(!draft.discoverable);
        assert_eq!(draft.source_defaults.post_visibility, "private");
        assert_eq!(draft.source_defaults.quote_policy, "followers");
        assert!(draft.source_defaults.sensitive);
        assert_eq!(draft.source_defaults.language.as_deref(), Some("ja"));
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

    #[test]
    fn account_source_defaults_falls_back_to_account_values() {
        let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
        record.default_post_visibility = "unlisted".to_owned();
        record.default_quote_policy = "followers".to_owned();
        record.default_sensitive = 1;
        record.default_language = Some("en".to_owned());
        let account = LocalAccount::from_record(record);

        let defaults = account_source_defaults(&account, &UpdateCredentialsRequest::default());

        assert_eq!(defaults.post_visibility, "unlisted");
        assert_eq!(defaults.quote_policy, "followers");
        assert!(defaults.sensitive);
        assert_eq!(defaults.language.as_deref(), Some("en"));
    }
}
