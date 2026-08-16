//! 联邦 Actor 端点（Layer 2）
//!
//! 本地用户的 ActivityPub Actor 表示，以及远程 Actor 获取/缓存。

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Serialize;
use serde_json::json;

use crate::federation::types::*;
use crate::services::image_cache::ImageCacheService;

/// GET /users/{username}
///
/// 返回本地用户的 AP Actor 对象
/// Accept: application/activity+json 时返回 AP JSON
///
/// Domain migration: when `Host` matches a recorded **old** base, the document
/// is served as the old actor with `movedTo`. On the configured (new) base,
/// `alsoKnownAs` lists previous actor ids from `federation_domain_aliases`.
pub async fn get_actor(
    State(db): State<DatabaseConnection>,
    Path(username): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let configured_base = get_base_url().await;
    let aliases = crate::federation::move_actor::load_domain_aliases(&db).await;
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let base_url =
        crate::federation::move_actor::resolve_serve_base(&configured_base, host, &aliases);

    // 查询用户 + 联邦密钥
    let user = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"SELECT u.id, u.username, u.display_name,
                      {avatar} AS avatar_url,
                      u.bio,
                      fk.public_key_pem, fk.key_id
               FROM users u
               LEFT JOIN federation_keys fk ON fk.user_id = u.id
               WHERE u.username = $1
               LIMIT 1"#,
                avatar = crate::services::avatar::avatar_snapshot_expr("u")
            ),
            [username.clone().into()],
        ))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database query failed"})),
            )
        })?;

    let row = user.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "User not found"})),
        )
    })?;

    let user_id: i32 = row.try_get("", "id").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to read user ID"})),
        )
    })?;
    let display_name: Option<String> = row.try_get("", "display_name").ok();
    let avatar_url: Option<String> = row.try_get("", "avatar_url").ok();
    let bio: Option<String> = row.try_get("", "bio").ok();
    let public_key_pem: Option<String> = row.try_get("", "public_key_pem").ok();
    let fetched_key_id: Option<String> = row.try_get("", "key_id").ok();

    // **G shared keys**: one RSA keypair per local user unless explicitly rotated
    // via POST /api/federation/keys/rotate. Domain Move retargets `key_id` host only
    // — never generates a fresh pair here when keys already exist. Same
    // `public_key_pem` is advertised on old and new actor URLs; `keyId` host
    // follows the document we serve (`#main-key`).
    let (pub_key, stored_kid) = match (public_key_pem, fetched_key_id) {
        (Some(pk), Some(ki)) if !pk.trim().is_empty() => (pk, ki),
        (Some(pk), None) if !pk.trim().is_empty() => (pk, key_id(&configured_base, &username)),
        _ => {
            // First-time only: generate once under configured base.
            ensure_user_federation_keys(&db, user_id, &username)
                .await
                .map_err(|e| {
                    tracing::error!(
                        "Failed to generate federation keys for user {}: {}",
                        username,
                        e
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Failed to initialize federation identity"})),
                    )
                })?
        }
    };

    // Prefer stored key_id when it belongs under the **serve base** (domain-move
    // **G** retargets host; same PEM). When Host resolves to the old base after
    // G, stored kid is already the new host — rewrite to `key_id(serve_base, …)`
    // so Move signatures (old origin) verify against the old actor document.
    // Matches `move_actor::local_actor_document_for_base`. Empty column → recompute.
    // 前缀判断而非 `contains`：`contains` 会把 serve base 出现在路径或 query
    // 任意位置的 key id 也认成"属于本站"，例如
    // `https://evil.example/x?u=https://myriad.example`。
    let kid = if key_id_belongs_to_base(&stored_kid, &base_url) {
        stored_kid
    } else {
        key_id(&base_url, &username)
    };

    let actor_id = actor_url(&base_url, &username);
    let (also_known_as, moved_to) =
        crate::federation::move_actor::actor_move_fields(&base_url, &username, &aliases);

    let actor = Actor {
        context: build_context(),
        actor_type: "Person".to_string(),
        id: actor_id.clone(),
        preferred_username: username.clone(),
        name: display_name,
        summary: bio,
        url: Some(format!("{}/profile/{}", get_frontend_url().await, username)),
        inbox: inbox_url(&base_url, &username),
        outbox: outbox_url(&base_url, &username),
        followers: followers_url(&base_url, &username),
        following: following_url(&base_url, &username),
        public_key: ActorPublicKey {
            id: kid,
            owner: actor_id,
            public_key_pem: pub_key,
        },
        icon: avatar_url.map(|_| MediaObject {
            media_type: "Image".to_string(),
            mime_type: None,
            url: avatar_proxy_url(&base_url, &username),
        }),
        image: None,
        // 声明实例级共享收件箱，远端才会把同一条公开活动合并成一次投递
        endpoints: Some(ActorEndpoints {
            shared_inbox: Some(shared_inbox_url(&base_url)),
        }),
        also_known_as,
        moved_to,
        mfp_instance_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        mfp_tapp_capabilities: None,
        mfp_channels_url: Some(format!("{}/users/{}/channels", base_url, username)),
    };

    // Actor 是远端反复拉取的文档：带上 ETag + Cache-Control，
    // 未变更时回 304，有效期内远端根本不会再问。
    Ok(crate::federation::http_cache::public_ap_document(
        &headers,
        AP_CONTENT_TYPE,
        serde_json::to_value(actor).unwrap(),
    ))
}

