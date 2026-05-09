use super::{
    MAX_IMAGE_UPLOAD_BYTES, ProfileMediaUpload, classify_media_kind,
    normalize_quote_approval_policy, parse_optional_bool,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde::de::Deserializer;
use std::collections::BTreeMap;
use worker::{FormData, FormEntry, Request};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct UpdateCredentialsRequest {
    pub(crate) display_name: Option<String>,
    pub(crate) note: Option<String>,
    #[serde(default, deserialize_with = "deserialize_fields_attributes")]
    pub(crate) fields_attributes: Option<Vec<UpdateCredentialsField>>,
    pub(crate) discoverable: Option<bool>,
    pub(crate) locked: Option<bool>,
    pub(crate) bot: Option<bool>,
    pub(crate) hide_collections: Option<bool>,
    pub(crate) indexable: Option<bool>,
    pub(crate) show_media: Option<bool>,
    pub(crate) show_media_replies: Option<bool>,
    pub(crate) show_featured: Option<bool>,
    pub(crate) avatar_description: Option<String>,
    pub(crate) header_description: Option<String>,
    pub(crate) source: Option<UpdateCredentialsSource>,
    #[serde(skip_deserializing)]
    pub(crate) avatar: Option<ProfileMediaUpload>,
    #[serde(skip_deserializing)]
    pub(crate) header: Option<ProfileMediaUpload>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct UpdateCredentialsSource {
    pub(crate) privacy: Option<String>,
    pub(crate) quote_policy: Option<String>,
    pub(crate) sensitive: Option<bool>,
    pub(crate) language: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct UpdateCredentialsField {
    pub(crate) name: Option<String>,
    pub(crate) value: Option<String>,
}

pub(crate) async fn parse_update_credentials_request(
    req: &mut Request,
) -> std::result::Result<UpdateCredentialsRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<UpdateCredentialsRequest>()
            .await
            .map_err(|error| format!("invalid JSON credentials payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form credentials payload: {error}"))?;
        let mut request = UpdateCredentialsRequest {
            display_name: form.get_field("display_name"),
            note: form.get_field("note"),
            fields_attributes: Some(parse_profile_fields_from_form(&form)),
            discoverable: parse_optional_bool(form.get_field("discoverable").as_deref())?,
            locked: parse_optional_bool(form.get_field("locked").as_deref())?,
            bot: parse_optional_bool(form.get_field("bot").as_deref())?,
            hide_collections: parse_optional_bool(form.get_field("hide_collections").as_deref())?,
            indexable: parse_optional_bool(form.get_field("indexable").as_deref())?,
            show_media: parse_optional_bool(form.get_field("show_media").as_deref())?,
            show_media_replies: parse_optional_bool(
                form.get_field("show_media_replies").as_deref(),
            )?,
            show_featured: parse_optional_bool(form.get_field("show_featured").as_deref())?,
            avatar_description: form.get_field("avatar_description"),
            header_description: form.get_field("header_description"),
            source: Some(UpdateCredentialsSource {
                privacy: form.get_field("source[privacy]"),
                quote_policy: form.get_field("source[quote_policy]"),
                sensitive: parse_optional_bool(form.get_field("source[sensitive]").as_deref())?,
                language: form.get_field("source[language]"),
            }),
            ..UpdateCredentialsRequest::default()
        };
        request.avatar = parse_profile_media_upload(form.get("avatar"), "avatar").await?;
        request.header = parse_profile_media_upload(form.get("header"), "header").await?;
        request
    };

    normalize_optional_text(&mut request.display_name, true);
    normalize_optional_text(&mut request.note, false);
    normalize_optional_text(&mut request.avatar_description, false);
    normalize_optional_text(&mut request.header_description, false);

    if let Some(fields) = request.fields_attributes.as_mut() {
        *fields = normalize_profile_fields(std::mem::take(fields));
        if fields.is_empty() {
            request.fields_attributes = None;
        }
    }

    if let Some(source) = request.source.as_mut() {
        if let Some(privacy) = source.privacy.as_mut() {
            *privacy = privacy.trim().to_ascii_lowercase();
            if privacy.is_empty() {
                source.privacy = None;
            } else if super::Visibility::parse(privacy).is_none() {
                return Err(
                    "source[privacy] must be one of: public, unlisted, private, direct".to_owned(),
                );
            }
        }

        if let Some(language) = source.language.as_mut() {
            *language = language.trim().to_ascii_lowercase();
            if language.is_empty() {
                source.language = None;
            }
        }

        source.quote_policy = normalize_quote_approval_policy(source.quote_policy.take())?;
    }

    Ok(request)
}

fn deserialize_fields_attributes<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<UpdateCredentialsField>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FieldsAttributes {
        List(Vec<UpdateCredentialsField>),
        Map(BTreeMap<String, UpdateCredentialsField>),
    }

    Ok(
        match Option::<FieldsAttributes>::deserialize(deserializer)? {
            Some(FieldsAttributes::List(fields)) => Some(fields),
            Some(FieldsAttributes::Map(fields)) => {
                let mut fields = fields.into_iter().collect::<Vec<_>>();
                fields.sort_by_key(|(key, _)| key.parse::<usize>().ok());
                Some(fields.into_iter().map(|(_, field)| field).collect())
            }
            None => None,
        },
    )
}

fn normalize_optional_text(value: &mut Option<String>, clear_if_empty: bool) {
    if let Some(current) = value.as_mut() {
        *current = current.trim().to_owned();
        if clear_if_empty && current.is_empty() {
            *value = None;
        }
    }
}

