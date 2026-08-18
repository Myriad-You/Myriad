use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use std::env;
use std::time::Duration;

/// Default startup connect attempts when `DATABASE_URL` is set.
/// With the 10s connect timeout, ~8 attempts + backoff covers ~30–90s of postgres cold-start.
const DEFAULT_MAX_ATTEMPTS: u32 = 8;
const DEFAULT_BASE_DELAY_MS: u64 = 2_000;
const DEFAULT_MAX_DELAY_MS: u64 = 8_000;

/// Tunable retry policy for startup (and optional hot-reload) DB connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectRetryConfig {
    pub max_attempts: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for ConnectRetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_delay: Duration::from_millis(DEFAULT_BASE_DELAY_MS),
            max_delay: Duration::from_millis(DEFAULT_MAX_DELAY_MS),
        }
    }
}

impl ConnectRetryConfig {
    /// Read override env vars (all optional):
    /// - `MYRIAD_DB_CONNECT_MAX_ATTEMPTS` (1–30, default 8)
    /// - `MYRIAD_DB_CONNECT_RETRY_BASE_MS` (default 2000)
    /// - `MYRIAD_DB_CONNECT_RETRY_MAX_MS` (default 8000)
    pub fn from_env() -> Self {
        let max_attempts = env::var("MYRIAD_DB_CONNECT_MAX_ATTEMPTS")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(DEFAULT_MAX_ATTEMPTS)
            .clamp(1, 30);

        let base_ms = env::var("MYRIAD_DB_CONNECT_RETRY_BASE_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_BASE_DELAY_MS)
            .clamp(100, 60_000);

        let max_ms = env::var("MYRIAD_DB_CONNECT_RETRY_MAX_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_DELAY_MS)
            .clamp(base_ms, 120_000);

        Self {
            max_attempts,
            base_delay: Duration::from_millis(base_ms),
            max_delay: Duration::from_millis(max_ms),
        }
    }

    /// Exponential backoff delay before the next attempt after a failed `attempt` (1-based).
    pub fn delay_before_retry(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        // attempt 1 → base, 2 → 2*base, 3 → 4*base, …
        let shift = attempt.saturating_sub(1).min(16);
        let mult = 1u64 << shift;
        let ms = self
            .base_delay
            .as_millis()
            .saturating_mul(mult as u128)
            .min(self.max_delay.as_millis());
        Duration::from_millis(ms as u64)
    }
}

/// Coarse classification for operator logs (best-effort from error text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbConnectErrorKind {
    Timeout,
    ConnectionRefused,
    Auth,
    Other,
}

impl DbConnectErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::ConnectionRefused => "connection_refused",
            Self::Auth => "auth",
            Self::Other => "other",
        }
    }
}

/// Classify a SeaORM connect/pool error without coupling to sqlx internals.
pub fn classify_connect_error(err: &DbErr) -> DbConnectErrorKind {
    classify_connect_error_message(&err.to_string())
}

/// Pure classifier over error text (unit-tested).
pub fn classify_connect_error_message(message: &str) -> DbConnectErrorKind {
    let s = message.to_ascii_lowercase();

    // Auth first: some auth failures mention "timeout" in unrelated wording.
    if s.contains("password authentication")
        || s.contains("authentication failed")
        || s.contains("auth failed")
        || s.contains("invalid password")
        || s.contains("no password supplied")
        || s.contains("password required")
        || (s.contains("role \"") && s.contains("does not exist"))
        // Postgres SQLSTATEs: 28P01 invalid_password, 28000 invalid_authorization_specification
        || s.contains("28p01")
        || s.contains("28000")
    {
        return DbConnectErrorKind::Auth;
    }

    if s.contains("connection refused")
        || s.contains("actively refused")
        || s.contains("econnrefused")
    {
        return DbConnectErrorKind::ConnectionRefused;
    }

    if s.contains("timed out")
        || s.contains("timeout")
        || s.contains("pool timed out")
        || s.contains("connection pool timed out")
    {
        return DbConnectErrorKind::Timeout;
    }

    DbConnectErrorKind::Other
}

