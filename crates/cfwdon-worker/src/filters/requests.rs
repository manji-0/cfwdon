use super::{KeywordInput, KeywordRequest, StatusFilterRequest, V1FilterRequest, V2FilterRequest};
use crate::{Error, Request, parse_optional_bool};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

pub(in crate::filters) fn normalize_contexts(
    contexts: Vec<String>,
) -> std::result::Result<Vec<String>, Error> {
    let mut normalized = Vec::new();
    for context in contexts {
        let context = context.trim().to_ascii_lowercase();
        if context.is_empty() {
            continue;
        }
        match context.as_str() {
            "home" | "notifications" | "public" | "thread" | "account" => {
                if !normalized.contains(&context) {
                    normalized.push(context);
                }
            }
            _ => {
                return Err(Error::RustError(
                    "context must be one of: home, notifications, public, thread, account"
                        .to_string(),
                ));
            }
        }
    }
    if normalized.is_empty() {
        return Err(Error::RustError(
            "at least one context is required".to_owned(),
        ));
    }
    Ok(normalized)
}

pub(in crate::filters) fn normalize_filter_action(
    value: Option<&str>,
) -> std::result::Result<String, Error> {
    let normalized = value.unwrap_or("warn").trim().to_ascii_lowercase();
    match normalized.as_str() {
        "warn" | "hide" | "blur" => Ok(normalized),
        _ => Err(Error::RustError(
            "filter_action must be one of: warn, hide, blur".to_owned(),
        )),
    }
}

pub(in crate::filters) fn expires_at_from_seconds(
    seconds: Option<i64>,
) -> std::result::Result<Option<String>, Error> {
    let Some(seconds) = seconds else {
        return Ok(None);
    };
    let now = OffsetDateTime::from_unix_timestamp(crate::now_unix_timestamp())
        .map_err(|error| Error::RustError(format!("invalid current unix timestamp: {error}")))?;
    let expires_at = (now + Duration::seconds(seconds))
        .format(&Rfc3339)
        .map_err(|error| Error::RustError(format!("failed to format expires_at: {error}")))?;
    Ok(Some(expires_at))
}

pub(in crate::filters) fn normalize_keyword(
    value: Option<&str>,
) -> std::result::Result<String, Error> {
    let keyword = value.unwrap_or_default().trim().to_owned();
    if keyword.is_empty() {
        return Err(Error::RustError("keyword must not be empty".to_owned()));
    }
    Ok(keyword)
}

pub(in crate::filters) fn parse_i64(
    value: Option<&str>,
    field: &str,
) -> std::result::Result<Option<i64>, Error> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value) => value
            .parse::<i64>()
            .map(Some)
            .map_err(|_| Error::RustError(format!("{field} must be an integer"))),
    }
}

pub(in crate::filters) fn parse_form_keyword_entries(
    body: &str,
) -> std::result::Result<Vec<KeywordInput>, Error> {
    let mut keywords = Vec::<KeywordInput>::new();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        let key = key.as_ref();
        if key.starts_with("keywords_attributes") && key.ends_with("[keyword]") {
            keywords.push(KeywordInput {
                keyword: Some(value.into_owned()),
                whole_word: None,
            });
            continue;
        }
        if key.starts_with("keywords_attributes") && key.ends_with("[whole_word]") {
            let whole_word = parse_optional_bool(Some(value.as_ref()))
                .map_err(Error::RustError)?
                .unwrap_or(true);
            if let Some(last) = keywords.last_mut() {
                last.whole_word = Some(whole_word);
            } else {
                keywords.push(KeywordInput {
                    keyword: None,
                    whole_word: Some(whole_word),
                });
            }
        }
    }
    Ok(keywords)
}

pub(in crate::filters) async fn parse_v1_filter_request(
    req: &mut Request,
) -> std::result::Result<V1FilterRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        return req
            .json::<V1FilterRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON filter payload: {error}")));
    }

    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid filter payload: {error}")))?;
    let mut request = V1FilterRequest::default();
    let mut contexts = Vec::new();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        match key.as_ref() {
            "phrase" => request.phrase = Some(value.into_owned()),
            "context[]" | "context" => contexts.push(value.into_owned()),
            "expires_in" => request.expires_in = parse_i64(Some(value.as_ref()), "expires_in")?,
            "irreversible" => {
                request.irreversible =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            "whole_word" => {
                request.whole_word =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            _ => {}
        }
    }
    if !contexts.is_empty() {
        request.context = Some(contexts);
    }
    Ok(request)
}

