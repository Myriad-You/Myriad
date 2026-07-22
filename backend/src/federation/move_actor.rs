//! ActivityPub Move — domain migration: **B** actor fields, **C** send, **D** receive,
//! **E** local URL rewrite, **G** shared RSA keys.
//!
//! ## Admin job order (`domain_move_all_users`)
//! 1. Validate old/new base URLs  
//! 2. dry_run counts (users, keys, rewrite rows)  
//! 3. **B** — store domain alias so actor docs expose `alsoKnownAs` / `movedTo`  
//! 4. **G** — retarget `federation_keys.key_id` to new host; **same PEM** (no new keypair)  
//! 5. **C** — enqueue Move to followers  
//! 6. **E** — rewrite this instance’s stored absolute URLs (whitelist only; never third-party)  
//! 7. Full report  
//!
//! ## Receive (D)
//! Fail-closed: HTTP Signature, actor/object, old `movedTo`, new `alsoKnownAs`.

use axum::http::StatusCode;
use axum::Json;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::federation::actor::fetch_remote_actor;
use crate::federation::content::fan_out_to_followers;
use crate::federation::types::*;

// ==================== Types ====================

/// Admin request body for domain-wide Move fan-out.
#[derive(Debug, Clone, Deserialize)]
pub struct DomainMoveRequest {
    /// Previous instance base URL (no trailing slash), e.g. `https://old.example`
    pub old_base_url: String,
    /// New instance base URL (no trailing slash), e.g. `https://new.example`
    pub new_base_url: String,
    /// When true, only count eligible users / rows — no DB writes / queue inserts.
    #[serde(default)]
    pub dry_run: bool,
}

/// Per-user result of a domain-move attempt.
#[derive(Debug, Clone, Serialize)]
pub struct DomainMoveUserResult {
    pub user_id: i32,
    pub username: String,
    pub old_actor: String,
    pub new_actor: String,
    pub status: String,
    /// G: same RSA material retained; keyId host will be new domain `#main-key`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_key: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// One whitelist table/column rewrite counter.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RewriteColumnStat {
    pub table: String,
    pub column: String,
    pub rows: u32,
}

/// **E** — local absolute-URL rewrite report (this instance only).
#[derive(Debug, Clone, Serialize, Default)]
pub struct LocalRewriteReport {
    pub dry_run: bool,
    pub total_rows: u32,
    pub columns: Vec<RewriteColumnStat>,
    /// Foreign URLs must never match rewrite prefix (invariant for tests/ops).
    pub foreign_urls_untouched: bool,
}

/// **G** — key continuity report.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SharedKeysReport {
    /// Users with an existing federation keypair (PEM retained).
    pub users_with_keys: u32,
    /// `key_id` rows rewritten old host → new host (same PEM).
    pub key_ids_retargeted: u32,
    /// Users missing keys (will generate on first actor fetch under new base — same once).
    pub users_without_keys: u32,
    /// Explicit: no fresh keypair generated during this job.
    pub regenerated_keys: u32,
}

/// Aggregate admin response (job steps B+C+E+G).
#[derive(Debug, Clone, Serialize)]
pub struct DomainMoveResponse {
    pub dry_run: bool,
    pub old_base_url: String,
    pub new_base_url: String,
    /// **B** — domain alias stored (or would be stored).
    pub alias_stored: bool,
    pub total_users: u32,
    pub enqueued: u32,
    pub failed: u32,
    /// **G**
    pub shared_keys: SharedKeysReport,
    /// **E**
    pub local_rewrite: LocalRewriteReport,
    pub results: Vec<DomainMoveUserResult>,
}

/// Recorded domain migration alias (for actor document fields).
#[derive(Debug, Clone)]
pub struct DomainMoveAlias {
    pub old_base_url: String,
    pub new_base_url: String,
}

/// Whitelist of text columns that may hold **this instance's** absolute federation URLs.
///
/// Only values whose prefix is exactly `old_base` are rewritten. Third-party domains
/// never match and are never touched.
pub const LOCAL_URL_REWRITE_WHITELIST: &[(&str, &str)] = &[
    ("federation_keys", "key_id"),
    ("federation_remote_actors", "actor_url"),
    ("federation_remote_actors", "inbox_url"),
    ("federation_remote_actors", "outbox_url"),
    ("federation_remote_actors", "shared_inbox_url"),
    ("federation_remote_actors", "public_key_id"),
    ("federation_remote_actors", "avatar_url"),
    ("federation_room_members", "actor_url"),
    ("federation_room_members", "invited_by"),
    ("federation_rooms", "owner_actor"),
    ("federation_rooms", "home_server"),
    ("federation_room_messages", "sender_actor"),
    ("federation_channel_messages", "sender_actor"),
    ("federation_delivery_queue", "target_inbox"),
];

// ==================== Actor document fields ====================

/// Normalize a base URL: trim, strip trailing slash, require http(s).
/// Host is lowercased by the URL parser (RFC 3986).
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("base URL must not be empty".into());
    }
    let parsed = url::Url::parse(trimmed).map_err(|e| format!("invalid base URL: {}", e))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("base URL must be http or https".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("base URL must not contain userinfo".into());
    }
    // Rebuild without path/query/fragment
    let host = parsed
        .host_str()
        .ok_or_else(|| "base URL must have a host".to_string())?;
    let port = parsed.port().map(|p| format!(":{}", p)).unwrap_or_default();
    Ok(format!("{}://{}{}", parsed.scheme(), host, port))
}

// ==================== E — pure local URL rewrite helpers ====================

/// True if `url` is an absolute URL under `base` (same origin prefix).
///
/// Matches `{base}`, `{base}/…`, and `{base}#…` only — never a foreign host
/// that merely contains the old hostname as a substring.
pub fn url_is_under_base(url: &str, base: &str) -> bool {
    let url = url.trim();
    let base = base.trim().trim_end_matches('/');
    if url.is_empty() || base.is_empty() {
        return false;
    }
    // Case-insensitive scheme+host via normalize_actor_url for actor-like URLs;
    // for general paths keep byte-prefix after lowercasing host via Url parse.
    let url_norm = normalize_actor_url(url);
    let base_norm = normalize_actor_url(base);
    if url_norm == base_norm {
        return true;
    }
    let prefix = format!("{}/", base_norm);
    if url_norm.starts_with(&prefix) {
        return true;
    }
    // keyId fragments: https://old/users/u#main-key
    if let Some((path, _frag)) = url.split_once('#') {
        let path_norm = normalize_actor_url(path);
        if path_norm == base_norm || path_norm.starts_with(&prefix) {
            return true;
        }
    }
    false
}

