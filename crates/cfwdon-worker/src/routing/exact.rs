use crate::{
    custom_emojis_response_direct, host_meta_json_response_from_env, host_meta_response_from_env,
    instance_domain_blocks_response_direct, instance_languages_response_from_env,
    instance_rules_response_direct, instance_summary_response_from_env,
    instance_v2_response_from_env, nodeinfo_21_response_from_env, nodeinfo_links_response_from_env,
    nodeinfo_response_from_env, oauth_authorization_server_response_from_env,
};
use worker::{Env, Response, Result};

pub(crate) async fn dispatch_exact_without_router(
    method: &str,
    path: &str,
    env: &Env,
) -> Result<Option<Response>> {
    let Some(kind) = exact_without_router_kind(method, path) else {
        return Ok(None);
    };

    match kind {
        ExactWithoutRouterKind::InstanceV1 => {
            instance_summary_response_from_env(env).await.map(Some)
        }
        ExactWithoutRouterKind::InstanceV2 => instance_v2_response_from_env(env).await.map(Some),
        ExactWithoutRouterKind::OauthAuthorizationServer => {
            oauth_authorization_server_response_from_env(env).map(Some)
        }
        ExactWithoutRouterKind::HostMeta => host_meta_response_from_env(env).map(Some),
        ExactWithoutRouterKind::HostMetaJson => host_meta_json_response_from_env(env).map(Some),
        ExactWithoutRouterKind::NodeinfoLinks => nodeinfo_links_response_from_env(env).map(Some),
        ExactWithoutRouterKind::Nodeinfo => nodeinfo_response_from_env(env).await.map(Some),
        ExactWithoutRouterKind::Nodeinfo21 => nodeinfo_21_response_from_env(env).await.map(Some),
        ExactWithoutRouterKind::InstanceRules => instance_rules_response_direct().map(Some),
        ExactWithoutRouterKind::InstanceDomainBlocks => {
            instance_domain_blocks_response_direct().map(Some)
        }
        ExactWithoutRouterKind::InstanceLanguages => {
            instance_languages_response_from_env(env).map(Some)
        }
        ExactWithoutRouterKind::CustomEmojis => custom_emojis_response_direct().map(Some),
    }
}

#[derive(Clone, Copy)]
enum ExactWithoutRouterKind {
    CustomEmojis,
    HostMeta,
    HostMetaJson,
    InstanceDomainBlocks,
    InstanceLanguages,
    InstanceRules,
    InstanceV1,
    InstanceV2,
    Nodeinfo,
    Nodeinfo21,
    NodeinfoLinks,
    OauthAuthorizationServer,
}

fn exact_without_router_kind(method: &str, path: &str) -> Option<ExactWithoutRouterKind> {
    if method != "GET" {
        return None;
    }

    match path {
        "/api/v1/instance" => Some(ExactWithoutRouterKind::InstanceV1),
        "/api/v2/instance" => Some(ExactWithoutRouterKind::InstanceV2),
        "/.well-known/oauth-authorization-server" => {
            Some(ExactWithoutRouterKind::OauthAuthorizationServer)
        }
        "/.well-known/host-meta" => Some(ExactWithoutRouterKind::HostMeta),
        "/.well-known/host-meta.json" => Some(ExactWithoutRouterKind::HostMetaJson),
        "/.well-known/nodeinfo" => Some(ExactWithoutRouterKind::NodeinfoLinks),
        "/nodeinfo/2.0" => Some(ExactWithoutRouterKind::Nodeinfo),
        "/nodeinfo/2.1" => Some(ExactWithoutRouterKind::Nodeinfo21),
        "/api/v1/instance/rules" => Some(ExactWithoutRouterKind::InstanceRules),
        "/api/v1/instance/domain_blocks" => Some(ExactWithoutRouterKind::InstanceDomainBlocks),
        "/api/v1/instance/languages" => Some(ExactWithoutRouterKind::InstanceLanguages),
        "/api/v1/custom_emojis" => Some(ExactWithoutRouterKind::CustomEmojis),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{ExactWithoutRouterKind, exact_without_router_kind};

    #[test]
    fn exact_without_router_only_handles_safe_get_routes() {
        assert!(matches!(
            exact_without_router_kind("GET", "/api/v1/instance"),
            Some(ExactWithoutRouterKind::InstanceV1)
        ));
        assert!(matches!(
            exact_without_router_kind("GET", "/.well-known/oauth-authorization-server"),
            Some(ExactWithoutRouterKind::OauthAuthorizationServer)
        ));
        assert!(matches!(
            exact_without_router_kind("GET", "/.well-known/host-meta"),
            Some(ExactWithoutRouterKind::HostMeta)
        ));
        assert!(matches!(
            exact_without_router_kind("GET", "/.well-known/host-meta.json"),
            Some(ExactWithoutRouterKind::HostMetaJson)
        ));
        assert!(exact_without_router_kind("POST", "/api/v1/instance").is_none());
        assert!(exact_without_router_kind("GET", "/api/v1/statuses/1").is_none());
    }
}
