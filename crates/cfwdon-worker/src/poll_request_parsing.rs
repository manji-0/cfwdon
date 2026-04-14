use super::{FormEntry, Request};
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
pub(crate) struct PollVoteRequest {
    pub(crate) choices: Vec<u32>,
}

pub(crate) async fn parse_poll_vote_request(
    req: &mut Request,
) -> std::result::Result<Vec<u32>, String> {
    let content_type = req
        .headers()
        .get("Content-Type")
        .map_err(|error| format!("failed to read Content-Type header: {error}"))?
        .unwrap_or_default()
        .to_ascii_lowercase();

    let choices = if content_type.contains("application/json") {
        req.json::<PollVoteRequest>()
            .await
            .map_err(|error| format!("invalid JSON poll vote payload: {error}"))?
            .choices
    } else {
        let form = req
            .form_data()
            .await
            .map_err(|error| format!("invalid form poll vote payload: {error}"))?;
        form.get_all("choices[]")
            .unwrap_or_default()
            .into_iter()
            .filter_map(|entry| match entry {
                FormEntry::Field(value) => value.trim().parse::<u32>().ok(),
                FormEntry::File(_) => None,
            })
            .collect()
    };

    if choices.is_empty() {
        return Err("choices must not be empty".to_owned());
    }
    let unique = choices.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != choices.len() {
        return Err("duplicate poll choices are not allowed".to_owned());
    }

    Ok(choices)
}
