//! 联邦 Actor 端点（Layer 2）
//!
//! 本地用户的 ActivityPub Actor 表示，以及远程 Actor 获取/缓存。

use axum::{
    extract::Path,
    http::{header, StatusCode},
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
pub async fn get_actor(
    Path(username): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let db = get_db()
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))))?;

    let base_url = get_base_url().await;

    // 查询用户 + 联邦密钥
    let user = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT u.id, u.username, u.display_name,
                      COALESCE(
                          NULLIF(
                              CASE
                                  WHEN u.avatar_url LIKE 'https://ui-avatars.com/%'
                                       OR u.avatar_url LIKE 'http://ui-avatars.com/%'
                                  THEN NULL
                                  ELSE u.avatar_url
                              END,
                              ''
                          ),
                          (
                              SELECT NULLIF(ui.avatar_url, '')
                              FROM user_identities ui
                              WHERE ui.user_id = u.id
                                AND ui.avatar_url IS NOT NULL
                                AND ui.avatar_url <> ''
                              ORDER BY ui.is_primary DESC, ui.last_login_at DESC NULLS LAST, ui.linked_at DESC
                              LIMIT 1
                          ),
                          NULLIF(u.avatar_url, '')
                      ) AS avatar_url,
                      u.bio,
                      fk.public_key_pem, fk.key_id
               FROM users u
               LEFT JOIN federation_keys fk ON fk.user_id = u.id
               WHERE u.username = $1
               LIMIT 1"#,
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

    // 如果用户还没有联邦密钥，自动生成
    let (pub_key, kid) = match (public_key_pem, fetched_key_id) {
        (Some(pk), Some(ki)) => (pk, ki),
        _ => {
            // 自动为该用户生成密钥对
            generate_and_store_keys(&db, user_id, &base_url, &username)
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

    let actor_id = actor_url(&base_url, &username);
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
        mfp_instance_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        mfp_tapp_capabilities: None,
        mfp_channels_url: Some(format!("{}/users/{}/channels", base_url, username)),
    };

    Ok((StatusCode::OK, Json(serde_json::to_value(actor).unwrap())))
}

/// GET /users/{username}/avatar
///
/// Proxies the local user's avatar so federated instances only see this Myriad
/// instance URL, not the upstream OAuth/provider avatar URL.
pub async fn get_avatar(Path(username): Path<String>) -> Response {
    let db = match get_db().await {
        Ok(db) => db,
        Err(e) => {
            return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))).into_response()
        }
    };

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
    Path(username): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let db = get_db()
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))))?;
    let base_url = get_base_url().await;

    // 验证用户存在
    let user = get_local_user(&db, &username).await?;
    let user_id: i32 = user.0;

    // 查询 follower 数量
    let count = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) as count FROM federation_follows WHERE user_id = $1 AND direction = 'incoming' AND status = 'accepted'",
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?
        .map(|r| r.try_get::<i64>("", "count").unwrap_or(0) as u64)
        .unwrap_or(0);

    let collection = OrderedCollection {
        context: build_ap_context(),
        collection_type: "OrderedCollection".to_string(),
        id: followers_url(&base_url, &username),
        total_items: count,
        first: if count > 0 {
            Some(format!("{}/users/{}/followers?page=1", base_url, username))
        } else {
            None
        },
        last: None,
    };

    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(collection).unwrap()),
    ))
}

/// GET /users/{username}/following
///
/// Following Collection
pub async fn get_following(
    Path(username): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let db = get_db()
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": e}))))?;
    let base_url = get_base_url().await;

    let user = get_local_user(&db, &username).await?;
    let user_id: i32 = user.0;

    let count = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) as count FROM federation_follows WHERE user_id = $1 AND direction = 'outgoing' AND status = 'accepted'",
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?
        .map(|r| r.try_get::<i64>("", "count").unwrap_or(0) as u64)
        .unwrap_or(0);

    let collection = OrderedCollection {
        context: build_ap_context(),
        collection_type: "OrderedCollection".to_string(),
        id: following_url(&base_url, &username),
        total_items: count,
        first: if count > 0 {
            Some(format!("{}/users/{}/following?page=1", base_url, username))
        } else {
            None
        },
        last: None,
    };

    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(collection).unwrap()),
    ))
}