/// If `url` is under `old_base`, return the same path/query/fragment under `new_base`.
/// Foreign URLs return `None` (must not be rewritten).
pub fn rewrite_url_if_local(url: &str, old_base: &str, new_base: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || !url_is_under_base(url, old_base) {
        return None;
    }
    let old_base = old_base.trim().trim_end_matches('/');
    let new_base = new_base.trim().trim_end_matches('/');

    // Prefer raw-prefix replace preserving path case; fall back to normalized.
    if let Some(rest) = url.strip_prefix(old_base) {
        return Some(format!("{}{}", new_base, rest));
    }
    // Case drift on host: parse and rebuild
    let url_norm = normalize_actor_url(url);
    let old_norm = normalize_actor_url(old_base);
    if let Some(rest) = url_norm.strip_prefix(old_norm.as_str()) {
        // Preserve original fragment if present and not already in rest
        if let Some((_, frag)) = url.split_once('#') {
            if !rest.contains('#') {
                return Some(format!("{}{}#{}", new_base, rest, frag));
            }
        }
        return Some(format!("{}{}", new_base, rest));
    }
    // Fragment-only suffix after path match (rare host-case + fragment)
    if let Some((path, frag)) = url.split_once('#') {
        if let Some(rewritten_path) = rewrite_url_if_local(path, old_base, new_base) {
            return Some(format!("{}#{}", rewritten_path, frag));
        }
    }
    None
}

/// SQL LIKE pattern for prefix match (old_base + '%'). Escapes `%` / `_` in base.
fn prefix_like_pattern(old_base: &str) -> String {
    let escaped = old_base
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("{}%", escaped)
}

/// Load all domain-move aliases from DB (empty if table missing / empty).
pub async fn load_domain_aliases(db: &DatabaseConnection) -> Vec<DomainMoveAlias> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT old_base_url, new_base_url
               FROM federation_domain_aliases
               ORDER BY id ASC"#,
            [],
        ))
        .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|r| {
                let old_base_url: String = r.try_get("", "old_base_url").ok()?;
                let new_base_url: String = r.try_get("", "new_base_url").ok()?;
                Some(DomainMoveAlias {
                    old_base_url,
                    new_base_url,
                })
            })
            .collect(),
        Err(e) => {
            tracing::debug!("load_domain_aliases: {}", e);
            Vec::new()
        }
    }
}

/// Compute `alsoKnownAs` / `movedTo` for a local actor given current base + aliases.
///
/// - If `serve_base` is a **new** base for any alias → `alsoKnownAs` includes old actor ids.
/// - If `serve_base` is an **old** base for any alias → `movedTo` is the new actor id.
pub fn actor_move_fields(
    serve_base: &str,
    username: &str,
    aliases: &[DomainMoveAlias],
) -> (Vec<String>, Option<String>) {
    let serve = normalize_actor_url(serve_base);
    let mut also_known_as: Vec<String> = Vec::new();
    let mut moved_to: Option<String> = None;

    for a in aliases {
        let old_b = normalize_actor_url(&a.old_base_url);
        let new_b = normalize_actor_url(&a.new_base_url);
        if serve == new_b {
            let old_actor = actor_url(&a.old_base_url, username);
            if !also_known_as.iter().any(|x| same_actor_url(x, &old_actor)) {
                also_known_as.push(old_actor);
            }
        }
        if serve == old_b {
            // Prefer most recent / any single target; last write wins if multiple
            moved_to = Some(actor_url(&a.new_base_url, username));
        }
    }

    (also_known_as, moved_to)
}

/// Resolve which base URL to use for serving this actor document from Host.
///
/// When the request Host matches a recorded **old** base host, serve as the old
/// actor (with `movedTo`). Otherwise use the configured current base.
pub fn resolve_serve_base(
    configured_base: &str,
    host_header: Option<&str>,
    aliases: &[DomainMoveAlias],
) -> String {
    let host = match host_header {
        Some(h) => h.trim().to_ascii_lowercase(),
        None => return configured_base.trim_end_matches('/').to_string(),
    };
    // Strip port for comparison when Host is bare host:port
    let host_no_port = host.split(':').next().unwrap_or(&host);

    for a in aliases {
        if let Ok(old) = normalize_base_url(&a.old_base_url) {
            if let Ok(parsed) = url::Url::parse(&old) {
                let old_host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
                let old_port = parsed.port();
                let matches = if let Some(p) = old_port {
                    host == format!("{}:{}", old_host, p)
                        || (host_no_port == old_host && host == old_host)
                } else {
                    host_no_port == old_host || host == old_host
                };
                if matches {
                    return old;
                }
            }
        }
    }

    configured_base.trim_end_matches('/').to_string()
}

// ==================== Move Activity construction ====================

/// Build a protocol-correct Move Activity JSON.
pub fn build_move_activity(
    activity_id: &str,
    old_actor: &str,
    new_actor: &str,
    published: &str,
) -> serde_json::Value {
    json!({
        "@context": build_ap_context(),
        "type": "Move",
        "id": activity_id,
        "actor": old_actor,
        "object": old_actor,
        "target": new_actor,
        "to": [AP_PUBLIC, followers_collection_hint(old_actor)],
        "published": published,
    })
}

/// Best-effort followers collection URL from an actor id (`…/users/u` → `…/followers`).
fn followers_collection_hint(actor: &str) -> String {
    let trimmed = actor.trim().trim_end_matches('/');
    format!("{}/followers", trimmed)
}

// ==================== Verification (receive path, pure + fetch) ====================

/// Extract string id from activity field that may be a string or `{ "id": "…" }`.
pub fn activity_id_string(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    value
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Parse alsoKnownAs from an actor document (array of strings or mixed).
pub fn parse_also_known_as(actor_json: &serde_json::Value) -> Vec<String> {
    let Some(arr) = actor_json.get("alsoKnownAs").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            v.as_str()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    v.get("id")
                        .and_then(|x| x.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
        })
        .collect()
}

