//! Origins this instance has served media under. Absolute URLs carrying one
//! of them cite local media; any other origin is foreign.

pub async fn configured_origins() -> Vec<String> {
    let mut origins = vec![crate::oauth_url_builder::SiteConfig::get_base_url().await];
    let config = crate::GLOBAL_CONFIG.read().await;
    origins.extend(
        config
            .base_url
            .iter()
            .chain(config.frontend_url.iter())
            .cloned(),
    );
    origins
}
