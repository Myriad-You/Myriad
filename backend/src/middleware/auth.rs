use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::{DecodingKey, Validation, decode};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex as AsyncMutex, watch};
use uuid::Uuid;

/// Browser / API session lifetime (days). Keep long-lived; revoke via `token_version`.
pub const JWT_TTL_DAYS: i64 = 30;
/// `auth_token` cookie Max-Age in seconds (matches JWT TTL).
pub const AUTH_COOKIE_MAX_AGE_SECS: i64 = JWT_TTL_DAYS * 24 * 60 * 60;

/// JWT Claims structure (must match auth issuance sites).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,      // User ID
    pub username: String, // Username
    pub is_admin: bool,   // Admin status
    /// Durable site owner (`users.is_owner`). Claim omitted → serde default false.
    #[serde(default)]
    pub is_owner: bool,
    pub exp: i64, // Expiration time
    pub iat: i64, // Issued at
    /// Session epoch (`users.token_version`). Claim omitted → serde default 0.
    #[serde(default)]
    pub tv: i64,
}

/// Current durable subject on routes where credentials are optional.
///
/// `None` means no supported credential was supplied. A presented credential
/// never becomes `None` because [`optional_current_auth_middleware`] rejects it
/// unless it passes the same current-state resolver as required auth.
#[derive(Debug, Clone)]
pub struct OptionalClaims(pub Option<Claims>);

/// Build claims for a durable user session (login / register / password reissue).
pub fn mint_session_claims(
    user_id: i32,
    username: impl Into<String>,
    is_admin: bool,
    is_owner: bool,
    token_version: i64,
) -> Claims {
    let now = chrono::Utc::now().timestamp();
    Claims {
        sub: user_id.to_string(),
        username: username.into(),
        is_admin,
        is_owner,
        exp: now + JWT_TTL_DAYS * 24 * 60 * 60,
        iat: now,
        tv: token_version,
    }
}

/// Encode claims with `JWT_SECRET`. Caller must set cookie / Authorization.
pub fn encode_session_token(claims: &Claims) -> Result<String, String> {
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| "JWT_SECRET not configured".to_string())?;
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        claims,
        &jsonwebtoken::EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| format!("JWT encode failed: {e}"))
}

/// `auth_token=…` Set-Cookie value for a newly issued session.
pub fn auth_cookie_value(token: &str, is_production: bool) -> String {
    format!(
        "auth_token={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={AUTH_COOKIE_MAX_AGE_SECS}{}",
        if is_production { "; Secure" } else { "" }
    )
}

/// `auth_token=…` Set-Cookie value for logout and invalid browser sessions.
/// Attributes must match issuance so an HttpOnly cookie can be removed by the
/// server; frontend JavaScript cannot delete it.
pub fn clear_auth_cookie_value(is_production: bool) -> String {
    format!(
        "auth_token=deleted; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT{}",
        if is_production { "; Secure" } else { "" }
    )
}

/// PostgreSQL channel used to fan out auth-state invalidations between service
/// instances.  NOTIFY is only a latency optimisation: the cache TTL below is
/// the correctness bound when a notification is missed.
const AUTH_CACHE_INVALIDATION_CHANNEL: &str = "myriad_auth_cache_invalidate";
/// Ordinary authenticated requests may use a cached auth snapshot for at most
/// five seconds.  Admin gates still perform an explicit live role check.
pub const AUTH_CACHE_TTL: Duration = Duration::from_secs(5);
/// Bound process memory even when a site sees an unbounded stream of user ids.
pub const AUTH_CACHE_CAPACITY: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AuthSnapshot {
    token_version: i64,
    is_admin: bool,
    is_owner: bool,
}

#[derive(Debug, Clone, Copy)]
struct AuthCacheEntry {
    snapshot: Option<AuthSnapshot>,
    inserted_at: Instant,
    last_access: Instant,
}

#[derive(Debug, Default)]
struct AuthCache {
    entries: HashMap<i32, AuthCacheEntry>,
    /// Monotonic process-local invalidation fence. A global fence keeps the
    /// metadata bounded; unrelated invalidations may conservatively skip one
    /// cache fill but can never resurrect a stale authorization snapshot.
    generation: u64,
}

static AUTH_CACHE: OnceLock<StdMutex<AuthCache>> = OnceLock::new();
static AUTH_CACHE_INFLIGHT: OnceLock<StdMutex<HashMap<i32, Arc<watch::Sender<()>>>>> =
    OnceLock::new();
static AUTH_CACHE_LISTENER_STARTED: OnceLock<AsyncMutex<bool>> = OnceLock::new();

struct AuthLoadOwner {
    user_id: i32,
    sender: Arc<watch::Sender<()>>,
}

impl Drop for AuthLoadOwner {
    fn drop(&mut self) {
        let sender = {
            let mut in_flight = auth_cache_inflight()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let owns_slot = in_flight
                .get(&self.user_id)
                .is_some_and(|current| Arc::ptr_eq(current, &self.sender));
            owns_slot.then(|| in_flight.remove(&self.user_id)).flatten()
        };
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
    }
}

enum AuthLoadSlot {
    Owner(AuthLoadOwner),
    Waiter(watch::Receiver<()>),
}

fn auth_cache() -> &'static StdMutex<AuthCache> {
    AUTH_CACHE.get_or_init(|| StdMutex::new(AuthCache::default()))
}

fn auth_cache_inflight() -> &'static StdMutex<HashMap<i32, Arc<watch::Sender<()>>>> {
    AUTH_CACHE_INFLIGHT.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn auth_cache_listener_started() -> &'static AsyncMutex<bool> {
    AUTH_CACHE_LISTENER_STARTED.get_or_init(|| AsyncMutex::new(false))
}

/// Read a cache entry, returning `Some(None)` for a cached missing account and
/// `None` for a miss/expired entry.  Keeping negative entries avoids repeatedly
/// probing deleted ids while still respecting the same short TTL.
fn auth_cache_get(user_id: i32) -> Option<Option<AuthSnapshot>> {
    let mut guard = match auth_cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let now = Instant::now();
    let expired = guard
        .entries
        .get(&user_id)
        .map(|entry| now.duration_since(entry.inserted_at) >= AUTH_CACHE_TTL)?;
    if expired {
        guard.entries.remove(&user_id);
        return None;
    }
    let entry = guard.entries.get_mut(&user_id)?;
    entry.last_access = now;
    Some(entry.snapshot)
}

#[cfg(test)]
fn auth_cache_put(user_id: i32, snapshot: Option<AuthSnapshot>) {
    let mut guard = match auth_cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    insert_auth_cache_entry(&mut guard, user_id, snapshot);
}

fn insert_auth_cache_entry(guard: &mut AuthCache, user_id: i32, snapshot: Option<AuthSnapshot>) {
    let now = Instant::now();
    if !guard.entries.contains_key(&user_id) && guard.entries.len() >= AUTH_CACHE_CAPACITY {
        if let Some((&oldest_id, _)) = guard
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.last_access)
        {
            guard.entries.remove(&oldest_id);
        }
    }
    guard.entries.insert(
        user_id,
        AuthCacheEntry {
            snapshot,
            inserted_at: now,
            last_access: now,
        },
    );
}

fn auth_cache_generation() -> u64 {
    let guard = auth_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.generation
}

/// Fill a miss only if no local or cross-instance invalidation happened since
/// the database load began. The generation comparison and insert share the
/// same lock as invalidation, closing the check-then-put race.
fn auth_cache_put_if_generation(
    user_id: i32,
    snapshot: Option<AuthSnapshot>,
    expected_generation: u64,
) -> bool {
    let mut guard = auth_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.generation != expected_generation {
        return false;
    }
    insert_auth_cache_entry(&mut guard, user_id, snapshot);
    true
}