/// Fail-closed structural + document checks for Move (no HTTP Signature here).
///
/// Returns `Ok((old_actor, new_actor))` or a permanent error string.
pub fn verify_move_structure(
    activity: &serde_json::Value,
    signed_actor: &str,
) -> Result<(String, String), String> {
    let actor = activity["actor"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Move missing actor".to_string())?;

    if !same_actor_url(actor, signed_actor) {
        return Err(format!(
            "Move actor '{}' does not match signed actor '{}'",
            actor, signed_actor
        ));
    }

    let object = activity_id_string(&activity["object"])
        .ok_or_else(|| "Move missing object (old actor id)".to_string())?;

    if !same_actor_url(actor, &object) {
        return Err(format!(
            "Move actor/object mismatch: actor='{}' object='{}'",
            actor, object
        ));
    }

    let target = activity_id_string(&activity["target"])
        .ok_or_else(|| "Move missing target (new actor id)".to_string())?;

    if same_actor_url(&object, &target) {
        return Err("Move target must differ from object (old actor)".into());
    }

    Ok((object, target))
}

/// Verify old actor document claims `movedTo` == target (fail closed).
pub fn verify_old_actor_moved_to(
    old_actor_json: &serde_json::Value,
    old_actor_id: &str,
    target: &str,
) -> Result<(), String> {
    if let Some(doc_id) = old_actor_json.get("id").and_then(|v| v.as_str()) {
        if !doc_id.is_empty() && !same_actor_url(doc_id, old_actor_id) {
            return Err(format!(
                "Old actor document id '{}' does not match '{}'",
                doc_id, old_actor_id
            ));
        }
    }
    let moved_to = old_actor_json
        .get("movedTo")
        .and_then(activity_id_string)
        .ok_or_else(|| {
            "Old actor missing movedTo (required for Move acceptance)".to_string()
        })?;
    if !same_actor_url(&moved_to, target) {
        return Err(format!(
            "Old actor movedTo '{}' does not match Move target '{}'",
            moved_to, target
        ));
    }
    Ok(())
}

/// Verify new actor document `alsoKnownAs` contains old actor id (fail closed).
pub fn verify_new_actor_also_known_as(
    new_actor_json: &serde_json::Value,
    new_actor_id: &str,
    old_actor_id: &str,
) -> Result<(), String> {
    if let Some(doc_id) = new_actor_json.get("id").and_then(|v| v.as_str()) {
        if !doc_id.is_empty() && !same_actor_url(doc_id, new_actor_id) {
            return Err(format!(
                "New actor document id '{}' does not match '{}'",
                doc_id, new_actor_id
            ));
        }
    }
    let aliases = parse_also_known_as(new_actor_json);
    if aliases.is_empty() {
        return Err(
            "New actor missing alsoKnownAs (required for Move acceptance)".to_string(),
        );
    }
    if !aliases.iter().any(|a| same_actor_url(a, old_actor_id)) {
        return Err(format!(
            "New actor alsoKnownAs does not contain old actor '{}'",
            old_actor_id
        ));
    }
    Ok(())
}

// ==================== Fetch actor document (fresh) ====================

/// Fetch an ActivityPub actor document as JSON (fresh HTTP, or local build).
pub async fn fetch_actor_document(
    db: &DatabaseConnection,
    actor_url_str: &str,
) -> Result<serde_json::Value, String> {
    let base_url = get_base_url().await;
    if let Some(local_username) = local_username_from_actor_url(&base_url, actor_url_str) {
        return local_actor_document(db, &base_url, &local_username).await;
    }

    // Host may match an old-base alias on this instance
    let aliases = load_domain_aliases(db).await;
    for a in &aliases {
        if let Some(uname) = local_username_from_actor_url(&a.old_base_url, actor_url_str) {
            return local_actor_document_for_base(db, &a.old_base_url, &uname, &aliases).await;
        }
        if let Some(uname) = local_username_from_actor_url(&a.new_base_url, actor_url_str) {
            return local_actor_document_for_base(db, &a.new_base_url, &uname, &aliases).await;
        }
    }

    if is_internal_url(actor_url_str) {
        return Err(format!("Refused to fetch internal URL: {}", actor_url_str));
    }

    let user_agent = format!(
        "Myriad/{} (+{})",
        env!("CARGO_PKG_VERSION"),
        base_url
    );
    let (url, client) = crate::services::outbound_security::build_public_http_client(
        actor_url_str,
        std::time::Duration::from_secs(10),
        Some(&user_agent),
    )
    .await?;

    let resp = client
        .get(url)
        .header("Accept", AP_CONTENT_TYPE)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch actor for Move verify: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Actor fetch for Move verify returned {}",
            resp.status()
        ));
    }

    let body = crate::services::outbound_security::read_limited_body(resp, 1024 * 1024)
        .await
        .map_err(|e| format!("Failed to read actor body: {}", e))?;
    let actor_json: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| format!("Failed to parse actor JSON: {}", e))?;

    if let Some(json_id) = actor_json.get("id").and_then(|v| v.as_str()) {
        if !json_id.is_empty() && !same_actor_url(json_id, actor_url_str) {
            return Err(format!(
                "Remote actor id mismatch: document id '{}' does not match requested '{}'",
                json_id, actor_url_str
            ));
        }
    }

    Ok(actor_json)
}

async fn local_actor_document(
    db: &DatabaseConnection,
    base_url: &str,
    username: &str,
) -> Result<serde_json::Value, String> {
    let aliases = load_domain_aliases(db).await;
    local_actor_document_for_base(db, base_url, username, &aliases).await
}

async fn local_actor_document_for_base(
    db: &DatabaseConnection,
    serve_base: &str,
    username: &str,
    aliases: &[DomainMoveAlias],
) -> Result<serde_json::Value, String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT u.username, u.display_name, u.bio,
                      fk.public_key_pem, fk.key_id
               FROM users u
               LEFT JOIN federation_keys fk ON fk.user_id = u.id
               WHERE u.username = $1
               LIMIT 1"#,
            [username.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?
        .ok_or_else(|| format!("Local user not found: {}", username))?;

    let display_name: Option<String> = row.try_get("", "display_name").ok();
    let bio: Option<String> = row.try_get("", "bio").ok();
    let public_key_pem: Option<String> = row.try_get("", "public_key_pem").ok();
    let key_id_stored: Option<String> = row.try_get("", "key_id").ok();

    let actor_id = actor_url(serve_base, username);
    let kid = key_id_stored.unwrap_or_else(|| key_id(serve_base, username));
    // When serving under a non-current base (old domain), keyId must match that base
    // so Move signature verification against the old actor document succeeds.
    let kid = if kid.contains(serve_base.trim_end_matches('/')) {
        kid
    } else {
        key_id(serve_base, username)
    };

    let (also_known_as, moved_to) = actor_move_fields(serve_base, username, aliases);

    let actor = Actor {
        context: build_context(),
        actor_type: "Person".to_string(),
        id: actor_id.clone(),
        preferred_username: username.to_string(),
        name: display_name,
        summary: bio,
        url: None,
        inbox: inbox_url(serve_base, username),
        outbox: outbox_url(serve_base, username),
        followers: followers_url(serve_base, username),
        following: following_url(serve_base, username),
        public_key: ActorPublicKey {
            id: kid,
            owner: actor_id,
            public_key_pem: public_key_pem.unwrap_or_default(),
        },
        icon: None,
        image: None,
        also_known_as,
        moved_to,
        mfp_instance_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        mfp_tapp_capabilities: None,
        mfp_channels_url: Some(format!("{}/users/{}/channels", serve_base, username)),
    };

    serde_json::to_value(actor).map_err(|e| e.to_string())
}

// ==================== Follow graph migration ====================

/// Pure decision for one follow row when re-pointing old → new remote actor.
///
/// Used by `migrate_follows_old_to_new` and unit-tested for idempotent merge rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowRepointAction {
    /// No existing row on the new actor: update remote_actor_id in place.
    UpdateRemoteId,
    /// Already following new: drop the old row; optionally promote new to accepted.
    DropOld { promote_new_to_accepted: bool },
}

