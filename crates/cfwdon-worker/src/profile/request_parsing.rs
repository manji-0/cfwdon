use super::{
    MAX_IMAGE_UPLOAD_BYTES, ProfileMediaUpload, classify_media_kind,
    normalize_quote_approval_policy, parse_optional_bool,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde::de::Deserializer;
use std::collections::BTreeMap;
use worker::{FormData, FormEntry, Request};

const MAX_PROFILE_FIELDS: usize = 4;
const MAX_ATTRIBUTION_DOMAINS: usize = 10;
const FORM_FIELDS_ATTRIBUTES_INDEX_LIMIT: usize = 256;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct UpdateCredentialsRequest {
    pub(crate) display_name: Option<String>,
    pub(crate) note: Option<String>,
    #[serde(default, deserialize_with = "deserialize_fields_attributes")]
    pub(crate) fields_attributes: FieldsAttributesUpdate,
    #[serde(default, deserialize_with = "deserialize_attribution_domains")]
    pub(crate) attribution_domains: AttributionDomainsUpdate,
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

/// Client omitted `fields_attributes` vs sent a (possibly empty) replacement list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum FieldsAttributesUpdate {
    #[default]
    Omitted,
    Set(Vec<UpdateCredentialsField>),
}

/// Client omitted `attribution_domains` vs sent a (possibly empty) replacement list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum AttributionDomainsUpdate {
    #[default]
    Omitted,
    Set(Vec<String>),
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct UpdateCredentialsSource {
    pub(crate) privacy: Option<String>,
    pub(crate) quote_policy: Option<String>,
    pub(crate) sensitive: Option<bool>,
    pub(crate) language: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub(crate) struct UpdateCredentialsField {
    pub(crate) name: Option<String>,
    pub(crate) value: Option<String>,
}

pub(crate) async fn parse_update_credentials_request(
    req: &mut Request,
) -> std::result::Result<UpdateCredentialsRequest, String> {
    let content_type = request_content_type(req)?;

    let mut request = if request_is_json(&content_type) {
        req.json::<UpdateCredentialsRequest>()
            .await
            .map_err(|error| format!("invalid JSON credentials payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form credentials payload: {error}"))?;
        update_credentials_request_from_form(form).await?
    };

    normalize_update_credentials_request(&mut request)?;
    Ok(request)
}

fn request_content_type(req: &Request) -> std::result::Result<String, String> {
    Ok(req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase())
}

fn request_is_json(content_type: &str) -> bool {
    content_type.contains("application/json")
}

async fn update_credentials_request_from_form(
    form: FormData,
) -> std::result::Result<UpdateCredentialsRequest, String> {
    let mut request = UpdateCredentialsRequest {
        display_name: form.get_field("display_name"),
        note: form.get_field("note"),
        fields_attributes: parse_fields_attributes_from_form(&form),
        attribution_domains: parse_attribution_domains_from_form(&form)?,
        discoverable: parse_optional_bool(form.get_field("discoverable").as_deref())?,
        locked: parse_optional_bool(form.get_field("locked").as_deref())?,
        bot: parse_optional_bool(form.get_field("bot").as_deref())?,
        hide_collections: parse_optional_bool(form.get_field("hide_collections").as_deref())?,
        indexable: parse_optional_bool(form.get_field("indexable").as_deref())?,
        show_media: parse_optional_bool(form.get_field("show_media").as_deref())?,
        show_media_replies: parse_optional_bool(form.get_field("show_media_replies").as_deref())?,
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
    Ok(request)
}

fn normalize_update_credentials_request(
    request: &mut UpdateCredentialsRequest,
) -> std::result::Result<(), String> {
    normalize_optional_text(&mut request.display_name, true);
    normalize_optional_text(&mut request.note, false);
    normalize_optional_text(&mut request.avatar_description, false);
    normalize_optional_text(&mut request.header_description, false);

    if let FieldsAttributesUpdate::Set(fields) = &mut request.fields_attributes {
        *fields = normalize_profile_fields(std::mem::take(fields));
    }

    if let AttributionDomainsUpdate::Set(domains) = &mut request.attribution_domains {
        *domains = normalize_attribution_domains(std::mem::take(domains))?;
    }

    if let Some(source) = request.source.as_mut() {
        normalize_update_credentials_source(source)?;
    }

    Ok(())
}

fn normalize_update_credentials_source(
    source: &mut UpdateCredentialsSource,
) -> std::result::Result<(), String> {
    if let Some(privacy) = source.privacy.as_mut() {
        *privacy = privacy.trim().to_ascii_lowercase();
        if privacy.is_empty() {
            source.privacy = None;
        } else if super::Visibility::parse(privacy).is_err() {
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

    source.quote_policy = normalize_quote_approval_policy(source.quote_policy.take())?
        .map(|policy| policy.as_str().to_owned());
    Ok(())
}

fn deserialize_fields_attributes<'de, D>(
    deserializer: D,
) -> std::result::Result<FieldsAttributesUpdate, D::Error>
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
            Some(FieldsAttributes::List(fields)) => FieldsAttributesUpdate::Set(fields),
            Some(FieldsAttributes::Map(fields)) => {
                let mut fields = fields.into_iter().collect::<Vec<_>>();
                fields.sort_by_key(|(key, _)| key.parse::<i64>().unwrap_or(i64::MAX));
                FieldsAttributesUpdate::Set(fields.into_iter().map(|(_, field)| field).collect())
            }
            None => FieldsAttributesUpdate::Omitted,
        },
    )
}

fn deserialize_attribution_domains<'de, D>(
    deserializer: D,
) -> std::result::Result<AttributionDomainsUpdate, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AttributionDomains {
        List(Vec<String>),
        Single(String),
    }

    Ok(
        match Option::<AttributionDomains>::deserialize(deserializer)? {
            Some(AttributionDomains::List(domains)) => AttributionDomainsUpdate::Set(domains),
            Some(AttributionDomains::Single(domain)) => AttributionDomainsUpdate::Set(vec![domain]),
            None => AttributionDomainsUpdate::Omitted,
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

fn parse_fields_attributes_from_form(form: &FormData) -> FieldsAttributesUpdate {
    let mut fields = Vec::new();
    let mut present = false;
    for index in 0..FORM_FIELDS_ATTRIBUTES_INDEX_LIMIT {
        let name_key = format!("fields_attributes[{index}][name]");
        let value_key = format!("fields_attributes[{index}][value]");
        let name_present = form.has(&name_key);
        let value_present = form.has(&value_key);
        if !name_present && !value_present {
            continue;
        }
        present = true;
        fields.push(UpdateCredentialsField {
            name: form.get_field(&name_key),
            value: form.get_field(&value_key),
        });
    }
    if present {
        FieldsAttributesUpdate::Set(fields)
    } else {
        FieldsAttributesUpdate::Omitted
    }
}

fn parse_attribution_domains_from_form(
    form: &FormData,
) -> std::result::Result<AttributionDomainsUpdate, String> {
    let mut domains = Vec::new();
    let mut present = false;

    if let Some(entries) = form.get_all("attribution_domains[]") {
        present = true;
        for entry in entries {
            if let FormEntry::Field(value) = entry {
                domains.push(value);
            }
        }
    }
    if let Some(value) = form.get_field("attribution_domains") {
        present = true;
        domains.push(value);
    }
    for index in 0..MAX_ATTRIBUTION_DOMAINS.saturating_mul(2) {
        let key = format!("attribution_domains[{index}]");
        if !form.has(&key) {
            continue;
        }
        present = true;
        if let Some(value) = form.get_field(&key) {
            domains.push(value);
        }
    }

    if present {
        Ok(AttributionDomainsUpdate::Set(domains))
    } else {
        Ok(AttributionDomainsUpdate::Omitted)
    }
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
        .take(MAX_PROFILE_FIELDS)
        .collect()
}

pub(crate) fn normalize_attribution_domains(
    domains: Vec<String>,
) -> std::result::Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for domain in domains {
        let Some(value) = normalize_attribution_domain(&domain)? else {
            continue;
        };
        if !normalized.iter().any(|existing| existing == &value) {
            normalized.push(value);
        }
        if normalized.len() > MAX_ATTRIBUTION_DOMAINS {
            return Err(format!(
                "attribution_domains must contain at most {MAX_ATTRIBUTION_DOMAINS} domains"
            ));
        }
    }
    Ok(normalized)
}

fn normalize_attribution_domain(value: &str) -> std::result::Result<Option<String>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or(without_scheme)
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return Ok(None);
    }
    if host.contains('@') || host.contains(' ') || host.contains(':') {
        return Err(format!("invalid attribution domain: {value}"));
    }
    if !host.contains('.') && host != "localhost" {
        return Err(format!("invalid attribution domain: {value}"));
    }
    if !host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
    {
        return Err(format!("invalid attribution domain: {value}"));
    }
    Ok(Some(host))
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
    use super::{
        AttributionDomainsUpdate, FieldsAttributesUpdate, UpdateCredentialsRequest,
        UpdateCredentialsSource, normalize_attribution_domains,
        normalize_update_credentials_request, parse_profile_media_data_url, request_is_json,
    };

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

        let FieldsAttributesUpdate::Set(fields) = request.fields_attributes else {
            panic!("fields should be set");
        };
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

        let FieldsAttributesUpdate::Set(fields) = request.fields_attributes else {
            panic!("fields should be set");
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name.as_deref(), Some("Website"));
        assert_eq!(fields[0].value.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn update_credentials_keeps_empty_fields_attributes_as_set() {
        let mut request: UpdateCredentialsRequest = serde_json::from_value(serde_json::json!({
            "fields_attributes": {}
        }))
        .expect("empty map should deserialize");
        normalize_update_credentials_request(&mut request).unwrap();
        assert_eq!(
            request.fields_attributes,
            FieldsAttributesUpdate::Set(Vec::new())
        );
    }

    #[test]
    fn update_credentials_accepts_attribution_domains() {
        let mut request: UpdateCredentialsRequest = serde_json::from_value(serde_json::json!({
            "attribution_domains": ["Example.COM", "https://blog.example/path", ""]
        }))
        .expect("attribution domains should deserialize");
        normalize_update_credentials_request(&mut request).unwrap();
        assert_eq!(
            request.attribution_domains,
            AttributionDomainsUpdate::Set(vec![
                "example.com".to_owned(),
                "blog.example".to_owned()
            ])
        );
    }

    #[test]
    fn normalize_attribution_domains_rejects_too_many() {
        let domains = (0..11)
            .map(|index| format!("example{index}.com"))
            .collect::<Vec<_>>();
        assert!(normalize_attribution_domains(domains).is_err());
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

    #[test]
    fn request_is_json_matches_json_content_types() {
        assert!(request_is_json("application/json"));
        assert!(request_is_json("application/json; charset=utf-8"));
        assert!(!request_is_json("multipart/form-data"));
    }

    #[test]
    fn normalize_update_credentials_request_trims_source_values() {
        let mut request = UpdateCredentialsRequest {
            display_name: Some("  Alice  ".to_owned()),
            note: Some("  hello  ".to_owned()),
            source: Some(UpdateCredentialsSource {
                privacy: Some(" Unlisted ".to_owned()),
                quote_policy: Some(" Followers ".to_owned()),
                sensitive: None,
                language: Some(" JA ".to_owned()),
            }),
            ..UpdateCredentialsRequest::default()
        };

        normalize_update_credentials_request(&mut request).unwrap();

        assert_eq!(request.display_name.as_deref(), Some("Alice"));
        assert_eq!(request.note.as_deref(), Some("hello"));
        let source = request.source.expect("source should remain present");
        assert_eq!(source.privacy.as_deref(), Some("unlisted"));
        assert_eq!(source.quote_policy.as_deref(), Some("followers"));
        assert_eq!(source.language.as_deref(), Some("ja"));
    }
}