/// GET /users/{username}/avatar
///
/// Proxies the local user's avatar so federated instances only see this Myriad
/// instance URL, not the upstream OAuth/provider avatar URL.
pub async fn get_avatar(
    State(db): State<DatabaseConnection>,
    Path(username): Path<String>,
) -> Response {
    let avatar_url = match get_local_avatar_url(&db, &username).await {
        Ok(Some(url)) => url,
        Ok(None) => return (StatusCode::NOT_FOUND, "Avatar not found").into_response(),
        Err(response) => return response,
    };

    if avatar_url.len() > 2048 {
        tracing::warn!(
            "Rejected federation avatar proxy for {}: source URL too long",
            username
        );
        return (StatusCode::BAD_REQUEST, "Avatar URL too long").into_response();
    }

    let base_url = get_base_url().await;
    if avatar_url.starts_with(&format!("{}/users/", base_url)) || avatar_url.starts_with("/users/")
    {
        tracing::warn!(
            "Rejected federation avatar proxy loop for {}: {}",
            username,
            avatar_url
        );
        return (StatusCode::BAD_GATEWAY, "Avatar proxy loop rejected").into_response();
    }

    let cache = ImageCacheService::new();
    let cached_path = match cache.cache_image(&avatar_url).await {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!(
                "Failed to cache proxied federation avatar for {}: {}",
                username,
                e
            );
            return (StatusCode::BAD_GATEWAY, "Failed to fetch avatar").into_response();
        }
    };
    let cached_path = cached_path
        .strip_prefix("/api/brew/image-cache")
        .map(|path| format!("/api/federation/avatar-cache{}", path))
        .unwrap_or(cached_path);

    let location = if cached_path.starts_with("http://") || cached_path.starts_with("https://") {
        cached_path
    } else {
        format!("{}{}", base_url, cached_path)
    };

    (
        StatusCode::FOUND,
        [
            (header::LOCATION, location),
            (
                header::CACHE_CONTROL,
                "public, max-age=3600, stale-while-revalidate=86400".to_string(),
            ),
        ],
    )
        .into_response()
}

/// GET /users/{username}/followers
///
/// Followers Collection
pub async fn get_followers(
    State(db): State<DatabaseConnection>,
    Path(username): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // 验证用户存在
    let user = get_local_user(&db, &username).await?;
    let user_id: i32 = user.0;

    // 查询 follower 数量
    let count = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) as count FROM federation_follows WHERE user_id = $1 AND direction = 'incoming' AND status = 'accepted'",
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?
        .map(|r| r.try_get::<i64>("", "count").unwrap_or(0) as u64)
        .unwrap_or(0);

    let collection = relationship_collection(followers_url(&base_url, &username), count);

    Ok(crate::federation::http_cache::public_ap_document(
        &headers,
        AP_CONTENT_TYPE,
        serde_json::to_value(collection).unwrap(),
    ))
}

/// GET /users/{username}/following
///
/// Following Collection
pub async fn get_following(
    State(db): State<DatabaseConnection>,
    Path(username): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let user = get_local_user(&db, &username).await?;
    let user_id: i32 = user.0;

    let count = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) as count FROM federation_follows WHERE user_id = $1 AND direction = 'outgoing' AND status = 'accepted'",
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?
        .map(|r| r.try_get::<i64>("", "count").unwrap_or(0) as u64)
        .unwrap_or(0);

    let collection = relationship_collection(following_url(&base_url, &username), count);

    Ok(crate::federation::http_cache::public_ap_document(
        &headers,
        AP_CONTENT_TYPE,
        serde_json::to_value(collection).unwrap(),
    ))
}

/// Relationship collections intentionally expose only a count for now.  A
/// `first` page is omitted until there is an explicit, privacy-reviewed member
/// enumeration policy and a matching page handler.
fn relationship_collection(id: String, total_items: u64) -> OrderedCollection {
    OrderedCollection {
        context: build_ap_context(),
        collection_type: "OrderedCollection".to_string(),
        id,
        total_items,
        first: None,
        last: None,
    }
}

/// 获取远程 Actor 信息（带缓存）
///
/// 如果缓存过期（>24h），重新从远程获取并写入 DB。
///
/// **Only call after authentication** (signed inbox handlers, outbound follow,
/// room/channel setup). Pre-signature verification must use
/// [`fetch_remote_actor_for_verify`] so a failed/forged request cannot poison
/// `federation_remote_actors` (MYR-022).
pub async fn fetch_remote_actor(
    db: &DatabaseConnection,
    actor_url_str: &str,
) -> Result<RemoteActorInfo, String> {
    fetch_remote_actor_inner(db, actor_url_str, false, true)
        .await
        .map(|r| r.info)
}

/// Force re-fetch remote Actor (ignore 24h cache) and **persist**.
///
/// Prefer [`fetch_remote_actor_for_verify`] + [`persist_verified_remote_actor`]
/// on the inbox auth path so untrusted documents never hit the DB first.
pub async fn fetch_remote_actor_fresh(
    db: &DatabaseConnection,
    actor_url_str: &str,
) -> Result<RemoteActorInfo, String> {
    fetch_remote_actor_inner(db, actor_url_str, true, true)
        .await
        .map(|r| r.info)
}

/// Resolve a remote actor for **HTTP Signature verification only** (MYR-022).
///
/// - May read a fresh row from `federation_remote_actors` (already trusted).
/// - May HTTP-fetch the actor document, but **does not** write to the DB.
/// - Call [`persist_verified_remote_actor`] only after the signature verifies.
///
/// Local same-instance actors still upsert (local DB material is not an
/// unauthenticated remote fetch).
pub async fn fetch_remote_actor_for_verify(
    db: &DatabaseConnection,
    actor_url_str: &str,
    force_refresh: bool,
) -> Result<ResolvedRemoteActor, String> {
    fetch_remote_actor_inner(db, actor_url_str, force_refresh, false).await
}

/// Persist an actor document that was used to successfully verify a signature.
///
/// No-op when `resolved` came from a trusted cache hit (`needs_persist == false`).
pub async fn persist_verified_remote_actor(
    db: &DatabaseConnection,
    resolved: &ResolvedRemoteActor,
) -> Result<RemoteActorInfo, String> {
    if !resolved.needs_persist {
        return Ok(resolved.info.clone());
    }
    let Some(ref doc) = resolved.document else {
        // Local-path resolve already persisted; nothing more to do.
        return Ok(resolved.info.clone());
    };
    upsert_remote_actor_document(db, doc).await
}

/// Outcome of actor resolution for signature verification (MYR-022).
#[derive(Debug, Clone)]
pub struct ResolvedRemoteActor {
    pub info: RemoteActorInfo,
    /// True when `info` (or `document`) came from an unauthenticated remote
    /// HTTP fetch and must not remain the sole authority until signature OK
    /// and [`persist_verified_remote_actor`] runs.
    pub needs_persist: bool,
    /// Full document for upsert after trust. Present only for ephemeral remote
    /// fetches (not cache hits / local actors).
    pub document: Option<RemoteActorDocument>,
}