/// Redact a database URL for logs: scheme, user, host, port, db name — never password.
///
/// Examples:
/// - `postgres://user:secret@db:5432/myriad` → `postgres://user:***@db:5432/myriad`
/// - `postgres://user@db:5432/myriad` → unchanged (no password)
/// - unparseable input → best-effort `user:pass@` scrub without echoing the secret
pub fn redact_database_url(database_url: &str) -> String {
    let trimmed = database_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(mut parsed) = url::Url::parse(trimmed) {
        if parsed.password().is_some() {
            // set_password(Some("***")) keeps userinfo shape operators expect
            let _ = parsed.set_password(Some("***"));
        }
        // Prefer no trailing slash for path-only DB names (Url often adds one).
        let mut out = parsed.to_string();
        if out.ends_with('/') && parsed.path() == "/" && parsed.query().is_none() {
            // empty path case: leave as-is
        } else if out.ends_with('/') && !trimmed.ends_with('/') {
            out.pop();
        }
        return out;
    }

    // Fallback when Url::parse fails (rare non-URL forms): scrub `scheme://user:pass@`
    redact_database_url_fallback(trimmed)
}

fn redact_database_url_fallback(input: &str) -> String {
    // Find scheme:// then userinfo before @, and mask password after first ':' in userinfo.
    let Some(scheme_end) = input.find("://") else {
        return "[unparseable-database-url]".to_string();
    };
    let after_scheme = &input[scheme_end + 3..];
    let Some(at) = after_scheme.find('@') else {
        // No credentials segment; still avoid dumping raw if it looks secret-ish
        return input.to_string();
    };
    let userinfo = &after_scheme[..at];
    let rest = &after_scheme[at..]; // includes '@'
    let scheme = &input[..scheme_end + 3];

    if let Some(colon) = userinfo.find(':') {
        let user = &userinfo[..colon];
        format!("{scheme}{user}:***{rest}")
    } else {
        input.to_string()
    }
}

pub async fn establish_connection(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opt = ConnectOptions::new(database_url.to_owned());

    // Pool size follows memory profile (default 2/24; saver 1/4). Applied at
    // connect only — changing profile later needs a reconnect/restart for pool.
    let min_c = crate::services::memory_profile::db_min_connections();
    let max_c = crate::services::memory_profile::db_max_connections().max(min_c);
    opt.max_connections(max_c)
        .min_connections(min_c)
        .connect_timeout(Duration::from_secs(10)) // 连接超时
        .acquire_timeout(Duration::from_secs(10)) // 获取连接超时
        .idle_timeout(Duration::from_secs(300)) // 空闲连接超时（5分钟）
        .max_lifetime(Duration::from_secs(3600)) // 连接最大存活时间（1小时）
        .sqlx_logging(false); // 关闭SQL日志以提升性能（开发时可设为true）

    let db = Database::connect(opt).await?;
    Ok(db)
}

/// Establish a connection with exponential backoff when `DATABASE_URL` is set but the DB is not
/// yet ready (common after compose/stack restart with an external Postgres).
///
/// Logs a redacted target once, then each failed attempt with a coarse error kind.
/// Does **not** change behavior when the caller never sets a URL (first-boot setup path).
pub async fn establish_connection_with_retry(
    database_url: &str,
) -> Result<DatabaseConnection, DbErr> {
    establish_connection_with_retry_config(database_url, ConnectRetryConfig::from_env()).await
}

