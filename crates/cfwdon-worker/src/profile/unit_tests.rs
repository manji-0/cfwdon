use crate::profile::activitypub_profile_attachments;
use cfwdon_domain::ProfileField;

#[test]
fn activitypub_profile_attachments_use_property_value_shape() {
    let fields = vec![ProfileField {
        name: "Pronouns".to_owned(),
        value: "they/them".to_owned(),
    }];
    let rendered = activitypub_profile_attachments(&fields);
    assert_eq!(rendered[0]["type"], serde_json::json!("PropertyValue"));
    assert_eq!(rendered[0]["name"], serde_json::json!("Pronouns"));
    assert_eq!(rendered[0]["value"], serde_json::json!("they/them"));
}