/// Parsed remote Actor fields ready for DB upsert (ephemeral until verified).
#[derive(Debug, Clone)]
pub struct RemoteActorDocument {
    pub actor_url: String,
    pub username: Option<String>,
    pub domain: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub summary: Option<String>,
    pub inbox_url: String,
    pub outbox_url: Option<String>,
    pub shared_inbox_url: Option<String>,
    pub public_key_pem: Option<String>,
    pub public_key_id: Option<String>,
    pub mfp_version: Option<String>,
}

impl RemoteActorDocument {
    fn to_info(&self, id: i32) -> RemoteActorInfo {
        RemoteActorInfo {
            id,
            actor_url: self.actor_url.clone(),
            username: self.username.clone(),
            domain: self.domain.clone(),
            display_name: self.display_name.clone(),
            avatar_url: self.avatar_url.clone(),
            inbox_url: self.inbox_url.clone(),
            public_key_pem: self.public_key_pem.clone(),
            public_key_id: self.public_key_id.clone(),
            mfp_version: self.mfp_version.clone(),
        }
    }
}

async fn fetch_remote_actor_inner(
    db: &DatabaseConnection,
    actor_url_str: &str,
    force_refresh: bool,
    persist: bool,
) -> Result<ResolvedRemoteActor, String> {
    // 先查本地缓存（除非强制刷新）
    if !force_refresh {
        if let Some(info) = lookup_cached_remote_actor(db, actor_url_str).await? {
            return Ok(ResolvedRemoteActor {
                info,
                needs_persist: false,
                document: None,
            });
        }
    }

    // Same-instance actors: build from local DB (no HTTP). Required for
    // multi-user Follow/Accept on localhost / private base_url — outbound
    // SSRF guards refuse those hosts. Local material is trusted.
    let base_url = get_base_url().await;
    if let Some(local_username) = local_username_from_actor_url(&base_url, actor_url_str) {
        let info =
            upsert_local_actor_as_remote(db, &base_url, &local_username, actor_url_str).await?;
        return Ok(ResolvedRemoteActor {
            info,
            needs_persist: false,
            document: None,
        });
    }

    let doc = fetch_remote_actor_document_http(actor_url_str).await?;

    if persist {
        let info = upsert_remote_actor_document(db, &doc).await?;
        return Ok(ResolvedRemoteActor {
            info,
            needs_persist: false,
            document: None,
        });
    }

    // Ephemeral: PEM available for verify; nothing written to DB yet (MYR-022).
    Ok(ResolvedRemoteActor {
        info: doc.to_info(0),
        needs_persist: true,
        document: Some(doc),
    })
}

async fn lookup_cached_remote_actor(
    db: &DatabaseConnection,
    actor_url_str: &str,
) -> Result<Option<RemoteActorInfo>, String> {
    let cached = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT id, actor_url, username, domain, display_name, avatar_url,
                      inbox_url, public_key_pem, public_key_id, mfp_version, last_fetched_at
               FROM federation_remote_actors
               WHERE actor_url = $1
               LIMIT 1"#,
            [actor_url_str.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            "Database error".to_string()
        })?;

    let Some(row) = cached else {
        return Ok(None);
    };

    let last_fetched: Option<chrono::DateTime<chrono::FixedOffset>> =
        row.try_get("", "last_fetched_at").ok();
    let is_fresh = last_fetched
        .map(|t| chrono::Utc::now().signed_duration_since(t).num_hours() < 24)
        .unwrap_or(false);

    if !is_fresh {
        return Ok(None);
    }

    Ok(Some(RemoteActorInfo {
        id: row.try_get("", "id").unwrap_or(0),
        actor_url: row.try_get("", "actor_url").unwrap_or_default(),
        username: row.try_get("", "username").ok(),
        domain: row.try_get("", "domain").unwrap_or_default(),
        display_name: row.try_get("", "display_name").ok(),
        avatar_url: row
            .try_get::<Option<String>>("", "avatar_url")
            .ok()
            .flatten(),
        inbox_url: row.try_get("", "inbox_url").unwrap_or_default(),
        public_key_pem: row.try_get("", "public_key_pem").ok(),
        public_key_id: row.try_get("", "public_key_id").ok(),
        mfp_version: row.try_get("", "mfp_version").ok(),
    }))
}

/// HTTP-only remote Actor fetch. Never writes to the database.
async fn fetch_remote_actor_document_http(
    actor_url_str: &str,
) -> Result<RemoteActorDocument, String> {
    // SSRF 防护：阻止请求内网地址
    if is_internal_url(actor_url_str) {
        return Err(format!("Refused to fetch internal URL: {}", actor_url_str));
    }

    let user_agent = format!(
        "Myriad/{} (+{})",
        env!("CARGO_PKG_VERSION"),
        get_base_url().await
    );
    let (actor_url, client) = crate::services::outbound_security::build_public_http_client(
        actor_url_str,
        std::time::Duration::from_secs(10),
        Some(&user_agent),
    )
    .await?;

    let resp = client
        .get(actor_url)
        .header("Accept", AP_CONTENT_TYPE)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch remote actor: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Remote actor returned status {}", resp.status()));
    }

    // Actor 文档正常几 KB；1MB 上限防止恶意实例撑爆内存
    let body = crate::services::outbound_security::read_limited_body(resp, 1024 * 1024)
        .await
        .map_err(|e| format!("Failed to read actor response: {}", e))?;
    let actor_json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("Failed to parse actor JSON: {}", e))?;

    // If the document declares an id, it must match the requested actor URL
    // (host case / trailing slash normalized). Prevents cache poisoning via
    // a URL that returns a different actor document.
    if let Some(json_id) = actor_json.get("id").and_then(|v| v.as_str()) {
        if !json_id.is_empty() && !same_actor_url(json_id, actor_url_str) {
            return Err(format!(
                "Remote actor id mismatch: document id '{}' does not match requested '{}'",
                json_id, actor_url_str
            ));
        }
    }

    let domain = extract_domain(actor_url_str).unwrap_or_default();
    let username_val = actor_json["preferredUsername"]
        .as_str()
        .map(|s| s.to_string());
    let display_name = actor_json["name"].as_str().map(|s| s.to_string());
    let avatar_url = extract_actor_icon_url(&actor_json, actor_url_str);
    let summary = actor_json["summary"].as_str().map(|s| s.to_string());
    let remote_inbox = actor_json["inbox"].as_str().unwrap_or("").to_string();
    let outbox = actor_json["outbox"].as_str().map(|s| s.to_string());
    let shared_inbox = actor_json["endpoints"]["sharedInbox"]
        .as_str()
        .map(|s| s.to_string());

    if !remote_inbox.is_empty() && is_internal_url(&remote_inbox) {
        return Err(format!(
            "Remote actor inbox points to internal URL: {}",
            remote_inbox
        ));
    }
    if let Some(ref si) = shared_inbox {
        if is_internal_url(si) {
            return Err(format!(
                "Remote actor shared inbox points to internal URL: {}",
                si
            ));
        }
    }
    let pk_pem = actor_json["publicKey"]["publicKeyPem"]
        .as_str()
        .map(|s| s.to_string());
    let pk_id = actor_json["publicKey"]["id"]
        .as_str()
        .map(|s| s.to_string());
    let mfp_ver = actor_json["myriad:instanceVersion"]
        .as_str()
        .map(|s| s.to_string());

    Ok(RemoteActorDocument {
        actor_url: actor_url_str.to_string(),
        username: username_val,
        domain,
        display_name,
        avatar_url,
        summary,
        inbox_url: remote_inbox,
        outbox_url: outbox,
        shared_inbox_url: shared_inbox,
        public_key_pem: pk_pem,
        public_key_id: pk_id,
        mfp_version: mfp_ver,
    })
}

