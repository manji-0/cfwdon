use crate::authorize_interaction::{
    authorize_interaction_document, authorize_interaction_url_from_base,
};
use url::Url;

#[test]
fn authorize_interaction_document_preserves_encoded_target_uri() {
    let html = authorize_interaction_document(
        "https://blog.kosui.me/users/kosui",
        "kosui",
        "kosui@blog.kosui.me",
        "https://blog.kosui.me/@kosui",
    );

    assert!(html.contains(
        "action=\"/authorize_interaction?uri=https%3A%2F%2Fblog.kosui.me%2Fusers%2Fkosui\""
    ));
    assert!(html.contains("name=\"uri\" value=\"https://blog.kosui.me/users/kosui\""));
    assert!(html.contains("kosui@blog.kosui.me"));
}

#[test]
fn authorize_interaction_login_return_url_preserves_target_uri() {
    let base_url = Url::parse("https://fedi.manji.app/authorize_interaction").unwrap();
    let authorize_url =
        authorize_interaction_url_from_base(base_url, "https://blog.kosui.me/users/kosui");

    assert_eq!(authorize_url.path(), "/authorize_interaction");
    assert_eq!(
        authorize_url.query_pairs().find(|(name, _)| name == "uri"),
        Some((
            std::borrow::Cow::Borrowed("uri"),
            std::borrow::Cow::Borrowed("https://blog.kosui.me/users/kosui"),
        ))
    );
}
