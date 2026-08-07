//! Setup installation capability.
//!
//! Any setup endpoint that mutates installation state is a control-plane operation.
//! Network reachability, CONFIG_MODE, or an empty `users` table are state predicates;
//! none of them are authorization. While setup is unclaimed, callers therefore need
//! a local installation capability (`X-Bootstrap-Token`).
//!
//! The token is either supplied explicitly through `MYRIAD_BOOTSTRAP_TOKEN` or
//! generated at startup and written to `DATA_DIR/.bootstrap-token` with owner-only
//! permissions. It is never written to normal logs. Creating the first owner consumes
//! the in-process capability and removes the generated token file, so replay in the
//! same process is rejected. On restart, a claimed installation does not initialize a
//! new capability; recovery CONFIG_MODE deliberately initializes a fresh capability.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use axum::http::HeaderMap;
use myriad_error::AppError;
use rand::{distr::Alphanumeric, RngExt};
use subtle::ConstantTimeEq;

/// Request header carrying the installation capability.
pub const BOOTSTRAP_TOKEN_HEADER: &str = "x-bootstrap-token";

const TOKEN_FILE_NAME: &str = ".bootstrap-token";
const MIN_OPERATOR_TOKEN_LEN: usize = 32;

#[derive(Debug)]
struct BootstrapGuard {
    token: Option<String>,
    generated_token_file: Option<PathBuf>,
}

impl BootstrapGuard {
    fn new(token: String, generated_token_file: Option<PathBuf>) -> Self {
        Self {
            token: Some(token),
            generated_token_file,
        }
    }

    fn authorize(&self, headers: &HeaderMap) -> Result<(), AppError> {
        let Some(expected) = self.token.as_deref() else {
            return Err(bootstrap_token_required_error());
        };

        let provided = headers
            .get(BOOTSTRAP_TOKEN_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");

        if bootstrap_token_matches(expected, provided) {
            Ok(())
        } else {
            Err(bootstrap_token_required_error())
        }
    }

    fn consume(&mut self) -> Option<PathBuf> {
        self.token.take();
        self.generated_token_file.take()
    }
}

/// Process-local installation capability. Absence is *not* authorization:
/// setup mutation fails closed until startup explicitly initializes this guard.
static BOOTSTRAP_GUARD: OnceLock<Mutex<BootstrapGuard>> = OnceLock::new();

fn token_file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(TOKEN_FILE_NAME)
}

/// Generate a high-entropy 48-character alphanumeric capability.
fn generate_token() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

fn validate_operator_token(token: &str) -> io::Result<()> {
    if token.len() < MIN_OPERATOR_TOKEN_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "MYRIAD_BOOTSTRAP_TOKEN must be at least {MIN_OPERATOR_TOKEN_LEN} characters"
            ),
        ));
    }
    Ok(())
}

/// Persist a generated capability without a world/group-readable creation window.
fn persist_generated_token(path: &Path, token: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };

    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Existing files may have been created by an older version with broader
        // permissions. Tighten permissions before writing the new capability.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    file.write_all(token.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;

    Ok(())
}

fn remove_token_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Operator-facing startup notice. The capability itself must never be embedded.
pub(crate) fn bootstrap_required_notice(generated_token_file: Option<&Path>) -> String {
    match generated_token_file {
        Some(path) => format!(
            "🔐 Setup mutations require a local installation capability.\n\
             Generated token is written ONLY to: {path}\n\
             (not echoed in logs). Send it as the `{header}` header.",
            path = path.display(),
            header = BOOTSTRAP_TOKEN_HEADER,
        ),
        None => format!(
            "🔐 Setup mutations require the installation capability supplied through \
             MYRIAD_BOOTSTRAP_TOKEN (not echoed in logs). Send it as the `{header}` header.",
            header = BOOTSTRAP_TOKEN_HEADER,
        ),
    }
}

/// True if `notice` accidentally contains the live token (defensive test/assert).
pub(crate) fn notice_leaks_token(notice: &str, token: &str) -> bool {
    let token = token.trim();
    token.len() >= 8 && notice.contains(token)
}

/// Stable fail-closed response for missing, invalid, uninitialized, or consumed capability.
pub(crate) fn bootstrap_token_required_error() -> AppError {
    AppError::unauthorized("Bootstrap token required")
        .with_message(
            "Setup mutations require the local installation capability from DATA_DIR/.bootstrap-token or MYRIAD_BOOTSTRAP_TOKEN.",
        )
        .with_hint(format!("Send the `{BOOTSTRAP_TOKEN_HEADER}` header"))
}