async fn upsert_remote_actor_document(
    db: &DatabaseConnection,
    doc: &RemoteActorDocument,
) -> Result<RemoteActorInfo, String> {
    let actor_id = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_remote_actors
                   (actor_url, username, domain, display_name, avatar_url, summary,
                    inbox_url, outbox_url, shared_inbox_url,
                    public_key_pem, public_key_id, mfp_version,
                    last_fetched_at, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, NOW(), NOW(), NOW())
               ON CONFLICT (actor_url) DO UPDATE SET
                   username = $2, display_name = $4, avatar_url = $5, summary = $6,
                   inbox_url = $7, outbox_url = $8, shared_inbox_url = $9,
                   public_key_pem = $10, public_key_id = $11, mfp_version = $12,
                   last_fetched_at = NOW(), updated_at = NOW()
               RETURNING id"#,
            [
                doc.actor_url.clone().into(),
                doc.username.clone().into(),
                doc.domain.clone().into(),
                doc.display_name.clone().into(),
                doc.avatar_url.clone().into(),
                doc.summary.clone().into(),
                doc.inbox_url.clone().into(),
                doc.outbox_url.clone().into(),
                doc.shared_inbox_url.clone().into(),
                doc.public_key_pem.clone().into(),
                doc.public_key_id.clone().into(),
                doc.mfp_version.clone().into(),
            ],
        ))
        .await
        .map_err(|e| format!("Failed to cache remote actor: {}", e))?
        .map(|r| r.try_get::<i32>("", "id").unwrap_or(0))
        .unwrap_or(0);

    let _ = upsert_instance(db, &doc.domain, doc.mfp_version.as_deref()).await;

    Ok(doc.to_info(actor_id))
}

/// 远程 Actor 简要信息
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RemoteActorInfo {
    pub id: i32,
    pub actor_url: String,
    pub username: Option<String>,
    pub domain: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub inbox_url: String,
    pub public_key_pem: Option<String>,
    pub public_key_id: Option<String>,
    pub mfp_version: Option<String>,
}

/// Cache a same-instance user as federation_remote_actors without HTTP fetch.
///
/// Enables multi-user Follow/Accept when base_url is localhost or otherwise
/// blocked by outbound SSRF guards.
async fn upsert_local_actor_as_remote(
    db: &DatabaseConnection,
    base_url: &str,
    username: &str,
    actor_url_str: &str,
) -> Result<RemoteActorInfo, String> {
    let user_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"SELECT id, username, display_name,
                      {avatar} AS avatar_url
               FROM users
               WHERE username = $1
               LIMIT 1"#,
                avatar = crate::services::avatar::avatar_snapshot_expr("users")
            ),
            [username.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            "Database error".to_string()
        })?
        .ok_or_else(|| format!("Local user not found: {}", username))?;

    let display_name: Option<String> = user_row
        .try_get::<Option<String>>("", "display_name")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let has_avatar = user_row
        .try_get::<Option<String>>("", "avatar_url")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .is_some();
    let avatar_url = if has_avatar {
        Some(format!(
            "{}/users/{}/avatar",
            base_url.trim_end_matches('/'),
            urlencoding::encode(username)
        ))
    } else {
        None
    };

    let canonical = actor_url(base_url, username);
    let domain = extract_domain(&canonical).unwrap_or_default();
    let inbox = inbox_url(base_url, username);
    let mfp_ver = Some(env!("CARGO_PKG_VERSION").to_string());

    // Prefer the requested URL for cache key stability when it already matches.
    let store_url = if same_actor_url(actor_url_str, &canonical) {
        actor_url_str.to_string()
    } else {
        canonical.clone()
    };

    let actor_id = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_remote_actors
                   (actor_url, username, domain, display_name, avatar_url,
                    inbox_url, mfp_version, last_fetched_at, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW(), NOW())
               ON CONFLICT (actor_url) DO UPDATE SET
                   username = EXCLUDED.username,
                   domain = EXCLUDED.domain,
                   display_name = COALESCE(EXCLUDED.display_name, federation_remote_actors.display_name),
                   avatar_url = COALESCE(EXCLUDED.avatar_url, federation_remote_actors.avatar_url),
                   inbox_url = EXCLUDED.inbox_url,
                   mfp_version = EXCLUDED.mfp_version,
                   last_fetched_at = NOW(),
                   updated_at = NOW()
               RETURNING id"#,
            [
                store_url.clone().into(),
                username.to_string().into(),
                domain.clone().into(),
                display_name.clone().into(),
                avatar_url.clone().into(),
                inbox.clone().into(),
                mfp_ver.clone().into(),
            ],
        ))
        .await
        .map_err(|e| format!("Failed to cache local actor: {}", e))?
        .map(|r| r.try_get::<i32>("", "id").unwrap_or(0))
        .unwrap_or(0);

    Ok(RemoteActorInfo {
        id: actor_id,
        actor_url: store_url,
        username: Some(username.to_string()),
        domain,
        display_name,
        avatar_url,
        inbox_url: inbox,
        public_key_pem: None,
        public_key_id: None,
        mfp_version: mfp_ver,
    })
}

