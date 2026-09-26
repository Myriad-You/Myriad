//! The site's public origin as it outlives a container: BASE_URL,
//! FRONTEND_URL and CORS_ORIGINS saved under DATA_DIR, loaded at startup by
//! every process after its own dotenv.

use std::path::Path;

/// Load durable site public origin overrides (DATA_DIR) after process dotenv.
/// Call once at startup so Docker volume outlives compose-injected CORS/BASE_URL.
pub fn load_durable_site_public_env() {
    if let Ok(data) = std::env::var("DATA_DIR") {
        let path = Path::new(&data).join("site_public.env");
        if path.is_file() {
            match dotenvy::from_path_override(&path) {
                Ok(_) => {
                    tracing::info!(
                        path = %path.display(),
                        "♻️ Loaded durable site public env (BASE_URL / FRONTEND_URL / CORS_ORIGINS)"
                    );
                    if let Ok(cors) = std::env::var("CORS_ORIGINS") {
                        crate::middleware::cors_runtime::set_cors_origins_csv(&cors);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to load durable site_public.env"
                    );
                }
            }
        }
    }
}