/// Constant-time compare of provided header value against expected token.
pub(crate) fn bootstrap_token_matches(expected: &str, provided: &str) -> bool {
    let provided = provided.trim();
    provided.len() == expected.len() && provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

/// Initialize the installation capability before exposing any mutable setup route.
///
/// If an operator supplied `MYRIAD_BOOTSTRAP_TOKEN`, that secret remains the only
/// source of truth and any stale generated file is removed best-effort. Otherwise a
/// fresh token is generated and must be persisted securely; failure is fatal so the
/// service never falls back to unauthenticated setup.
pub fn init_for_setup(data_dir: &Path) -> io::Result<()> {
    if BOOTSTRAP_GUARD.get().is_some() {
        return Ok(());
    }

    let token_file = token_file_path(data_dir);
    let operator_token = std::env::var("MYRIAD_BOOTSTRAP_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let (token, generated_token_file) = if let Some(token) = operator_token {
        validate_operator_token(&token)?;
        if let Err(error) = remove_token_file(&token_file) {
            tracing::warn!(
                path = %token_file.display(),
                "Failed to remove stale generated bootstrap token file: {error}"
            );
        }
        (token, None)
    } else {
        // Setup commonly restarts once DATABASE_URL is saved. Reuse the same
        // generated capability across that restart so the operator does not have
        // to race a moving secret; it remains valid only until the first owner claim.
        let existing = std::fs::read_to_string(&token_file)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| validate_operator_token(value).is_ok());
        let token = existing.unwrap_or_else(generate_token);
        persist_generated_token(&token_file, &token)?;
        (token, Some(token_file))
    };

    let notice = bootstrap_required_notice(generated_token_file.as_deref());
    debug_assert!(
        !notice_leaks_token(&notice, &token),
        "bootstrap_required_notice must never embed the token"
    );

    let guard = BootstrapGuard::new(token, generated_token_file);
    if BOOTSTRAP_GUARD.set(Mutex::new(guard)).is_ok() {
        tracing::warn!("{notice}");
    }
    Ok(())
}

/// Remove a stale generated token when startup proves the installation is already claimed.
pub fn remove_stale_token_file(data_dir: &Path) {
    let path = token_file_path(data_dir);
    if let Err(error) = remove_token_file(&path) {
        tracing::warn!(
            path = %path.display(),
            "Failed to remove stale bootstrap token file: {error}"
        );
    }
}

/// Authorize a setup mutation. Uninitialized and consumed state fail closed.
pub fn require_bootstrap(headers: &HeaderMap) -> Result<(), AppError> {
    let Some(guard) = BOOTSTRAP_GUARD.get() else {
        tracing::warn!(
            "🚨 Setup request REJECTED: installation capability was not initialized"
        );
        return Err(bootstrap_token_required_error());
    };

    let guard = guard.lock().map_err(|_| {
        AppError::internal("Bootstrap capability state unavailable")
            .with_message("Setup capability state is unavailable; retry after restarting the service.")
    })?;

    if guard.authorize(headers).is_ok() {
        return Ok(());
    }

    tracing::warn!(
        "🚨 Setup request REJECTED: missing, invalid, or consumed {} header",
        BOOTSTRAP_TOKEN_HEADER
    );
    Err(bootstrap_token_required_error())
}

/// Consume the installation capability after the first owner transaction commits.
///
/// Memory is authoritative: file deletion failure is logged but cannot make the token
/// valid again in this process. A claimed installation will not reinitialize the guard
/// on restart, so a stale file is inert and removed during startup.
pub fn consume_bootstrap() {
    let Some(guard) = BOOTSTRAP_GUARD.get() else {
        return;
    };

    let token_file = match guard.lock() {
        Ok(mut guard) => guard.consume(),
        Err(_) => {
            tracing::error!(
                "Bootstrap capability state poisoned after owner creation; token file cleanup skipped"
            );
            return;
        }
    };

    if let Some(path) = token_file {
        if let Err(error) = remove_token_file(&path) {
            tracing::warn!(
                path = %path.display(),
                "Owner created and in-memory capability consumed, but token file cleanup failed: {error}"
            );
        }
    }
}