/// Extract avatar URL from ActivityPub Actor `icon` field.
/// Supports object, array of objects, and bare string forms.
/// Relative paths are resolved against the actor URL origin.
fn extract_actor_icon_url(actor_json: &serde_json::Value, actor_url_str: &str) -> Option<String> {
    let icon = &actor_json["icon"];
    if icon.is_null() {
        return None;
    }

    let raw = icon
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            icon["url"]
                .as_str()
                .or_else(|| icon["href"].as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            icon.as_array().and_then(|arr| {
                arr.iter().find_map(|item| {
                    item.as_str()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            item["url"]
                                .as_str()
                                .or_else(|| item["href"].as_str())
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(|s| s.to_string())
                        })
                })
            })
        })?;

    Some(resolve_media_url(actor_url_str, &raw))
}

/// Resolve a media URL that may be absolute or relative to the actor document.
fn resolve_media_url(actor_url_str: &str, media_url: &str) -> String {
    let media_url = media_url.trim();
    if media_url.starts_with("https://") || media_url.starts_with("http://") {
        return media_url.to_string();
    }
    if let Ok(base) = url::Url::parse(actor_url_str) {
        if let Ok(joined) = base.join(media_url) {
            return joined.to_string();
        }
    }
    media_url.to_string()
}

/// 本地登录用户的联邦身份摘要。
#[derive(Debug, Clone, Serialize)]
pub struct LocalFederationIdentity {
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub domain: String,
    pub handle: String,
    pub acct: String,
    pub webfinger_resource: String,
    pub actor_url: String,
    pub inbox_url: String,
    pub outbox_url: String,
    pub followers_url: String,
    pub following_url: String,
    pub profile_url: String,
}

/// 构造当前用户可分享给远端的联邦身份。
///
/// Also ensures federation keys exist so Aro opening federation settings (or any
/// client calling `GET /api/federation/identity`) initializes signing material
/// before the first outbound delivery.
pub async fn get_local_identity(
    db: &DatabaseConnection,
    username: &str,
) -> LocalFederationIdentity {
    let base_url = get_base_url().await;
    let frontend_url = get_frontend_url().await;
    let domain = extract_domain(&base_url).unwrap_or_else(|| base_url.clone());
    let acct = format!("{}@{}", username, domain);
    let mut display_name: Option<String> = None;
    let mut avatar_url: Option<String> = None;

    if let Ok(Some(row)) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"SELECT id, display_name,
                      {avatar} AS avatar_url
               FROM users
               WHERE username = $1
               LIMIT 1"#,
                avatar = crate::services::avatar::avatar_snapshot_expr("users")
            ),
            [username.to_string().into()],
        ))
        .await
    {
        let user_id: i32 = row.try_get("", "id").unwrap_or(0);
        display_name = row.try_get("", "display_name").ok();
        avatar_url = row.try_get("", "avatar_url").ok();
        if user_id > 0 {
            if let Err(e) = ensure_user_federation_keys(db, user_id, username).await {
                tracing::warn!(
                    user_id = user_id,
                    username = %username,
                    error = %e,
                    "Failed to ensure federation keys on identity lookup"
                );
            }
        }
    }

    LocalFederationIdentity {
        username: username.to_string(),
        display_name,
        avatar_url: avatar_url.map(|_| avatar_proxy_url(&base_url, username)),
        domain: domain.clone(),
        handle: format!("@{}", acct),
        acct: acct.clone(),
        webfinger_resource: format!("acct:{}", acct),
        actor_url: actor_url(&base_url, username),
        inbox_url: inbox_url(&base_url, username),
        outbox_url: outbox_url(&base_url, username),
        followers_url: followers_url(&base_url, username),
        following_url: following_url(&base_url, username),
        profile_url: format!("{}/profile/{}", frontend_url, username),
    }
}

fn avatar_proxy_url(base_url: &str, username: &str) -> String {
    format!(
        "{}/users/{}/avatar",
        base_url,
        urlencoding::encode(username)
    )
}

async fn get_local_avatar_url(
    db: &DatabaseConnection,
    username: &str,
) -> Result<Option<String>, Response> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"SELECT {avatar} AS avatar_url
               FROM users
               WHERE username = $1
               LIMIT 1"#,
                avatar = crate::services::avatar::avatar_snapshot_expr("users")
            ),
            [username.to_string().into()],
        ))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database query failed"})),
            )
                .into_response()
        })?;

    Ok(row.and_then(|r| r.try_get::<Option<String>>("", "avatar_url").ok().flatten()))
}

// 辅助函数

/// Whether stored public key material is missing or empty and needs generation.
///
/// Existing non-empty PEMs must never be rotated by ensure/generate paths.
pub(crate) fn needs_federation_key_generation(public_key_pem: Option<&str>) -> bool {
    match public_key_pem {
        Some(pk) => pk.trim().is_empty(),
        None => true,
    }
}

/// Ensure the local user has a federation keypair.
///
/// Generates and stores keys only when missing or empty. Never rotates an
/// existing non-empty keypair (actor id / username unchanged).
///
/// Call sites (defense-in-depth):
/// - `GET /api/federation/identity` (`get_local_identity`)
/// - `GET /users/{username}` actor document
/// - Room fan-out, ring `add_peer`, follow outbound
///
/// **Universal recovery:** `delivery::load_user_keypair_ensuring` also ensures
/// once before sign so every `federation_delivery_queue` producer (room, ring,
/// follow, content, channel, inbox, interactions, file_transfer) recovers
/// already-queued jobs without patching each INSERT.
///
/// Returns `(public_key_pem, key_id)`.
pub async fn ensure_user_federation_keys(
    db: &impl ConnectionTrait,
    user_id: i32,
    username: &str,
) -> Result<(String, String), String> {
    if let Some((pub_pem, kid)) = load_stored_federation_keys(db, user_id).await? {
        if !needs_federation_key_generation(Some(&pub_pem)) {
            let base_url = get_base_url().await;
            let resolved_kid = if kid.trim().is_empty() {
                key_id(&base_url, username)
            } else {
                kid
            };
            return Ok((pub_pem, resolved_kid));
        }
    }

    let base_url = get_base_url().await;
    generate_and_store_keys(db, user_id, &base_url, username).await
}