fn claim_auth_load_slot(user_id: i32) -> AuthLoadSlot {
    let mut in_flight = auth_cache_inflight()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(sender) = in_flight.get(&user_id) {
        return AuthLoadSlot::Waiter(sender.subscribe());
    }

    let (sender, _receiver) = watch::channel(());
    let sender = Arc::new(sender);
    in_flight.insert(user_id, Arc::clone(&sender));
    AuthLoadSlot::Owner(AuthLoadOwner { user_id, sender })
}

/// Drop one user's snapshot in this process.  This is intentionally separate
/// from [`notify_auth_cache_invalidation`] so failed NOTIFY does not leave a
/// stale local authorization decision behind.
pub fn invalidate_auth_cache_local(user_id: i32) {
    if user_id <= 0 {
        return;
    }
    let mut guard = match auth_cache().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.generation = guard.generation.wrapping_add(1);
    guard.entries.remove(&user_id);
}

/// Invalidate this process and publish the same invalidation to peer instances.
/// Call after a committed role/session/account write.  A missed notification is
/// safe because ordinary auth entries are short-lived.
pub async fn notify_auth_cache_invalidation(
    db: &impl ConnectionTrait,
    user_id: i32,
) -> Result<(), sea_orm::DbErr> {
    invalidate_auth_cache_local(user_id);
    if user_id <= 0 {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_notify($1, $2)",
        [
            AUTH_CACHE_INVALIDATION_CHANNEL.into(),
            user_id.to_string().into(),
        ],
    ))
    .await
    .map(|_| ())
}

/// Start one process-wide LISTEN task lazily, using the app's existing pool.
/// A missed notification is safe because every entry expires after
/// [`AUTH_CACHE_TTL`].
async fn ensure_auth_cache_listener(db: &DatabaseConnection) {
    if !matches!(db.get_database_backend(), DatabaseBackend::Postgres) {
        return;
    }

    let mut started = auth_cache_listener_started().lock().await;
    if *started {
        return;
    }
    *started = true;

    let pool = db.get_postgres_connection_pool().clone();
    tokio::spawn(async move {
        loop {
            match sea_orm::sqlx::postgres::PgListener::connect_with(&pool).await {
                Ok(mut listener) => match listener.listen(AUTH_CACHE_INVALIDATION_CHANNEL).await {
                    Err(error) => {
                        tracing::warn!(error = %error, "auth cache LISTEN setup failed");
                    }
                    _ => {
                        tracing::debug!(
                            channel = AUTH_CACHE_INVALIDATION_CHANNEL,
                            "auth cache invalidation listener started"
                        );
                        loop {
                            match listener.recv().await {
                                Ok(notification) => {
                                    if let Ok(user_id) = notification.payload().parse::<i32>() {
                                        invalidate_auth_cache_local(user_id);
                                    }
                                }
                                Err(error) => {
                                    tracing::warn!(error = %error, "auth cache LISTEN connection lost");
                                    break;
                                }
                            }
                        }
                    }
                },
                Err(error) => {
                    tracing::debug!(error = %error, "auth cache listener connection unavailable");
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

/// Authentication middleware - verifies JWT token + session epoch
/// Returns 401 if token is missing, invalid, or revoked
pub async fn auth_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();

    match authenticate_request(headers, &db).await {
        Ok(claims) => {
            record_user_presence(&claims, db);
            // Token is valid, inject claims into request extensions
            let mut req = req;
            req.extensions_mut().insert(claims);
            next.run(req).await
        }
        Err(error_response) => *error_response,
    }
}

/// Optional durable authentication for public routes.
///
/// Absence is anonymous. A presented JWT must pass cryptographic validation,
/// current user existence, session epoch and current-role resolution.
pub async fn optional_current_auth_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    let credential_source = auth_credential_source(req.headers());
    match authenticate_optional_request(req.headers(), &db).await {
        Ok(claims) => {
            if let Some(claims) = claims.as_ref() {
                record_user_presence(claims, db);
            }
            let mut req = req;
            req.extensions_mut().insert(OptionalClaims(claims));
            next.run(req).await
        }
        Err(error_response) => {
            clear_invalid_cookie_response(*error_response, credential_source).await
        }
    }
}

/// How a public route treats a presented credential that fails verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvalidCredential {
    /// 401, same as required auth (clears a stale cookie).
    Reject,
    /// Continue as anonymous. Only a 401 downgrades; server errors still fail.
    Anonymous,
}

/// Optional auth plus the current-admin proof, for routes whose handlers need
/// both "who is this" and "is this still an admin".
///
/// Absence is anonymous; a presented credential must pass the same resolver as
/// [`auth_middleware`] and is otherwise rejected. A valid subject lands in the
/// extensions as `Claims` and [`OptionalClaims`]; [`CurrentAdminVerified`] is
/// added only after [`ensure_current_admin_on`] succeeds.
pub async fn optional_current_admin_auth_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    optional_admin_auth(db, req, next, InvalidCredential::Reject).await
}

/// Same as [`optional_current_admin_auth_middleware`], except an invalid,
/// expired or revoked credential continues as anonymous instead of 401. For
/// routes that have always served cached content regardless of credentials.
pub async fn lenient_current_admin_auth_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    optional_admin_auth(db, req, next, InvalidCredential::Anonymous).await
}

async fn optional_admin_auth(
    db: DatabaseConnection,
    mut req: Request,
    next: Next,
    invalid: InvalidCredential,
) -> Response {
    let credential_source = auth_credential_source(req.headers());
    let claims = match authenticate_optional_request(req.headers(), &db).await {
        Ok(claims) => claims,
        Err(error_response)
            if invalid == InvalidCredential::Anonymous
                && error_response.status() == StatusCode::UNAUTHORIZED =>
        {
            None
        }
        Err(error_response) => {
            return clear_invalid_cookie_response(*error_response, credential_source).await;
        }
    };
    if let Some(claims) = claims.as_ref() {
        record_user_presence(claims, db.clone());
        // Forbidden only means "not a current admin"; anything else (database
        // failure, malformed subject) must not silently downgrade the caller.
        match ensure_current_admin_on(claims, &db).await {
            Ok(()) => {
                req.extensions_mut().insert(CurrentAdminVerified(()));
            }
            Err((StatusCode::FORBIDDEN, _)) => {}
            Err(error) => return error.into_response(),
        }
        req.extensions_mut().insert(claims.clone());
    }
    req.extensions_mut().insert(OptionalClaims(claims));
    next.run(req).await
}

/// 每用户至少间隔 60s 才落一次库，避免高频请求放大写入。
const PRESENCE_WRITE_INTERVAL: Duration = Duration::from_secs(60);
/// Drop map entries older than this so long-lived processes do not retain every
/// user_id forever. Must be ≥ [`PRESENCE_WRITE_INTERVAL`].
const PRESENCE_MAP_TTL: Duration = Duration::from_secs(15 * 60);
/// 两次活跃间隔 ≤300s 视为持续在线，计入 online_seconds；更长间隔视为离线后重新上线。
const PRESENCE_SESSION_GAP_SECS: i64 = 300;

static PRESENCE_WRITE_TIMES: OnceLock<StdMutex<HashMap<i32, Instant>>> = OnceLock::new();

fn presence_write_due(user_id: i32) -> bool {
    let map = PRESENCE_WRITE_TIMES.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut guard = match map.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let now = Instant::now();
    // Opportunistic TTL prune (cheap when map is small; caps growth on multi-user sites).
    if guard.len() > 64 {
        guard.retain(|_, last| now.duration_since(*last) < PRESENCE_MAP_TTL);
    }
    match guard.get(&user_id) {
        Some(last) if now.duration_since(*last) < PRESENCE_WRITE_INTERVAL => false,
        _ => {
            guard.insert(user_id, now);
            true
        }
    }
}

/// 节流更新 users.last_seen_at / online_seconds（异步、尽力而为）。
/// 游客（负数 ID）不记录。DB 由 middleware `State` 注入。
pub fn record_user_presence(claims: &Claims, db: DatabaseConnection) {
    let Ok(user_id) = claims.sub.parse::<i32>() else {
        return;
    };
    if user_id <= 0 || !presence_write_due(user_id) {
        return;
    }
    tokio::spawn(async move {
        let result = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET \
                     online_seconds = online_seconds + CASE \
                         WHEN last_seen_at IS NOT NULL AND NOW() - last_seen_at <= make_interval(secs => $2) \
                         THEN EXTRACT(EPOCH FROM (NOW() - last_seen_at))::BIGINT \
                         ELSE 0 END, \
                     last_seen_at = NOW() \
                 WHERE id = $1",
                [user_id.into(), PRESENCE_SESSION_GAP_SECS.into()],
            ))
            .await;
        if let Err(e) = result {
            tracing::debug!("Failed to record presence for user {}: {}", user_id, e);
        }
    });
}

