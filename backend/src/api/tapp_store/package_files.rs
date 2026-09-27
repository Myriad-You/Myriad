//! HTTP error adapters for package filesystem services.
use crate::error::HttpError;
pub(crate) use crate::services::tapp_packages::package_files::*;
#[cfg(test)]
use axum::http::StatusCode;

#[cfg(test)]
pub(crate) fn tapp_filesystem_error_status(error: &std::io::Error) -> StatusCode {
    StatusCode::from_u16(
        crate::services::tapp_package_fs::filesystem_error_status_hint(error.kind()),
    )
    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}
pub(crate) fn unsupported_package_structure(reason: &str) -> HttpError {
    crate::services::tapp_packages::package_files::unsupported_package_structure(reason).into()
}
