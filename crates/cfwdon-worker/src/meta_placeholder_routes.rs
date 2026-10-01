use std::collections::BTreeMap;
use worker::{Response, Result};

mod account_registration;
mod annual_reports;
mod donation_campaigns;
mod email_confirmation;
mod follower_removal;
mod listings;
mod oauth;
mod oembed;

pub(crate) mod streaming;

pub(crate) use account_registration::create_account_placeholder_response;
pub(crate) use annual_reports::{
    annual_report_action_response, annual_report_response, annual_report_state_response,
    annual_reports_response,
};
pub(crate) use donation_campaigns::donation_campaigns_response;
pub(crate) use email_confirmation::{
    check_email_confirmation_response, create_email_confirmation_response,
    email_confirmation_page_response,
};
pub(crate) use follower_removal::remove_from_followers_response;
pub(crate) use listings::{accounts_index_response, statuses_index_placeholder_response};
pub(crate) use oauth::{
    app_verify_credentials_response, oauth_authorization_server_response,
    oauth_authorization_server_response_from_env, oauth_userinfo_response,
};
pub(crate) use oembed::oembed_response;
pub(crate) use streaming::streaming_placeholder_response;

#[cfg(test)]
pub(crate) use donation_campaigns::build_donation_campaign_document;
#[cfg(test)]
pub(crate) use oauth::{
    build_app_verify_credentials_document, build_oauth_authorization_server_document,
    build_oauth_userinfo_document,
};

fn invalid_access_token_response() -> Result<Response> {
    let mut response = Response::from_json(&serde_json::json!({
        "error": "The access token is invalid",
    }))?
    .with_status(401);
    response
        .headers_mut()
        .set("WWW-Authenticate", r#"Bearer error="invalid_token""#)?;
    Ok(response)
}

fn outside_authorized_scopes_response() -> Result<Response> {
    Ok(Response::from_json(&serde_json::json!({
        "error": "This action is outside the authorized scopes",
    }))?
    .with_status(403))
}

fn validation_failed_response(details: BTreeMap<&'static str, Vec<String>>) -> Result<Response> {
    let mut messages = Vec::new();
    for (field, field_errors) in &details {
        let label = match *field {
            "username" => "Username",
            "email" => "Email",
            "password" => "Password",
            "agreement" => "Agreement",
            _ => field,
        };
        for error in field_errors {
            messages.push(format!("{label} {error}"));
        }
    }
    Ok(Response::from_json(&serde_json::json!({
        "error": format!("Validation failed: {}", messages.join(", ")),
        "details": details,
    }))?
    .with_status(422))
}