/// Validate a single `.env` value before writing it.
pub fn validate_env_value(key: &str, value: &str) -> Result<(), String> {
    if value.contains('\n') || value.contains('\r') {
        return Err(format!(
            "{key} contains a line break; that would inject additional .env entries"
        ));
    }
    if value.contains('\0') {
        return Err(format!("{key} contains a NUL byte"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_token(token: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(token) = token {
            headers.insert(BOOTSTRAP_TOKEN_HEADER, token.parse().expect("header value"));
        }
        headers
    }

    #[test]
    fn env_value_validation_blocks_crlf_injection() {
        assert!(validate_env_value("JWT_SECRET", "abc\nADMIN_OVERRIDE=1").is_err());
        assert!(validate_env_value("JWT_SECRET", "abc\r\nADMIN_OVERRIDE=1").is_err());
        assert!(validate_env_value("JWT_SECRET", "abc\rdef").is_err());
        assert!(validate_env_value("JWT_SECRET", "abc\0def").is_err());
        assert!(validate_env_value("JWT_SECRET", "a-perfectly-normal-secret").is_ok());
        assert!(validate_env_value("DATABASE_URL", "postgres://a:b==@h/d").is_ok());
    }

    #[test]
    fn generated_tokens_are_long_and_unique() {
        let a = generate_token();
        let b = generate_token();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn short_operator_capability_is_rejected() {
        assert!(validate_operator_token("short").is_err());
        assert!(validate_operator_token("0123456789abcdef0123456789abcdef").is_ok());
    }

    #[test]
    fn uninitialized_process_guard_fails_closed() {
        let error = require_bootstrap(&headers_with_token(Some(
            "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL",
        )))
        .expect_err("uninitialized guard must never authorize setup");
        assert_eq!(error.status_u16(), 401);
    }

    #[test]
    fn bootstrap_guard_rejects_missing_wrong_and_replay() {
        let expected = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        let mut guard = BootstrapGuard::new(expected.to_string(), None);

        let missing = guard
            .authorize(&headers_with_token(None))
            .expect_err("missing token must fail");
        assert_eq!(missing.status_u16(), 401);

        let wrong = guard
            .authorize(&headers_with_token(Some(
                "xbcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL",
            )))
            .expect_err("wrong token must fail");
        assert_eq!(wrong.status_u16(), 401);

        guard
            .authorize(&headers_with_token(Some(expected)))
            .expect("exact capability must pass");

        guard.consume();
        let replay = guard
            .authorize(&headers_with_token(Some(expected)))
            .expect_err("consumed capability must not replay");
        assert_eq!(replay.status_u16(), 401);
    }

    #[test]
    fn generated_token_file_is_private_and_removable() {
        let dir = std::env::temp_dir().join(format!("myriad-bootstrap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = token_file_path(&dir);
        let token = generate_token();

        persist_generated_token(&path, &token).expect("persist token");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read token"),
            format!("{token}\n")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        remove_token_file(&path).expect("remove token");
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bootstrap_required_notice_never_embeds_token() {
        let token = "SuperSecretBootstrapTokenValueABCDEF1234567890XYZ";
        let path = Path::new("/var/lib/myriad/.bootstrap-token");
        let generated_notice = bootstrap_required_notice(Some(path));
        assert!(!notice_leaks_token(&generated_notice, token));
        assert!(generated_notice.contains(path.to_str().unwrap()));
        assert!(generated_notice.contains(BOOTSTRAP_TOKEN_HEADER));
        assert!(generated_notice.contains("not echoed in logs"));

        let env_notice = bootstrap_required_notice(None);
        assert!(!notice_leaks_token(&env_notice, token));
        assert!(env_notice.contains("MYRIAD_BOOTSTRAP_TOKEN"));
        assert!(env_notice.contains(BOOTSTRAP_TOKEN_HEADER));
    }

    #[test]
    fn notice_leaks_token_detects_embedded_secret() {
        let token = "abcdefghijklmnop";
        assert!(notice_leaks_token(&format!("Token: {token}"), token));
        assert!(!notice_leaks_token("no secrets here", token));
        assert!(!notice_leaks_token("short", "ab"));
    }

    #[test]
    fn bootstrap_token_matches_accepts_exact_and_rejects_wrong() {
        let expected = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        assert_eq!(expected.len(), 48);
        assert!(bootstrap_token_matches(expected, expected));
        assert!(bootstrap_token_matches(expected, &format!("  {expected}  ")));
        let mut wrong = expected.as_bytes().to_vec();
        wrong[0] ^= 0x01;
        let wrong_s = String::from_utf8(wrong).expect("ascii");
        assert!(!bootstrap_token_matches(expected, &wrong_s));
        assert!(!bootstrap_token_matches(expected, ""));
        assert!(!bootstrap_token_matches(
            expected,
            &expected[..expected.len() - 1]
        ));
    }

    #[test]
    fn bootstrap_token_required_error_is_401_without_secret() {
        let error = bootstrap_token_required_error();
        assert_eq!(error.status_u16(), 401);
        assert_eq!(error.error_label(), "Bootstrap token required");
        let json = error.to_json();
        let rendered = json.to_string();
        assert!(!rendered.contains("Token:"), "{rendered}");
        assert!(
            json["message"]
                .as_str()
                .unwrap_or("")
                .contains("bootstrap-token")
                || json["message"]
                    .as_str()
                    .unwrap_or("")
                    .contains("MYRIAD_BOOTSTRAP_TOKEN"),
            "{json}"
        );
        assert!(
            json["hint"]
                .as_str()
                .unwrap_or("")
                .contains(BOOTSTRAP_TOKEN_HEADER),
            "{json}"
        );
    }

    #[tokio::test]
    async fn bootstrap_token_required_http_response_is_401_json() {
        use crate::error::HttpError;
        use axum::body::to_bytes;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        let response = HttpError(bootstrap_token_required_error()).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(value["error"], "Bootstrap token required");
    }
}
