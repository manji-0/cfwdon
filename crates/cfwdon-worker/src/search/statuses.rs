use super::helpers::{
    normalize_search_match_text, normalize_search_query_input, search_text_match_rank,
};
use crate::content_helpers::extract_mentions_from_text;
use crate::extract_hashtags_from_text;
use crate::find_media_attachments_by_status_id;
use crate::parse_remote_http_url;
use crate::{
    MastodonStatusResponse, build_local_status_response, build_remote_status_response,
    can_view_local_status, find_local_status_by_object_uri, is_public_activitypub_visibility,
    load_in_reply_to_account_id, resolve_remote_status_by_url,
};
use cfwdon_core::AppConfig;
use cfwdon_domain::LocalAccount;
use time::format_description::parse as parse_format_description;
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, OffsetDateTime};
use worker::{D1Database, Result};

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ParsedStatusSearchQuery {
    pub(crate) text_query: String,
    pub(crate) included_text_terms: Vec<String>,
    pub(crate) excluded_text_terms: Vec<String>,
    pub(crate) from: Option<String>,
    pub(crate) not_from: Option<String>,
    pub(crate) before: Option<String>,
    pub(crate) after: Option<String>,
    pub(crate) excluded_before: Option<String>,
    pub(crate) excluded_after: Option<String>,
    pub(crate) excluded_during: Vec<(String, String)>,
    pub(crate) language: Option<String>,
    pub(crate) not_language: Option<String>,
    pub(crate) is_reply: Option<bool>,
    pub(crate) is_sensitive: Option<bool>,
    pub(crate) is_boost: Option<bool>,
    pub(crate) is_quote: Option<bool>,
    pub(crate) has_media: Option<bool>,
    pub(crate) has_poll: Option<bool>,
    pub(crate) has_embed: Option<bool>,
    pub(crate) in_public: Option<bool>,
    pub(crate) in_library: Option<bool>,
    pub(crate) unsatisfiable: bool,
}

fn merge_exact_filter(current: &mut Option<String>, next: String, unsatisfiable: &mut bool) {
    match current {
        Some(existing) if existing != &next => *unsatisfiable = true,
        Some(_) => {}
        None => *current = Some(next),
    }
}

fn merge_boolean_filter(current: &mut Option<bool>, next: bool, unsatisfiable: &mut bool) {
    match current {
        Some(existing) if *existing != next => *unsatisfiable = true,
        Some(_) => {}
        None => *current = Some(next),
    }
}