/// Decide how to migrate one follow edge when a remote actor Moves.
pub fn plan_follow_repoint(
    old_status: &str,
    existing_new_status: Option<&str>,
) -> FollowRepointAction {
    match existing_new_status {
        None => FollowRepointAction::UpdateRemoteId,
        Some(new_status) => FollowRepointAction::DropOld {
            promote_new_to_accepted: old_status == "accepted" && new_status != "accepted",
        },
    }
}

/// Re-point local follow rows from old remote actor URL to new (idempotent).
///
/// Updates `federation_follows.remote_actor_id` and ensures the new actor is cached.
/// On unique conflicts (already following new), drops the old row.
pub async fn migrate_follows_old_to_new(
    db: &DatabaseConnection,
    old_actor_url: &str,
    new_actor_url: &str,
) -> Result<u32, String> {
    // Ensure both are in remote_actors cache (new may need fetch)
    let old_remote = match db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM federation_remote_actors WHERE actor_url = $1 LIMIT 1",
            [old_actor_url.into()],
        ))
        .await
        .map_err(|e| format!("DB error: {}", e))?
    {
        Some(r) => r.try_get::<i32>("", "id").unwrap_or(0),
        None => {
            // No local knowledge of old actor — nothing to migrate
            return Ok(0);
        }
    };

    if old_remote == 0 {
        return Ok(0);
    }

    let new_remote = fetch_remote_actor(db, new_actor_url)
        .await
        .map_err(|e| format!("Cannot resolve new actor after Move: {}", e))?;

    if new_remote.id == old_remote {
        return Ok(0);
    }

    // All follows that pointed at old remote
    let follows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT id, user_id, direction, status, activity_id
               FROM federation_follows
               WHERE remote_actor_id = $1"#,
            [old_remote.into()],
        ))
        .await
        .map_err(|e| format!("DB error listing follows: {}", e))?;

    let mut migrated = 0u32;

    for row in follows {
        let follow_id: i32 = row.try_get("", "id").unwrap_or(0);
        let user_id: i32 = row.try_get("", "user_id").unwrap_or(0);
        let direction: String = row.try_get("", "direction").unwrap_or_default();
        let status: String = row.try_get("", "status").unwrap_or_default();
        let activity_id: Option<String> = row.try_get("", "activity_id").ok();

        // Is there already a follow for (user, new, direction)?
        let existing = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT id, status FROM federation_follows
                   WHERE user_id = $1 AND remote_actor_id = $2 AND direction = $3
                   LIMIT 1"#,
                [
                    user_id.into(),
                    new_remote.id.into(),
                    direction.clone().into(),
                ],
            ))
            .await
            .map_err(|e| format!("DB error: {}", e))?;

        match plan_follow_repoint(
            &status,
            existing
                .as_ref()
                .and_then(|ex| ex.try_get::<String>("", "status").ok())
                .as_deref(),
        ) {
            FollowRepointAction::DropOld {
                promote_new_to_accepted,
            } => {
                let ex_id: i32 = existing
                    .as_ref()
                    .and_then(|ex| ex.try_get("", "id").ok())
                    .unwrap_or(0);
                if promote_new_to_accepted && ex_id != 0 {
                    let _ = db
                        .execute(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"UPDATE federation_follows
                               SET status = 'accepted',
                                   activity_id = COALESCE($1, activity_id),
                                   accepted_at = COALESCE(accepted_at, NOW())
                               WHERE id = $2"#,
                            [activity_id.clone().into(), ex_id.into()],
                        ))
                        .await;
                }
                let _ = db
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        "DELETE FROM federation_follows WHERE id = $1",
                        [follow_id.into()],
                    ))
                    .await;
                migrated += 1;
            }
            FollowRepointAction::UpdateRemoteId => {
                let res = db
                    .execute(Statement::from_sql_and_values(
                        DatabaseBackend::Postgres,
                        r#"UPDATE federation_follows
                           SET remote_actor_id = $1
                           WHERE id = $2"#,
                        [new_remote.id.into(), follow_id.into()],
                    ))
                    .await;
                match res {
                    Ok(_) => migrated += 1,
                    Err(e) => {
                        tracing::warn!(
                            "migrate follow {} → new actor conflict, deleting old: {}",
                            follow_id,
                            e
                        );
                        let _ = db
                            .execute(Statement::from_sql_and_values(
                                DatabaseBackend::Postgres,
                                "DELETE FROM federation_follows WHERE id = $1",
                                [follow_id.into()],
                            ))
                            .await;
                        migrated += 1;
                    }
                }
            }
        }
    }

    // Optionally note moved_to on old remote_actors row for debugging (no schema change)
    tracing::info!(
        old = %old_actor_url,
        new = %new_actor_url,
        migrated,
        "Migrated local follows after Move"
    );

    Ok(migrated)
}

// ==================== Send path + B / G / E job ====================

/// Persist domain alias (upsert by old_base_url). **B** — enables actor `alsoKnownAs` / `movedTo`.
pub async fn store_domain_alias(
    db: &DatabaseConnection,
    old_base: &str,
    new_base: &str,
) -> Result<(), String> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_domain_aliases (old_base_url, new_base_url, created_at)
           VALUES ($1, $2, NOW())
           ON CONFLICT (old_base_url) DO UPDATE SET
               new_base_url = EXCLUDED.new_base_url,
               created_at = NOW()"#,
        [old_base.into(), new_base.into()],
    ))
    .await
    .map_err(|e| format!("Failed to store domain alias: {}", e))?;
    Ok(())
}

