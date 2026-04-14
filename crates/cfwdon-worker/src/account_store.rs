use super::{
    count_accepted_following, count_local_followers, count_remote_followers, count_rows,
    parse_profile_fields_json,
};
use cfwdon_domain::LocalAccount;
use serde::Deserialize;
use worker::d1::D1Type;
use worker::{D1Database, Result};

#[derive(Debug, Deserialize)]
pub(crate) struct AccountRow {
    pub(crate) id: String,
    pub(crate) username: String,
    pub(crate) access_email: String,
    pub(crate) display_name: String,
    pub(crate) bio_html: String,
    pub(crate) bio_text: String,
    pub(crate) fields_json: String,
    pub(crate) discoverable: i32,
    pub(crate) default_post_visibility: String,
    pub(crate) default_sensitive: i32,
    pub(crate) default_language: Option<String>,
    pub(crate) avatar_object_key: Option<String>,
    pub(crate) avatar_content_type: Option<String>,
    pub(crate) header_object_key: Option<String>,
    pub(crate) header_content_type: Option<String>,
    pub(crate) private_key_jwk: String,
    pub(crate) public_key_pem: String,
    pub(crate) created_at: String,
}

#[derive(Debug, Default)]
pub(crate) struct AccountStats {
    pub(crate) followers_count: u64,
    pub(crate) following_count: u64,
    pub(crate) statuses_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DirectoryOrder {
    Active,
    New,
}

pub(crate) fn directory_order(value: Option<&str>) -> DirectoryOrder {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("new") => DirectoryOrder::New,
        _ => DirectoryOrder::Active,
    }
}

pub(crate) async fn list_discoverable_accounts(
    db: &D1Database,
    limit: u32,
    offset: u32,
    order: DirectoryOrder,
) -> Result<Vec<LocalAccount>> {
    let sql = match order {
        DirectoryOrder::Active => {
            "SELECT a.id, a.username, a.access_email, a.display_name, a.bio_html, a.bio_text, a.fields_json, a.discoverable, a.default_post_visibility, a.default_sensitive, a.default_language, a.avatar_object_key, a.avatar_content_type, a.header_object_key, a.header_content_type, a.private_key_jwk, a.public_key_pem, a.created_at
             FROM accounts a
             LEFT JOIN statuses s
               ON s.account_id = a.id
             WHERE a.discoverable = 1
             GROUP BY a.id
             ORDER BY COALESCE(MAX(s.created_at), a.created_at) DESC, a.username ASC
             LIMIT ?1
             OFFSET ?2"
        }
        DirectoryOrder::New => {
            "SELECT id, username, access_email, display_name, bio_html, bio_text, fields_json, discoverable, default_post_visibility, default_sensitive, default_language, avatar_object_key, avatar_content_type, header_object_key, header_content_type, private_key_jwk, public_key_pem, created_at
             FROM accounts
             WHERE discoverable = 1
             ORDER BY created_at DESC, username ASC
             LIMIT ?1
             OFFSET ?2"
        }
    };

    let bindings = [
        D1Type::Integer(limit as i32),
        D1Type::Integer(offset as i32),
    ];
    let result = db.prepare(sql).bind_refs(bindings.iter())?.all().await?;

    Ok(result
        .results::<AccountRow>()?
        .into_iter()
        .map(LocalAccount::from)
        .collect())
}

pub(crate) async fn load_account_stats(db: &D1Database, account_id: &str) -> Result<AccountStats> {
    Ok(AccountStats {
        followers_count: count_remote_followers(db, account_id).await?
            + count_local_followers(db, account_id).await?,
        following_count: count_accepted_following(db, account_id).await?,
        statuses_count: count_rows(
            db,
            "SELECT COUNT(*) AS count FROM statuses WHERE account_id = ?1",
            account_id,
        )
        .await?,
    })
}

impl From<AccountRow> for LocalAccount {
    fn from(value: AccountRow) -> Self {
        Self {
            id: value.id,
            username: value.username,
            access_email: value.access_email,
            display_name: value.display_name,
            bio_html: value.bio_html,
            bio_text: value.bio_text,
            fields: parse_profile_fields_json(&value.fields_json),
            discoverable: value.discoverable != 0,
            default_post_visibility: value.default_post_visibility,
            default_sensitive: value.default_sensitive != 0,
            default_language: value.default_language,
            avatar_object_key: value.avatar_object_key,
            avatar_content_type: value.avatar_content_type,
            header_object_key: value.header_object_key,
            header_content_type: value.header_content_type,
            private_key_jwk: value.private_key_jwk,
            public_key_pem: value.public_key_pem,
            created_at: value.created_at,
        }
    }
}
