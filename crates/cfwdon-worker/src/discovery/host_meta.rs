use crate::{
    AppConfig, CACHE_TTL_STATIC_METADATA, Response, Result, RouteContext,
    cache_public_json_response, cache_public_response, load_config, webfinger_lrdd_template,
};
use worker::ResponseBody;

pub(crate) async fn host_meta_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    host_meta_response_for_config(&config)
}

pub(crate) fn host_meta_response_from_env(env: &worker::Env) -> Result<Response> {
    let config = crate::load_config_from_env(env);
    host_meta_response_for_config(&config)
}

pub(crate) async fn host_meta_json_response(ctx: RouteContext<()>) -> Result<Response> {
    let config = load_config(&ctx);
    host_meta_json_response_for_config(&config)
}

pub(crate) fn host_meta_json_response_from_env(env: &worker::Env) -> Result<Response> {
    let config = crate::load_config_from_env(env);
    host_meta_json_response_for_config(&config)
}

fn host_meta_response_for_config(config: &AppConfig) -> Result<Response> {
    let template = webfinger_lrdd_template(config);
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <XRD xmlns=\"http://docs.oasis-open.org/ns/xri/xrd-1.0\">\n\
           <Link rel=\"lrdd\" template=\"{}\"/>\n\
         </XRD>\n",
        escape_xml_attr(&template)
    );
    let mut response = Response::from_body(ResponseBody::Body(body.into_bytes()))?;
    response
        .headers_mut()
        .set("Content-Type", "application/xrd+xml; charset=utf-8")?;
    // CORS is applied centrally via `is_cors_enabled_path("/.well-known/host-meta")`.
    cache_public_response(response, CACHE_TTL_STATIC_METADATA)
}

fn host_meta_json_response_for_config(config: &AppConfig) -> Result<Response> {
    // CORS is applied centrally via `is_cors_enabled_path("/.well-known/host-meta.json")`.
    cache_public_json_response(
        &host_meta_jrd_document(config),
        HOST_META_JRD_CONTENT_TYPE,
        CACHE_TTL_STATIC_METADATA,
        &[],
    )
}

const HOST_META_JRD_CONTENT_TYPE: &str = "application/jrd+json";

fn host_meta_jrd_document(config: &AppConfig) -> serde_json::Value {
    serde_json::json!({
        "links": [{
            "rel": "lrdd",
            "template": webfinger_lrdd_template(config),
        }]
    })
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_meta_body_includes_webfinger_lrdd_template() {
        let config = cfwdon_core::AppConfig::new("example.com", "cfwdon", "test instance");
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <XRD xmlns=\"http://docs.oasis-open.org/ns/xri/xrd-1.0\">\n\
               <Link rel=\"lrdd\" template=\"{}\"/>\n\
             </XRD>\n",
            escape_xml_attr(&crate::webfinger_lrdd_template(&config))
        );
        assert!(
            body.contains("template=\"https://example.com/.well-known/webfinger?resource={uri}\"")
        );
    }

    #[test]
    fn host_meta_jrd_document_matches_rfc6415_shape() {
        let config = cfwdon_core::AppConfig::new("example.com", "cfwdon", "test instance");
        let document = host_meta_jrd_document(&config);
        assert_eq!(HOST_META_JRD_CONTENT_TYPE, "application/jrd+json");
        assert_eq!(
            document,
            serde_json::json!({
                "links": [{
                    "rel": "lrdd",
                    "template": "https://example.com/.well-known/webfinger?resource={uri}",
                }]
            })
        );
        assert_eq!(
            document["links"][0]["template"],
            crate::webfinger_lrdd_template(&config)
        );
    }

    #[test]
    fn host_meta_jrd_serializes_with_json_escaping() {
        let config = cfwdon_core::AppConfig::new("example.com", "cfwdon", "test instance");
        let encoded = serde_json::to_string(&host_meta_jrd_document(&config)).unwrap();
        assert!(encoded.contains(r#""rel":"lrdd""#));
        assert!(
            encoded.contains(
                r#""template":"https://example.com/.well-known/webfinger?resource={uri}""#
            )
        );
        assert!(!encoded.contains("&quot;"));
        assert!(!encoded.contains("&amp;"));
    }
}
