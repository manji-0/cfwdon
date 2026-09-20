use serde::Deserialize;
use std::collections::HashMap;

mod documents;
mod matcher;
mod queries;
mod requests;
mod routes;

pub(in crate::filters) use documents::{
    filter_summary_document, keyword_document, split_filter_context, status_filter_document,
    v1_filter_document, v2_filter_document, v2_filter_document_from_parts,
};
pub(crate) use matcher::{load_account_filter_matcher, load_status_filtered};
pub(crate) use queries::load_latest_filter_updated_at;
pub(in crate::filters) use queries::{
    create_filter_keyword_row, create_filter_row, create_filter_status_row,
    delete_filter_keyword_row, delete_filter_row, delete_filter_status_row, find_filter,
    find_filter_keyword, find_filter_status, find_v1_filter, list_filter_keywords,
    list_filter_keywords_for_filters, list_filter_statuses, list_filter_statuses_for_filters,
    list_filters, list_v1_filters, replace_filter_keywords, update_filter_keyword_row,
    update_filter_row,
};
pub(in crate::filters) use requests::{
    expires_at_from_seconds, normalize_contexts, normalize_filter_action, normalize_keyword,
    parse_keyword_request, parse_status_filter_request, parse_v1_filter_request,
    parse_v2_filter_request,
};
pub(crate) use routes::{
    create_filter_keyword_response, create_filter_status_response, create_filter_v1_response,
    create_filter_v2_response, delete_filter_keyword_response, delete_filter_status_response,
    delete_filter_v1_response, delete_filter_v2_response, filter_keyword_response,
    filter_keywords_response, filter_status_response, filter_statuses_response, filter_v1_response,
    filter_v2_response, filters_v1_response, filters_v2_response, update_filter_keyword_response,
    update_filter_v1_response, update_filter_v2_response,
};

#[derive(Debug, Deserialize)]
struct FilterRow {
    id: String,
    title: String,
    context_csv: String,
    expires_at: Option<String>,
    filter_action: String,
}

#[derive(Debug, Deserialize)]
struct FilterKeywordRow {
    id: String,
    filter_id: String,
    keyword: String,
    whole_word: i32,
}

#[derive(Debug, Deserialize)]
struct FilterStatusRow {
    id: String,
    #[serde(default)]
    filter_id: String,
    status_id: String,
}

#[derive(Debug, Deserialize)]
struct V1FilterRow {
    id: String,
    phrase: String,
    context_csv: String,
    expires_at: Option<String>,
    filter_action: String,
    whole_word: i32,
}

#[derive(Debug, Default)]
pub(crate) struct AccountFilterMatcher {
    filters: Vec<FilterRow>,
    keywords_by_filter_id: HashMap<String, Vec<FilterKeywordRow>>,
    statuses_by_filter_id: HashMap<String, Vec<FilterStatusRow>>,
}

#[derive(Debug, Default, Deserialize)]
struct KeywordInput {
    keyword: Option<String>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct V1FilterRequest {
    phrase: Option<String>,
    context: Option<Vec<String>>,
    expires_in: Option<i64>,
    irreversible: Option<bool>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct V2FilterRequest {
    title: Option<String>,
    context: Option<Vec<String>>,
    expires_in: Option<i64>,
    filter_action: Option<String>,
    #[serde(alias = "keywords_attributes")]
    keywords: Option<Vec<KeywordInput>>,
    phrase: Option<String>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct KeywordRequest {
    keyword: Option<String>,
    whole_word: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct StatusFilterRequest {
    status_id: Option<String>,
}
