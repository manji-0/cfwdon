use crate::auth::find_authenticated_local_account;
use crate::db_session::bind_request_d1;
use crate::runtime_config::load_config;
use crate::tracked_d1::D1Database;
use worker::{Request, Response, Result, RouteContext};

pub(crate) fn build_donation_campaign_document(
    config: &cfwdon_core::AppConfig,
) -> Option<serde_json::Value> {
    let value =
        serde_json::from_str::<serde_json::Value>(config.donation_campaign_json.as_deref()?)
            .ok()?;
    value.as_object()?;
    Some(value)
}

async fn is_authenticated_request(
    req: &Request,
    db: &D1Database,
    config: &cfwdon_core::AppConfig,
) -> Result<bool> {
    Ok(find_authenticated_local_account(req, db, config)
        .await?
        .is_some())
}

pub(crate) async fn donation_campaigns_response(
    req: Request,
    ctx: RouteContext<()>,
) -> Result<Response> {
    let config = load_config(&ctx);
    let db = bind_request_d1(&ctx, &config)?;
    if !is_authenticated_request(&req, &db, &config).await? {
        return Ok(Response::from_json(&serde_json::json!({
            "error": "This method requires an authenticated user",
        }))?
        .with_status(422));
    }
    let Some(document) = build_donation_campaign_document(&config) else {
        return Ok(Response::empty()?.with_status(204));
    };
    Ok(Response::from_json(&document)?.with_status(200))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfwdon_core::AppConfig;

    #[test]
    fn donation_campaign_document_uses_configured_upstream_shape() {
        let mut config = AppConfig::new("https://social.example", "cfwdon", "test instance");
        config.donation_campaign_json = Some(
            serde_json::json!({
                "id": "campaign-1",
                "banner_message": "Hi",
                "banner_button_text": "Donate!",
                "donation_message": "Hi!",
                "donation_button_text": "Money",
                "donation_success_post": "Success post",
                "amounts": {
                    "one_time": {
                        "EUR": [1, 2, 3],
                        "USD": [4, 5, 6],
                    },
                    "monthly": {
                        "EUR": [1],
                        "USD": [2],
                    },
                },
                "default_currency": "EUR",
                "donation_url": "https://sponsor.joinmastodon.org/donate/new",
                "locale": "en",
            })
            .to_string(),
        );

        let document = build_donation_campaign_document(&config).unwrap();

        assert_eq!(
            document.pointer("/id"),
            Some(&serde_json::json!("campaign-1"))
        );
        assert_eq!(
            document.pointer("/amounts/one_time/USD/2"),
            Some(&serde_json::json!(6))
        );
        assert_eq!(
            document.pointer("/donation_url"),
            Some(&serde_json::json!(
                "https://sponsor.joinmastodon.org/donate/new"
            ))
        );
    }
}