fn tokenize_status_search_query(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in normalize_search_query_input(query).chars() {
        if escaped {
            current.push('\\');
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                escaped = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current);
                    current = String::new();
                }
            }
            _ => current.push(ch),
        }
    }

    if escaped {
        current.push('\\');
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn status_search_token_has_unescaped_trailing_quote(value: &str) -> bool {
    if !value.ends_with('"') {
        return false;
    }

    value
        .chars()
        .rev()
        .skip(1)
        .take_while(|ch| *ch == '\\')
        .count()
        % 2
        == 0
}

fn unescape_status_search_token(value: &str) -> String {
    let mut unescaped = String::new();
    let mut escaped = false;

    for ch in value.chars() {
        if escaped {
            if ch == '"' || ch == '\\' || ch.is_whitespace() {
                unescaped.push(ch);
            } else {
                unescaped.push('\\');
                unescaped.push(ch);
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            unescaped.push(ch);
        }
    }

    if escaped {
        unescaped.push('\\');
    }

    unescaped
}

fn unquote_status_search_token(value: &str) -> String {
    let value = value.trim();
    let value = if value.len() >= 2
        && value.starts_with('"')
        && status_search_token_has_unescaped_trailing_quote(value)
    {
        &value[1..value.len() - 1]
    } else {
        value
    };

    unescape_status_search_token(value)
        .trim_start_matches('@')
        .trim()
        .to_owned()
}

fn fallback_status_search_term(token: &str) -> String {
    if let Some((prefix, value)) = token.split_once(':') {
        let prefix = prefix.trim().to_ascii_lowercase();
        let value = unquote_status_search_token(value);
        if !prefix.is_empty() && !value.is_empty() {
            return format!("{prefix} {value}");
        }
    }
    unquote_status_search_token(token)
}

fn normalize_status_search_language(value: &str) -> Option<String> {
    let normalized = unquote_status_search_token(value).to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    Some(
        normalized
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_owned(),
    )
}

fn normalize_status_search_timestamp(value: &str) -> Option<String> {
    let value = unquote_status_search_token(value);
    if value.is_empty() {
        return None;
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        let timestamp = if value.len() >= 13 {
            OffsetDateTime::from_unix_timestamp_nanos(value.parse::<i128>().ok()? * 1_000_000)
                .ok()?
        } else {
            OffsetDateTime::from_unix_timestamp(value.parse::<i64>().ok()?).ok()?
        };
        return timestamp.format(&Rfc3339).ok();
    }
    if let Ok(timestamp) = OffsetDateTime::parse(&value, &Rfc3339) {
        return timestamp.format(&Rfc3339).ok();
    }

    let format = parse_format_description("[year]-[month]-[day]").ok()?;
    let date = Date::parse(&value, &format).ok()?;
    date.with_hms(0, 0, 0)
        .ok()?
        .assume_utc()
        .format(&Rfc3339)
        .ok()
}

fn next_day_status_search_timestamp(value: &str) -> Option<String> {
    let value = unquote_status_search_token(value);
    if value.is_empty() {
        return None;
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return normalize_status_search_timestamp(&value);
    }
    if let Ok(timestamp) = OffsetDateTime::parse(&value, &Rfc3339) {
        return timestamp.format(&Rfc3339).ok();
    }

    let format = parse_format_description("[year]-[month]-[day]").ok()?;
    let date = Date::parse(&value, &format).ok()? + Duration::days(1);
    date.with_hms(0, 0, 0)
        .ok()?
        .assume_utc()
        .format(&Rfc3339)
        .ok()
}

pub(crate) fn earlier_status_search_bound(
    left: Option<String>,
    right: Option<String>,
) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub(crate) fn later_status_search_bound(
    left: Option<String>,
    right: Option<String>,
) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn split_status_search_negation(token: &str) -> (bool, &str) {
    if let Some(value) = token.strip_prefix('-') {
        (true, value)
    } else if let Some(value) = token.strip_prefix('+') {
        (false, value)
    } else {
        (false, token)
    }
}

fn set_status_search_text_terms(parsed: &mut ParsedStatusSearchQuery, terms: Vec<String>) {
    parsed.text_query = terms.join(" ").trim().to_owned();
    parsed.included_text_terms = terms;
}

fn merge_status_search_from_filter(
    parsed: &mut ParsedStatusSearchQuery,
    value: String,
    negated: bool,
) {
    if negated {
        merge_exact_filter(
            &mut parsed.not_from,
            value.clone(),
            &mut parsed.unsatisfiable,
        );
        if parsed.from.as_deref() == Some(value.as_str()) {
            parsed.unsatisfiable = true;
        }
    } else {
        merge_exact_filter(&mut parsed.from, value.clone(), &mut parsed.unsatisfiable);
        if parsed.not_from.as_deref() == Some(value.as_str()) {
            parsed.unsatisfiable = true;
        }
    }
}

fn merge_status_search_language_filter(
    parsed: &mut ParsedStatusSearchQuery,
    value: String,
    negated: bool,
) {
    if negated {
        merge_exact_filter(
            &mut parsed.not_language,
            value.clone(),
            &mut parsed.unsatisfiable,
        );
        if parsed.language.as_deref() == Some(value.as_str()) {
            parsed.unsatisfiable = true;
        }
    } else {
        merge_exact_filter(
            &mut parsed.language,
            value.clone(),
            &mut parsed.unsatisfiable,
        );
        if parsed.not_language.as_deref() == Some(value.as_str()) {
            parsed.unsatisfiable = true;
        }
    }
}

fn merge_status_search_before_filter(
    parsed: &mut ParsedStatusSearchQuery,
    value: Option<String>,
    negated: bool,
) {
    if negated {
        parsed.excluded_before = later_status_search_bound(parsed.excluded_before.take(), value);
    } else {
        parsed.before = earlier_status_search_bound(parsed.before.take(), value);
    }
}

fn merge_status_search_after_filter(
    parsed: &mut ParsedStatusSearchQuery,
    value: Option<String>,
    negated: bool,
) {
    if negated {
        parsed.excluded_after = earlier_status_search_bound(parsed.excluded_after.take(), value);
    } else {
        parsed.after = later_status_search_bound(parsed.after.take(), value);
    }
}

fn merge_status_search_during_filter(
    parsed: &mut ParsedStatusSearchQuery,
    start: Option<String>,
    end: Option<String>,
    negated: bool,
) {
    if negated {
        if let (Some(start), Some(end)) = (start, end) {
            parsed.excluded_during.push((start, end));
        }
    } else {
        parsed.after = later_status_search_bound(parsed.after.take(), start);
        parsed.before = earlier_status_search_bound(parsed.before.take(), end);
    }
}

fn merge_status_search_is_filter(parsed: &mut ParsedStatusSearchQuery, value: &str, negated: bool) {
    match unquote_status_search_token(value)
        .to_ascii_lowercase()
        .as_str()
    {
        "reply" => merge_boolean_filter(&mut parsed.is_reply, !negated, &mut parsed.unsatisfiable),
        "sensitive" => merge_boolean_filter(
            &mut parsed.is_sensitive,
            !negated,
            &mut parsed.unsatisfiable,
        ),
        "boost" | "reblog" => {
            merge_boolean_filter(&mut parsed.is_boost, !negated, &mut parsed.unsatisfiable)
        }
        "quote" => merge_boolean_filter(&mut parsed.is_quote, !negated, &mut parsed.unsatisfiable),
        _ => {}
    }
}

fn merge_status_search_has_filter(
    parsed: &mut ParsedStatusSearchQuery,
    value: &str,
    negated: bool,
) {
    match unquote_status_search_token(value)
        .to_ascii_lowercase()
        .as_str()
    {
        "media" => merge_boolean_filter(&mut parsed.has_media, !negated, &mut parsed.unsatisfiable),
        "poll" => merge_boolean_filter(&mut parsed.has_poll, !negated, &mut parsed.unsatisfiable),
        "embed" | "link" | "preview" => {
            merge_boolean_filter(&mut parsed.has_embed, !negated, &mut parsed.unsatisfiable)
        }
        _ => {}
    }
}

fn merge_status_search_in_filter(parsed: &mut ParsedStatusSearchQuery, value: &str, negated: bool) {
    match unquote_status_search_token(value)
        .to_ascii_lowercase()
        .as_str()
    {
        "public" => {
            merge_boolean_filter(&mut parsed.in_public, !negated, &mut parsed.unsatisfiable)
        }
        "library" => {
            merge_boolean_filter(&mut parsed.in_library, !negated, &mut parsed.unsatisfiable)
        }
        _ => {}
    }
}

fn apply_status_search_prefixed_filter(
    parsed: &mut ParsedStatusSearchQuery,
    prefix: &str,
    value: &str,
    negated: bool,
) -> bool {
    match prefix.trim().to_ascii_lowercase().as_str() {
        "from" => {
            let value = unquote_status_search_token(value);
            if !value.is_empty() {
                merge_status_search_from_filter(parsed, value, negated);
            }
        }
        "before" => {
            let value = normalize_status_search_timestamp(value);
            merge_status_search_before_filter(parsed, value, negated);
        }
        "after" => {
            let value = normalize_status_search_timestamp(value);
            merge_status_search_after_filter(parsed, value, negated);
        }
        "during" => {
            let start = normalize_status_search_timestamp(value);
            let end = next_day_status_search_timestamp(value);
            merge_status_search_during_filter(parsed, start, end, negated);
        }
        "language" => {
            if let Some(value) = normalize_status_search_language(value) {
                merge_status_search_language_filter(parsed, value, negated);
            }
        }
        "is" => merge_status_search_is_filter(parsed, value, negated),
        "has" => merge_status_search_has_filter(parsed, value, negated),
        "in" => merge_status_search_in_filter(parsed, value, negated),
        _ => return false,
    }
    true
}

fn collect_status_search_term(
    parsed: &mut ParsedStatusSearchQuery,
    terms: &mut Vec<String>,
    token: &str,
    negated: bool,
) {
    let value = fallback_status_search_term(token);
    if value.is_empty() {
        return;
    }
    if negated {
        parsed.excluded_text_terms.push(value);
    } else {
        terms.push(value);
    }
}

pub(crate) fn parse_status_search_query(query: &str) -> ParsedStatusSearchQuery {
    let mut parsed = ParsedStatusSearchQuery::default();
    let mut terms = Vec::new();

    for token in tokenize_status_search_query(query) {
        let (negated, token) = split_status_search_negation(&token);

        if let Some((prefix, value)) = token.split_once(':')
            && apply_status_search_prefixed_filter(&mut parsed, prefix, value, negated)
        {
            continue;
        }
        collect_status_search_term(&mut parsed, &mut terms, token, negated);
    }

    set_status_search_text_terms(&mut parsed, terms);
    parsed
}

pub(crate) async fn resolve_search_status(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &str,
) -> Result<Option<MastodonStatusResponse>> {
    let query = query.trim();
    if parse_remote_http_url(query).is_err() {
        return Ok(None);
    }

    if let Some(status) = find_local_status_by_object_uri(db, config, query).await? {
        let Some(account) = crate::find_account_by_id(db, &status.account_id).await? else {
            return Ok(None);
        };
        if !can_view_local_status(db, &status, Some(viewer), &account).await? {
            return Ok(None);
        }
        let media = find_media_attachments_by_status_id(db, &status.id).await?;
        return Ok(Some(
            build_local_status_response(
                db,
                config,
                Some(viewer),
                &status,
                &account,
                load_in_reply_to_account_id(db, &status).await?,
                media,
            )
            .await?,
        ));
    }

    if let Some((status, actor)) = resolve_remote_status_by_url(db, config, query).await? {
        if !is_public_activitypub_visibility(&status.visibility) {
            return Ok(None);
        }
        return Ok(Some(
            build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
        ));
    }

    Ok(None)
}

fn status_text_match_rank(query: &str, candidate: &str) -> u8 {
    let terms = query
        .split_whitespace()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if terms.len() == 1 && status_search_term_is_hashtag(terms[0]) {
        return if status_search_term_matches(terms[0], candidate, "") {
            0
        } else {
            4
        };
    }

    let phrase_rank = search_text_match_rank(query, candidate);
    if phrase_rank < 3 {
        return phrase_rank;
    }

    if terms.len() > 1
        && terms
            .iter()
            .all(|value| status_search_term_matches(value, candidate, ""))
    {
        3
    } else {
        4
    }
}

fn status_search_term_is_hashtag(term: &str) -> bool {
    term.trim().starts_with('#')
}

fn status_search_term_matches(term: &str, content: &str, spoiler_text: &str) -> bool {
    let term = term.trim();
    if term.is_empty() {
        return false;
    }

    if let Some(hashtag) = term.strip_prefix('#') {
        let hashtag = hashtag.trim().to_ascii_lowercase();
        if hashtag.is_empty() {
            return false;
        }
        let content_tags = extract_hashtags_from_text(content);
        let spoiler_tags = extract_hashtags_from_text(spoiler_text);
        return content_tags.iter().any(|value| value == &hashtag)
            || spoiler_tags.iter().any(|value| value == &hashtag);
    }

    let term = normalize_search_match_text(term);
    let content = normalize_search_match_text(content);
    let spoiler_text = normalize_search_match_text(spoiler_text);
    content.contains(&term) || spoiler_text.contains(&term)
}

fn status_phrase_term_rank(parsed_query: &ParsedStatusSearchQuery, candidate: &str) -> u8 {
    parsed_query
        .included_text_terms
        .iter()
        .filter(|term| term.contains(char::is_whitespace) || status_search_term_is_hashtag(term))
        .map(|term| status_text_match_rank(term, candidate))
        .min()
        .unwrap_or(4)
}

pub(crate) fn status_search_rank(
    parsed_query: &ParsedStatusSearchQuery,
    content: &str,
    spoiler_text: &str,
) -> (u8, u8, u8) {
    let combined = if spoiler_text.trim().is_empty() {
        content.to_owned()
    } else if content.trim().is_empty() {
        spoiler_text.to_owned()
    } else {
        format!("{content}\n{spoiler_text}")
    };
    [
        (
            status_text_match_rank(&parsed_query.text_query, content),
            status_phrase_term_rank(parsed_query, content),
            0u8,
        ),
        (
            status_text_match_rank(&parsed_query.text_query, spoiler_text),
            status_phrase_term_rank(parsed_query, spoiler_text),
            1u8,
        ),
        (
            status_text_match_rank(&parsed_query.text_query, &combined),
            status_phrase_term_rank(parsed_query, &combined),
            2u8,
        ),
    ]
    .into_iter()
    .min()
    .unwrap_or((4, 4, 2))
}

pub(crate) fn status_matches_search_syntax(
    parsed_query: &ParsedStatusSearchQuery,
    content: &str,
    spoiler_text: &str,
    in_reply_to: bool,
    sensitive: bool,
    is_boost: bool,
    is_quote: bool,
    language: Option<&str>,
) -> bool {
    if parsed_query.unsatisfiable {
        return false;
    }
    if parsed_query
        .included_text_terms
        .iter()
        .any(|value| !status_search_term_matches(value, content, spoiler_text))
    {
        return false;
    }
    if parsed_query
        .excluded_text_terms
        .iter()
        .any(|value| status_search_term_matches(value, content, spoiler_text))
    {
        return false;
    }
    if let Some(expected) = parsed_query.is_reply
        && in_reply_to != expected
    {
        return false;
    }
    if let Some(expected) = parsed_query.is_sensitive
        && sensitive != expected
    {
        return false;
    }
    if let Some(expected) = parsed_query.is_boost
        && is_boost != expected
    {
        return false;
    }
    if let Some(expected) = parsed_query.is_quote
        && is_quote != expected
    {
        return false;
    }
    if let Some(expected_language) = parsed_query.language.as_deref()
        && language
            .and_then(normalize_status_search_language)
            .as_deref()
            != Some(expected_language)
    {
        return false;
    }
    if let Some(excluded_language) = parsed_query.not_language.as_deref()
        && language
            .and_then(normalize_status_search_language)
            .as_deref()
            == Some(excluded_language)
    {
        return false;
    }
    true
}

pub(crate) fn status_matches_search_timestamp(
    parsed_query: &ParsedStatusSearchQuery,
    timestamp: &str,
) -> bool {
    if let Some(excluded_before) = parsed_query.excluded_before.as_deref()
        && timestamp < excluded_before
    {
        return false;
    }
    if let Some(excluded_after) = parsed_query.excluded_after.as_deref()
        && timestamp > excluded_after
    {
        return false;
    }
    if parsed_query
        .excluded_during
        .iter()
        .any(|(start, end)| timestamp >= start.as_str() && timestamp < end.as_str())
    {
        return false;
    }
    true
}

fn status_search_seed_query(parsed_query: &ParsedStatusSearchQuery) -> &str {
    parsed_query
        .included_text_terms
        .iter()
        .max_by_key(|value| value.len())
        .map(String::as_str)
        .unwrap_or_default()
}

pub(crate) fn status_search_query_terms(parsed_query: &ParsedStatusSearchQuery) -> Vec<String> {
    let mut terms = parsed_query
        .included_text_terms
        .iter()
        .map(|term| term.trim().to_owned())
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();

    if terms.is_empty() {
        let seed = status_search_seed_query(parsed_query).trim();
        if !seed.is_empty() {
            terms.push(seed.to_owned());
        }
        return terms;
    }

    let query = parsed_query.text_query.trim();
    if !query.is_empty() && !terms.iter().any(|term| term == query) {
        terms.push(query.to_owned());
    }

    terms
}

pub(crate) fn status_matches_search_metadata(
    parsed_query: &ParsedStatusSearchQuery,
    has_media: bool,
    has_poll: bool,
    has_embed: bool,
) -> bool {
    if let Some(expected) = parsed_query.has_media
        && has_media != expected
    {
        return false;
    }
    if let Some(expected) = parsed_query.has_poll
        && has_poll != expected
    {
        return false;
    }
    if let Some(expected) = parsed_query.has_embed
        && has_embed != expected
    {
        return false;
    }
    true
}

pub(crate) fn status_matches_search_scope(
    parsed_query: &ParsedStatusSearchQuery,
    is_public: bool,
    is_library: bool,
) -> bool {
    if let Some(expected) = parsed_query.in_public
        && is_public != expected
    {
        return false;
    }
    if let Some(expected) = parsed_query.in_library
        && is_library != expected
    {
        return false;
    }
    true
}

pub(crate) fn status_is_searchable_by_scope(
    parsed_query: &ParsedStatusSearchQuery,
    is_public: bool,
    is_library: bool,
) -> bool {
    (is_public || is_library) && status_matches_search_scope(parsed_query, is_public, is_library)
}

pub(crate) fn text_mentions_search_library_viewer(
    config: &AppConfig,
    text_content: &str,
    viewer_username: &str,
) -> bool {
    extract_mentions_from_text(text_content, config)
        .into_iter()
        .any(|handle| handle.username == viewer_username)
}
