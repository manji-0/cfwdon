use cfwdon_domain::{LocalAccount, LocalAccountRecord};

pub(crate) fn actor_fixture_account() -> LocalAccount {
    let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
    record.bio_html = "<p>Hello</p>".to_owned();
    record.bio_text = "Hello".to_owned();
    record.discoverable = 1;
    record.fields_json = r#"[{"name":"Website","value":"https://example.com"}]"#.to_owned();
    record.default_quote_policy = "public".to_owned();
    record.default_language = Some("ja".to_owned());
    record.avatar_object_key = Some("media/account/avatar/alice".to_owned());
    record.avatar_content_type = Some("image/png".to_owned());
    record.header_object_key = Some("media/account/header/alice".to_owned());
    record.header_content_type = Some("image/jpeg".to_owned());
    LocalAccount::from_record(record)
}

pub(crate) fn actor_fixture_account_locked_bot() -> LocalAccount {
    let mut record = LocalAccountRecord::test_fixture("acct-1", "alice");
    record.bio_html = "<p>Hello</p>".to_owned();
    record.bio_text = "Hello".to_owned();
    record.discoverable = 1;
    record.fields_json = r#"[{"name":"Website","value":"https://example.com"}]"#.to_owned();
    record.locked = 1;
    record.bot = 1;
    record.default_quote_policy = "public".to_owned();
    record.default_language = Some("ja".to_owned());
    record.avatar_object_key = Some("media/account/avatar/alice".to_owned());
    record.avatar_content_type = Some("image/png".to_owned());
    record.header_object_key = Some("media/account/header/alice".to_owned());
    record.header_content_type = Some("image/jpeg".to_owned());
    LocalAccount::from_record(record)
}
