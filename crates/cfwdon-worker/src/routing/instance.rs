use crate::{
    create_domain_block_response, delete_domain_block_response, domain_blocks_preview_response,
    domain_blocks_response, instance_activity_response, instance_domain_blocks_response,
    instance_extended_description_response, instance_languages_response, instance_peers_response,
    instance_peers_search_response, instance_privacy_policy_response, instance_rules_response,
    instance_summary_response, instance_terms_of_service_response,
    instance_terms_of_service_version_response, instance_translation_languages_response,
    instance_v2_response,
};
use worker::Router;

pub(crate) fn add_instance_routes(router: Router<'static, ()>) -> Router<'static, ()> {
    router
        .get_async("/api/v1/instance", |_req, ctx| async move {
            instance_summary_response(ctx).await
        })
        .get_async("/api/v1/instance/peers", |_req, ctx| async move {
            instance_peers_response(ctx).await
        })
        .get_async("/api/v1/peers/search", |req, ctx| async move {
            instance_peers_search_response(req, ctx).await
        })
        .get_async("/api/v1/instance/activity", |_req, ctx| async move {
            instance_activity_response(ctx).await
        })
        .get_async("/api/v1/instance/rules", |_req, ctx| async move {
            instance_rules_response(ctx).await
        })
        .get_async("/api/v1/instance/domain_blocks", |_req, ctx| async move {
            instance_domain_blocks_response(ctx).await
        })
        .get_async("/api/v1/domain_blocks/preview", |req, ctx| async move {
            domain_blocks_preview_response(req, ctx).await
        })
        .get_async("/api/v1/domain_blocks", |req, ctx| async move {
            domain_blocks_response(req, ctx).await
        })
        .post_async("/api/v1/domain_blocks", |req, ctx| async move {
            create_domain_block_response(req, ctx).await
        })
        .delete_async("/api/v1/domain_blocks", |req, ctx| async move {
            delete_domain_block_response(req, ctx).await
        })
        .get_async(
            "/api/v1/instance/extended_description",
            |_req, ctx| async move { instance_extended_description_response(ctx).await },
        )
        .get_async("/api/v1/instance/privacy_policy", |_req, ctx| async move {
            instance_privacy_policy_response(ctx).await
        })
        .get_async(
            "/api/v1/instance/translation_languages",
            |_req, ctx| async move { instance_translation_languages_response(ctx).await },
        )
        .get_async(
            "/api/v1/instance/terms_of_service",
            |_req, ctx| async move { instance_terms_of_service_response(ctx).await },
        )
        .get_async(
            "/api/v1/instance/terms_of_service/:date",
            |_req, ctx| async move { instance_terms_of_service_version_response(ctx).await },
        )
        .get_async("/api/v1/instance/languages", |_req, ctx| async move {
            instance_languages_response(ctx).await
        })
        .get_async("/api/v2/instance", |_req, ctx| async move {
            instance_v2_response(ctx).await
        })
}