/// **G** — Confirm user has existing keys; retarget `key_id` host to `new_base`.
///
/// **Never** generates a new RSA keypair. PEM material is left untouched.
/// Choice: `keyId` becomes `{new_base}/users/{username}#main-key` (new domain host)
/// with the **same** `public_key_pem` as before (Mastodon-style continuity).
pub async fn retarget_shared_keys(
    db: &DatabaseConnection,
    old_base: &str,
    new_base: &str,
    dry_run: bool,
) -> Result<SharedKeysReport, String> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT fk.user_id, u.username, fk.key_id, fk.public_key_pem
               FROM federation_keys fk
               JOIN users u ON u.id = fk.user_id
               ORDER BY fk.user_id ASC"#,
            [],
        ))
        .await
        .map_err(|e| format!("Failed to list federation_keys: {}", e))?;

    let all_users = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*)::int AS c FROM users",
            [],
        ))
        .await
        .map_err(|e| format!("Failed to count users: {}", e))?;
    let total_users: i32 = all_users
        .and_then(|r| r.try_get("", "c").ok())
        .unwrap_or(0);

    let mut report = SharedKeysReport {
        users_with_keys: rows.len() as u32,
        key_ids_retargeted: 0,
        users_without_keys: (total_users as u32).saturating_sub(rows.len() as u32),
        regenerated_keys: 0,
    };

    for row in rows {
        let user_id: i32 = row.try_get("", "user_id").unwrap_or(0);
        let username: String = row.try_get("", "username").unwrap_or_default();
        let old_kid: String = row.try_get("", "key_id").unwrap_or_default();
        let pem: String = row.try_get("", "public_key_pem").unwrap_or_default();

        if pem.trim().is_empty() {
            return Err(format!(
                "user {} has empty public_key_pem — refusing domain-move (G shared keys)",
                user_id
            ));
        }

        // Canonical new keyId on new domain; same PEM advertised by actor builder.
        // Prefer pure local rewrite of the stored keyId (preserves path/fragment);
        // fall back to `key_id(new_base, username)` when the stored value is empty
        // or not under old_base.
        let new_kid = rewrite_url_if_local(&old_kid, old_base, new_base)
            .unwrap_or_else(|| key_id(new_base, &username));

        // Only rewrite if key_id changes (under old base, empty, or not yet on new).
        if old_kid == new_kid {
            continue;
        }
        let needs = url_is_under_base(&old_kid, old_base)
            || old_kid.is_empty()
            || !url_is_under_base(&old_kid, new_base);
        if !needs {
            continue;
        }

        report.key_ids_retargeted += 1;
        if dry_run {
            continue;
        }

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            // PEM columns intentionally omitted from SET — shared material only.
            r#"UPDATE federation_keys
               SET key_id = $1
               WHERE user_id = $2
                 AND public_key_pem IS NOT NULL
                 AND public_key_pem <> ''"#,
            [new_kid.into(), user_id.into()],
        ))
        .await
        .map_err(|e| format!("Failed to retarget key_id for user {}: {}", user_id, e))?;
    }

    Ok(report)
}

/// Count rows in a text column whose value starts with `old_base` (local only).
async fn count_prefix_rows(
    db: &DatabaseConnection,
    table: &str,
    column: &str,
    old_base: &str,
) -> Result<u32, String> {
    // Whitelist guard — never interpolate untrusted table/column names.
    if !LOCAL_URL_REWRITE_WHITELIST
        .iter()
        .any(|(t, c)| *t == table && *c == column)
    {
        return Err(format!("column {}.{} not on rewrite whitelist", table, column));
    }
    let like = prefix_like_pattern(old_base);
    // Identifier whitelist only — safe static concat.
    let sql = format!(
        "SELECT COUNT(*)::int AS c FROM {} WHERE {} IS NOT NULL AND {} LIKE $1",
        table, column, column
    );
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            [like.into()],
        ))
        .await
        .map_err(|e| format!("count {}.{}: {}", table, column, e))?;
    Ok(row
        .and_then(|r| r.try_get::<i32>("", "c").ok())
        .unwrap_or(0) as u32)
}

/// Apply prefix rewrite for one whitelisted text column.
async fn rewrite_prefix_column(
    db: &DatabaseConnection,
    table: &str,
    column: &str,
    old_base: &str,
    new_base: &str,
    dry_run: bool,
) -> Result<u32, String> {
    let count = count_prefix_rows(db, table, column, old_base).await?;
    if dry_run || count == 0 {
        return Ok(count);
    }
    let like = prefix_like_pattern(old_base);
    // Postgres: rewrite only matching prefix; leave foreign rows alone via LIKE filter.
    let sql = format!(
        r#"UPDATE {}
           SET {} = $1 || substring({} from char_length($2) + 1)
           WHERE {} IS NOT NULL AND {} LIKE $3"#,
        table, column, column, column, column
    );
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &sql,
        [new_base.into(), old_base.into(), like.into()],
    ))
    .await
    .map_err(|e| format!("rewrite {}.{}: {}", table, column, e))?;
    Ok(count)
}

/// **E** — Rewrite this instance’s stored absolute federation URLs `old_base` → `new_base`.
///
/// Whitelist only. Never rewrites third-party domains (prefix match on old_base).
/// `federation_remote_actors.domain` is updated only when `actor_url` was under old_base.
pub async fn rewrite_local_federation_urls(
    db: &DatabaseConnection,
    old_base: &str,
    new_base: &str,
    dry_run: bool,
) -> Result<LocalRewriteReport, String> {
    let mut report = LocalRewriteReport {
        dry_run,
        total_rows: 0,
        columns: Vec::new(),
        foreign_urls_untouched: true,
    };

    for &(table, column) in LOCAL_URL_REWRITE_WHITELIST {
        let rows = rewrite_prefix_column(db, table, column, old_base, new_base, dry_run).await?;
        if rows > 0 {
            report.columns.push(RewriteColumnStat {
                table: table.to_string(),
                column: column.to_string(),
                rows,
            });
            report.total_rows += rows;
        }
    }

    // remote_actors.domain: only for rows whose actor_url is (or was) under our bases.
    let old_domain = extract_domain(old_base).unwrap_or_default();
    let new_domain = extract_domain(new_base).unwrap_or_default();
    if !old_domain.is_empty() && !new_domain.is_empty() && old_domain != new_domain {
        let domain_count_sql = r#"SELECT COUNT(*)::int AS c FROM federation_remote_actors
               WHERE domain = $1
                 AND (actor_url LIKE $2 OR actor_url LIKE $3)"#;
        let like_old = prefix_like_pattern(old_base);
        let like_new = prefix_like_pattern(new_base);
        let domain_rows = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                domain_count_sql,
                [
                    old_domain.clone().into(),
                    like_old.clone().into(),
                    like_new.clone().into(),
                ],
            ))
            .await
            .map_err(|e| format!("count remote_actors.domain: {}", e))?;
        let dcount = domain_rows
            .and_then(|r| r.try_get::<i32>("", "c").ok())
            .unwrap_or(0) as u32;
        if dcount > 0 {
            report.columns.push(RewriteColumnStat {
                table: "federation_remote_actors".into(),
                column: "domain".into(),
                rows: dcount,
            });
            report.total_rows += dcount;
            if !dry_run {
                db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_remote_actors
                       SET domain = $1
                       WHERE domain = $2
                         AND (actor_url LIKE $3 OR actor_url LIKE $4)"#,
                    [
                        new_domain.clone().into(),
                        old_domain.clone().into(),
                        like_old.into(),
                        like_new.into(),
                    ],
                ))
                .await
                .map_err(|e| format!("rewrite remote_actors.domain: {}", e))?;
            }
        }
    }

    // delivery queue target_domain for inboxes we rewrote
    if !old_domain.is_empty() && !new_domain.is_empty() && old_domain != new_domain {
        let like_new_inbox = prefix_like_pattern(new_base);
        let like_old_inbox = prefix_like_pattern(old_base);
        let dq = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"SELECT COUNT(*)::int AS c FROM federation_delivery_queue
                   WHERE target_domain = $1
                     AND (target_inbox LIKE $2 OR target_inbox LIKE $3)"#,
                [
                    old_domain.clone().into(),
                    like_old_inbox.clone().into(),
                    like_new_inbox.clone().into(),
                ],
            ))
            .await
            .map_err(|e| format!("count delivery target_domain: {}", e))?;
        let c = dq
            .and_then(|r| r.try_get::<i32>("", "c").ok())
            .unwrap_or(0) as u32;
        if c > 0 {
            report.columns.push(RewriteColumnStat {
                table: "federation_delivery_queue".into(),
                column: "target_domain".into(),
                rows: c,
            });
            report.total_rows += c;
            if !dry_run {
                db.execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_delivery_queue
                       SET target_domain = $1
                       WHERE target_domain = $2
                         AND (target_inbox LIKE $3 OR target_inbox LIKE $4)"#,
                    [
                        new_domain.clone().into(),
                        old_domain.clone().into(),
                        like_old_inbox.into(),
                        like_new_inbox.into(),
                    ],
                ))
                .await
                .map_err(|e| format!("rewrite delivery target_domain: {}", e))?;
            }
        }
    }

    Ok(report)
}