/// Admin-only middleware - verifies JWT token and checks admin status
/// Returns 403 if user is not an admin
///
/// Checks both the signed claim and the current database role (`ensure_current_admin_on`)。
pub async fn admin_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();

    match authenticate_request(headers, &db).await {
        Ok(claims) => {
            record_user_presence(&claims, db.clone());
            if let Err((status, body)) = ensure_current_admin_on(&claims, &db).await {
                tracing::warn!(
                    "⚠️  User {} (is_admin={}) attempted to access admin-only endpoint (Forbidden)",
                    claims.username,
                    claims.is_admin
                );
                return (status, body).into_response();
            }

            tracing::info!(
                "✅ Admin access granted to user: {} (is_admin=true)",
                claims.username
            );

            // 注入 claims，供后续 `Extension(Claims)`；并标记本请求已完成当前管理员核验，
            // 供 `AdminClaims` 复用，不再二次查库。
            let mut req = req;
            req.extensions_mut().insert(claims);
            req.extensions_mut().insert(CurrentAdminVerified(()));
            next.run(req).await
        }
        Err(error_response) => *error_response,
    }
}

/// Request-scoped proof that `admin_middleware` (or the optional-auth variants
/// above) already ran [`ensure_current_admin_on`] successfully for the `Claims`
/// in this request.
///
/// The private field keeps it constructible only here, so neither a header nor
/// another module can forge it. It holds no identity copy and never outlives the
/// request; the next request is checked against the database again.
#[derive(Clone, Copy, Debug)]
pub struct CurrentAdminVerified(());

/// Verify the signed admin claim against the current database state.
///
/// This prevents a demoted admin from keeping admin access until the old JWT
/// expires.
pub async fn ensure_current_admin_on(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if !claims.is_admin {
        return Err(admin_forbidden());
    }

    let user_id =
        crate::services::tapp_ownership::positive_user_id(&claims.sub).ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Unauthorized",
                    "message": "Invalid user ID in authorization token."
                })),
            )
        })?;

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT is_admin FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("Failed to verify current admin status: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Database error",
                    "code": "database_error",
                    "message": "Administrator status cannot be verified."
                })),
            )
        })?;

    let is_admin = row
        .map(|r| r.try_get::<bool>("", "is_admin"))
        .transpose()
        .map_err(|e| {
            tracing::error!(%e, "Failed to decode current admin status");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Database error",
                    "code": "database_error",
                    "message": "Administrator status cannot be verified."
                })),
            )
        })?
        .unwrap_or(false);

    if is_admin {
        Ok(())
    } else {
        Err(admin_forbidden())
    }
}

/// Load the current durable authorization snapshot.
///
/// A cache miss deliberately fetches all auth facts needed by ordinary request
/// authorization in one query.  `None` is retained as a negative cache entry,
/// representing a deleted/missing account and therefore a revoked session.
async fn load_auth_snapshot(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Option<AuthSnapshot>, sea_orm::DbErr> {
    if user_id <= 0 {
        return Ok(None);
    }

    loop {
        if let Some(snapshot) = auth_cache_get(user_id) {
            return Ok(snapshot);
        }

        // Listener setup is only needed on a real cache miss. Keeping the hit
        // path database-free also makes the cache's authorization boundary
        // independently testable.
        ensure_auth_cache_listener(db).await;

        // Only one request per user performs the miss query. Other concurrent
        // requests wait for that result, then take the now-populated cache hit.
        let owner = match claim_auth_load_slot(user_id) {
            AuthLoadSlot::Waiter(mut receiver) => {
                // The watch version is retained even if the owner completes
                // before this await, avoiding a lost wakeup under a burst.
                let _ = receiver.changed().await;
                continue;
            }
            AuthLoadSlot::Owner(owner) => owner,
        };
        let load_generation = auth_cache_generation();

        let result = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT COALESCE(token_version, 0) AS token_version, \
                        COALESCE(is_admin, false) AS is_admin, \
                        COALESCE(is_owner, false) AS is_owner \
                 FROM users WHERE id = $1 LIMIT 1",
                [user_id.into()],
            ))
            .await
            .map(|row| {
                row.map(|row| AuthSnapshot {
                    token_version: row
                        .try_get::<i32>("", "token_version")
                        .ok()
                        .map(i64::from)
                        .or_else(|| row.try_get::<i64>("", "token_version").ok())
                        .unwrap_or(0),
                    is_admin: row.try_get::<bool>("", "is_admin").unwrap_or(false),
                    is_owner: row.try_get::<bool>("", "is_owner").unwrap_or(false),
                })
            });

        if let Ok(snapshot) = &result {
            let _ = auth_cache_put_if_generation(user_id, *snapshot, load_generation);
        }

        // The owner guard is held across the database await. Its Drop always
        // clears the slot and wakes waiters, including on task abort or panic.
        drop(owner);
        return result;
    }
}

/// Pure session-epoch check used by auth middleware and unit tests.
///
/// - Missing user (`None`) → revoked (deleted account)
/// - Claim `tv` must equal stored version
pub fn session_epoch_matches(claim_tv: i64, db_version: Option<i64>) -> bool {
    match db_version {
        Some(v) => claim_tv == v,
        None => false,
    }
}

/// Load and validate the current snapshot for a JWT.
///
/// Guests (`sub` ≤ 0) skip the DB check — they are not durable sessions.
/// Missing durable users fail closed as revoked sessions.
async fn validated_auth_snapshot(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<Option<AuthSnapshot>, Box<Response>> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| unauthorized_session_response())?;
    if user_id <= 0 {
        return Ok(None);
    }

    let snapshot = load_auth_snapshot(db, user_id).await.map_err(|e| {
        tracing::error!("Failed to load token_version for user {}: {}", user_id, e);
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Database error",
                    "code": "database_error",
                    "message": "Session cannot be verified."
                })),
            )
                .into_response(),
        )
    })?;

    let Some(snapshot) = snapshot else {
        tracing::debug!(
            user_id,
            "JWT session refers to a missing user — treating token as revoked"
        );
        return Err(unauthorized_session_response());
    };

    if !session_epoch_matches(claims.tv, Some(snapshot.token_version)) {
        tracing::debug!(
            user_id,
            claim_tv = claims.tv,
            db_version = snapshot.token_version,
            "JWT session epoch mismatch — treating token as revoked"
        );
        return Err(unauthorized_session_response());
    }

    Ok(Some(snapshot))
}

