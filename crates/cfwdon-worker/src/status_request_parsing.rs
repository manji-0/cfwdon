use super::{StatusDraft, normalize_status_poll, parse_media_ids_from_form, parse_optional_bool};
use serde::Deserialize;
use worker::{FormData, FormEntry, Request};

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CreateStatusRequest {
    pub(crate) status: Option<String>,
    pub(crate) media_ids: Option<Vec<String>>,
    pub(crate) poll: Option<CreateStatusPollRequest>,
    pub(crate) in_reply_to_id: Option<String>,
    pub(crate) sensitive: Option<bool>,
    pub(crate) spoiler_text: Option<String>,
    pub(crate) visibility: Option<String>,
    pub(crate) language: Option<String>,
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
pub(crate) struct AccountStatusesQuery {
    pub(crate) limit: Option<u32>,
    pub(crate) only_media: Option<bool>,
    pub(crate) exclude_replies: Option<bool>,
    pub(crate) exclude_reblogs: Option<bool>,
    pub(crate) pinned: Option<bool>,
    pub(crate) tagged: Option<String>,
}

pub(crate) async fn parse_status_draft(
    req: &mut Request,
) -> std::result::Result<StatusDraft, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let request = if content_type.contains("application/json") {
        req.json::<CreateStatusRequest>()
            .await
            .map_err(|error| format!("invalid JSON status payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form status payload: {error}"))?;

        CreateStatusRequest {
            status: form.get_field("status"),
            media_ids: parse_media_ids_from_form(&form),
            poll: parse_status_poll_from_form(&form)?,
            in_reply_to_id: form.get_field("in_reply_to_id"),
            sensitive: parse_optional_bool(form.get_field("sensitive").as_deref())?,
            spoiler_text: form.get_field("spoiler_text"),
            visibility: form.get_field("visibility"),
            language: form.get_field("language"),
        }
    };

    let text = request.status.unwrap_or_default().trim().to_owned();
    let poll = normalize_status_poll(request.poll)?;
    let media_ids = request
        .media_ids
        .unwrap_or_default()
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if text.is_empty() && media_ids.is_empty() && poll.is_none() {
        return Err("status, media_ids, or poll must be present".to_owned());
    }
    if media_ids.len() > 4 {
        return Err("a maximum of 4 media attachments is supported".to_owned());
    }
    if poll.is_some() && !media_ids.is_empty() {
        return Err("poll cannot be combined with media attachments yet".to_owned());
    }

    let visibility = match request.visibility.as_deref().map(str::trim) {
        Some("") | None => super::Visibility::Public,
        Some(value) => super::Visibility::parse(value).ok_or_else(|| {
            "visibility must be one of: public, unlisted, private, direct".to_owned()
        })?,
    };

    Ok(StatusDraft {
        text,
        visibility,
        spoiler_text: request.spoiler_text.unwrap_or_default().trim().to_owned(),
        sensitive: request.sensitive.unwrap_or(false),
        language: request
            .language
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty()),
        in_reply_to_id: request
            .in_reply_to_id
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        media_ids,
        poll,
    })
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