/// Explicit key rotation result (username / actor id unchanged).
#[derive(Debug, Clone, serde::Serialize)]
pub struct FederationKeyRotationResult {
    pub public_key_pem: String,
    pub key_id: String,
    pub previous_public_key_pem: Option<String>,
    /// Rows enqueued for Update(Person) fan-out (0 if no followers or enqueue fail)
    pub update_queued: u32,
    /// Peers should re-fetch `GET /users/{username}` for the new publicKey.
    pub note: String,
}

/// Rotate federation signing keys for a local user.
///
/// **Explicit only** — never called from `ensure_user_federation_keys`.
/// Generates a new RSA keypair, overwrites `federation_keys` (same
/// `user_id` / key_id path pattern), and best-effort fans out an
/// ActivityPub `Update` of the Person actor so followers can refresh.
///
/// Actor URL and username are **not** changed. Remote peers that miss the
/// Update can still re-fetch the actor document.
pub async fn rotate_user_federation_keys(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
) -> Result<FederationKeyRotationResult, String> {
    if username.trim().is_empty() || user_id <= 0 {
        return Err("user_id and username required for key rotation".into());
    }

    let previous = load_stored_federation_keys(db, user_id)
        .await?
        .map(|(pem, _)| pem)
        .filter(|p| !p.trim().is_empty());

    let base_url = get_base_url().await;
    let (pub_pem, kid) = force_store_new_keys(db, user_id, &base_url, username).await?;

    // Best-effort Update(Person) so followers learn the new publicKey.
    let update_queued =
        broadcast_person_key_update(db, user_id, username, &base_url, &pub_pem, &kid)
            .await
            .unwrap_or(0);

    Ok(FederationKeyRotationResult {
        public_key_pem: pub_pem,
        key_id: kid,
        previous_public_key_pem: previous,
        update_queued,
        note: "Actor publicKey rotated. Peers should re-fetch GET /users/{username} if they miss Update(Person).".into(),
    })
}

/// Force-overwrite key material (used only by explicit rotate).
async fn force_store_new_keys(
    db: &DatabaseConnection,
    user_id: i32,
    base_url: &str,
    username: &str,
) -> Result<(String, String), String> {
    let keypair = crate::federation::keys::KeyPair::generate()
        .map_err(|e| format!("Key generation failed: {}", e))?;

    let pub_pem = keypair
        .public_key_pem()
        .map_err(|e| format!("PEM encoding failed: {}", e))?;

    // 新私钥直接用 v1 信封（数据密钥），不再绑定 JWT_SECRET
    let encrypted = keypair
        .encrypt_private_key()
        .map_err(|e| format!("Key encryption failed: {}", e))?;

    let kid = key_id(base_url, username);

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_keys (user_id, public_key_pem, private_key_encrypted, key_id, algorithm, created_at, rotated_at)
           VALUES ($1, $2, $3, $4, 'RSA-SHA256', NOW(), NOW())
           ON CONFLICT (user_id) DO UPDATE SET
               public_key_pem = EXCLUDED.public_key_pem,
               private_key_encrypted = EXCLUDED.private_key_encrypted,
               key_id = EXCLUDED.key_id,
               rotated_at = NOW()"#,
        [
            user_id.into(),
            pub_pem.clone().into(),
            encrypted.into(),
            kid.clone().into(),
        ],
    ))
    .await
    .map_err(|e| format!("Failed to store rotated keys: {}", e))?;

    // Concurrent rotates: last writer wins. Return DB winner so Update(Person)
    // and the API response match what peers will fetch / what delivery signs with.
    match load_stored_federation_keys(db, user_id).await? {
        Some((stored_pem, stored_kid)) if !needs_federation_key_generation(Some(&stored_pem)) => {
            let resolved_kid = if stored_kid.trim().is_empty() {
                kid
            } else {
                stored_kid
            };
            Ok((stored_pem, resolved_kid))
        }
        _ => Ok((pub_pem, kid)),
    }
}

/// Enqueue Update(Person) to incoming followers with the new publicKey.
async fn broadcast_person_key_update(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    base_url: &str,
    public_key_pem: &str,
    kid: &str,
) -> Result<u32, String> {
    let actor_id = actor_url(base_url, username);
    let activity_id = generate_activity_id(base_url);
    let update = json!({
        "@context": build_context(),
        "type": "Update",
        "id": &activity_id,
        "actor": &actor_id,
        "to": [crate::federation::types::AP_PUBLIC, followers_url(base_url, username)],
        "object": {
            "type": "Person",
            "id": &actor_id,
            "preferredUsername": username,
            "inbox": inbox_url(base_url, username),
            "outbox": outbox_url(base_url, username),
            "followers": followers_url(base_url, username),
            "following": following_url(base_url, username),
            "publicKey": {
                "id": kid,
                "owner": &actor_id,
                "publicKeyPem": public_key_pem,
            }
        }
    });

    let act_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
               (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Update', 'Person', $3, true, NOW())
               RETURNING id"#,
            [
                activity_id.into(),
                user_id.into(),
                update.clone().into(),
            ],
        ))
        .await
        .map_err(|e| format!("Failed to record Update activity: {}", e))?;

    let act_db_id: i32 = act_row.and_then(|r| r.try_get("", "id").ok()).unwrap_or(0);
    if act_db_id <= 0 {
        return Ok(0);
    }

    let queued =
        crate::federation::content::fan_out_to_followers(db, user_id, act_db_id, &update).await;
    tracing::info!(
        user_id = user_id,
        username = %username,
        queued = queued,
        "Broadcast Update(Person) after key rotation"
    );
    Ok(queued)
}