pub async fn establish_connection_with_retry_config(
    database_url: &str,
    cfg: ConnectRetryConfig,
) -> Result<DatabaseConnection, DbErr> {
    let target = redact_database_url(database_url);
    tracing::info!(
        db_target = %target,
        max_attempts = cfg.max_attempts,
        "database connect: starting (with retry/backoff if needed)"
    );

    let mut last_err: Option<DbErr> = None;

    for attempt in 1..=cfg.max_attempts {
        match establish_connection(database_url).await {
            Ok(db) => {
                if attempt > 1 {
                    tracing::info!(
                        db_target = %target,
                        attempt,
                        "database connect: succeeded after retry"
                    );
                } else {
                    tracing::info!(db_target = %target, "database connect: succeeded");
                }
                return Ok(db);
            }
            Err(e) => {
                let kind = classify_connect_error(&e);
                let will_retry = attempt < cfg.max_attempts;
                if will_retry {
                    let delay = cfg.delay_before_retry(attempt);
                    tracing::warn!(
                        db_target = %target,
                        attempt,
                        max_attempts = cfg.max_attempts,
                        error_kind = kind.as_str(),
                        retry_in_ms = delay.as_millis() as u64,
                        error = %e,
                        "database connect: attempt failed; retrying"
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    tracing::error!(
                        db_target = %target,
                        attempt,
                        max_attempts = cfg.max_attempts,
                        error_kind = kind.as_str(),
                        error = %e,
                        "database connect: all attempts exhausted"
                    );
                }
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| DbErr::Custom("database connect failed".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_password_in_standard_postgres_url() {
        let raw = "postgres://myuser:s3cret-pass@db.internal:5432/myriad";
        let redacted = redact_database_url(raw);
        assert!(
            !redacted.contains("s3cret-pass"),
            "password leaked: {redacted}"
        );
        assert!(
            redacted.contains("myuser"),
            "user should remain: {redacted}"
        );
        assert!(redacted.contains("***"), "mask missing: {redacted}");
        assert!(
            redacted.contains("db.internal")
                && redacted.contains("5432")
                && redacted.contains("myriad"),
            "host/port/db missing: {redacted}"
        );
        assert!(
            redacted.starts_with("postgres://"),
            "scheme missing: {redacted}"
        );
    }

    #[test]
    fn redacts_password_with_special_chars() {
        let raw = "postgresql://u:p%40ss%2Fword@localhost:5432/app";
        let redacted = redact_database_url(raw);
        assert!(
            !redacted.to_ascii_lowercase().contains("p%40ss"),
            "{redacted}"
        );
        assert!(!redacted.contains("p@ss"), "{redacted}");
        assert!(redacted.contains("***"), "{redacted}");
        assert!(
            redacted.contains("u@") || redacted.contains("u:***@"),
            "{redacted}"
        );
    }

    #[test]
    fn leaves_url_without_password_intact() {
        let raw = "postgres://myuser@db:5432/myriad";
        let redacted = redact_database_url(raw);
        assert_eq!(redacted, "postgres://myuser@db:5432/myriad");
    }

    #[test]
    fn redacts_via_fallback_when_unparseable_as_url() {
        // Invalid host characters that still look like a DSN with user:pass@
        let raw = "postgres://admin:hunter2@[not-a-valid";
        let redacted = redact_database_url(raw);
        // Either full parse failure path or fallback — password must not appear.
        assert!(!redacted.contains("hunter2"), "password leaked: {redacted}");
    }

    #[test]
    fn empty_url_redacts_to_empty() {
        assert_eq!(redact_database_url(""), "");
        assert_eq!(redact_database_url("   "), "");
    }

    #[test]
    fn classifies_timeout() {
        assert_eq!(
            classify_connect_error_message(
                "Failed to acquire connection from pool: Connection pool timed out"
            ),
            DbConnectErrorKind::Timeout
        );
        assert_eq!(
            classify_connect_error_message("Connection Error: pool timed out while waiting"),
            DbConnectErrorKind::Timeout
        );
    }

    #[test]
    fn classifies_refused() {
        assert_eq!(
            classify_connect_error_message(
                "Connection Error: error connecting to server: Connection refused (os error 111)"
            ),
            DbConnectErrorKind::ConnectionRefused
        );
        assert_eq!(
            classify_connect_error_message("tcp connect error: ECONNREFUSED"),
            DbConnectErrorKind::ConnectionRefused
        );
    }

    #[test]
    fn classifies_auth() {
        assert_eq!(
            classify_connect_error_message("password authentication failed for user \"myriad\""),
            DbConnectErrorKind::Auth
        );
        assert_eq!(
            classify_connect_error_message("FATAL: 28P01: password authentication failed"),
            DbConnectErrorKind::Auth
        );
    }

    #[test]
    fn classifies_other() {
        assert_eq!(
            classify_connect_error_message("relation \"users\" does not exist"),
            DbConnectErrorKind::Other
        );
    }

    #[test]
    fn retry_delay_grows_then_caps() {
        let cfg = ConnectRetryConfig {
            max_attempts: 8,
            base_delay: Duration::from_millis(2_000),
            max_delay: Duration::from_millis(8_000),
        };
        assert_eq!(cfg.delay_before_retry(1), Duration::from_millis(2_000));
        assert_eq!(cfg.delay_before_retry(2), Duration::from_millis(4_000));
        assert_eq!(cfg.delay_before_retry(3), Duration::from_millis(8_000));
        assert_eq!(cfg.delay_before_retry(4), Duration::from_millis(8_000));
        assert_eq!(cfg.delay_before_retry(5), Duration::from_millis(8_000));
    }

    #[test]
    fn default_retry_config_is_sensible() {
        let cfg = ConnectRetryConfig::default();
        assert!(cfg.max_attempts >= 5 && cfg.max_attempts <= 10);
        assert!(cfg.base_delay.as_millis() >= 500);
        assert!(cfg.max_delay >= cfg.base_delay);
    }
}