fn apply_current_roles(mut claims: Claims, snapshot: AuthSnapshot) -> Claims {
    // Never grant a role to a token that was minted without it, but do
    // immediately remove demoted roles from stale claims.
    if !snapshot.is_admin {
        claims.is_admin = false;
    }
    if !snapshot.is_owner {
        claims.is_owner = false;
    }
    claims
}

fn unauthorized_session_response() -> Box<Response> {
    Box::new(
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Invalid token",
                "message": "Session has been revoked. Please login again."
            })),
        )
            .into_response(),
    )
}

/// Cryptographic JWT verify **plus** server-side session epoch check.
///
/// Prefer this over [`verify_jwt_token`] for any path that must fail closed
/// after logout / password change / account deletion.
pub async fn authenticate_request(
    headers: &HeaderMap,
    db: &DatabaseConnection,
) -> Result<Claims, Box<Response>> {
    let mut claims = verify_jwt_token(headers)?;
    if let Some(snapshot) = validated_auth_snapshot(&claims, db).await? {
        // This keeps ordinary authorization decisions bounded by the cache TTL
        // even when a role-change NOTIFY is missed.
        claims = apply_current_roles(claims, snapshot);
    }
    Ok(claims)
}

/// Revalidate claims already bound by authenticated server code to a short-lived
/// capability. Never use this on claims supplied by an external request body.
pub(crate) async fn revalidate_bound_claims(
    claims: &Claims,
    db: &DatabaseConnection,
) -> Result<Claims, Box<Response>> {
    if claims.exp <= chrono::Utc::now().timestamp()
        || crate::services::tapp_ownership::positive_user_id(&claims.sub).is_none()
    {
        return Err(unauthorized_session_response());
    }
    let snapshot = validated_auth_snapshot(claims, db)
        .await?
        .ok_or_else(unauthorized_session_response)?;
    Ok(apply_current_roles(claims.clone(), snapshot))
}

/// Resolve a current subject when authentication is optional.
///
/// This differs from [`authenticate_request`] only for the no-credential case.
/// Invalid, revoked, deleted-user and stale-role JWTs never silently downgrade
/// to anonymous access.
pub async fn authenticate_optional_request(
    headers: &HeaderMap,
    db: &DatabaseConnection,
) -> Result<Option<Claims>, Box<Response>> {
    if extract_auth_token(headers).is_none() {
        return Ok(None);
    }
    authenticate_request(headers, db).await.map(Some)
}

/// Atomically revoke the expected session epoch and return the new value.
///
/// logout 调用。后续 JWT 对不上 `session_epoch_matches` 即 401。
/// 密码修改自行 `UPDATE token_version`。用户不存在或版本已变化返回 `None`。
pub async fn bump_token_version(
    db: &DatabaseConnection,
    user_id: i32,
    expected_version: i64,
) -> Result<Option<i64>, sea_orm::DbErr> {
    if user_id <= 0 {
        return Ok(None);
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE users SET token_version = COALESCE(token_version, 0) + 1, updated_at = NOW() \
             WHERE id = $1 AND COALESCE(token_version, 0)::BIGINT = $2 RETURNING token_version",
            [user_id.into(), expected_version.into()],
        ))
        .await?;
    let new_version = row.and_then(|r| {
        r.try_get::<i32>("", "token_version")
            .ok()
            .map(i64::from)
            .or_else(|| r.try_get::<i64>("", "token_version").ok())
    });
    if new_version.is_none() {
        return Ok(None);
    }
    // Remove the local entry even if NOTIFY cannot be delivered.  Publishing
    // after the committed UPDATE keeps peer caches bounded by the five-second
    // TTL in the event of a transient notification failure.
    if let Err(error) = notify_auth_cache_invalidation(db, user_id).await {
        tracing::warn!(user_id, error = %error, "auth cache invalidation NOTIFY failed after token bump");
    }
    Ok(new_version)
}

fn admin_forbidden() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "Forbidden",
            "message": "Administrator access required. Only current admin users can perform this action."
        })),
    )
}

const GUEST_SESSION_COOKIE: &str = "myriad_guest_session";
const GUEST_SESSION_MAX_AGE: i64 = 30 * 24 * 60 * 60;

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (cookie_name, value) = cookie.trim().split_once('=')?;
                (cookie_name == name).then_some(value)
            })
        })
}

fn guest_signature(secret: &[u8], session_id: &str) -> Option<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).ok()?;
    mac.update(b"myriad-guest-session-v1\0");
    mac.update(session_id.as_bytes());
    Some(mac.finalize().into_bytes().to_vec())
}

fn sign_guest_session(secret: &[u8], session_id: &str) -> Option<String> {
    let signature = guest_signature(secret, session_id)?;
    Some(format!(
        "{session_id}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn verify_guest_session(secret: &[u8], token: &str) -> Option<String> {
    let (session_id, encoded_signature) = token.split_once('.')?;
    if session_id.len() != 32 || !session_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let supplied = URL_SAFE_NO_PAD.decode(encoded_signature).ok()?;
    let expected = guest_signature(secret, session_id)?;
    (supplied.len() == expected.len() && supplied.as_slice().ct_eq(expected.as_slice()).into())
        .then(|| session_id.to_ascii_lowercase())
}

/// Stable negative subject id for a browser guest session.
///
/// ## Capacity / collision bound
/// Postgres `tapp_runtime_registry.subject_id` (and related columns) are
/// `INTEGER`, so the id must fit in signed 32-bit. We use the full negative
/// `i32` range `i32::MIN ..= -1` (~2³¹ values). Birthday bound for ~50%
/// collision probability among *distinct concurrent guest sessions* is on the
/// order of √(π · 2³¹ / 2) ≈ **~58k** sessions — residual collisions only
/// alias guest storage / rate-limit namespaces, never escalate to a real user
/// (real users are non-negative).
///
/// ## Hash material
/// Full SHA-256 is XOR-folded into 31 bits. Cookie session tokens are HMAC of the
/// 32-hex session id; only the derived numeric subject is this fold.
///
/// A true 63-bit negative `i64` would need `BIGINT` subject columns site-wide;
/// until then this is the strongest scheme that still fits the DB type.
fn guest_id(session_id: &str) -> i32 {
    let digest = Sha256::digest(session_id.as_bytes());
    let mut acc = 0u64;
    for chunk in digest.as_chunks::<8>().0 {
        acc ^= u64::from_be_bytes(*chunk);
    }
    // 31 payload bits → always map into i32::MIN ..= -1 (never 0 / positive).
    let bits31 = (acc & 0x7FFF_FFFF) as u32;
    if bits31 == 0 {
        i32::MIN
    } else {
        -(bits31 as i32)
    }
}

/// Optional authentication middleware - allows guest access
///
/// 给允许游客主体的路由注入 Claims：有效 token → Claims；无 token → 游客 Claims；
/// 无效/已撤销 token 拒绝，不降级为游客。本中间件只注入 Claims，不计算授予权限。
///
/// 游客 ID 策略：
/// - 使用浏览器持有的 HttpOnly 签名 session，而不是共享出口 IP
/// - 同一浏览器 session 获得稳定的负数 ID
/// - 负数 ID 与正数用户 ID 区分，便于管理
///
/// 游客 Claims 的 is_admin 为 false。
pub async fn optional_auth_middleware(
    State(db): State<DatabaseConnection>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();
    let credential_source = auth_credential_source(headers);
    let mut set_guest_cookie = None;
    // Missing credentials become a signed guest. Presented credentials must
    // pass the full current-state path and never downgrade to guest.
    let claims = match authenticate_optional_request(headers, &db).await {
        Ok(Some(claims)) => {
            record_user_presence(&claims, db);
            claims
        }
        Ok(None) => {
            let secret = match env::var("JWT_SECRET") {
                Ok(secret) if !secret.is_empty() => secret,
                _ => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(AppError::public_json(
                            "Guest session signing is unavailable",
                        )),
                    )
                        .into_response();
                }
            };
            let session_id = cookie_value(headers, GUEST_SESSION_COOKIE)
                .and_then(|token| verify_guest_session(secret.as_bytes(), token))
                .unwrap_or_else(|| {
                    let session_id = Uuid::new_v4().simple().to_string();
                    set_guest_cookie = sign_guest_session(secret.as_bytes(), &session_id);
                    session_id
                });
            let guest_id = guest_id(&session_id);
            tracing::debug!(guest_id, "Guest access through signed browser session");

            Claims {
                sub: guest_id.to_string(),
                username: format!("guest:{}", &session_id[..8]),
                is_admin: false,
                is_owner: false,
                exp: chrono::Utc::now().timestamp() + GUEST_SESSION_MAX_AGE,
                iat: chrono::Utc::now().timestamp(),
                tv: 0,
            }
        }
        Err(error_response) => {
            return clear_invalid_cookie_response(*error_response, credential_source).await;
        }
    };

    let mut req = req;
    req.extensions_mut().insert(claims);
    let mut response = next.run(req).await;
    if let Some(token) = set_guest_cookie {
        let is_production = crate::oauth_url_builder::SiteConfig::is_production().await;
        let cookie = format!(
            "{GUEST_SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={GUEST_SESSION_MAX_AGE}{}",
            if is_production { "; Secure" } else { "" }
        );
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// Verify JWT token from Authorization header or Cookie
/// Verify JWT for handlers that need claims outside the middleware pipeline.
pub fn verify_jwt_token(headers: &HeaderMap) -> Result<Claims, Box<Response>> {
    let token = extract_auth_token(headers).ok_or_else(|| {
        tracing::debug!("Missing or invalid Authorization header/cookie");
        Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Unauthorized",
                    "message": "Missing or invalid authorization token. Please login first."
                })),
            )
                .into_response(),
        )
    })?;

    // Get JWT secret
    let jwt_secret = env::var("JWT_SECRET").map_err(|_| {
        tracing::error!("JWT_SECRET not configured");
        Box::new(
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Server configuration error",
                    "message": "Authentication system not properly configured"
                })),
            )
                .into_response(),
        )
    })?;

    // Decode and verify token
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| {
        tracing::debug!("Invalid JWT token: {:?}", e);
        Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "Invalid token",
                    "message": "Token is invalid or expired. Please login again."
                })),
            )
                .into_response(),
        )
    })?;

    Ok(token_data.claims)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthCredentialSource {
    Authorization,
    Cookie,
}

