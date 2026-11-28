//! The rig API (`/rig-api/v1/*`, a tenant's API key) and the site admin's
//! management of it (`/api/admin/rig-api/*`, a site admin's session).
use super::*;

pub(crate) fn build_rig_api_router(
    app_state: crate::state::AppState,
) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    use axum::routing::{delete, put};
    let admin = || from_fn_with_state(app_state.clone(), middleware::auth::admin_middleware);
    let tenant = Router::<crate::state::AppState>::new()
        .route("/me", get(api::rig_api::me))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            middleware::rig_api_key::rig_api_key_auth,
        ));
    Router::<crate::state::AppState>::new()
        .nest(api::rig_api::PREFIX, tenant)
        .route(
            "/api/admin/rig-api/tenants",
            get(api::rig_api::admin::list_tenants)
                .post(api::rig_api::admin::create_tenant)
                .route_layer(admin()),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}",
            get(api::rig_api::admin::get_tenant)
                .patch(api::rig_api::admin::update_tenant)
                .route_layer(admin()),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}/limits",
            put(api::rig_api::admin::set_limits).route_layer(admin()),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}/keys",
            post(api::rig_api::admin::issue_key).route_layer(admin()),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}/keys/{key_id}",
            delete(api::rig_api::admin::revoke_key).route_layer(admin()),
        )
        .route(
            "/api/admin/rig-api/defaults",
            get(api::rig_api::admin::get_defaults)
                .put(api::rig_api::admin::set_defaults)
                .route_layer(admin()),
        )
}