pub(in crate::filters) async fn parse_v2_filter_request(
    req: &mut Request,
) -> std::result::Result<V2FilterRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    if content_type.contains("application/json") {
        return req
            .json::<V2FilterRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON v2 filter payload: {error}")));
    }

    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid v2 filter payload: {error}")))?;
    let mut request = V2FilterRequest::default();
    let mut contexts = Vec::new();
    let mut keywords = parse_form_keyword_entries(&body)?;
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        match key.as_ref() {
            "title" => request.title = Some(value.into_owned()),
            "phrase" => request.phrase = Some(value.into_owned()),
            "context[]" | "context" => contexts.push(value.into_owned()),
            "expires_in" => request.expires_in = parse_i64(Some(value.as_ref()), "expires_in")?,
            "filter_action" => request.filter_action = Some(value.into_owned()),
            "whole_word" => {
                request.whole_word =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            _ => {}
        }
    }
    if !contexts.is_empty() {
        request.context = Some(contexts);
    }
    if !keywords.is_empty() {
        request.keywords = Some(std::mem::take(&mut keywords));
    }
    Ok(request)
}

pub(in crate::filters) async fn parse_keyword_request(
    req: &mut Request,
) -> std::result::Result<KeywordRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("application/json") {
        return req
            .json::<KeywordRequest>()
            .await
            .map_err(|error| Error::RustError(format!("invalid JSON keyword payload: {error}")));
    }
    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid keyword payload: {error}")))?;
    let mut request = KeywordRequest::default();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        match key.as_ref() {
            "keyword" => request.keyword = Some(value.into_owned()),
            "whole_word" => {
                request.whole_word =
                    parse_optional_bool(Some(value.as_ref())).map_err(Error::RustError)?
            }
            _ => {}
        }
    }
    Ok(request)
}

pub(in crate::filters) async fn parse_status_filter_request(
    req: &mut Request,
) -> std::result::Result<StatusFilterRequest, Error> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| Error::RustError(format!("failed to read Content-Type header: {error}")))?
        .unwrap_or_default()
        .to_ascii_lowercase();
    if content_type.contains("application/json") {
        return req.json::<StatusFilterRequest>().await.map_err(|error| {
            Error::RustError(format!("invalid JSON status filter payload: {error}"))
        });
    }
    let body = req
        .text()
        .await
        .map_err(|error| Error::RustError(format!("invalid status filter payload: {error}")))?;
    let mut request = StatusFilterRequest::default();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        if key == "status_id" {
            request.status_id = Some(value.into_owned());
        }
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_contexts_trims_lowercases_and_deduplicates() {
        assert_eq!(
            normalize_contexts(vec![
                " Home ".to_owned(),
                "PUBLIC".to_owned(),
                "home".to_owned(),
                " thread ".to_owned(),
            ])
            .unwrap(),
            vec!["home", "public", "thread"]
        );
    }

    #[test]
    fn normalize_contexts_rejects_empty_and_unknown_values() {
        assert!(normalize_contexts(vec![" ".to_owned()]).is_err());
        assert!(normalize_contexts(vec!["home".to_owned(), "unknown".to_owned()]).is_err());
    }

    #[test]
    fn normalize_filter_action_accepts_current_values() {
        assert_eq!(normalize_filter_action(Some("warn")).unwrap(), "warn");
        assert_eq!(normalize_filter_action(Some("hide")).unwrap(), "hide");
        assert_eq!(normalize_filter_action(Some("blur")).unwrap(), "blur");
    }

    #[test]
    fn parse_form_keyword_entries_pairs_keyword_and_whole_word() {
        let rows = parse_form_keyword_entries(
            "keywords_attributes[][keyword]=mute&keywords_attributes[][whole_word]=false",
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].keyword.as_deref(), Some("mute"));
        assert_eq!(rows[0].whole_word, Some(false));
    }
}