async fn load_stored_federation_keys(
    db: &impl ConnectionTrait,
    user_id: i32,
) -> Result<Option<(String, String)>, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT public_key_pem, key_id FROM federation_keys WHERE user_id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error loading federation keys: {}", e);
            "Database error".to_string()
        })?;

    Ok(row.map(|r| {
        let pub_pem: String = r.try_get("", "public_key_pem").unwrap_or_default();
        let kid: String = r.try_get("", "key_id").unwrap_or_default();
        (pub_pem, kid)
    }))
}

/// Generate a new keypair and store it. Only overwrites on conflict when the
/// existing row has an empty public key (no rotation of live keys).
async fn generate_and_store_keys(
    db: &impl ConnectionTrait,
    user_id: i32,
    base_url: &str,
    username: &str,
) -> Result<(String, String), String> {
    let keypair = crate::federation::keys::KeyPair::generate()
        .map_err(|e| format!("Key generation failed: {}", e))?;

    let pub_pem = keypair
        .public_key_pem()
        .map_err(|e| format!("PEM encoding failed: {}", e))?;

    // 新私钥直接用 v1 信封（数据密钥），不再绑定 JWT_SECRET
    let encrypted = keypair
        .encrypt_private_key()
        .map_err(|e| format!("Key encryption failed: {}", e))?;

    let kid = key_id(base_url, username);

    // Same ON CONFLICT shape as before, but only apply the update when the
    // stored public key is missing/empty so concurrent ensures cannot rotate.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_keys (user_id, public_key_pem, private_key_encrypted, key_id, algorithm, created_at)
           VALUES ($1, $2, $3, $4, 'RSA-SHA256', NOW())
           ON CONFLICT (user_id) DO UPDATE SET
               public_key_pem = EXCLUDED.public_key_pem,
               private_key_encrypted = EXCLUDED.private_key_encrypted,
               key_id = EXCLUDED.key_id,
               rotated_at = NOW()
           WHERE TRIM(COALESCE(federation_keys.public_key_pem, '')) = ''"#,
        [
            user_id.into(),
            pub_pem.clone().into(),
            encrypted.into(),
            kid.clone().into(),
        ],
    ))
    .await
    .map_err(|e| format!("Failed to store keys: {}", e))?;

    // Re-read so a concurrent winner's keys are returned instead of our discarded pair.
    match load_stored_federation_keys(db, user_id).await? {
        Some((stored_pem, stored_kid)) if !needs_federation_key_generation(Some(&stored_pem)) => {
            let resolved_kid = if stored_kid.trim().is_empty() {
                kid
            } else {
                stored_kid
            };
            Ok((stored_pem, resolved_kid))
        }
        _ => Ok((pub_pem, kid)),
    }
}

/// Upsert 实例信息
async fn upsert_instance(
    db: &DatabaseConnection,
    domain: &str,
    mfp_version: Option<&str>,
) -> Result<(), String> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_instances (domain, mfp_version, trust_level, last_seen_at, created_at)
           VALUES ($1, $2, 1, NOW(), NOW())
           ON CONFLICT (domain) DO UPDATE SET
               mfp_version = COALESCE($2, federation_instances.mfp_version),
               last_seen_at = NOW(), updated_at = NOW()"#,
        [domain.into(), mfp_version.into()],
    ))
    .await
    .map_err(|e| format!("Failed to upsert instance: {}", e))?;
    Ok(())
}

/// 获取本地用户 (id, username)
async fn get_local_user(
    db: &DatabaseConnection,
    username: &str,
) -> Result<(i32, String), (StatusCode, Json<serde_json::Value>)> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, username FROM users WHERE username = $1 LIMIT 1",
            [username.into()],
        ))
        .await
        .map_err(db_err)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "User not found"})),
            )
        })?;

    Ok((
        row.try_get("", "id").unwrap_or(0),
        row.try_get("", "username").unwrap_or_default(),
    ))
}

async fn get_base_url() -> String {
    crate::federation::types::get_base_url().await
}

async fn get_frontend_url() -> String {
    let config = crate::GLOBAL_CONFIG.read().await;
    let frontend_url = config.frontend_url.clone().unwrap_or_else(|| {
        config
            .base_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", config.server_host, config.server_port))
    });
    frontend_url.trim_end_matches('/').to_string()
}