fn auth_cookie_token(headers: &HeaderMap) -> Option<&str> {
    cookie_value(headers, "auth_token")
}

fn auth_credential_source(headers: &HeaderMap) -> Option<AuthCredentialSource> {
    if headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some()
    {
        Some(AuthCredentialSource::Authorization)
    } else {
        auth_cookie_token(headers).map(|_| AuthCredentialSource::Cookie)
    }
}

fn extract_auth_token(headers: &HeaderMap) -> Option<&str> {
    match auth_credential_source(headers)? {
        AuthCredentialSource::Authorization => headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer ")),
        AuthCredentialSource::Cookie => auth_cookie_token(headers),
    }
}

async fn clear_invalid_cookie_response(
    mut response: Response,
    credential_source: Option<AuthCredentialSource>,
) -> Response {
    if credential_source != Some(AuthCredentialSource::Cookie)
        || response.status() != StatusCode::UNAUTHORIZED
    {
        return response;
    }

    let is_production = crate::oauth_url_builder::SiteConfig::is_production().await;
    if let Ok(value) = HeaderValue::from_str(&clear_auth_cookie_value(is_production)) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

#[cfg(test)]
mod tests {
    #[test]
    fn admin_recheck_requires_positive_subject() {
        let src = include_str!("auth.rs");
        let body = src
            .split("pub async fn ensure_current_admin_on")
            .nth(1)
            .expect("ensure_current_admin_on");
        assert!(body.contains("positive_user_id"));
    }

    use super::{
        AUTH_CACHE_CAPACITY, AUTH_CACHE_TTL, AUTH_COOKIE_MAX_AGE_SECS, AuthLoadSlot, AuthSnapshot,
        JWT_TTL_DAYS, apply_current_roles, auth_cache_generation, auth_cache_get, auth_cache_put,
        auth_cache_put_if_generation, authenticate_optional_request, claim_auth_load_slot,
        encode_session_token, guest_id, invalidate_auth_cache_local, mint_session_claims,
        optional_current_auth_middleware, revalidate_bound_claims, session_epoch_matches,
        sign_guest_session, verify_guest_session, verify_jwt_token,
    };
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
    use sea_orm::DatabaseConnection;
    use std::sync::{Mutex, Once, OnceLock};
    use std::time::{Duration, Instant};

    static INIT_JWT: Once = Once::new();
    static AUTH_CACHE_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    const TEST_JWT_SECRET: &str = "auth-unit-test-jwt-secret-at-least-32-bytes";

    /// True when `JWT_SECRET` is missing or empty (unusable as an HMAC key).
    fn jwt_secret_is_unset() -> bool {
        match std::env::var("JWT_SECRET") {
            Ok(s) => s.is_empty(),
            Err(_) => true,
        }
    }

    fn ensure_jwt_secret() {
        INIT_JWT.call_once(|| {
            // Missing *and* empty JWT_SECRET are both unsafe for HS256.
            if jwt_secret_is_unset() {
                // SAFETY: unit tests, set once before concurrent use.
                unsafe { std::env::set_var("JWT_SECRET", TEST_JWT_SECRET) };
            }
        });
        // If another test left an empty secret after Once already ran, repair it.
        if jwt_secret_is_unset() {
            // SAFETY: unit tests only.
            unsafe { std::env::set_var("JWT_SECRET", TEST_JWT_SECRET) };
        }
    }

    #[test]
    fn ensure_jwt_secret_treats_empty_like_missing() {
        // Empty string must be handled like missing — never mint with a zero-length key.
        // SAFETY: sequential unit tests; restore is best-effort.
        let previous = std::env::var("JWT_SECRET").ok();
        unsafe { std::env::set_var("JWT_SECRET", "") };
        assert!(jwt_secret_is_unset());
        ensure_jwt_secret();
        let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET set");
        assert!(!secret.is_empty());
        assert_eq!(secret, TEST_JWT_SECRET);
        match previous {
            Some(v) if !v.is_empty() => unsafe { std::env::set_var("JWT_SECRET", v) },
            _ => unsafe { std::env::set_var("JWT_SECRET", TEST_JWT_SECRET) },
        }
    }

    #[test]
    fn jwt_issue_and_verify_roundtrip() {
        // jsonwebtoken 11: HS256 encode/decode via rust_crypto backend must stay
        // wire-compatible for login cookies and Authorization Bearer tokens.
        ensure_jwt_secret();
        let claims = mint_session_claims(42, "roundtrip-user", true, false, 7);
        let token = encode_session_token(&claims).expect("encode_session_token");

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).expect("header"),
        );
        let verified = verify_jwt_token(&headers).expect("verify_jwt_token");
        assert_eq!(verified.sub, "42");
        assert_eq!(verified.username, "roundtrip-user");
        assert!(verified.is_admin);
        assert!(!verified.is_owner);
        assert_eq!(verified.tv, 7);
        assert_eq!(verified.exp, claims.exp);
        assert_eq!(verified.iat, claims.iat);

        // Cookie path used by browser sessions
        let mut cookie_headers = HeaderMap::new();
        cookie_headers.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("auth_token={token}")).expect("cookie"),
        );
        let from_cookie = verify_jwt_token(&cookie_headers).expect("cookie verify");
        assert_eq!(from_cookie.sub, verified.sub);
        assert_eq!(from_cookie.tv, verified.tv);
    }

    #[test]
    fn jwt_tampered_token_is_rejected() {
        ensure_jwt_secret();
        let claims = mint_session_claims(1, "u", false, false, 0);
        let token = encode_session_token(&claims).expect("encode");
        // Flip last character so signature fails.
        let mut bad = token;
        let last = bad.pop().expect("non-empty");
        bad.push(if last == 'A' { 'B' } else { 'A' });

        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {bad}")).expect("header"),
        );
        assert!(verify_jwt_token(&headers).is_err());
    }

    #[test]
    fn site_management_routes_reject_ordinary_and_anonymous_users() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        ensure_jwt_secret();
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        auth_cache_put(
            79,
            Some(AuthSnapshot {
                token_version: 0,
                is_admin: false,
                is_owner: false,
            }),
        );
        let token = encode_session_token(&mint_session_claims(79, "ordinary", false, false, 0))
            .expect("token");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let app = crate::router::test_api_router(crate::state::AppState::new(
                DatabaseConnection::default(),
                crate::config::AppConfig::default(),
                crate::config::DynamicConfig::default(),
            ));
            for (method, path, body) in [
                ("GET", "/api/config", ""),
                (
                    "POST",
                    "/api/config/test",
                    r#"{"platform":"Discord","config":{}}"#,
                ),
                (
                    "POST",
                    "/api/prompt/generate",
                    r#"{"title":"test","summary":"test"}"#,
                ),
            ] {
                for authenticated in [false, true] {
                    let mut request = Request::builder()
                        .method(method)
                        .uri(path)
                        .header(header::CONTENT_TYPE, "application/json");
                    if authenticated {
                        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
                    }
                    let response = app
                        .clone()
                        .oneshot(request.body(Body::from(body)).expect("request"))
                        .await
                        .expect("response");
                    assert_eq!(
                        response.status(),
                        if authenticated {
                            StatusCode::FORBIDDEN
                        } else {
                            StatusCode::UNAUTHORIZED
                        },
                        "{method} {path}, authenticated={authenticated}"
                    );
                }
            }
        });
    }

    #[tokio::test]
    async fn role_read_failures_are_unavailable_and_recover_without_demotion() {
        use crate::services::tapp_ownership::{TappAccessError, subject_is_admin};
        use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("AUTH_TEST_DATABASE_URL") else {
            return;
        };
        let mut options = ConnectOptions::new(url);
        options.max_connections(1).min_connections(1);
        let db = Database::connect(options).await.unwrap();
        let claims = super::mint_session_claims(2, "subject", true, false, 0);
        for sql in [
            "CREATE TEMP TABLE users (id INTEGER PRIMARY KEY, is_admin BOOLEAN, is_owner BOOLEAN)",
            "INSERT INTO users VALUES (1, true, true), (2, NULL, false)",
        ] {
            db.execute_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                .await
                .unwrap();
        }
        // A NULL result cannot be decoded as bool; it is not a false role.
        assert_eq!(
            super::ensure_current_admin_on(&claims, &db)
                .await
                .unwrap_err()
                .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(matches!(
            subject_is_admin(&db, 2).await,
            Err(TappAccessError::Database)
        ));
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "ALTER TABLE users RENAME COLUMN is_admin TO unavailable",
        ))
        .await
        .unwrap();
        assert_eq!(
            super::ensure_current_admin_on(&claims, &db)
                .await
                .unwrap_err()
                .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert!(matches!(
            subject_is_admin(&db, 2).await,
            Err(TappAccessError::Database)
        ));
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "ALTER TABLE users RENAME COLUMN unavailable TO is_admin",
        ))
        .await
        .unwrap();
        for (value, allowed) in [("true", true), ("false", false)] {
            db.execute_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!("UPDATE users SET is_admin = {value} WHERE id = 2"),
            ))
            .await
            .unwrap();
            assert_eq!(
                super::ensure_current_admin_on(&claims, &db).await.is_ok(),
                allowed
            );
            assert_eq!(subject_is_admin(&db, 2).await.unwrap(), allowed);
        }
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "DELETE FROM users WHERE id = 2",
        ))
        .await
        .unwrap();
        assert_eq!(
            super::ensure_current_admin_on(&claims, &db)
                .await
                .unwrap_err()
                .0,
            StatusCode::FORBIDDEN
        );
        assert!(!subject_is_admin(&db, 2).await.unwrap());
        db.close().await.unwrap();
    }

    /// Run against an explicit test DB; a single-connection TEMP table keeps
    /// the fixture separate from durable users and exercises the actual SQL.
    #[tokio::test]
    async fn logout_epoch_update_rejects_stale_and_concurrent_replays() {
        use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Statement};

        let Ok(url) = std::env::var("AUTH_TEST_DATABASE_URL") else {
            return;
        };
        let mut options = ConnectOptions::new(url);
        options.max_connections(1).min_connections(1);
        let db = Database::connect(options).await.expect("connect test DB");
        db.execute_raw(Statement::from_string(DatabaseBackend::Postgres,
            "CREATE TEMP TABLE users (id INTEGER PRIMARY KEY, token_version INTEGER, updated_at TIMESTAMPTZ)"))
            .await.unwrap();
        db.execute_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "INSERT INTO users (id, token_version) VALUES (1, 4), (2, 9)",
        ))
        .await
        .unwrap();

        let (a, b) = tokio::join!(
            super::bump_token_version(&db, 1, 4),
            super::bump_token_version(&db, 1, 4)
        );
        let (a, b) = (a.unwrap(), b.unwrap());
        assert!(matches!((a, b), (Some(5), None) | (None, Some(5))));
        assert_eq!(super::bump_token_version(&db, 1, 4).await.unwrap(), None);
        assert_eq!(super::bump_token_version(&db, 1, 5).await.unwrap(), Some(6));
        assert_eq!(super::bump_token_version(&db, 1, 4).await.unwrap(), None);
        assert_eq!(super::bump_token_version(&db, 999, 0).await.unwrap(), None);
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT token_version FROM users ORDER BY id",
            ))
            .await
            .unwrap();
        assert_eq!(rows[0].try_get::<i32>("", "token_version").unwrap(), 6);
        assert_eq!(rows[1].try_get::<i32>("", "token_version").unwrap(), 9);
        db.close().await.unwrap();
    }

    #[test]
    fn phantasi_comment_routes_reject_revoked_sessions() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        ensure_jwt_secret();
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        let token = encode_session_token(&mint_session_claims(80, "revoked", false, false, 0))
            .expect("token");
        auth_cache_put(
            80,
            Some(AuthSnapshot {
                token_version: 1,
                is_admin: false,
                is_owner: false,
            }),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let state = crate::state::AppState::new(
                DatabaseConnection::default(),
                crate::config::AppConfig::default(),
                crate::config::DynamicConfig::default(),
            );
            let app = axum::Router::new()
                .nest(
                    "/api/phantasi",
                    crate::api::phantasi::create_phantasi_routes(state.clone()),
                )
                .with_state(state);
            // 游客可读，空连接上不能走查库。这里只卡撤销/假 token 仍 401。
            for path in [
                "/api/phantasi/items/1/comments",
                "/api/phantasi/comments/1/replies",
            ] {
                for value in [format!("Bearer {token}"), "Bearer invalid".to_string()] {
                    let response = app
                        .clone()
                        .oneshot(
                            Request::builder()
                                .uri(path)
                                .header(header::AUTHORIZATION, value)
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
                }
            }
        });
    }

    #[test]
    fn optional_current_auth_only_treats_absence_as_anonymous() {
        ensure_jwt_secret();
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        let db = sea_orm::DatabaseConnection::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        let anonymous = runtime
            .block_on(authenticate_optional_request(&HeaderMap::new(), &db))
            .expect("missing credentials are anonymous");
        assert!(anonymous.is_none());

        let current = mint_session_claims(77, "current", true, true, 4);
        let token = encode_session_token(&current).expect("encode current token");
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).expect("header"),
        );
        auth_cache_put(
            77,
            Some(AuthSnapshot {
                token_version: 4,
                is_admin: false,
                is_owner: false,
            }),
        );
        let resolved = runtime
            .block_on(authenticate_optional_request(&headers, &db))
            .expect("current token")
            .expect("current subject");
        assert_eq!(resolved.sub, "77");
        assert!(
            !resolved.is_admin,
            "current demotion must override stale JWT role"
        );
        assert!(
            !resolved.is_owner,
            "current owner state must override stale JWT role"
        );

        invalidate_auth_cache_local(77);
        auth_cache_put(
            77,
            Some(AuthSnapshot {
                token_version: 5,
                is_admin: false,
                is_owner: false,
            }),
        );
        assert!(
            runtime
                .block_on(authenticate_optional_request(&headers, &db))
                .is_err(),
            "revoked token must not downgrade to anonymous"
        );

        invalidate_auth_cache_local(77);
        auth_cache_put(77, None);
        assert!(
            runtime
                .block_on(authenticate_optional_request(&headers, &db))
                .is_err(),
            "deleted user must not downgrade to anonymous"
        );
    }

    #[test]
    fn optional_current_auth_http_rejects_revoked_cookie_and_clears_it() {
        use axum::{Router, body::Body, middleware::from_fn_with_state, routing::get};
        use tower::ServiceExt;

        ensure_jwt_secret();
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();

        let claims = mint_session_claims(78, "revoked", false, false, 4);
        let token = encode_session_token(&claims).expect("encode revoked token");
        auth_cache_put(
            78,
            Some(AuthSnapshot {
                token_version: 5,
                is_admin: false,
                is_owner: false,
            }),
        );

        let app = Router::new()
            .route("/", get(|| async { StatusCode::OK }))
            .route_layer(from_fn_with_state(
                sea_orm::DatabaseConnection::default(),
                optional_current_auth_middleware,
            ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let response = runtime
            .block_on(
                app.oneshot(
                    axum::http::Request::builder()
                        .uri("/")
                        .header(header::COOKIE, format!("auth_token={token}"))
                        .body(Body::empty())
                        .expect("request"),
                ),
            )
            .expect("middleware response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let clear_cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .expect("revoked HttpOnly cookie must be cleared by the server");
        assert!(clear_cookie.starts_with("auth_token=deleted;"));
        assert!(clear_cookie.contains("HttpOnly"));
        assert!(clear_cookie.contains("Max-Age=0"));
    }

    #[test]
    fn optional_admin_auth_keeps_invalid_credential_semantics_per_mode() {
        use crate::extract::OptionalViewer;
        use axum::{Router, body::Body, middleware::from_fn_with_state, routing::get};
        use tower::ServiceExt;

        ensure_jwt_secret();
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        // 81 current ordinary user, 82 revoked session, 83 admin claim whose live
        // check hits the (disconnected) database.
        for (id, token_version, is_admin) in [(81, 0, false), (82, 1, false), (83, 0, true)] {
            auth_cache_put(
                id,
                Some(AuthSnapshot {
                    token_version,
                    is_admin,
                    is_owner: false,
                }),
            );
        }
        let bearer = |id: i32, admin: bool| {
            let claims = mint_session_claims(id, "viewer", admin, false, 0);
            format!("Bearer {}", encode_session_token(&claims).expect("token"))
        };
        let viewer = get(|viewer: OptionalViewer| async move {
            let sub = viewer.claims.map(|claims| claims.sub).unwrap_or_default();
            format!("{sub}:{}", viewer.is_admin)
        });
        let strict = Router::new()
            .route("/", viewer.clone())
            .route_layer(from_fn_with_state(
                DatabaseConnection::default(),
                super::optional_current_admin_auth_middleware,
            ));
        let lenient = Router::new()
            .route("/", viewer.clone())
            .route_layer(from_fn_with_state(
                DatabaseConnection::default(),
                super::lenient_current_admin_auth_middleware,
            ));
        let unmounted = Router::new().route("/", viewer);
        let call = |app: axum::Router, auth: Option<String>| async move {
            let mut request = axum::http::Request::builder().uri("/");
            if let Some(auth) = auth {
                request = request.header(header::AUTHORIZATION, auth);
            }
            let response = app
                .oneshot(request.body(Body::empty()).expect("request"))
                .await
                .expect("response");
            let status = response.status();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body");
            (status, String::from_utf8_lossy(&body).into_owned())
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let ok = |body: &str| (StatusCode::OK, body.to_string());
            assert_eq!(call(strict.clone(), None).await, ok(":false"));
            assert_eq!(
                call(strict.clone(), Some(bearer(81, false))).await,
                ok("81:false")
            );
            for invalid in ["Bearer invalid".to_string(), bearer(82, false)] {
                assert_eq!(
                    call(strict.clone(), Some(invalid.clone())).await.0,
                    StatusCode::UNAUTHORIZED
                );
                assert_eq!(call(lenient.clone(), Some(invalid)).await, ok(":false"));
            }
            // A failed live admin check never downgrades to a non-admin viewer.
            for app in [strict, lenient] {
                assert_eq!(
                    call(app, Some(bearer(83, true))).await.0,
                    StatusCode::INTERNAL_SERVER_ERROR
                );
            }
            // Missing middleware is a wiring bug, not an anonymous viewer.
            assert_eq!(
                call(unmounted, Some(bearer(81, false))).await.0,
                StatusCode::INTERNAL_SERVER_ERROR
            );
        });
    }

    #[test]
    fn signed_guest_session_is_stable_and_tamper_evident() {
        let secret = b"test-secret-at-least-thirty-two-bytes-long";
        let session_id = "0123456789abcdef0123456789abcdef";
        let token = sign_guest_session(secret, session_id).expect("session signs");
        assert_eq!(
            verify_guest_session(secret, &token).as_deref(),
            Some(session_id)
        );
        assert!(verify_guest_session(secret, &format!("{token}x")).is_none());
        assert!(verify_guest_session(b"different-secret", &token).is_none());
    }

    #[test]
    fn guest_ids_are_negative_and_browser_session_scoped() {
        let first = guest_id("0123456789abcdef0123456789abcdef");
        assert!(first < 0);
        assert_eq!(first, guest_id("0123456789abcdef0123456789abcdef"));
        assert_ne!(first, guest_id("fedcba9876543210fedcba9876543210"));
        // Never collide with real user ids (non-negative).
        assert!(first <= -1);
        // Distinct sessions should almost always differ under full-hash fold.
        let many: std::collections::HashSet<i32> = (0u32..256)
            .map(|i| guest_id(&format!("{i:032x}")))
            .collect();
        assert_eq!(
            many.len(),
            256,
            "unexpected guest_id collision in 256 samples"
        );
    }

    #[test]
    fn session_epoch_matches_requires_equal_version() {
        assert!(session_epoch_matches(0, Some(0)));
        assert!(session_epoch_matches(3, Some(3)));
        assert!(!session_epoch_matches(0, Some(1)));
        assert!(!session_epoch_matches(2, Some(1)));
        // Deleted / missing user → fail closed
        assert!(!session_epoch_matches(0, None));
        assert!(!session_epoch_matches(5, None));
    }

    fn clear_auth_cache_for_test() {
        let cache = super::auth_cache();
        let mut guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.entries.clear();
        guard.generation = 0;
        super::auth_cache_inflight()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn auth_cache_test_guard() -> std::sync::MutexGuard<'static, ()> {
        AUTH_CACHE_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn auth_cache_positive_role_and_session_snapshot_is_reusable() {
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        let snapshot = AuthSnapshot {
            token_version: 4,
            is_admin: true,
            is_owner: true,
        };
        auth_cache_put(101, Some(snapshot));
        // A second ordinary request can consume the same snapshot without a
        // database query; the combined miss fact includes both role flags.
        assert_eq!(auth_cache_get(101), Some(Some(snapshot)));
        assert!(session_epoch_matches(4, Some(snapshot.token_version)));
        let claims = mint_session_claims(101, "admin", true, true, 4);
        let claims = apply_current_roles(claims, snapshot);
        assert!(claims.is_admin && claims.is_owner);
    }

    #[test]
    fn auth_cache_invalidation_covers_demotion_deletion_and_token_epoch() {
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        auth_cache_put(
            102,
            Some(AuthSnapshot {
                token_version: 0,
                is_admin: true,
                is_owner: false,
            }),
        );
        // Role demotion/deletion must not leave a positive authorization fact
        // in this process after the mutator publishes its invalidation.
        invalidate_auth_cache_local(102);
        assert_eq!(auth_cache_get(102), None);
        assert!(!session_epoch_matches(0, None));
        assert!(!session_epoch_matches(0, Some(1)));
    }

    #[test]
    fn auth_cache_invalidation_fences_an_older_miss_result() {
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        let generation = auth_cache_generation();

        invalidate_auth_cache_local(104);
        assert!(!auth_cache_put_if_generation(
            104,
            Some(AuthSnapshot {
                token_version: 0,
                is_admin: true,
                is_owner: true,
            }),
            generation,
        ));
        assert_eq!(auth_cache_get(104), None);
    }

    #[test]
    fn auth_single_flight_owner_drop_wakes_waiter_and_releases_slot() {
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();

        let owner = match claim_auth_load_slot(105) {
            AuthLoadSlot::Owner(owner) => owner,
            AuthLoadSlot::Waiter(_) => panic!("first claimant must own the load slot"),
        };
        let mut waiter = match claim_auth_load_slot(105) {
            AuthLoadSlot::Waiter(waiter) => waiter,
            AuthLoadSlot::Owner(_) => panic!("second claimant must wait"),
        };

        // Models cancellation: dropping the owner future drops this guard.
        drop(owner);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("build cancellation test runtime");
        let _ = runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_millis(100), waiter.changed()).await
            })
            .expect("owner cancellation must not strand waiters");

        let replacement = match claim_auth_load_slot(105) {
            AuthLoadSlot::Owner(owner) => owner,
            AuthLoadSlot::Waiter(_) => panic!("cancelled owner slot must be reusable"),
        };
        drop(replacement);
    }

    #[test]
    fn stale_demoted_claim_cannot_keep_current_admin_role() {
        let stale_claims = mint_session_claims(103, "demoted", true, true, 0);
        let current = AuthSnapshot {
            token_version: 0,
            is_admin: false,
            is_owner: false,
        };
        let claims = apply_current_roles(stale_claims, current);
        assert!(!claims.is_admin);
        assert!(!claims.is_owner);
    }

    #[test]
    fn auth_cache_missed_notification_is_bounded_by_short_ttl() {
        assert!(AUTH_CACHE_TTL <= Duration::from_secs(5));
        let expired = Instant::now() - AUTH_CACHE_TTL;
        assert!(Instant::now().duration_since(expired) >= AUTH_CACHE_TTL);
    }

    #[test]
    fn auth_cache_is_bounded_by_capacity() {
        let _test_guard = auth_cache_test_guard();
        clear_auth_cache_for_test();
        for user_id in 1..=(AUTH_CACHE_CAPACITY as i32 + 1) {
            auth_cache_put(
                user_id,
                Some(AuthSnapshot {
                    token_version: user_id as i64,
                    is_admin: false,
                    is_owner: false,
                }),
            );
        }
        let guard = super::auth_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(guard.entries.len() <= AUTH_CACHE_CAPACITY);
    }

    #[test]
    fn mint_session_claims_embeds_token_version_and_30d_ttl() {
        let claims = mint_session_claims(7, "alice", false, true, 4);
        assert_eq!(claims.sub, "7");
        assert_eq!(claims.username, "alice");
        assert!(!claims.is_admin);
        assert!(claims.is_owner);
        assert_eq!(claims.tv, 4);
        let span = claims.exp - claims.iat;
        assert_eq!(span, JWT_TTL_DAYS * 24 * 60 * 60);
        assert_eq!(AUTH_COOKIE_MAX_AGE_SECS, span);
    }

    #[test]
    fn claims_tv_defaults_when_absent_in_json() {
        // Claim omitted `tv` → serde default 0.
        let json = r#"{"sub":"1","username":"u","is_admin":false,"exp":1,"iat":0}"#;
        let claims: super::Claims = serde_json::from_str(json).expect("deserialize");
        assert_eq!(claims.tv, 0);
        assert!(!claims.is_owner);
    }

    #[tokio::test]
    async fn a_bound_realtime_capability_expires_before_database_access() {
        let mut claims = mint_session_claims(7, "alice", false, false, 0);
        claims.exp = chrono::Utc::now().timestamp() - 1;
        let error = revalidate_bound_claims(&claims, &DatabaseConnection::default())
            .await
            .expect_err("expired binding must fail before a database query");
        assert_eq!(error.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_claims_reuse_the_admin_middleware_proof_without_a_second_query() {
        use axum::extract::FromRequestParts;
        let state = crate::state::AppState::new(
            DatabaseConnection::default(),
            crate::config::AppConfig::default(),
            crate::config::DynamicConfig::default(),
        );
        let claims = super::Claims {
            sub: "7".into(),
            username: "admin".into(),
            is_admin: true,
            is_owner: false,
            exp: 0,
            iat: 0,
            tv: 0,
        };
        let request = |verified: bool| {
            let mut request = axum::http::Request::builder().body(()).unwrap();
            request.extensions_mut().insert(claims.clone());
            if verified {
                request
                    .extensions_mut()
                    .insert(super::CurrentAdminVerified(()));
            }
            request.into_parts().0
        };
        // Proof present: no database access (the disconnected DB would fail).
        let mut parts = request(true);
        let admin = crate::extract::AdminClaims::from_request_parts(&mut parts, &state)
            .await
            .expect("admin_middleware proof is reused");
        assert_eq!(admin.0.sub, "7");
        // No proof (auth_middleware-only routes): the live admin check still runs.
        let mut parts = request(false);
        let error = crate::extract::AdminClaims::from_request_parts(&mut parts, &state)
            .await
            .expect_err("without the proof the current admin check hits the DB");
        assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn admin_proof_is_inserted_only_after_the_live_admin_check() {
        let src = include_str!("auth.rs");
        let body = src
            .split("pub async fn admin_middleware")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn ensure_current_admin_on").next())
            .expect("admin_middleware");
        let check = body.find("ensure_current_admin_on(").expect("check");
        let proof = body.find("CurrentAdminVerified(())").expect("proof");
        assert!(check < proof);

        let optional = src
            .split("async fn optional_admin_auth(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .expect("optional_admin_auth");
        let check = optional.find("ensure_current_admin_on(").expect("check");
        let proof = optional.find("CurrentAdminVerified(())").expect("proof");
        assert!(check < proof);
        assert_eq!(optional.matches("CurrentAdminVerified(())").count(), 1);
    }
}
