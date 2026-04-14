use super::{
    AccountReference, D1Database, FormEntry, Request, find_status_by_id, parse_optional_bool,
};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct CreateReportRequest {
    pub(crate) account_id: String,
    pub(crate) status_ids: Option<Vec<String>>,
    pub(crate) comment: Option<String>,
    pub(crate) category: Option<String>,
    pub(crate) forward: Option<bool>,
}

pub(crate) async fn parse_create_report_request(
    req: &mut Request,
) -> std::result::Result<CreateReportRequest, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut request = if content_type.contains("application/json") {
        req.json::<CreateReportRequest>()
            .await
            .map_err(|error| format!("invalid JSON report payload: {error}"))?
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form report payload: {error}"))?;
        CreateReportRequest {
            account_id: form.get_field("account_id").unwrap_or_default(),
            status_ids: form.get_all("status_ids[]").map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|entry| match entry {
                        FormEntry::Field(value) => Some(value),
                        FormEntry::File(_) => None,
                    })
                    .collect()
            }),
            comment: form.get_field("comment"),
            category: form.get_field("category"),
            forward: parse_optional_bool(form.get_field("forward").as_deref())?,
        }
    };

    request.account_id = request.account_id.trim().to_owned();
    if request.account_id.is_empty() {
        return Err("account_id is required".to_owned());
    }
    if let Some(comment) = request.comment.as_mut() {
        *comment = comment.trim().to_owned();
    }
    if request
        .comment
        .as_deref()
        .map(|value| value.chars().count() > 1000)
        .unwrap_or(false)
    {
        return Err("comment must be at most 1000 characters".to_owned());
    }
    if let Some(category) = request.category.as_mut() {
        *category = category.trim().to_ascii_lowercase();
        if category.is_empty() {
            request.category = None;
        }
    }
    if let Some(status_ids) = request.status_ids.as_mut() {
        *status_ids = status_ids
            .iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        status_ids.sort();
        status_ids.dedup();
    }

    match request.category.as_deref().unwrap_or("other") {
        "spam" | "violation" | "other" | "legal" => {}
        _ => return Err("category must be one of: spam, legal, violation, other".to_owned()),
    }

    Ok(request)
}

pub(crate) async fn validate_report_status_ids(
    db: &D1Database,
    target: &AccountReference,
    status_ids: &[String],
) -> std::result::Result<(), String> {
    if status_ids.is_empty() {
        return Ok(());
    }
    let AccountReference::Local(target_account) = target else {
        return Err("status_ids are only supported for local accounts".to_owned());
    };
    for status_id in status_ids {
        let Some(status) = find_status_by_id(db, status_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Err("status not found".to_owned());
        };
        if status.account_id != target_account.id {
            return Err("status_ids must belong to the reported account".to_owned());
        }
    }
    Ok(())
}