fn parse_profile_fields_from_form(form: &FormData) -> Vec<UpdateCredentialsField> {
    (0..8)
        .filter_map(|index| {
            let name = form.get_field(&format!("fields_attributes[{index}][name]"));
            let value = form.get_field(&format!("fields_attributes[{index}][value]"));
            if name.is_none() && value.is_none() {
                None
            } else {
                Some(UpdateCredentialsField { name, value })
            }
        })
        .collect()
}

fn normalize_profile_fields(fields: Vec<UpdateCredentialsField>) -> Vec<UpdateCredentialsField> {
    fields
        .into_iter()
        .filter_map(|mut field| {
            if let Some(name) = field.name.as_mut() {
                *name = name.trim().to_owned();
            }
            if let Some(value) = field.value.as_mut() {
                *value = value.trim().to_owned();
            }
            let name = field.name.filter(|value| !value.is_empty());
            let value = field.value.filter(|value| !value.is_empty());
            match (name, value) {
                (Some(name), Some(value)) => Some(UpdateCredentialsField {
                    name: Some(name),
                    value: Some(value),
                }),
                _ => None,
            }
        })
        .take(4)
        .collect()
}

async fn parse_profile_media_upload(
    entry: Option<FormEntry>,
    object_kind: &'static str,
) -> std::result::Result<Option<ProfileMediaUpload>, String> {
    let Some(entry) = entry else {
        return Ok(None);
    };
    let file = match entry {
        FormEntry::File(file) => file,
        FormEntry::Field(value) => return parse_profile_media_data_url(&value, object_kind),
    };
    let content_type = file.type_().trim().to_ascii_lowercase();
    if content_type.is_empty() {
        return Err(format!("{object_kind} is missing a content type"));
    }
    let kind = classify_media_kind(&content_type)
        .ok_or_else(|| format!("unsupported {object_kind} content type: {content_type}"))?;
    if kind != super::MediaKind::Image {
        return Err(format!("{object_kind} must be an image"));
    }
    let bytes = file
        .bytes()
        .await
        .map_err(|error| format!("failed to read {object_kind} upload: {error}"))?;
    if bytes.is_empty() {
        return Err(format!("{object_kind} must not be empty"));
    }
    if bytes.len() > MAX_IMAGE_UPLOAD_BYTES {
        return Err(format!(
            "{object_kind} exceeds the {} byte image limit",
            MAX_IMAGE_UPLOAD_BYTES
        ));
    }

    Ok(Some(ProfileMediaUpload {
        bytes,
        content_type,
        object_kind,
    }))
}

fn parse_profile_media_data_url(
    value: &str,
    object_kind: &'static str,
) -> std::result::Result<Option<ProfileMediaUpload>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }

    let Some((metadata, encoded)) = value.split_once(',') else {
        return Err(format!(
            "{object_kind} must be sent as multipart file data or a data URL"
        ));
    };
    let Some(content_type) = metadata
        .strip_prefix("data:")
        .and_then(|metadata| metadata.strip_suffix(";base64"))
    else {
        return Err(format!("{object_kind} data URL must be base64 encoded"));
    };
    let content_type = content_type.trim().to_ascii_lowercase();
    if content_type.is_empty() {
        return Err(format!("{object_kind} is missing a content type"));
    }
    let kind = classify_media_kind(&content_type)
        .ok_or_else(|| format!("unsupported {object_kind} content type: {content_type}"))?;
    if kind != super::MediaKind::Image {
        return Err(format!("{object_kind} must be an image"));
    }
    let bytes = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|error| format!("invalid {object_kind} data URL: {error}"))?;
    if bytes.is_empty() {
        return Err(format!("{object_kind} must not be empty"));
    }
    if bytes.len() > MAX_IMAGE_UPLOAD_BYTES {
        return Err(format!(
            "{object_kind} exceeds the {} byte image limit",
            MAX_IMAGE_UPLOAD_BYTES
        ));
    }

    Ok(Some(ProfileMediaUpload {
        bytes,
        content_type,
        object_kind,
    }))
}

#[cfg(test)]
mod tests {
    use super::{UpdateCredentialsRequest, parse_profile_media_data_url};

    #[test]
    fn update_credentials_accepts_json_fields_attributes_map() {
        let request: UpdateCredentialsRequest = serde_json::from_value(serde_json::json!({
            "display_name": "Alice",
            "fields_attributes": {
                "1": { "name": "Second", "value": "https://example.com/second" },
                "0": { "name": "First", "value": "https://example.com/first" }
            }
        }))
        .expect("map-shaped fields_attributes should deserialize");

        let fields = request.fields_attributes.expect("fields should be present");
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name.as_deref(), Some("First"));
        assert_eq!(
            fields[0].value.as_deref(),
            Some("https://example.com/first")
        );
        assert_eq!(fields[1].name.as_deref(), Some("Second"));
        assert_eq!(
            fields[1].value.as_deref(),
            Some("https://example.com/second")
        );
    }

    #[test]
    fn update_credentials_accepts_json_fields_attributes_list() {
        let request: UpdateCredentialsRequest = serde_json::from_value(serde_json::json!({
            "fields_attributes": [
                { "name": "Website", "value": "https://example.com" }
            ]
        }))
        .expect("list-shaped fields_attributes should deserialize");

        let fields = request.fields_attributes.expect("fields should be present");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name.as_deref(), Some("Website"));
        assert_eq!(fields[0].value.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn profile_media_upload_accepts_data_url_field() {
        let upload = parse_profile_media_data_url("data:image/png;base64,aGVsbG8=", "avatar")
            .expect("data URL should parse")
            .expect("data URL should produce upload");

        assert_eq!(upload.bytes, b"hello");
        assert_eq!(upload.content_type, "image/png");
        assert_eq!(upload.object_kind, "avatar");
    }
}
