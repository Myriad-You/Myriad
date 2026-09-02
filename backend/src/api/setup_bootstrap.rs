//! Installation control plane.
//!
//! `CONFIG_MODE`, database reachability, and an empty administrator table are
//! state predicates, not authorization. The open setup window is the browser
//! wizard; it does not inspect the transport peer. Orchestration may set
//! `MYRIAD_SETUP_SECRET`; every setup mutation must match that passphrase when
//! it is present. The first owner claim closes the window; a durable claimed
//! marker prevents replay after restart.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use axum::http::HeaderMap;
use myriad_error::AppError;
use subtle::ConstantTimeEq;

/// 安装暗号环境变量。编排预置后，所有安装写操作必须对上。
const SETUP_SECRET_ENV: &str = "MYRIAD_SETUP_SECRET";

/// 安装暗号请求头。JSON 体里的 `setup_secret` 也可以。
const SETUP_SECRET_HEADER: &str = "x-setup-secret";

const CLAIMED_MARKER_FILE_NAME: &str = ".bootstrap-claimed";
const LEGACY_TOKEN_FILE_NAME: &str = ".bootstrap-token";
const LEGACY_ROTATION_MARKER_FILE_NAME: &str = ".bootstrap-rotating";

#[derive(Debug)]
struct InstallationWindow {
    open: bool,
    claimed_marker_file: PathBuf,
}

impl InstallationWindow {
    fn authorize(&self) -> Result<(), AppError> {
        if !self.open {
            return Err(setup_window_closed_error());
        }
        Ok(())
    }

    fn consume(&mut self) {
        self.open = false;
    }

    fn reopen(&mut self) {
        self.open = true;
    }
}

/// Absence is fail-closed. Startup must initialize this whenever setup is open.
static INSTALLATION_WINDOW: OnceLock<Mutex<InstallationWindow>> = OnceLock::new();

fn claimed_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CLAIMED_MARKER_FILE_NAME)
}

fn validate_private_file(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "setup marker path is not a regular file: {}",
                path.display()
            ),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("setup marker must be mode 0600: {}", path.display()),
            ));
        }
        let parent_uid = path
            .parent()
            .map(fs::metadata)
            .transpose()?
            .map(|parent| parent.uid());
        if parent_uid.is_some_and(|uid| metadata.uid() != uid) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "setup marker is not owned by the backend uid: {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(metadata)
}

