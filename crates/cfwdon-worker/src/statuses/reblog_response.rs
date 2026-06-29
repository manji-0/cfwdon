use super::{
    AppConfig, LocalAccount, MastodonAccountResponse, MastodonStatusResponse, RemoteActorRow,
    RemoteStatusRow, StatusRow, actor_url,
};

pub(super) fn remote_reblog_wrapper_response_from_embedded(
    embedded: Option<MastodonStatusResponse>,
    wrapper_status: &RemoteStatusRow,
    wrapper_actor: &RemoteActorRow,
    config: &AppConfig,
) -> MastodonStatusResponse {
    let reblog = embedded_reblog_value(&embedded);
    let mut response = embedded.unwrap_or_else(|| {
        MastodonStatusResponse::from_remote_row(wrapper_status, wrapper_actor, config)
    });
    response.id = wrapper_status.id.clone();
    response.created_at = wrapper_status.published_at.clone();
    response.in_reply_to_id = wrapper_status.in_reply_to_uri.clone();
    response.in_reply_to_account_id = None;
    response.visibility = wrapper_status.visibility.as_str().to_owned();
    response.uri = wrapper_status.object_uri.clone();
    response.url = wrapper_status
        .url
        .clone()
        .unwrap_or_else(|| wrapper_status.object_uri.clone());
    response.account = MastodonAccountResponse::from_remote_actor(wrapper_actor);
    response.reblog = reblog;
    clear_reblog_wrapper_body(&mut response);
    response
}

pub(super) fn local_reblog_wrapper_response_from_embedded(
    embedded: Option<MastodonStatusResponse>,
    wrapper_status: &StatusRow,
    wrapper_account: &LocalAccount,
    in_reply_to_account_id: Option<String>,
    config: &AppConfig,
) -> MastodonStatusResponse {
    let reblog = embedded_reblog_value(&embedded);
    let mut response = embedded.unwrap_or_else(|| {
        MastodonStatusResponse::from_row(
            wrapper_status,
            wrapper_account,
            config,
            in_reply_to_account_id.clone(),
            Vec::new(),
        )
    });
    response.id = wrapper_status.id.clone();
    response.created_at = wrapper_status.created_at.clone();
    response.in_reply_to_id = wrapper_status.in_reply_to_id.clone();
    response.in_reply_to_account_id = in_reply_to_account_id;
    response.visibility = wrapper_status.visibility.as_str().to_owned();
    response.uri = wrapper_status.ap_id.clone().unwrap_or_else(|| {
        format!(
            "{}/statuses/{}",
            actor_url(config, wrapper_account.username()),
            wrapper_status.id
        )
    });
    response.url = response.uri.clone();
    response.account = MastodonAccountResponse::from_account(wrapper_account, config);
    response.reblog = reblog;
    clear_reblog_wrapper_body(&mut response);
    response
}

fn embedded_reblog_value(embedded: &Option<MastodonStatusResponse>) -> Option<serde_json::Value> {
    embedded
        .as_ref()
        .map(|status| serde_json::to_value(status).unwrap_or(serde_json::Value::Null))
}

fn clear_reblog_wrapper_body(response: &mut MastodonStatusResponse) {
    response.content.clear();
    response.text = None;
    response.media_attachments.clear();
    response.mentions.clear();
    response.tags.clear();
    response.emojis.clear();
    response.card = None;
    response.poll = None;
    response.quote = None;
}
