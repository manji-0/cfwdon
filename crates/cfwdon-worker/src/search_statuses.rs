use std::cmp::Reverse;

use crate::content_helpers::extract_mentions_from_text;
use crate::extract_hashtags_from_text;
use crate::find_media_attachments_by_status_id;
use crate::parse_remote_http_url;
use crate::{
    AccountReference, MastodonStatusResponse, build_local_status_response,
    build_remote_status_card_value, build_remote_status_response, build_status_card_value,
    can_view_local_status, find_account_by_username, find_local_status_by_object_uri,
    find_remote_actor_by_username_domain, find_remote_status_attachments_by_status_id,
    find_remote_status_by_id, find_remote_status_poll_by_status_id, find_status_by_id,
    find_status_poll_by_status_id, is_local_status_bookmarked_by, is_local_status_favourited_by,
    is_local_status_reblogged_by, is_public_activitypub_visibility, is_remote_status_bookmarked_by,
    is_remote_status_favourited_by, is_remote_status_reblogged_by, load_in_reply_to_account_id,
    normalize_search_match_text, normalize_search_query_input, parse_lookup_handle,
    resolve_account_reference, resolve_remote_status_by_url, search_local_status_rows,
    search_remote_status_rows, search_text_match_rank, strip_html_tags,
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

fn earlier_status_search_bound(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn later_status_search_bound(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

pub(crate) fn parse_status_search_query(query: &str) -> ParsedStatusSearchQuery {
    let mut parsed = ParsedStatusSearchQuery::default();
    let mut terms = Vec::new();

    for token in tokenize_status_search_query(query) {
        let (negated, token) = if let Some(value) = token.strip_prefix('-') {
            (true, value)
        } else if let Some(value) = token.strip_prefix('+') {
            (false, value)
        } else {
            (false, token.as_str())
        };

        if let Some((prefix, value)) = token.split_once(':') {
            match prefix.trim().to_ascii_lowercase().as_str() {
                "from" => {
                    let value = unquote_status_search_token(value);
                    if !value.is_empty() {
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
                            merge_exact_filter(
                                &mut parsed.from,
                                value.clone(),
                                &mut parsed.unsatisfiable,
                            );
                            if parsed.not_from.as_deref() == Some(value.as_str()) {
                                parsed.unsatisfiable = true;
                            }
                        }
                    }
                    continue;
                }
                "before" => {
                    let value = normalize_status_search_timestamp(value);
                    if negated {
                        parsed.excluded_before =
                            later_status_search_bound(parsed.excluded_before.take(), value);
                    } else {
                        parsed.before = earlier_status_search_bound(parsed.before.take(), value);
                    }
                    continue;
                }
                "after" => {
                    let value = normalize_status_search_timestamp(value);
                    if negated {
                        parsed.excluded_after =
                            earlier_status_search_bound(parsed.excluded_after.take(), value);
                    } else {
                        parsed.after = later_status_search_bound(parsed.after.take(), value);
                    }
                    continue;
                }
                "during" => {
                    let start = normalize_status_search_timestamp(value);
                    let end = next_day_status_search_timestamp(value);
                    if negated {
                        if let (Some(start), Some(end)) = (start, end) {
                            parsed.excluded_during.push((start, end));
                        }
                    } else {
                        parsed.after = later_status_search_bound(parsed.after.take(), start);
                        parsed.before = earlier_status_search_bound(parsed.before.take(), end);
                    }
                    continue;
                }
                "language" => {
                    if let Some(value) = normalize_status_search_language(value) {
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
                    continue;
                }
                "is" => {
                    match unquote_status_search_token(value)
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "reply" => merge_boolean_filter(
                            &mut parsed.is_reply,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        "sensitive" => merge_boolean_filter(
                            &mut parsed.is_sensitive,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        "boost" | "reblog" => merge_boolean_filter(
                            &mut parsed.is_boost,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        "quote" => merge_boolean_filter(
                            &mut parsed.is_quote,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        _ => {}
                    }
                    continue;
                }
                "has" => {
                    match unquote_status_search_token(value)
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "media" => merge_boolean_filter(
                            &mut parsed.has_media,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        "poll" => merge_boolean_filter(
                            &mut parsed.has_poll,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        "embed" | "link" | "preview" => merge_boolean_filter(
                            &mut parsed.has_embed,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        _ => {}
                    }
                    continue;
                }
                "in" => {
                    match unquote_status_search_token(value)
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "public" => merge_boolean_filter(
                            &mut parsed.in_public,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        "library" => merge_boolean_filter(
                            &mut parsed.in_library,
                            !negated,
                            &mut parsed.unsatisfiable,
                        ),
                        _ => {}
                    }
                    continue;
                }
                _ => {}
            }
        }
        let value = fallback_status_search_term(token);
        if value.is_empty() {
            continue;
        }
        if negated {
            parsed.excluded_text_terms.push(value);
        } else {
            terms.push(value);
        }
    }

    parsed.included_text_terms = terms.clone();
    parsed.text_query = terms.join(" ").trim().to_owned();
    parsed
}

async fn resolve_status_search_from_reference(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    value: &str,
) -> Result<Option<AccountReference>> {
    if value.eq_ignore_ascii_case("me") {
        return Ok(Some(AccountReference::Local(viewer.clone())));
    }

    let handle = match parse_lookup_handle(value, config) {
        Ok(handle) => handle,
        Err(_) => return Ok(None),
    };

    if handle.is_local_to(&config.instance_domain) {
        return Ok(find_account_by_username(db, &handle.username)
            .await?
            .map(AccountReference::Local));
    }

    Ok(find_remote_actor_by_username_domain(
        db,
        &handle.username,
        handle.domain.as_deref().unwrap_or_default(),
    )
    .await?
    .map(AccountReference::Remote))
}

fn account_reference_identity(reference: &AccountReference) -> &str {
    match reference {
        AccountReference::Local(account) => &account.id,
        AccountReference::Remote(actor) => &actor.actor_uri,
    }
}

fn merge_status_search_account_reference(
    current: Option<AccountReference>,
    syntax: Option<AccountReference>,
) -> Option<AccountReference> {
    match (current, syntax) {
        (Some(current), Some(syntax))
            if account_reference_identity(&current) == account_reference_identity(&syntax) =>
        {
            Some(current)
        }
        (Some(_), Some(_)) => None,
        (Some(current), None) => Some(current),
        (None, Some(syntax)) => Some(syntax),
        (None, None) => None,
    }
}

async fn resolve_search_status_bound_timestamp(
    db: &D1Database,
    status_id: Option<&str>,
) -> Result<Option<String>> {
    let Some(status_id) = status_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if let Some(status) = find_status_by_id(db, status_id).await? {
        return Ok(Some(status.created_at));
    }
    if let Some(status) = find_remote_status_by_id(db, status_id).await? {
        return Ok(Some(status.published_at));
    }
    Ok(None)
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

fn account_reference_matches_owner(
    reference: &AccountReference,
    local_account_id: Option<&str>,
    remote_actor_uri: Option<&str>,
) -> bool {
    match reference {
        AccountReference::Local(account) => Some(account.id.as_str()) == local_account_id,
        AccountReference::Remote(actor) => Some(actor.actor_uri.as_str()) == remote_actor_uri,
    }
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

async fn local_status_is_in_search_library(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    status: &crate::StatusRow,
) -> Result<bool> {
    if status.account_id == viewer.id {
        return Ok(true);
    }
    if is_local_status_favourited_by(db, &viewer.id, status).await? {
        return Ok(true);
    }
    if is_local_status_bookmarked_by(db, &viewer.id, status).await? {
        return Ok(true);
    }
    if is_local_status_reblogged_by(db, &viewer.id, status).await? {
        return Ok(true);
    }
    if text_mentions_search_library_viewer(config, &status._text_content, &viewer.username) {
        return Ok(true);
    }
    Ok(false)
}

async fn remote_status_is_in_search_library(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    status: &crate::RemoteStatusRow,
) -> Result<bool> {
    if is_remote_status_favourited_by(db, &viewer.id, &status.id).await? {
        return Ok(true);
    }
    if is_remote_status_bookmarked_by(db, &viewer.id, &status.id).await? {
        return Ok(true);
    }
    if is_remote_status_reblogged_by(db, &viewer.id, &status.id).await? {
        return Ok(true);
    }
    if text_mentions_search_library_viewer(
        config,
        &strip_html_tags(&status.content_html),
        &viewer.username,
    ) {
        return Ok(true);
    }
    Ok(false)
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

pub(crate) async fn search_statuses_for_v2(
    db: &D1Database,
    config: &AppConfig,
    viewer: &LocalAccount,
    query: &str,
    limit: u32,
    offset: u32,
    account_id: Option<&str>,
    max_id: Option<&str>,
    min_id: Option<&str>,
) -> Result<Vec<MastodonStatusResponse>> {
    let parsed_query = parse_status_search_query(query);
    if parsed_query.unsatisfiable {
        return Ok(Vec::new());
    }

    let account_reference = match account_id {
        Some(account_id) => resolve_account_reference(db, account_id).await?,
        None => None,
    };
    if account_id.is_some() && account_reference.is_none() {
        return Ok(Vec::new());
    }
    let syntax_account_reference = match parsed_query.from.as_deref() {
        Some(value) => resolve_status_search_from_reference(db, config, viewer, value).await?,
        None => None,
    };
    let excluded_account_reference = match parsed_query.not_from.as_deref() {
        Some(value) => resolve_status_search_from_reference(db, config, viewer, value).await?,
        None => None,
    };
    let account_reference =
        merge_status_search_account_reference(account_reference, syntax_account_reference);
    if parsed_query.from.is_some() && account_reference.is_none() {
        return Ok(Vec::new());
    }
    // Oversample to keep relevance-ranked matches from being truncated before the final sort.
    let query_limit = limit.saturating_add(offset).saturating_mul(4).clamp(limit, 200);
    let cursor_max_timestamp = resolve_search_status_bound_timestamp(db, max_id).await?;
    let cursor_min_timestamp = resolve_search_status_bound_timestamp(db, min_id).await?;
    let max_timestamp =
        earlier_status_search_bound(cursor_max_timestamp.clone(), parsed_query.before.clone());
    let min_timestamp =
        later_status_search_bound(cursor_min_timestamp.clone(), parsed_query.after.clone());
    let search_terms = status_search_query_terms(&parsed_query);
    let max_id = if max_timestamp == cursor_max_timestamp {
        max_id
    } else {
        None
    };
    let min_id = if min_timestamp == cursor_min_timestamp {
        min_id
    } else {
        None
    };
    let mut entries = Vec::new();

    if !matches!(
        account_reference.as_ref(),
        Some(AccountReference::Remote(_))
    ) {
        let local_account_filter = match account_reference.as_ref() {
            Some(AccountReference::Local(account)) => Some(account.id.as_str()),
            _ => None,
        };
        for status in search_local_status_rows(
            db,
            &search_terms,
            query_limit,
            local_account_filter,
            max_id,
            max_timestamp.as_deref(),
            min_id,
            min_timestamp.as_deref(),
        )
        .await?
        {
            let Some(owner) = crate::find_account_by_id(db, &status.account_id).await? else {
                continue;
            };
            if excluded_account_reference
                .as_ref()
                .is_some_and(|reference| {
                    account_reference_matches_owner(reference, Some(&owner.id), None)
                })
            {
                continue;
            }
            if !can_view_local_status(db, &status, Some(viewer), &owner).await? {
                continue;
            }
            let is_public = status.visibility == "public";
            let is_library = local_status_is_in_search_library(db, config, viewer, &status).await?;
            if !status_is_searchable_by_scope(&parsed_query, is_public, is_library) {
                continue;
            }
            if !status_matches_search_syntax(
                &parsed_query,
                &status._text_content,
                &status.spoiler_text,
                status.in_reply_to_id.is_some(),
                status.sensitive != 0,
                status.boost_of_uri.is_some(),
                status.quote_of_uri.is_some(),
                status.language.as_deref(),
            ) {
                continue;
            }
            if !status_matches_search_timestamp(&parsed_query, &status.created_at) {
                continue;
            }
            let media = find_media_attachments_by_status_id(db, &status.id).await?;
            if !status_matches_search_metadata(
                &parsed_query,
                !media.is_empty(),
                find_status_poll_by_status_id(db, &status.id)
                    .await?
                    .is_some(),
                build_status_card_value(&status._text_content).is_some(),
            ) {
                continue;
            }
            let in_reply_to_account_id = load_in_reply_to_account_id(db, &status).await?;
            entries.push((
                (
                    status_search_rank(&parsed_query, &status._text_content, &status.spoiler_text),
                    Reverse(status.created_at.clone()),
                    Reverse(status.id.clone()),
                ),
                build_local_status_response(
                    db,
                    config,
                    Some(viewer),
                    &status,
                    &owner,
                    in_reply_to_account_id,
                    media,
                )
                .await?,
            ));
        }
    }

    if !matches!(account_reference.as_ref(), Some(AccountReference::Local(_))) {
        let remote_actor_filter = match account_reference.as_ref() {
            Some(AccountReference::Remote(actor)) => Some(actor.actor_uri.as_str()),
            _ => None,
        };
        for (status, actor) in search_remote_status_rows(
            db,
            &search_terms,
            query_limit,
            remote_actor_filter,
            max_id,
            max_timestamp.as_deref(),
            min_id,
            min_timestamp.as_deref(),
        )
        .await?
        {
            let is_public = is_public_activitypub_visibility(&status.visibility);
            if excluded_account_reference
                .as_ref()
                .is_some_and(|reference| {
                    account_reference_matches_owner(reference, None, Some(&status.actor_uri))
                })
            {
                continue;
            }
            let is_library =
                remote_status_is_in_search_library(db, config, viewer, &status).await?;
            if !status_is_searchable_by_scope(&parsed_query, is_public, is_library) {
                continue;
            }
            if !status_matches_search_syntax(
                &parsed_query,
                &strip_html_tags(&status.content_html),
                &status.spoiler_text,
                status.in_reply_to_uri.is_some(),
                status.sensitive != 0,
                status.boost_of_uri.is_some(),
                status.quote_of_uri.is_some(),
                status.language.as_deref(),
            ) {
                continue;
            }
            if !status_matches_search_timestamp(&parsed_query, &status.published_at) {
                continue;
            }
            let remote_attachments =
                find_remote_status_attachments_by_status_id(db, &status.id).await?;
            if !status_matches_search_metadata(
                &parsed_query,
                !remote_attachments.is_empty(),
                find_remote_status_poll_by_status_id(db, &status.id)
                    .await?
                    .is_some(),
                build_remote_status_card_value(
                    &strip_html_tags(&status.content_html),
                    &remote_attachments,
                )
                .is_some(),
            ) {
                continue;
            }
            entries.push((
                (
                    status_search_rank(
                        &parsed_query,
                        &strip_html_tags(&status.content_html),
                        &status.spoiler_text,
                    ),
                    Reverse(status.published_at.clone()),
                    Reverse(status.id.clone()),
                ),
                build_remote_status_response(db, config, Some(viewer), &status, &actor).await?,
            ));
        }
    }

    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(entries
        .into_iter()
        .skip(offset as usize)
        .map(|(_, value)| value)
        .take(limit as usize)
        .collect())
}