/// Shared confirm gate for POST /api/federation/keys/rotate (and unit tests).
///
/// Must live above `mod tests` (clippy `items_after_test_module`).
pub fn rotation_confirm_accepted(body: &serde_json::Value) -> bool {
    body.get("confirm").and_then(|v| v.as_bool()) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_actor_document_to_info_is_ephemeral_until_id_assigned() {
        // MYR-022: HTTP-fetched documents start with id=0 and must not be treated
        // as a trusted cache row until persist_verified_remote_actor assigns an id.
        let doc = RemoteActorDocument {
            actor_url: "https://peer.example/users/alice".into(),
            username: Some("alice".into()),
            domain: "peer.example".into(),
            display_name: Some("Alice".into()),
            avatar_url: None,
            summary: Some("hi".into()),
            inbox_url: "https://peer.example/users/alice/inbox".into(),
            outbox_url: Some("https://peer.example/users/alice/outbox".into()),
            shared_inbox_url: Some("https://peer.example/inbox".into()),
            public_key_pem: Some(
                "-----BEGIN PUBLIC KEY-----\nMIIB\n-----END PUBLIC KEY-----\n".into(),
            ),
            public_key_id: Some("https://peer.example/users/alice#main-key".into()),
            mfp_version: Some("0.3.30".into()),
        };
        let ephemeral = doc.to_info(0);
        assert_eq!(ephemeral.id, 0);
        assert_eq!(ephemeral.actor_url, doc.actor_url);
        assert!(ephemeral.public_key_pem.is_some());
        let trusted = doc.to_info(42);
        assert_eq!(trusted.id, 42);
        assert_eq!(trusted.public_key_id, doc.public_key_id);
    }

    #[test]
    fn resolved_remote_actor_needs_persist_flag_semantics() {
        // Cache hits / local upserts: needs_persist=false, no document.
        // Ephemeral HTTP: needs_persist=true + document for later upsert.
        let cached = ResolvedRemoteActor {
            info: RemoteActorInfo {
                id: 7,
                actor_url: "https://peer.example/users/bob".into(),
                username: Some("bob".into()),
                domain: "peer.example".into(),
                display_name: None,
                avatar_url: None,
                inbox_url: "https://peer.example/users/bob/inbox".into(),
                public_key_pem: Some("PEM".into()),
                public_key_id: Some("https://peer.example/users/bob#main-key".into()),
                mfp_version: None,
            },
            needs_persist: false,
            document: None,
        };
        assert!(!cached.needs_persist);
        assert!(cached.document.is_none());

        let doc = RemoteActorDocument {
            actor_url: "https://evil.example/users/mallory".into(),
            username: Some("mallory".into()),
            domain: "evil.example".into(),
            display_name: None,
            avatar_url: None,
            summary: None,
            inbox_url: "https://evil.example/users/mallory/inbox".into(),
            outbox_url: None,
            shared_inbox_url: None,
            public_key_pem: Some("ATTACKER-PEM".into()),
            public_key_id: Some("https://evil.example/users/mallory#main-key".into()),
            mfp_version: None,
        };
        let ephemeral = ResolvedRemoteActor {
            info: doc.to_info(0),
            needs_persist: true,
            document: Some(doc),
        };
        assert!(ephemeral.needs_persist);
        assert!(ephemeral.document.is_some());
        assert_eq!(ephemeral.info.id, 0);
    }

    #[test]
    fn needs_generation_when_missing() {
        assert!(needs_federation_key_generation(None));
    }

    #[test]
    fn needs_generation_when_empty_or_whitespace() {
        assert!(needs_federation_key_generation(Some("")));
        assert!(needs_federation_key_generation(Some("   \n\t  ")));
    }

    #[test]
    fn no_generation_when_pem_present() {
        let pem = "-----BEGIN PUBLIC KEY-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A\n-----END PUBLIC KEY-----\n";
        assert!(!needs_federation_key_generation(Some(pem)));
        // Non-empty material must not be treated as missing (no rotate).
        assert!(!needs_federation_key_generation(Some("not-empty-pem")));
    }

    #[test]
    fn ensure_never_treats_live_pem_as_missing() {
        // Contract: ensure_user_federation_keys must not call force_store when
        // needs_federation_key_generation is false. Rotation is explicit only.
        let live = "-----BEGIN PUBLIC KEY-----\nEXISTING\n-----END PUBLIC KEY-----\n";
        assert!(!needs_federation_key_generation(Some(live)));
        assert!(!needs_federation_key_generation(Some("x")));
    }

    #[test]
    fn key_rotation_confirm_gate() {
        // API body must pass confirm:true; pure gate used by the handler.
        assert!(!rotation_confirm_accepted(&serde_json::json!({})));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": false})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": "yes"})
        ));
        assert!(rotation_confirm_accepted(
            &serde_json::json!({"confirm": true})
        ));
        // Nested / wrong-type must never pass (bridge/UI mistakes).
        assert!(!rotation_confirm_accepted(&serde_json::json!({
            "confirm": {"nested": true}
        })));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": 1})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": null})
        ));
        // Extra fields OK as long as confirm:true is present.
        assert!(rotation_confirm_accepted(&serde_json::json!({
            "confirm": true,
            "reason": "compromised laptop"
        })));
    }

    #[test]
    fn key_rotation_confirm_rejects_truthy_non_bool() {
        // JSON numbers / null / missing nested keys must not open rotate.
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": 1})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": null})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": "true"})
        ));
        assert!(!rotation_confirm_accepted(&serde_json::json!({"ok": true})));
    }

    #[test]
    fn needs_generation_rejects_whitespace_only_pem() {
        assert!(needs_federation_key_generation(Some("\n\n")));
        assert!(needs_federation_key_generation(Some(" \t")));
        // Any non-whitespace material is treated as present (no silent rotate).
        assert!(!needs_federation_key_generation(Some("BEGIN")));
    }

    #[test]
    fn rotation_confirm_requires_boolean_true_only() {
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": [true]})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": {"ok": true}})
        ));
        assert!(rotation_confirm_accepted(
            &serde_json::json!({"confirm": true, "extra": 1})
        ));
    }

    #[test]
    fn rotation_confirm_accepted_only_true_bool() {
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": null})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": 1})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": "true"})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"Confirm": true})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": [true]})
        ));
        assert!(rotation_confirm_accepted(
            &serde_json::json!({"confirm": true, "extra": 1})
        ));
    }

    #[test]
    fn needs_federation_key_generation_boundary() {
        // Single non-whitespace char is present material → no generate.
        assert!(!needs_federation_key_generation(Some(".")));
        assert!(!needs_federation_key_generation(Some("0")));
        // Only whitespace → needs generation
        assert!(needs_federation_key_generation(Some("\r\n  ")));
        assert!(needs_federation_key_generation(None));
    }

    #[test]
    fn r34_rotation_confirm_accepted_requires_json_true() {
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": 0})
        ));
        assert!(!rotation_confirm_accepted(
            &serde_json::json!({"confirm": "true"})
        ));
        assert!(!rotation_confirm_accepted(&serde_json::json!({"ok": true})));
        assert!(rotation_confirm_accepted(
            &serde_json::json!({"confirm": true})
        ));
    }

    #[test]
    fn r35_rotation_confirm_accepted_ignores_nested_confirm() {
        assert!(!rotation_confirm_accepted(&serde_json::json!({
            "body": {"confirm": true}
        })));
    }

    #[test]
    fn r36_needs_federation_key_generation_whitespace_only() {
        assert!(needs_federation_key_generation(Some("\t\t")));
        assert!(!needs_federation_key_generation(Some("pem-bytes")));
    }

    #[test]
    fn relationship_collection_does_not_advertise_a_fake_first_page() {
        let followers =
            relationship_collection("https://example.test/users/alice/followers".into(), 3);
        let following =
            relationship_collection("https://example.test/users/alice/following".into(), 2);

        assert_eq!(followers.total_items, 3);
        assert_eq!(following.total_items, 2);
        assert!(followers.first.is_none());
        assert!(following.first.is_none());
        assert!(serde_json::to_value(followers)
            .unwrap()
            .get("first")
            .is_none());
    }
}