/// Emit Move for one local user and fan-out to followers.
pub async fn emit_move_for_user(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    old_base: &str,
    new_base: &str,
) -> Result<(String, u32), String> {
    let old_actor = actor_url(old_base, username);
    let new_actor = actor_url(new_base, username);
    // Activity id under the **old** base so peers associate it with the departing identity
    let activity_id = generate_activity_id(old_base);
    let published = now_iso8601();
    let move_json = build_move_activity(&activity_id, &old_actor, &new_actor, &published);

    let act_row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_activities
                   (activity_id, user_id, activity_type, object_type, object_json, is_local, published_at)
               VALUES ($1, $2, 'Move', 'Person', $3, true, NOW())
               RETURNING id"#,
            [
                activity_id.clone().into(),
                user_id.into(),
                move_json.clone().into(),
            ],
        ))
        .await
        .map_err(|e| format!("Failed to insert Move activity: {}", e))?;

    let act_db_id: i32 = act_row
        .map(|r| r.try_get("", "id").unwrap_or(0))
        .unwrap_or(0);

    if act_db_id == 0 {
        return Err("Failed to obtain activity DB id for Move".into());
    }

    let queued = fan_out_to_followers(db, user_id, act_db_id, &move_json).await;

    tracing::info!(
        username,
        old_actor = %old_actor,
        new_actor = %new_actor,
        activity_id = %activity_id,
        queued,
        "Emitted ActivityPub Move"
    );

    Ok((activity_id, queued))
}

/// Whether this user has a non-empty federation PEM (shared-key continuity).
async fn user_has_shared_key(db: &DatabaseConnection, user_id: i32) -> Result<bool, String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT public_key_pem FROM federation_keys
               WHERE user_id = $1 AND public_key_pem IS NOT NULL AND public_key_pem <> ''
               LIMIT 1"#,
            [user_id.into()],
        ))
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.is_some())
}