fn remove_file_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn persist_claimed_marker(path: &Path) -> io::Result<()> {
    if path.exists() {
        validate_private_file(path)?;
        return Ok(());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(b"claimed\n")?;
    file.sync_all()?;
    validate_private_file(path)?;
    Ok(())
}

fn remove_legacy_token_files(data_dir: &Path) -> io::Result<()> {
    remove_file_if_present(&data_dir.join(LEGACY_TOKEN_FILE_NAME))?;
    remove_file_if_present(&data_dir.join(LEGACY_ROTATION_MARKER_FILE_NAME))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimedMarkerPlan {
    OpenFresh,
    RemoveStaleAndOpen,
    KeepClosed,
}

fn claimed_marker_plan(
    marker_present: bool,
    database_verified_unclaimed: bool,
) -> ClaimedMarkerPlan {
    match (marker_present, database_verified_unclaimed) {
        (false, _) => ClaimedMarkerPlan::OpenFresh,
        (true, true) => ClaimedMarkerPlan::RemoveStaleAndOpen,
        (true, false) => ClaimedMarkerPlan::KeepClosed,
    }
}

fn setup_window_closed_error() -> AppError {
    AppError::unauthorized("Setup window closed")
        .with_message("安装向导已关闭。认领之后请先修库，不要再用 setup 改宿主配置。")
}

pub(crate) fn secret_matches(expected: &str, provided: &str) -> bool {
    let provided = provided.trim();
    provided.len() == expected.len() && provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

/// Initialize the unclaimed setup window. Repeated calls in the same process
/// keep the original state.
pub fn init_for_setup(data_dir: &Path, database_verified_unclaimed: bool) -> io::Result<()> {
    if INSTALLATION_WINDOW.get().is_some() {
        return Ok(());
    }
    let claimed_marker_file = claimed_marker_path(data_dir);
    let marker_present = match fs::symlink_metadata(&claimed_marker_file) {
        Ok(_) => {
            validate_private_file(&claimed_marker_file)?;
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error),
    };
    match claimed_marker_plan(marker_present, database_verified_unclaimed) {
        ClaimedMarkerPlan::RemoveStaleAndOpen => {
            remove_file_if_present(&claimed_marker_file)?;
        }
        ClaimedMarkerPlan::KeepClosed => {
            remove_legacy_token_files(data_dir)?;
            let state = InstallationWindow {
                open: false,
                claimed_marker_file,
            };
            if INSTALLATION_WINDOW.set(Mutex::new(state)).is_ok() {
                tracing::warn!(
                    "Installation is durably claimed; setup window stays closed. Repair the database; do not reopen setup."
                );
            }
            return Ok(());
        }
        ClaimedMarkerPlan::OpenFresh => {}
    }
    remove_legacy_token_files(data_dir)?;

    let secret_note = if setup_secret_is_configured() {
        "Owner claim and other setup writes require MYRIAD_SETUP_SECRET."
    } else {
        "MYRIAD_SETUP_SECRET is unset; the first browser to finish the wizard becomes owner."
    };
    let state = InstallationWindow {
        open: true,
        claimed_marker_file,
    };
    if INSTALLATION_WINDOW.set(Mutex::new(state)).is_ok() {
        tracing::warn!("Installation setup window is open. {secret_note}");
    }
    Ok(())
}

/// Process window is open. Uninitialized is fail-closed (claimed, or not inited).
pub fn setup_window_is_open() -> bool {
    INSTALLATION_WINDOW
        .get()
        .and_then(|state| state.lock().ok().map(|window| window.open))
        .unwrap_or(false)
}

/// Check that the setup window is still open. Peer address is not a gate.
pub fn require_setup_window() -> Result<(), AppError> {
    let Some(state) = INSTALLATION_WINDOW.get() else {
        tracing::warn!("Setup request rejected: installation window is uninitialized");
        return Err(setup_window_closed_error());
    };
    let state = state.lock().map_err(|_| {
        AppError::internal("Setup window state unavailable")
            .with_message("Installation window state is poisoned; restart required.")
    })?;
    state.authorize()
}

/// Durably mark the installation claimed, then close the process window
/// immediately before the owner transaction commits. Marker failure leaves the
/// window open so the database transaction can be rolled back and retried.
pub fn consume_setup() -> io::Result<()> {
    let Some(state) = INSTALLATION_WINDOW.get() else {
        return Err(io::Error::other("installation window is uninitialized"));
    };
    let mut state = state
        .lock()
        .map_err(|_| io::Error::other("installation window mutex is poisoned"))?;
    persist_claimed_marker(&state.claimed_marker_file)?;
    remove_legacy_token_files(
        state
            .claimed_marker_file
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    )?;
    state.consume();
    Ok(())
}

/// Undo `consume_setup` after the owner transaction rolls back.
///
/// The marker is removed so a later restart does not treat this process as
/// claimed. The in-memory window reopens even if that unlink fails, so the
/// same process can retry without a restart.
pub fn reopen_setup_after_failed_claim() -> io::Result<()> {
    let Some(state) = INSTALLATION_WINDOW.get() else {
        return Err(io::Error::other("installation window is uninitialized"));
    };
    let mut state = state
        .lock()
        .map_err(|_| io::Error::other("installation window mutex is poisoned"))?;
    let remove_error = remove_file_if_present(&state.claimed_marker_file).err();
    state.reopen();
    match remove_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Emergency fail-closed transition for a path that has already obtained
/// durable owner proof but could not persist cleanup metadata.
pub fn invalidate_setup_in_memory() -> io::Result<()> {
    let Some(state) = INSTALLATION_WINDOW.get() else {
        return Err(io::Error::other("installation window is uninitialized"));
    };
    state
        .lock()
        .map_err(|_| io::Error::other("installation window mutex is poisoned"))?
        .consume();
    Ok(())
}

/// Claimed installations do not reopen setup on restart.
pub fn mark_claimed_on_disk(data_dir: &Path) -> io::Result<()> {
    persist_claimed_marker(&claimed_marker_path(data_dir))?;
    remove_legacy_token_files(data_dir)
}

fn unquote_env(value: &str) -> &str {
    let value = value.trim();
    let bytes = value.as_bytes();
    if value.len() >= 2
        && ((bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

/// 从 `.env` 一行或进程环境里取出可用的安装暗号。
fn setup_secret_from_env_value(raw: Option<&str>) -> Option<String> {
    raw.map(unquote_env)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn setup_secret_mismatch_error() -> AppError {
    AppError::unauthorized("Setup secret required")
        .with_message("安装暗号不对。请从服务器 .env 的 MYRIAD_SETUP_SECRET 复制后再试。")
        .with_hint(format!(
            "Send `{SETUP_SECRET_HEADER}` or JSON field `setup_secret`"
        ))
}

/// Whether the process has a usable installation passphrase.
pub(crate) fn setup_secret_is_configured() -> bool {
    setup_secret_from_env_value(std::env::var(SETUP_SECRET_ENV).ok().as_deref()).is_some()
}

/// 纯函数：只有编排预置了暗号时才校验；没配则放行（向导自己填库）。
fn check_setup_secret(expected: Option<&str>, provided: &str) -> Result<(), AppError> {
    let Some(expected) = setup_secret_from_env_value(expected) else {
        return Ok(());
    };
    if secret_matches(&expected, provided) {
        Ok(())
    } else {
        Err(setup_secret_mismatch_error())
    }
}

/// 安装写操作校验安装暗号。
///
/// 仅当进程环境里已有 `MYRIAD_SETUP_SECRET`（编排 / deploy 写入）才要求对上。
/// 向导自己填库时不会预置这枚值，不挡。HTTP 响应不回传正文。
pub(crate) fn require_setup_secret(
    headers: &HeaderMap,
    body_secret: Option<&str>,
) -> Result<(), AppError> {
    let expected = std::env::var(SETUP_SECRET_ENV).ok();
    let from_header = headers
        .get(SETUP_SECRET_HEADER)
        .and_then(|value| value.to_str().ok());
    let provided = first_nonempty_setup_secret(from_header, body_secret);
    check_setup_secret(expected.as_deref(), provided)
}

/// Header 优先；空字符串不当成已提供，避免盖住 JSON body。
fn first_nonempty_setup_secret<'a>(header: Option<&'a str>, body: Option<&'a str>) -> &'a str {
    header
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| body.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or("")
}

/// 校验单个 `.env` 值是否可以安全写入。
///
/// `.env` 是逐行 `KEY=VALUE` 的格式，值里出现 CR/LF 就能凭空造出新的一行，
/// 也就是注入任意环境变量。NUL 会截断多数解析器，一并拒绝。
#[allow(dead_code)] // 仅测试调用：本仓无生产调用点（编译器已核）。
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

    fn window(open: bool) -> InstallationWindow {
        InstallationWindow {
            open,
            claimed_marker_file: PathBuf::from(".bootstrap-claimed"),
        }
    }

    #[test]
    fn open_window_accepts_requests_until_closed() {
        assert!(window(true).authorize().is_ok());
        assert!(window(false).authorize().is_err());
        let mut consumed = window(true);
        consumed.consume();
        assert!(consumed.authorize().is_err());
        consumed.reopen();
        assert!(consumed.authorize().is_ok());
    }

    #[test]
    fn failed_claim_removes_marker_and_reopens_window() {
        let dir =
            std::env::temp_dir().join(format!("myriad-setup-reopen-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let marker = claimed_marker_path(&dir);
        persist_claimed_marker(&marker).unwrap();
        let mut consumed = InstallationWindow {
            open: false,
            claimed_marker_file: marker.clone(),
        };
        assert!(consumed.authorize().is_err());
        remove_file_if_present(&consumed.claimed_marker_file).unwrap();
        consumed.reopen();
        assert!(consumed.authorize().is_ok());
        assert!(!marker.exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn claimed_marker_without_database_proof_keeps_window_closed() {
        assert_eq!(
            claimed_marker_plan(false, false),
            ClaimedMarkerPlan::OpenFresh
        );
        assert_eq!(
            claimed_marker_plan(false, true),
            ClaimedMarkerPlan::OpenFresh
        );
        assert_eq!(
            claimed_marker_plan(true, true),
            ClaimedMarkerPlan::RemoveStaleAndOpen
        );
        assert_eq!(
            claimed_marker_plan(true, false),
            ClaimedMarkerPlan::KeepClosed
        );
    }

    #[test]
    fn claimed_marker_is_private_and_legacy_token_files_are_removed() {
        let dir = std::env::temp_dir().join(format!("myriad-setup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let token = dir.join(LEGACY_TOKEN_FILE_NAME);
        let rotating = dir.join(LEGACY_ROTATION_MARKER_FILE_NAME);
        fs::write(&token, "leftover\n").unwrap();
        fs::write(&rotating, "rotating\n").unwrap();
        mark_claimed_on_disk(&dir).unwrap();
        assert!(claimed_marker_path(&dir).exists());
        assert!(!token.exists());
        assert!(!rotating.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(claimed_marker_path(&dir))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn secret_matches_accepts_exact_and_rejects_wrong() {
        let expected = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        assert_eq!(expected.len(), 48);
        assert!(secret_matches(expected, expected));
        assert!(secret_matches(expected, &format!("  {expected}  ")));
        let mut wrong = expected.as_bytes().to_vec();
        wrong[0] ^= 0x01;
        let wrong_s = String::from_utf8(wrong).expect("ascii");
        assert!(!secret_matches(expected, &wrong_s));
        assert!(!secret_matches(expected, ""));
        assert!(!secret_matches(expected, &expected[..expected.len() - 1]));
    }

    #[test]
    fn setup_window_closed_http_response_is_401_json() {
        let error = setup_window_closed_error();
        assert_eq!(error.status_u16(), 401);
        let body = error.to_json();
        assert_eq!(body["error"], "Setup window closed");
        assert!(body["message"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
    }

    #[test]
    fn secret_compare_and_env_value_validation_are_strict() {
        let token = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKL";
        assert!(secret_matches(token, token));
        assert!(secret_matches(token, &format!("  {token}  ")));
        assert!(!secret_matches(token, ""));
        assert!(validate_env_value("JWT_SECRET", "abc\nADMIN_OVERRIDE=1").is_err());
        assert!(validate_env_value("JWT_SECRET", "abc\0def").is_err());
        assert!(validate_env_value("DATABASE_URL", "postgres://a:b==@h/d").is_ok());
    }

    #[tokio::test]
    async fn setup_window_closed_http_response_is_401_json_body() {
        use crate::error::HttpError;
        use axum::body::to_bytes;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;

        let resp = HttpError(setup_window_closed_error()).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let bytes = to_bytes(resp.into_body(), 64 * 1024).await.expect("body");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(v["error"], "Setup window closed");
    }

    #[test]
    fn setup_secret_from_env_value_strips_quotes_and_empties() {
        assert_eq!(
            setup_secret_from_env_value(Some("  abc123  ")).as_deref(),
            Some("abc123")
        );
        assert_eq!(
            setup_secret_from_env_value(Some("\"quoted-secret\"")).as_deref(),
            Some("quoted-secret")
        );
        assert_eq!(setup_secret_from_env_value(Some("")), None);
        assert_eq!(setup_secret_from_env_value(Some("   ")), None);
        assert_eq!(setup_secret_from_env_value(None), None);
    }

    #[test]
    fn check_setup_secret_skips_when_not_configured() {
        assert!(check_setup_secret(None, "anything").is_ok());
        assert!(check_setup_secret(Some(""), "anything").is_ok());
        assert!(check_setup_secret(Some("   "), "").is_ok());
    }

    #[test]
    fn check_setup_secret_requires_match_when_configured() {
        assert!(check_setup_secret(Some("correct-phrase"), "wrong").is_err());
        assert!(check_setup_secret(Some("correct-phrase"), "").is_err());
        assert!(check_setup_secret(Some("correct-phrase"), "correct-phrase").is_ok());
        assert!(check_setup_secret(Some("\"correct-phrase\""), "correct-phrase").is_ok());
    }

    #[test]
    fn first_nonempty_setup_secret_prefers_header_and_skips_blanks() {
        assert_eq!(
            first_nonempty_setup_secret(Some(" header "), Some("body")),
            "header"
        );
        assert_eq!(
            first_nonempty_setup_secret(Some("   "), Some(" body ")),
            "body"
        );
        assert_eq!(first_nonempty_setup_secret(Some(""), Some("body")), "body");
        assert_eq!(first_nonempty_setup_secret(None, Some("body")), "body");
        assert_eq!(first_nonempty_setup_secret(Some(""), Some("")), "");
        assert_eq!(first_nonempty_setup_secret(None, None), "");
    }
}
