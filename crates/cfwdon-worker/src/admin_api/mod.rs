use crate::{admin_ui_response, is_admin_ui_path};
use deliveries::{admin_deliveries_response, admin_retry_delivery_response};
use emojis::{
    admin_emojis_create_response, admin_emojis_delete_response, admin_emojis_list_response,
    admin_emojis_update_response,
};
use relays::{
    admin_relays_create_response, admin_relays_delete_response, admin_relays_disable_response,
    admin_relays_list_response,
};
use reports::{admin_reports_response, admin_resolve_report_response};
use session::{admin_me_response, is_admin_api_path};
use worker::{Env, Request, Response, Result, Router};

pub(crate) async fn dispatch_admin_route(
    req: Request,
    env: Env,
    method: &str,
    path: &str,
) -> Result<Response> {
    if is_admin_api_path(path) {
        return admin_api_router().run(req, env).await;
    }

    if method == "GET" && is_admin_ui_path(path) {
        return Router::new()
            .get_async("/admin", |req, ctx| async move {
                admin_ui_response(req, ctx).await
            })
            .get_async("/admin/", |req, ctx| async move {
                admin_ui_response(req, ctx).await
            })
            .get_async("/admin/*rest", |req, ctx| async move {
                admin_ui_response(req, ctx).await
            })
            .run(req, env)
            .await;
    }

    Response::error("Not Found", 404)
}

fn admin_api_router() -> Router<'static, ()> {
    Router::new()
        .get_async("/api/cfwdon/admin/me", |req, ctx| async move {
            admin_me_response(req, ctx).await
        })
        .get_async("/api/cfwdon/admin/reports", |req, ctx| async move {
            admin_reports_response(req, ctx).await
        })
        .post_async(
            "/api/cfwdon/admin/reports/:id/resolve",
            |req, ctx| async move { admin_resolve_report_response(req, ctx).await },
        )
        .get_async("/api/cfwdon/admin/deliveries", |req, ctx| async move {
            admin_deliveries_response(req, ctx).await
        })
        .post_async(
            "/api/cfwdon/admin/deliveries/:id/retry",
            |req, ctx| async move { admin_retry_delivery_response(req, ctx).await },
        )
        .get_async("/api/cfwdon/admin/emojis", |req, ctx| async move {
            admin_emojis_list_response(req, ctx).await
        })
        .post_async("/api/cfwdon/admin/emojis", |req, ctx| async move {
            admin_emojis_create_response(req, ctx).await
        })
        .patch_async("/api/cfwdon/admin/emojis/:id", |req, ctx| async move {
            admin_emojis_update_response(req, ctx).await
        })
        .delete_async("/api/cfwdon/admin/emojis/:id", |req, ctx| async move {
            admin_emojis_delete_response(req, ctx).await
        })
        .get_async("/api/cfwdon/admin/relays", |req, ctx| async move {
            admin_relays_list_response(req, ctx).await
        })
        .post_async("/api/cfwdon/admin/relays", |req, ctx| async move {
            admin_relays_create_response(req, ctx).await
        })
        .post_async(
            "/api/cfwdon/admin/relays/:id/disable",
            |req, ctx| async move { admin_relays_disable_response(req, ctx).await },
        )
        .delete_async(
            "/api/cfwdon/admin/relays/:id",
            |req, ctx| async move { admin_relays_delete_response(req, ctx).await },
        )
}

mod deliveries;
mod emojis;
mod guard;
mod relays;
mod reports;
mod session;
