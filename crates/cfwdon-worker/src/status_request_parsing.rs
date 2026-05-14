use super::{
    StatusDraft, normalize_quote_approval_policy, normalize_status_poll, parse_media_ids_from_form,
    parse_optional_bool,
};
use serde::Deserialize;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use worker::{FormData, FormEntry, Request};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CreateStatusRequest {
    pub(crate) status: Option<String>,
    pub(crate) media_ids: Option<Vec<String>>,
    pub(crate) poll: Option<CreateStatusPollRequest>,
    pub(crate) in_reply_to_id: Option<String>,
    pub(crate) scheduled_at: Option<String>,
    pub(crate) quoted_status_id: Option<String>,
    pub(crate) quote_approval_policy: Option<String>,
    pub(crate) sensitive: Option<bool>,
    pub(crate) spoiler_text: Option<String>,
    pub(crate) visibility: Option<String>,
    pub(crate) language: Option<String>,
}

pub(crate) struct ParsedStatusDraft {
    pub(crate) draft: StatusDraft,
    pub(crate) idempotency_key: Option<String>,
    pub(crate) scheduled_at: Option<String>,
    pub(crate) quoted_status_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CreateStatusPollRequest {
    pub(crate) options: Option<Vec<String>>,
    pub(crate) expires_in: Option<u64>,
    pub(crate) multiple: Option<bool>,
    pub(crate) hide_totals: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DeleteStatusQuery {
    pub(crate) delete_media: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct UpdateStatusRequest {
    pub(crate) status: Option<String>,
    pub(crate) media_ids: Option<Vec<String>>,
    pub(crate) media_attributes: Option<Vec<StatusMediaAttributeRequest>>,
    pub(crate) poll: Option<CreateStatusPollRequest>,
    pub(crate) sensitive: Option<bool>,
    pub(crate) spoiler_text: Option<String>,
    pub(crate) language: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StatusMediaAttributeRequest {
    pub(crate) id: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) focus: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AccountStatusesQuery {
    pub(crate) limit: Option<u32>,
    #[serde(rename = "max_id")]
    pub(crate) max_id: Option<String>,
    #[serde(rename = "since_id")]
    pub(crate) since_id: Option<String>,
    #[serde(rename = "min_id")]
    pub(crate) min_id: Option<String>,
    pub(crate) only_media: Option<bool>,
    pub(crate) exclude_replies: Option<bool>,
    pub(crate) exclude_reblogs: Option<bool>,
    pub(crate) pinned: Option<bool>,
    pub(crate) tagged: Option<String>,
}

pub(crate) async fn parse_status_draft(
    req: &mut Request,
) -> std::result::Result<ParsedStatusDraft, String> {
    let idempotency_key = read_idempotency_key(req)?;
    let request = read_create_status_request(req).await?;
    parsed_status_draft_from_request(request, idempotency_key)
}

fn read_idempotency_key(req: &Request) -> std::result::Result<Option<String>, String> {
    req.headers()
        .get("Idempotency-Key")
        .map_err(|error| format!("failed to read Idempotency-Key header: {error}"))
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

async fn read_create_status_request(
    req: &mut Request,
) -> std::result::Result<CreateStatusRequest, String> {
    let content_type = request_content_type(req)?;
    let request = if request_is_json(&content_type) {
        req.json::<CreateStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON status payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form status payload: {error}"))?;
        create_status_request_from_form(&form)?
    };
    Ok(request)
}

fn create_status_request_from_form(
    form: &FormData,
) -> std::result::Result<CreateStatusRequest, String> {
    Ok(CreateStatusRequest {
        status: form.get_field("status"),
        media_ids: parse_media_ids_from_form(form),
        poll: parse_status_poll_from_form(form)?,
        in_reply_to_id: form.get_field("in_reply_to_id"),
        scheduled_at: form.get_field("scheduled_at"),
        quoted_status_id: form.get_field("quoted_status_id"),
        quote_approval_policy: form.get_field("quote_approval_policy"),
        sensitive: parse_optional_bool(form.get_field("sensitive").as_deref())?,
        spoiler_text: form.get_field("spoiler_text"),
        visibility: form.get_field("visibility"),
        language: form.get_field("language"),
    })
}

fn parsed_status_draft_from_request(
    request: CreateStatusRequest,
    idempotency_key: Option<String>,
) -> std::result::Result<ParsedStatusDraft, String> {
    let scheduled_at = normalize_scheduled_at(request.scheduled_at.as_deref())?;
    let text = request.status.unwrap_or_default().trim().to_owned();
    let poll = normalize_status_poll(request.poll)?;
    let media_ids = normalize_status_media_ids(request.media_ids);
    let quoted_status_id = normalized_optional_string(request.quoted_status_id);
    validate_status_draft_inputs(
        &text,
        &media_ids,
        poll.is_some(),
        quoted_status_id.as_deref(),
    )?;
    let visibility = status_visibility_from_request(request.visibility.as_deref())?;
    let quote_approval_policy = normalize_quote_approval_policy(request.quote_approval_policy)?;

    Ok(ParsedStatusDraft {
        draft: StatusDraft {
            text,
            visibility,
            spoiler_text: request.spoiler_text.unwrap_or_default().trim().to_owned(),
            sensitive: request.sensitive.unwrap_or(false),
            language: request
                .language
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty()),
            quote_approval_policy,
            in_reply_to_id: normalized_optional_string(request.in_reply_to_id),
            media_ids,
            poll,
        },
        idempotency_key: idempotency_key
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        scheduled_at,
        quoted_status_id,
    })
}

fn validate_status_draft_inputs(
    text: &str,
    media_ids: &[String],
    has_poll: bool,
    quoted_status_id: Option<&str>,
) -> std::result::Result<(), String> {
    if text.is_empty() && media_ids.is_empty() && !has_poll {
        return Err("status, media_ids, or poll must be present".to_owned());
    }
    if media_ids.len() > 4 {
        return Err("a maximum of 4 media attachments is supported".to_owned());
    }
    if has_poll && !media_ids.is_empty() {
        return Err("poll cannot be combined with media attachments yet".to_owned());
    }
    if quoted_status_id.is_some() && (has_poll || !media_ids.is_empty()) {
        return Err(
            "quoted statuses cannot be combined with media attachments or polls".to_owned(),
        );
    }
    Ok(())
}

fn status_visibility_from_request(
    value: Option<&str>,
) -> std::result::Result<super::Visibility, String> {
    match value.map(str::trim) {
        Some("") | None => Ok(super::Visibility::Public),
        Some(value) => super::Visibility::parse(value).ok_or_else(|| {
            "visibility must be one of: public, unlisted, private, direct".to_owned()
        }),
    }
}

fn normalized_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_status_media_ids(media_ids: Option<Vec<String>>) -> Vec<String> {
    media_ids
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect()
}

pub(crate) fn normalize_scheduled_at(
    value: Option<&str>,
) -> std::result::Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| "scheduled_at must be a valid RFC 3339 datetime".to_owned())?
        .format(&Rfc3339)
        .map(Some)
        .map_err(|error| format!("failed to format scheduled_at: {error}"))
}

pub(crate) fn validate_scheduled_at_minimum_offset(value: &str) -> std::result::Result<(), String> {
    let scheduled_at = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| "scheduled_at must be a valid RFC 3339 datetime".to_owned())?;
    if scheduled_at <= OffsetDateTime::now_utc() + Duration::minutes(5) {
        return Err(
            "Validation failed: Scheduled at The scheduled date must be in the future".to_owned(),
        );
    }
    Ok(())
}

fn parse_status_poll_from_form(
    form: &FormData,
) -> std::result::Result<Option<CreateStatusPollRequest>, String> {
    let options = form.get_all("poll[options][]").map(|entries| {
        entries
            .into_iter()
            .filter_map(|entry| match entry {
                FormEntry::Field(value) => Some(value),
                FormEntry::File(_) => None,
            })
            .collect::<Vec<_>>()
    });
    let expires_in = form
        .get_field("poll[expires_in]")
        .and_then(|value| value.trim().parse::<u64>().ok());
    let multiple = parse_optional_bool(form.get_field("poll[multiple]").as_deref())?;
    let hide_totals = parse_optional_bool(form.get_field("poll[hide_totals]").as_deref())?;

    if options.is_none() && expires_in.is_none() && multiple.is_none() && hide_totals.is_none() {
        Ok(None)
    } else {
        Ok(Some(CreateStatusPollRequest {
            options,
            expires_in,
            multiple,
            hide_totals,
        }))
    }
}

pub(crate) async fn parse_update_status_request(
    req: &mut Request,
) -> std::result::Result<UpdateStatusRequest, String> {
    let request = read_update_status_request(req).await?;
    Ok(normalize_update_status_request(request))
}

async fn read_update_status_request(
    req: &mut Request,
) -> std::result::Result<UpdateStatusRequest, String> {
    let content_type = request_content_type(req)?;
    if request_is_json(&content_type) {
        req.json::<UpdateStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON status payload: {error}"))
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form status payload: {error}"))?;
        update_status_request_from_form(&form)
    }
}

fn update_status_request_from_form(
    form: &FormData,
) -> std::result::Result<UpdateStatusRequest, String> {
    Ok(UpdateStatusRequest {
        status: form.get_field("status"),
        media_ids: parse_media_ids_from_form(form),
        media_attributes: Some(parse_status_media_attributes_from_form(form)),
        poll: parse_status_poll_from_form(form)?,
        sensitive: parse_optional_bool(form.get_field("sensitive").as_deref())?,
        spoiler_text: form.get_field("spoiler_text"),
        language: form.get_field("language"),
    })
}

fn normalize_update_status_request(mut request: UpdateStatusRequest) -> UpdateStatusRequest {
    if let Some(status) = request.status.as_mut() {
        *status = status.trim().to_owned();
    }
    if let Some(spoiler_text) = request.spoiler_text.as_mut() {
        *spoiler_text = spoiler_text.trim().to_owned();
    }
    if let Some(language) = request.language.as_mut() {
        *language = language.trim().to_ascii_lowercase();
        if language.is_empty() {
            request.language = None;
        }
    }
    if let Some(media_ids) = request.media_ids.as_mut() {
        *media_ids = media_ids
            .iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect();
    }
    request.media_attributes = request
        .media_attributes
        .map(normalize_status_media_attributes)
        .filter(|attributes| !attributes.is_empty());

    request
}

fn normalize_status_media_attributes(
    media_attributes: Vec<StatusMediaAttributeRequest>,
) -> Vec<StatusMediaAttributeRequest> {
    media_attributes
        .into_iter()
        .filter_map(normalize_status_media_attribute)
        .collect()
}

fn normalize_status_media_attribute(
    mut attribute: StatusMediaAttributeRequest,
) -> Option<StatusMediaAttributeRequest> {
    if let Some(id) = attribute.id.as_mut() {
        *id = id.trim().to_owned();
    }
    if let Some(description) = attribute.description.as_mut() {
        *description = description.trim().to_owned();
    }
    if let Some(focus) = attribute.focus.as_mut() {
        *focus = focus.trim().to_owned();
    }
    let id = attribute.id.filter(|value| !value.is_empty());
    let description = attribute.description;
    let focus = attribute.focus.filter(|value| !value.is_empty());
    if id.is_none() && description.is_none() && focus.is_none() {
        None
    } else {
        Some(StatusMediaAttributeRequest {
            id,
            description,
            focus,
        })
    }
}

fn parse_status_media_attributes_from_form(form: &FormData) -> Vec<StatusMediaAttributeRequest> {
    (0..4)
        .filter_map(|index| {
            let id = form.get_field(&format!("media_attributes[{index}][id]"));
            let description = form.get_field(&format!("media_attributes[{index}][description]"));
            let focus = form.get_field(&format!("media_attributes[{index}][focus]"));
            if id.is_none() && description.is_none() && focus.is_none() {
                None
            } else {
                Some(StatusMediaAttributeRequest {
                    id,
                    description,
                    focus,
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_visibility_from_request_defaults_and_normalizes() {
        assert_eq!(
            status_visibility_from_request(None).unwrap(),
            super::super::Visibility::Public
        );
        assert_eq!(
            status_visibility_from_request(Some(" unlisted ")).unwrap(),
            super::super::Visibility::Unlisted
        );
        assert!(status_visibility_from_request(Some("friends")).is_err());
    }

    #[test]
    fn validate_status_draft_inputs_rejects_empty_and_conflicting_payloads() {
        assert!(validate_status_draft_inputs("", &[], false, None).is_err());
        assert!(
            validate_status_draft_inputs(
                "",
                &[
                    "1".to_owned(),
                    "2".to_owned(),
                    "3".to_owned(),
                    "4".to_owned(),
                    "5".to_owned()
                ],
                false,
                None,
            )
            .is_err()
        );
        assert!(validate_status_draft_inputs("", &["1".to_owned()], true, None).is_err());
        assert!(
            validate_status_draft_inputs("quote", &["1".to_owned()], false, Some("status-1"))
                .is_err()
        );
        assert!(validate_status_draft_inputs("hello", &[], false, None).is_ok());
    }

    #[test]
    fn parsed_status_draft_from_request_trims_fields() {
        let request = CreateStatusRequest {
            status: Some("  hello  ".to_owned()),
            in_reply_to_id: Some(" reply-1 ".to_owned()),
            quoted_status_id: Some(" quoted-1 ".to_owned()),
            spoiler_text: Some(" spoiler ".to_owned()),
            sensitive: Some(true),
            visibility: Some(" private ".to_owned()),
            language: Some(" JA ".to_owned()),
            ..CreateStatusRequest::default()
        };

        let parsed = parsed_status_draft_from_request(request, Some(" key ".to_owned())).unwrap();

        assert_eq!(parsed.draft.text, "hello");
        assert_eq!(parsed.draft.in_reply_to_id.as_deref(), Some("reply-1"));
        assert_eq!(parsed.quoted_status_id.as_deref(), Some("quoted-1"));
        assert_eq!(parsed.draft.spoiler_text, "spoiler");
        assert!(parsed.draft.sensitive);
        assert_eq!(
            parsed.draft.visibility,
            super::super::Visibility::FollowersOnly
        );
        assert_eq!(parsed.draft.language.as_deref(), Some("ja"));
        assert_eq!(parsed.idempotency_key.as_deref(), Some("key"));
    }

    #[test]
    fn normalize_update_status_request_trims_fields() {
        let request = UpdateStatusRequest {
            status: Some("  edited  ".to_owned()),
            media_ids: Some(vec![
                " media-1 ".to_owned(),
                String::new(),
                " media-2 ".to_owned(),
            ]),
            media_attributes: Some(vec![
                StatusMediaAttributeRequest {
                    id: Some(" media-1 ".to_owned()),
                    description: Some(" alt text ".to_owned()),
                    focus: Some(" 0.1,0.2 ".to_owned()),
                },
                StatusMediaAttributeRequest {
                    id: Some(" ".to_owned()),
                    description: None,
                    focus: Some(" ".to_owned()),
                },
            ]),
            spoiler_text: Some(" cw ".to_owned()),
            language: Some(" JA ".to_owned()),
            ..UpdateStatusRequest::default()
        };

        let normalized = normalize_update_status_request(request);

        assert_eq!(normalized.status.as_deref(), Some("edited"));
        assert_eq!(
            normalized.media_ids.as_deref(),
            Some(["media-1".to_owned(), "media-2".to_owned()].as_slice())
        );
        let media_attributes = normalized.media_attributes.unwrap();
        assert_eq!(media_attributes.len(), 1);
        assert_eq!(media_attributes[0].id.as_deref(), Some("media-1"));
        assert_eq!(media_attributes[0].description.as_deref(), Some("alt text"));
        assert_eq!(media_attributes[0].focus.as_deref(), Some("0.1,0.2"));
        assert_eq!(normalized.spoiler_text.as_deref(), Some("cw"));
        assert_eq!(normalized.language.as_deref(), Some("ja"));
    }
}
