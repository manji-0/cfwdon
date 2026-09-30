use crate::discovery::remote_follow_base_url;

#[test]
fn remote_follow_base_url_accepts_domain_handle_acct_and_url() {
    assert_eq!(
        remote_follow_base_url("Social.Example").unwrap(),
        "https://social.example/"
    );
    assert_eq!(
        remote_follow_base_url("@alice@Social.Example").unwrap(),
        "https://social.example/"
    );
    assert_eq!(
        remote_follow_base_url("acct:alice@Social.Example").unwrap(),
        "https://social.example/"
    );
    assert_eq!(
        remote_follow_base_url("https://Social.Example/@alice").unwrap(),
        "https://social.example/"
    );
}

#[test]
fn remote_follow_base_url_rejects_pathy_domains() {
    let error = remote_follow_base_url("social.example/@alice").unwrap_err();
    assert!(error.to_string().contains("hostname"));
}