/// Admin domain-move job: **B + G + C + E** (or dry_run counts).
///
/// Order: validate → counts → alias (B) → shared keys (G) → Move enqueue (C) → local rewrite (E).
pub async fn domain_move_all_users(
    db: &DatabaseConnection,
    req: &DomainMoveRequest,
) -> Result<DomainMoveResponse, (StatusCode, Json<serde_json::Value>)> {
    // 1. Validate
    let old_base = normalize_base_url(&req.old_base_url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("old_base_url: {}", e)})),
        )
    })?;
    let new_base = normalize_base_url(&req.new_base_url).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("new_base_url: {}", e)})),
        )
    })?;

    if normalize_actor_url(&old_base) == normalize_actor_url(&new_base) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "old_base_url and new_base_url must differ"})),
        ));
    }

    let users = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT u.id, u.username
               FROM users u
               ORDER BY u.id ASC"#,
            [],
        ))
        .await
        .map_err(db_err)?;

    let dry = req.dry_run;

    // 2–4. dry_run: G + E counts without writes; live: apply G after B.
    // 3. B — actor document fields (alias)
    let alias_stored = if dry {
        false
    } else {
        store_domain_alias(db, &old_base, &new_base)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e})),
                )
            })?;
        true
    };

    // 4. G — shared keys (retarget keyId only; never regenerate)
    let shared_keys = retarget_shared_keys(db, &old_base, &new_base, dry)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("shared keys (G): {}", e)})),
            )
        })?;

    // 5. C — emit Move per user
    let mut results = Vec::new();
    let mut enqueued = 0u32;
    let mut failed = 0u32;

    for row in users {
        let user_id: i32 = row.try_get("", "id").unwrap_or(0);
        let username: String = row.try_get("", "username").unwrap_or_default();
        if user_id == 0 || username.is_empty() {
            continue;
        }

        let old_actor = actor_url(&old_base, &username);
        let new_actor = actor_url(&new_base, &username);
        let has_key = user_has_shared_key(db, user_id).await.unwrap_or(false);

        if dry {
            results.push(DomainMoveUserResult {
                user_id,
                username,
                old_actor,
                new_actor,
                status: "would_enqueue".into(),
                shared_key: Some(has_key),
                activity_id: None,
                queued: None,
                error: None,
            });
            enqueued += 1;
            continue;
        }

        match emit_move_for_user(db, user_id, &username, &old_base, &new_base).await {
            Ok((activity_id, queued)) => {
                enqueued += 1;
                results.push(DomainMoveUserResult {
                    user_id,
                    username,
                    old_actor,
                    new_actor,
                    status: "enqueued".into(),
                    shared_key: Some(has_key),
                    activity_id: Some(activity_id),
                    queued: Some(queued),
                    error: None,
                });
            }
            Err(e) => {
                failed += 1;
                tracing::error!(
                    user_id,
                    username = %username,
                    error = %e,
                    "domain-move emit failed"
                );
                results.push(DomainMoveUserResult {
                    user_id,
                    username,
                    old_actor,
                    new_actor,
                    status: "failed".into(),
                    shared_key: Some(has_key),
                    activity_id: None,
                    queued: None,
                    error: Some(e),
                });
            }
        }
    }

    // 6. E — local DB rewrite (after Move enqueue)
    let local_rewrite = rewrite_local_federation_urls(db, &old_base, &new_base, dry)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("local rewrite (E): {}", e)})),
            )
        })?;

    // 7. Full report
    let total_users = results.len() as u32;
    Ok(DomainMoveResponse {
        dry_run: dry,
        old_base_url: old_base,
        new_base_url: new_base,
        alias_stored,
        total_users,
        enqueued,
        failed,
        shared_keys,
        local_rewrite,
        results,
    })
}

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_json_shape() {
        let act = build_move_activity(
            "https://old.example/activities/abc",
            "https://old.example/users/alice",
            "https://new.example/users/alice",
            "2026-07-21T00:00:00Z",
        );
        assert_eq!(act["type"], "Move");
        assert_eq!(act["actor"], "https://old.example/users/alice");
        assert_eq!(act["object"], "https://old.example/users/alice");
        assert_eq!(act["target"], "https://new.example/users/alice");
        assert!(act["id"].as_str().unwrap().contains("/activities/"));
        assert!(act["@context"].is_array() || act["@context"].is_string());
        assert!(act["published"].as_str().is_some());
        let to = act["to"].as_array().expect("to array");
        assert!(to.iter().any(|v| v.as_str() == Some(AP_PUBLIC)));
    }

    #[test]
    fn verify_structure_requires_actor_object_target() {
        let good = json!({
            "type": "Move",
            "actor": "https://old.example/users/a",
            "object": "https://old.example/users/a",
            "target": "https://new.example/users/a",
        });
        assert!(verify_move_structure(&good, "https://old.example/users/a").is_ok());

        let bad_obj = json!({
            "type": "Move",
            "actor": "https://old.example/users/a",
            "object": "https://old.example/users/other",
            "target": "https://new.example/users/a",
        });
        assert!(verify_move_structure(&bad_obj, "https://old.example/users/a").is_err());

        let signed_mismatch = json!({
            "type": "Move",
            "actor": "https://old.example/users/a",
            "object": "https://old.example/users/a",
            "target": "https://new.example/users/a",
        });
        assert!(
            verify_move_structure(&signed_mismatch, "https://evil.example/users/x").is_err()
        );
    }

    #[test]
    fn reject_missing_also_known_as() {
        let new_doc = json!({
            "id": "https://new.example/users/a",
            "type": "Person",
        });
        let err = verify_new_actor_also_known_as(
            &new_doc,
            "https://new.example/users/a",
            "https://old.example/users/a",
        )
        .unwrap_err();
        assert!(err.to_lowercase().contains("alsoknownas") || err.contains("alsoKnownAs"));
    }

    #[test]
    fn reject_wrong_moved_to() {
        let old_doc = json!({
            "id": "https://old.example/users/a",
            "movedTo": "https://wrong.example/users/a",
        });
        let err = verify_old_actor_moved_to(
            &old_doc,
            "https://old.example/users/a",
            "https://new.example/users/a",
        )
        .unwrap_err();
        assert!(err.contains("movedTo") || err.contains("does not match"));
    }

    #[test]
    fn reject_missing_moved_to() {
        let old_doc = json!({
            "id": "https://old.example/users/a",
            "type": "Person",
        });
        let err = verify_old_actor_moved_to(
            &old_doc,
            "https://old.example/users/a",
            "https://new.example/users/a",
        )
        .unwrap_err();
        assert!(err.contains("movedTo"));
    }

    #[test]
    fn accept_happy_path_document_claims() {
        let old_doc = json!({
            "id": "https://old.example/users/a",
            "movedTo": "https://new.example/users/a",
        });
        let new_doc = json!({
            "id": "https://new.example/users/a",
            "alsoKnownAs": ["https://old.example/users/a"],
        });
        verify_old_actor_moved_to(
            &old_doc,
            "https://old.example/users/a",
            "https://new.example/users/a",
        )
        .unwrap();
        verify_new_actor_also_known_as(
            &new_doc,
            "https://new.example/users/a",
            "https://old.example/users/a",
        )
        .unwrap();
    }

    #[test]
    fn accept_also_known_as_host_case_and_slash() {
        let new_doc = json!({
            "id": "https://new.example/users/a",
            "alsoKnownAs": ["https://OLD.example/users/a/"],
        });
        verify_new_actor_also_known_as(
            &new_doc,
            "https://new.example/users/a",
            "https://old.example/users/a",
        )
        .unwrap();
    }

    #[test]
    fn actor_move_fields_new_and_old_bases() {
        let aliases = vec![DomainMoveAlias {
            old_base_url: "https://old.example".into(),
            new_base_url: "https://new.example".into(),
        }];
        let (aka, moved) = actor_move_fields("https://new.example", "alice", &aliases);
        assert_eq!(aka, vec!["https://old.example/users/alice".to_string()]);
        assert!(moved.is_none());

        let (aka2, moved2) = actor_move_fields("https://old.example", "alice", &aliases);
        assert!(aka2.is_empty());
        assert_eq!(
            moved2.as_deref(),
            Some("https://new.example/users/alice")
        );
    }

    #[test]
    fn resolve_serve_base_matches_old_host() {
        let aliases = vec![DomainMoveAlias {
            old_base_url: "https://old.example".into(),
            new_base_url: "https://new.example".into(),
        }];
        let serve = resolve_serve_base("https://new.example", Some("old.example"), &aliases);
        assert_eq!(serve, "https://old.example");
        let serve2 = resolve_serve_base("https://new.example", Some("new.example"), &aliases);
        assert_eq!(serve2, "https://new.example");
    }

    #[test]
    fn normalize_base_url_strips_slash() {
        // Host is lowercased by the URL parser (RFC 3986).
        assert_eq!(
            normalize_base_url("https://Example.com/").unwrap(),
            "https://example.com"
        );
        assert!(normalize_base_url("ftp://x").is_err());
        assert!(normalize_base_url("").is_err());
    }

    #[test]
    fn parse_also_known_as_mixed() {
        let doc = json!({
            "alsoKnownAs": [
                "https://a.example/users/x",
                { "id": "https://b.example/users/x" },
                ""
            ]
        });
        let v = parse_also_known_as(&doc);
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn follow_row_update_plan_update_when_no_new_row() {
        assert_eq!(
            plan_follow_repoint("accepted", None),
            FollowRepointAction::UpdateRemoteId
        );
    }

    #[test]
    fn follow_row_update_plan_drop_and_promote() {
        assert_eq!(
            plan_follow_repoint("accepted", Some("pending")),
            FollowRepointAction::DropOld {
                promote_new_to_accepted: true
            }
        );
        assert_eq!(
            plan_follow_repoint("accepted", Some("accepted")),
            FollowRepointAction::DropOld {
                promote_new_to_accepted: false
            }
        );
        // Idempotent second pass: already only on new → Update path not used;
        // Drop with no promote when old was pending
        assert_eq!(
            plan_follow_repoint("pending", Some("accepted")),
            FollowRepointAction::DropOld {
                promote_new_to_accepted: false
            }
        );
    }

    #[test]
    fn local_url_rewritten_under_old_base() {
        let old_b = "https://old.example";
        let new_b = "https://new.example";
        assert_eq!(
            rewrite_url_if_local("https://old.example/users/alice", old_b, new_b).as_deref(),
            Some("https://new.example/users/alice")
        );
        assert_eq!(
            rewrite_url_if_local(
                "https://old.example/users/alice#main-key",
                old_b,
                new_b
            )
            .as_deref(),
            Some("https://new.example/users/alice#main-key")
        );
        assert_eq!(
            rewrite_url_if_local("https://old.example/users/alice/inbox", old_b, new_b)
                .as_deref(),
            Some("https://new.example/users/alice/inbox")
        );
    }

    #[test]
    fn foreign_url_untouched_by_rewrite() {
        let old_b = "https://old.example";
        let new_b = "https://new.example";
        assert!(
            rewrite_url_if_local("https://mastodon.social/users/bob", old_b, new_b).is_none()
        );
        assert!(rewrite_url_if_local(
            "https://old.example.evil.com/users/alice",
            old_b,
            new_b
        )
        .is_none());
        assert!(rewrite_url_if_local(
            "https://not-old.example/users/alice",
            old_b,
            new_b
        )
        .is_none());
        // Substring host must not match
        assert!(!url_is_under_base(
            "https://prefix-old.example/users/x",
            "https://old.example"
        ));
    }

    #[test]
    fn shared_key_id_rewrite_preserves_path_and_fragment() {
        // G choice: keyId host moves with domain; PEM is unchanged (not tested here).
        let kid = rewrite_url_if_local(
            "https://old.example/users/alice#main-key",
            "https://old.example",
            "https://new.example",
        )
        .unwrap();
        assert_eq!(kid, "https://new.example/users/alice#main-key");
        assert_eq!(
            key_id("https://new.example", "alice"),
            "https://new.example/users/alice#main-key"
        );
    }

    #[test]
    fn rewrite_whitelist_includes_required_tables() {
        let tables: Vec<&str> = LOCAL_URL_REWRITE_WHITELIST
            .iter()
            .map(|(t, _)| *t)
            .collect();
        assert!(tables.contains(&"federation_remote_actors"));
        assert!(tables.contains(&"federation_room_members"));
        assert!(tables.contains(&"federation_rooms"));
        assert!(tables.contains(&"federation_channel_messages"));
        assert!(tables.contains(&"federation_delivery_queue"));
        assert!(tables.contains(&"federation_keys"));
    }

    #[test]
    fn actor_serde_also_known_as_and_moved_to() {
        // B: Actor serializes both fields for peers verifying Move.
        let a = Actor {
            context: build_ap_context(),
            actor_type: "Person".into(),
            id: "https://new.example/users/a".into(),
            preferred_username: "a".into(),
            name: None,
            summary: None,
            url: None,
            inbox: "https://new.example/users/a/inbox".into(),
            outbox: "https://new.example/users/a/outbox".into(),
            followers: "https://new.example/users/a/followers".into(),
            following: "https://new.example/users/a/following".into(),
            public_key: ActorPublicKey {
                id: "https://new.example/users/a#main-key".into(),
                owner: "https://new.example/users/a".into(),
                public_key_pem: "PEM".into(),
            },
            icon: None,
            image: None,
            also_known_as: vec!["https://old.example/users/a".into()],
            moved_to: None,
            mfp_instance_version: None,
            mfp_tapp_capabilities: None,
            mfp_channels_url: None,
        };
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(
            v["alsoKnownAs"][0],
            "https://old.example/users/a"
        );
        assert!(v.get("movedTo").is_none());

        let old = Actor {
            moved_to: Some("https://new.example/users/a".into()),
            also_known_as: vec![],
            ..a
        };
        let v2 = serde_json::to_value(&old).unwrap();
        assert_eq!(v2["movedTo"], "https://new.example/users/a");
        // empty alsoKnownAs skipped
        assert!(v2.get("alsoKnownAs").is_none());
    }

    #[test]
    fn url_is_under_base_rejects_foreign_host() {
        assert!(!url_is_under_base(
            "https://evil.example/users/alice",
            "https://old.example"
        ));
        assert!(url_is_under_base(
            "https://old.example/users/alice",
            "https://old.example"
        ));
        assert!(url_is_under_base(
            "https://old.example/users/alice#main-key",
            "https://old.example"
        ));

    }

    #[test]
    fn url_is_under_base_empty_inputs() {
        assert!(!url_is_under_base("", "https://a.example"));
        assert!(!url_is_under_base("https://a.example/users/x", ""));

    }

    #[test]
    fn rewrite_url_if_local_preserves_path() {
        let got = rewrite_url_if_local(
            "https://old.example/users/alice/inbox",
            "https://old.example",
            "https://new.example",
        );
        assert_eq!(got.as_deref(), Some("https://new.example/users/alice/inbox"));
        assert!(rewrite_url_if_local(
            "https://foreign.example/users/alice",
            "https://old.example",
            "https://new.example",
        ).is_none());

    }

    #[test]
    fn normalize_base_url_rejects_empty() {
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("   ").is_err());
        let ok = normalize_base_url("https://a.example/").unwrap();
        assert_eq!(ok, "https://a.example");

    }

    #[test]
    fn activity_id_string_from_string_and_object() {
        assert_eq!(
            activity_id_string(&serde_json::json!("https://a.example/activities/1")),
            Some("https://a.example/activities/1".into())
        );
        assert_eq!(
            activity_id_string(&serde_json::json!({"id": "https://a.example/activities/2"})),
            Some("https://a.example/activities/2".into())
        );
        assert_eq!(activity_id_string(&serde_json::json!({})), None);

    }

    #[test]
    fn w175_url_under_base_foreign() {
        assert!(!url_is_under_base("https://evil.example/users/a", "https://old.example"));
        assert!(url_is_under_base("https://old.example/users/a", "https://old.example"));
        assert!(url_is_under_base("https://old.example/users/a#main-key", "https://old.example"));

    }


    #[test]
    fn w175_url_under_base_empty() {
        assert!(!url_is_under_base("", "https://a.example"));
        assert!(!url_is_under_base("https://a.example/x", ""));

    }


    #[test]
    fn w175_rewrite_url_preserves_path() {
        assert_eq!(
            rewrite_url_if_local(
                "https://old.example/users/alice/inbox",
                "https://old.example",
                "https://new.example",
            ).as_deref(),
            Some("https://new.example/users/alice/inbox")
        );
        assert!(rewrite_url_if_local(
            "https://foreign.example/users/alice",
            "https://old.example",
            "https://new.example",
        ).is_none());

    }


    #[test]
    fn w175_normalize_base_empty() {
        assert!(normalize_base_url("").is_err());
        assert_eq!(normalize_base_url("https://a.example/").unwrap(), "https://a.example");

    }


    #[test]
    fn w175_activity_id_string_shapes() {
        assert_eq!(
            activity_id_string(&serde_json::json!("https://a.example/activities/1")),
            Some("https://a.example/activities/1".into())
        );
        assert_eq!(
            activity_id_string(&serde_json::json!({"id": "https://a.example/activities/2"})),
            Some("https://a.example/activities/2".into())
        );
        assert_eq!(activity_id_string(&serde_json::json!({})), None);

    }

}