/// 获取远程 Actor 信息（带缓存）
///
/// 如果缓存过期（>24h），重新从远程获取
pub async fn fetch_remote_actor(
    db: &DatabaseConnection,
    actor_url_str: &str,
) -> Result<RemoteActorInfo, String> {
    // 先查本地缓存
    let cached = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT id, actor_url, username, domain, display_name, avatar_url,
                      inbox_url, public_key_pem, public_key_id, mfp_version, last_fetched_at
               FROM federation_remote_actors
               WHERE actor_url = $1
               LIMIT 1"#,
            [actor_url_str.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?;

    if let Some(row) = cached {
        let last_fetched: Option<chrono::DateTime<chrono::FixedOffset>> =
            row.try_get("", "last_fetched_at").ok();
        let is_fresh = last_fetched
            .map(|t| chrono::Utc::now().signed_duration_since(t).num_hours() < 24)
            .unwrap_or(false);

        if is_fresh {
            return Ok(RemoteActorInfo {
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
            });
        }
    }

    // 从远程获取 Actor JSON
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
    let actor_json: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| format!("Failed to parse actor JSON: {}", e))?;

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

    // 提取关键字段
    let domain = extract_domain(actor_url_str).unwrap_or_default();
    let username_val = actor_json["preferredUsername"]
        .as_str()
        .map(|s| s.to_string());
    let display_name = actor_json["name"].as_str().map(|s| s.to_string());
    // ActivityPub icon may be an object, an array of objects, or a bare URL string
    let avatar_url = extract_actor_icon_url(&actor_json, actor_url_str);
    let summary = actor_json["summary"].as_str().map(|s| s.to_string());
    let remote_inbox = actor_json["inbox"].as_str().unwrap_or("").to_string();
    let outbox = actor_json["outbox"].as_str().map(|s| s.to_string());
    let shared_inbox = actor_json["endpoints"]["sharedInbox"]
        .as_str()
        .map(|s| s.to_string());

    // 验证 inbox URL 不指向内网（防止 SSRF 通过伪造 inbox）
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

    // Upsert 到缓存
    let actor_id = db
        .query_one(Statement::from_sql_and_values(
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
                actor_url_str.into(),
                username_val.clone().into(),
                domain.clone().into(),
                display_name.clone().into(),
                avatar_url.clone().into(),
                summary.into(),
                remote_inbox.clone().into(),
                outbox.into(),
                shared_inbox.into(),
                pk_pem.clone().into(),
                pk_id.clone().into(),
                mfp_ver.clone().into(),
            ],
        ))
        .await
        .map_err(|e| format!("Failed to cache remote actor: {}", e))?
        .map(|r| r.try_get::<i32>("", "id").unwrap_or(0))
        .unwrap_or(0);

    // 同时更新/创建实例记录
    let _ = upsert_instance(db, &domain, mfp_ver.as_deref()).await;

    Ok(RemoteActorInfo {
        id: actor_id,
        actor_url: actor_url_str.to_string(),
        username: username_val,
        domain,
        display_name,
        avatar_url,
        inbox_url: remote_inbox,
        public_key_pem: pk_pem,
        public_key_id: pk_id,
        mfp_version: mfp_ver,
    })
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
pub async fn get_local_identity(username: &str) -> LocalFederationIdentity {
    let base_url = get_base_url().await;
    let frontend_url = get_frontend_url().await;
    let domain = extract_domain(&base_url).unwrap_or_else(|| base_url.clone());
    let acct = format!("{}@{}", username, domain);
    let mut display_name: Option<String> = None;
    let mut avatar_url: Option<String> = None;

    if let Ok(db) = get_db().await {
        if let Ok(Some(row)) = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT display_name,
                          COALESCE(
                              NULLIF(
                                  CASE
                                      WHEN avatar_url LIKE 'https://ui-avatars.com/%'
                                           OR avatar_url LIKE 'http://ui-avatars.com/%'
                                      THEN NULL
                                      ELSE avatar_url
                                  END,
                                  ''
                              ),
                              (
                                  SELECT NULLIF(ui.avatar_url, '')
                                  FROM user_identities ui
                                  WHERE ui.user_id = users.id
                                    AND ui.avatar_url IS NOT NULL
                                    AND ui.avatar_url <> ''
                                  ORDER BY ui.is_primary DESC, ui.last_login_at DESC NULLS LAST, ui.linked_at DESC
                                  LIMIT 1
                              ),
                              NULLIF(avatar_url, '')
                          ) AS avatar_url
                   FROM users
                   WHERE username = $1
                   LIMIT 1"#,
                [username.to_string().into()],
            ))
            .await
        {
            display_name = row.try_get("", "display_name").ok();
            avatar_url = row.try_get("", "avatar_url").ok();
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
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT COALESCE(
                      NULLIF(
                          CASE
                              WHEN avatar_url LIKE 'https://ui-avatars.com/%'
                                   OR avatar_url LIKE 'http://ui-avatars.com/%'
                              THEN NULL
                              ELSE avatar_url
                          END,
                          ''
                      ),
                      (
                          SELECT NULLIF(ui.avatar_url, '')
                          FROM user_identities ui
                          WHERE ui.user_id = users.id
                            AND ui.avatar_url IS NOT NULL
                            AND ui.avatar_url <> ''
                          ORDER BY ui.is_primary DESC, ui.last_login_at DESC NULLS LAST, ui.linked_at DESC
                          LIMIT 1
                      ),
                      NULLIF(avatar_url, '')
                  ) AS avatar_url
               FROM users
               WHERE username = $1
               LIMIT 1"#,
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

// ==================== 辅助函数 ====================

/// 为用户生成联邦密钥并存库
async fn generate_and_store_keys(
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

    let jwt_secret = {
        let config = crate::GLOBAL_CONFIG.read().await;
        config.jwt_secret.clone()
    };

    let encrypted = keypair
        .encrypt_private_key(&jwt_secret)
        .map_err(|e| format!("Key encryption failed: {}", e))?;

    let kid = key_id(base_url, username);

    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_keys (user_id, public_key_pem, private_key_encrypted, key_id, algorithm, created_at)
           VALUES ($1, $2, $3, $4, 'RSA-SHA256', NOW())
           ON CONFLICT (user_id) DO UPDATE SET
               public_key_pem = $2, private_key_encrypted = $3, key_id = $4, rotated_at = NOW()"#,
        [
            user_id.into(),
            pub_pem.clone().into(),
            encrypted.into(),
            kid.clone().into(),
        ],
    ))
    .await
    .map_err(|e| format!("Failed to store keys: {}", e))?;

    Ok((pub_pem, kid))
}

/// Upsert 实例信息
async fn upsert_instance(
    db: &DatabaseConnection,
    domain: &str,
    mfp_version: Option<&str>,
) -> Result<(), String> {
    db.execute(Statement::from_sql_and_values(
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
        .query_one(Statement::from_sql_and_values(
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

async fn get_db() -> Result<DatabaseConnection, String> {
    let db_opt = crate::DB_CONNECTION.read().await;
    db_opt
        .clone()
        .ok_or_else(|| "Database not connected".to_string())
}
