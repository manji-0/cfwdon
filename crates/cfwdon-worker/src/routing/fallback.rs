use super::{
    accounts::add_account_routes,
    activitypub::{JSON_CONTENT_TYPE, add_activitypub_routes, static_head_response},
    alpha::add_alpha_routes,
    conversations::add_conversation_routes,
    filters::add_filter_routes,
    http::PLAIN_TEXT_CONTENT_TYPE,
    instance::add_instance_routes,
    lists::add_list_routes,
    media::add_media_routes,
    meta::add_meta_routes,
    notifications::add_notification_routes,
    oauth::add_oauth_routes,
    polls::add_poll_routes,
    push::add_push_routes,
    search::add_search_routes,
    statuses::add_status_routes,
    tags::add_tag_routes,
    timelines::add_timeline_routes,
};
use crate::root_document;
use worker::{Env, Request, Response, Result, Router};

pub(crate) async fn run_fallback_router(req: Request, env: Env) -> Result<Response> {
    let router = Router::new()
        .get("/", |_req, _ctx| Response::from_json(&root_document()))
        .head_async("/", |_req, _ctx| async move {
            static_head_response(JSON_CONTENT_TYPE)
        })
        .get("/healthz", |_req, _ctx| Response::ok("ok"))
        .head_async("/healthz", |_req, _ctx| async move {
            static_head_response(PLAIN_TEXT_CONTENT_TYPE)
        });
    let router = add_oauth_routes(router);
    let router = add_activitypub_routes(router);
    let router = add_alpha_routes(router);
    let router = add_instance_routes(router);
    let router = add_timeline_routes(router);
    let router = add_status_routes(router);
    let router = add_account_routes(router);
    let router = add_notification_routes(router);
    let router = add_media_routes(router);
    let router = add_filter_routes(router);
    let router = add_list_routes(router);
    let router = add_push_routes(router);
    let router = add_poll_routes(router);
    let router = add_tag_routes(router);
    let router = add_search_routes(router);
    let router = add_conversation_routes(router);
    let router = add_meta_routes(router);

    router.run(req, env).await
}
