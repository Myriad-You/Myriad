// Configuration types, build, settings backup, save, and public UI handlers.

use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};


/// Fixed-length secret mask shown to the UI (no real characters).
pub(crate) fn mask_secret_display_value() -> String {
    "••••••••".to_string()
}

/// True when the client re-submitted a masked secret (save must keep DB value;
/// platform_test should fall back to saved config / env).
pub(crate) fn is_masked_secret_value(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() {
        return false;
    }
    // save historically accepted both bullet and asterisk masks
    v.starts_with("••")
        || v.starts_with("**")
        || v == "********"
        || v == mask_secret_display_value()
        || (v.chars().all(|c| c == '•' || c == '*') && v.len() >= 4)
}

/// Form secret usable only when non-empty and not a mask.
pub(crate) fn form_secret_if_plaintext(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty() && !is_masked_secret_value(s))
        .map(|s| s.to_string())
}

/// Persist a platform credential field into DB updates.
///
/// Semantics (data platforms only — not AI/OAuth omit-empty-keep):
/// - masked (`••••` / `****…`) → skip (keep existing DB value)
/// - empty string → insert `""` so clear persists
/// - non-empty plaintext → set new value
fn insert_platform_field(
    updates: &mut std::collections::HashMap<String, Value>,
    db_key: &str,
    value: &str,
) {
    if is_masked_secret_value(value) {
        return;
    }
    updates.insert(db_key.to_string(), Value::String(value.to_string()));
}

/// Whether a platform field should be written to `.env`.
/// Mask keeps the existing env value; empty clears it.
fn should_write_platform_env_field(value: &str) -> bool {
    !is_masked_secret_value(value)
}

/// Extract a numeric playlist id from a bare id or a NetEase / QQ Music URL.
///
/// Config UI hints show full links (`?id=2884035`, `/playlist/8039305244`); the
/// music proxy only accepts digits. Unrecognized input is returned trimmed.
pub(crate) fn normalize_music_playlist_id(raw: &str) -> String {
    let s = raw.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return s.to_string();
    }
    // NetEase / generic: id=123456
    if let Some(idx) = s.find("id=") {
        let rest = &s[idx + 3..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits;
        }
    }
    // QQ Music path: …/playlist/8039305244
    if let Some(idx) = s.find("/playlist/") {
        let rest = &s[idx + "/playlist/".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits;
        }
    }
    // Fallback: longest consecutive digit run (len >= 5)
    let mut best = String::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else {
            if cur.len() > best.len() {
                best = cur.clone();
            }
            cur.clear();
        }
    }
    if cur.len() > best.len() {
        best = cur;
    }
    if best.len() >= 5 {
        best
    } else {
        s.to_string()
    }
}

/// Sanitize a wallpaper URL for persistence.
///
/// - Empty → empty (clear wallpaper)
/// - Absolute `http`/`https` only (no `data:`, `javascript:`, credentials)
/// - Same-origin path `/...` allowed
/// - Blocks loopback / private / link-local / special hostnames (visitor browsers
///   must not be pointed at intranet targets via public UI config)
///
/// Returns `None` when the value must not be stored as-is (caller should skip
/// the update rather than write a dangerous URL).
pub(crate) fn sanitize_wallpaper_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }

    // Same-origin path only (single leading slash, not protocol-relative //)
    if s.starts_with('/') && !s.starts_with("//") {
        // Reject `/javascript:...` style smuggling
        if s.len() > 1 {
            let rest = &s[1..];
            if let Some(colon) = rest.find(':') {
                let scheme = &rest[..colon];
                if scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
                    && !scheme.is_empty()
                    && scheme
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic())
                {
                    return None;
                }
            }
        }
        return Some(s.to_string());
    }

    let candidate = if let Some(rest) = s.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        s.to_string()
    };

    let parsed = url::Url::parse(&candidate).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    let host = parsed.host_str()?;
    if is_blocked_wallpaper_host(host) {
        return None;
    }
    Some(parsed.to_string())
}

fn is_blocked_wallpaper_host(host: &str) -> bool {
    let h = host.trim().trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
    if h.is_empty() {
        return true;
    }
    if h == "localhost"
        || h == "0.0.0.0"
        || h == "::"
        || h == "::1"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with(".arpa")
        || h.ends_with(".lan")
        || h.ends_with(".home")
        || h.ends_with(".corp")
    {
        return true;
    }

    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return is_blocked_wallpaper_ip(ip);
    }
    false
}

fn is_blocked_wallpaper_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                // CGNAT 100.64.0.0/10
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
                || v4.octets()[0] >= 224
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_wallpaper_ip(std::net::IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Same-origin path `/...` (not `//host`), rejecting `/javascript:...` smuggling.
fn sanitize_site_relative_path(s: &str) -> Option<String> {
    if !s.starts_with('/') || s.starts_with("//") {
        return None;
    }
    if s.len() > 1 {
        let rest = &s[1..];
        if let Some(colon) = rest.find(':') {
            let scheme = &rest[..colon];
            if !scheme.is_empty()
                && scheme
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
            {
                return None;
            }
        }
    }
    Some(s.to_string())
}

/// Favicon / logo: empty, same-origin path, http(s), or `data:image/*` (local upload).
/// Private hosts allowed (self-host LAN). No wallpaper-style host block.
pub(crate) fn sanitize_site_favicon_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    if let Some(path) = sanitize_site_relative_path(s) {
        return Some(path);
    }
    // data:image/... only (uploaded favicon); reject data:text/html etc.
    if let Some(rest) = s.strip_prefix("data:") {
        let lower = rest.to_ascii_lowercase();
        if lower.starts_with("image/") {
            return Some(s.to_string());
        }
        return None;
    }
    sanitize_http_url_allow_private(s)
}

/// OG / share image: empty, path, http(s). No data: (crawlers need fetchable URLs).
pub(crate) fn sanitize_site_og_image_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    if let Some(path) = sanitize_site_relative_path(s) {
        return Some(path);
    }
    sanitize_http_url_allow_private(s)
}

/// Tracker script URL (Umami): empty or http(s). Private hosts OK for self-host.
pub(crate) fn sanitize_umami_script_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    sanitize_http_url_allow_private(s)
}

/// Outbound HTTP proxy: empty, or http(s)/socks* (LAN proxies are normal).
pub(crate) fn sanitize_proxy_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let parsed = url::Url::parse(s).ok()?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" | "socks4" | "socks4a" => {}
        _ => return None,
    }
    parsed.host_str()?;
    Some(s.to_string())
}

/// API base URL (Gemini / GitHub / etc.): empty or http(s); private OK for reverse proxies.
///
/// Bare origins are stored **without** a trailing slash. `url::Url::to_string()`
/// normalizes `https://host` → `https://host/`; callers join paths onto the base,
/// so a trailing `/` would produce double slashes and break the UI persist tests.
pub(crate) fn sanitize_http_base_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let safe = sanitize_http_url_allow_private(s)?;
    Some(safe.trim_end_matches('/').to_string())
}

/// http(s) only; protocol-relative → https. **Does not** block private hosts.
fn sanitize_http_url_allow_private(raw: &str) -> Option<String> {
    let candidate = if let Some(rest) = raw.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        raw.to_string()
    };
    let parsed = url::Url::parse(&candidate).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    parsed.host_str()?;
    Some(parsed.to_string())
}

/// Apply a clearable URL field: empty writes empty; invalid skips the update.
fn insert_sanitized_clearable_url(
    updates: &mut std::collections::HashMap<String, serde_json::Value>,
    db_key: &str,
    raw: &str,
    sanitize: fn(&str) -> Option<String>,
) {
    match sanitize(raw) {
        Some(safe) => {
            updates.insert(db_key.to_string(), serde_json::Value::String(safe));
        }
        None => {
            tracing::warn!(
                key = db_key,
                value = %raw,
                "Rejecting config URL that failed scheme/format policy (keeping previous value)"
            );
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigResponse {
    pub platforms: Vec<PlatformConfig>,
    pub auto_fetch: Option<PlatformAutoFetchConfig>,
    pub ai_config: AiConfig,
    pub tripo_config: TripoConfig,
    pub report_config: ReportConfig,
    pub ui_config: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformAutoFetchConfig {
    pub enabled: bool,
    pub interval_hours: i32,
}

impl Default for PlatformAutoFetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 24,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct PlatformConfig {
    pub name: String,
    pub enabled: bool,
    pub has_token: bool,
    pub config_fields: Vec<ConfigField>,
    pub description: String,
    pub icon: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub value: String,
    pub placeholder: String,
    pub required: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub enabled: bool,
    // AI 图片生成配置
    pub image_provider: String,
    pub config_fields: Vec<ConfigField>,
}

/// 独立的 3D 生成配置。它不属于图片生成 provider；图片只作为 3D 管线输入。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TripoConfig {
    pub enabled: bool,
    pub configured: bool,
    pub config_fields: Vec<ConfigField>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportConfig {
    pub topic_style: String,
    pub config_fields: Vec<ConfigField>,
}

/// 管理端 `ui_config`：**只暴露 bag**（`config_fields`）。
/// 历史 typed 镜像字段（wallpaper/pet/theme/proxy…）已废弃——保存只读 bag，
/// 公开运行时配置走 `GET /api/config/ui`。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub config_fields: Vec<ConfigField>,
}

pub(crate) fn resolve_platform_enabled(explicit_enabled: Option<bool>, fallback_enabled: bool) -> bool {
    explicit_enabled.unwrap_or(fallback_enabled)
}

/// 按管理员配置的平台顺序对平台列表排序。
/// `order` 中的平台按其顺序排在前面，未列出的平台保持原有默认顺序排在最后。
pub(crate) fn sort_platforms_by_order(platforms: &mut [PlatformConfig], order: Option<&Vec<String>>) {
    let Some(order) = order else { return };
    let rank = |name: &str| -> usize {
        order
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(usize::MAX)
    };
    // 稳定排序：未列出的平台（rank == MAX）保持彼此间的默认相对顺序
    platforms.sort_by_key(|p| rank(&p.name));
}

/// DB Option 或 env 是否有非空字符串（空串 / 纯空白不算已配置）
pub(crate) fn nonempty_db(opt: Option<&String>) -> bool {
    opt.is_some_and(|s| !s.trim().is_empty())
}

pub(crate) fn nonempty_env(key: &str) -> bool {
    std::env::var(key).ok().is_some_and(|s| !s.trim().is_empty())
}

pub(crate) async fn build_config(db: &DatabaseConnection, reveal_sensitive: bool) -> ConfigResponse {
    let db = db.clone();
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Prefer DB when present (including intentional empty clear); else process env.
    // Same clearable semantics as SEO/analytics (`db_or_env_clearable`).
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_or_env_clearable(db_val, env_key, "")
    };

    // Helper to mask sensitive values (passwords, API keys, tokens).
    // Must stay aligned with [`is_masked_secret_value`] (save + platform_test).
    let mask_sensitive = |value: String| -> String {
        if value.trim().is_empty() || reveal_sensitive {
            if value.trim().is_empty() {
                String::new()
            } else {
                value
            }
        } else {
            mask_secret_display_value()
        }
    };

    let has_bangumi_username = nonempty_db(db_config.as_ref().and_then(|c| c.bangumi_username.as_ref()))
        || nonempty_env("BANGUMI_USERNAME");
    let has_bangumi_access_token =
        nonempty_db(db_config.as_ref().and_then(|c| c.bangumi_access_token.as_ref()))
            || nonempty_env("BANGUMI_ACCESS_TOKEN");
    let has_bangumi_identity = has_bangumi_username || has_bangumi_access_token;
    let github_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.github_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.github_username.as_ref()))
            || nonempty_env("GITHUB_USERNAME"),
    );
    let bilibili_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bilibili_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.bilibili_uid.as_ref()))
            || nonempty_env("BILIBILI_UID"),
    );
    let steam_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.steam_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.steam_api_key.as_ref()))
            || nonempty_env("STEAM_API_KEY"),
    );
    let has_youtube_key = nonempty_db(db_config.as_ref().and_then(|c| c.youtube_api_key.as_ref()))
        || nonempty_env("YOUTUBE_API_KEY");
    let has_youtube_channel =
        nonempty_db(db_config.as_ref().and_then(|c| c.youtube_channel_id.as_ref()))
            || nonempty_env("YOUTUBE_CHANNEL_ID");
    let youtube_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.youtube_enabled),
        has_youtube_key && has_youtube_channel,
    );
    let netease_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.netease_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.netease_user_id.as_ref()))
            || nonempty_env("NETEASE_USER_ID"),
    );
    let bangumi_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bangumi_enabled),
        has_bangumi_identity,
    );
    let has_x_username = nonempty_db(db_config.as_ref().and_then(|c| c.x_username.as_ref()))
        || nonempty_env("X_USERNAME");
    let has_x_bearer = nonempty_db(db_config.as_ref().and_then(|c| c.x_bearer_token.as_ref()))
        || nonempty_env("X_BEARER_TOKEN");
    let x_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.x_enabled),
        has_x_username && has_x_bearer,
    );
    let has_discord_token =
        nonempty_db(db_config.as_ref().and_then(|c| c.discord_access_token.as_ref()))
            || nonempty_env("DISCORD_ACCESS_TOKEN");
    let discord_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.discord_enabled),
        has_discord_token,
    );
    let has_mal_username = nonempty_db(db_config.as_ref().and_then(|c| c.mal_username.as_ref()))
        || nonempty_env("MAL_USERNAME");
    let mal_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.mal_enabled),
        has_mal_username,
    );
    let has_openxbl_key = nonempty_db(db_config.as_ref().and_then(|c| c.openxbl_api_key.as_ref()))
        || nonempty_env("OPENXBL_API_KEY")
        || nonempty_env("XBL_API_KEY");
    let has_xbox_gamertag = nonempty_db(db_config.as_ref().and_then(|c| c.xbox_gamertag.as_ref()))
        || nonempty_env("XBOX_GAMERTAG");
    let xbox_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.xbox_enabled),
        has_openxbl_key && has_xbox_gamertag,
    );
    let has_psn_npsso = nonempty_db(db_config.as_ref().and_then(|c| c.psn_npsso.as_ref()))
        || nonempty_env("PSN_NPSSO");
    let has_psn_online_id = nonempty_db(db_config.as_ref().and_then(|c| c.psn_online_id.as_ref()))
        || nonempty_env("PSN_ONLINE_ID");
    let psn_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.psn_enabled),
        has_psn_npsso && has_psn_online_id,
    );

    let config = ConfigResponse {
        platforms: vec![
            PlatformConfig {
                name: "GitHub".to_string(),
                enabled: github_enabled,
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.github_token.as_ref()))
                    || nonempty_env("GITHUB_TOKEN"),
                icon: "".to_string(),
                description: "Repos, stars, and contributions".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "GitHub Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.github_username.clone()),
                            "GITHUB_USERNAME",
                        ),
                        placeholder: "octocat".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "token".to_string(),
                        label: "Personal Access Token (Optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.github_token.clone()),
                            "GITHUB_TOKEN",
                        )),
                        placeholder: "ghp_xxxxxxxxxxxx (Increases API rate limit)".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "Bilibili".to_string(),
                enabled: bilibili_enabled,
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.bilibili_uid.as_ref()))
                    || nonempty_env("BILIBILI_UID"),
                icon: "".to_string(),
                description: "Favorites, anime, and viewing history".to_string(),
                config_fields: vec![ConfigField {
                    key: "uid".to_string(),
                    label: "User ID (UID)".to_string(),
                    field_type: "number".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.bilibili_uid.clone()),
                        "BILIBILI_UID",
                    ),
                    placeholder: "123456789".to_string(),
                    required: true,
                }],
            },
            PlatformConfig {
                name: "Steam".to_string(),
                enabled: steam_enabled,
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.steam_api_key.as_ref()))
                    || nonempty_env("STEAM_API_KEY"),
                icon: "".to_string(),
                description: "Library, wishlist, and play stats".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "Steam API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.steam_api_key.clone()),
                            "STEAM_API_KEY",
                        )),
                        placeholder: "Get from steamcommunity.com/dev/apikey".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "steam_id".to_string(),
                        label: "Steam ID".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.steam_id.clone()),
                            "STEAM_ID",
                        ),
                        placeholder: "76561198XXXXXXXXX".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "YouTube".to_string(),
                enabled: youtube_enabled,
                has_token: has_youtube_key,
                icon: "".to_string(),
                description: "Public channel stats and recent uploads".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "YouTube Data API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.youtube_api_key.clone()),
                            "YOUTUBE_API_KEY",
                        )),
                        placeholder: "Google Cloud → YouTube Data API v3 key".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "channel_id".to_string(),
                        label: "Channel ID or @handle".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.youtube_channel_id.clone()),
                            "YOUTUBE_CHANNEL_ID",
                        ),
                        placeholder: "UCxxxxx or @GoogleDevelopers".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "Netease Music".to_string(),
                enabled: netease_enabled,
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.netease_user_id.as_ref()))
                    || nonempty_env("NETEASE_USER_ID"),
                icon: "".to_string(),
                description: "Liked songs and music taste".to_string(),
                config_fields: vec![ConfigField {
                    key: "user_id".to_string(),
                    label: "User ID".to_string(),
                    field_type: "number".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.netease_user_id.clone()),
                        "NETEASE_USER_ID",
                    ),
                    placeholder: "Your Netease Cloud Music user ID".to_string(),
                    required: true,
                }],
            },
            PlatformConfig {
                name: "Bangumi".to_string(),
                enabled: bangumi_enabled,
                has_token: has_bangumi_identity,
                icon: "".to_string(),
                description: "Collections, ratings, and watching status".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "Bangumi Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.bangumi_username.clone()),
                            "BANGUMI_USERNAME",
                        ),
                        placeholder: "your Bangumi username".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.bangumi_access_token.clone()),
                            "BANGUMI_ACCESS_TOKEN",
                        )),
                        placeholder: "Bearer token for private collections".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_agent".to_string(),
                        label: "User-Agent".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.bangumi_user_agent.clone()),
                            "BANGUMI_USER_AGENT",
                        ),
                        placeholder: "myriad/Myriad".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "X".to_string(),
                enabled: x_enabled,
                has_token: has_x_bearer,
                icon: "".to_string(),
                description: "Profile and posts, with sharing".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "X Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.x_username.clone()),
                            "X_USERNAME",
                        ),
                        placeholder: String::new(),
                        required: true,
                    },
                    ConfigField {
                        key: "bearer_token".to_string(),
                        label: "Bearer Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.x_bearer_token.clone()),
                            "X_BEARER_TOKEN",
                        )),
                        placeholder: "From developer.x.com App keys (read-only sync)".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "Discord".to_string(),
                enabled: discord_enabled,
                has_token: has_discord_token,
                icon: "".to_string(),
                description: "Profile, servers, and linked accounts".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.discord_access_token.clone()),
                            "DISCORD_ACCESS_TOKEN",
                        )),
                        placeholder:
                            "OAuth user token (scopes: identify guilds connections)".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "refresh_token".to_string(),
                        label: "Refresh Token (Recommended)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.discord_refresh_token.clone()),
                            "DISCORD_REFRESH_TOKEN",
                        )),
                        placeholder:
                            "Optional; enables auto-refresh when access token expires".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_id".to_string(),
                        label: "User ID (auto-filled after test)".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.discord_user_id.clone()),
                            "DISCORD_USER_ID",
                        ),
                        placeholder: "Discord snowflake id".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "MyAnimeList".to_string(),
                enabled: mal_enabled,
                has_token: has_mal_username,
                icon: "".to_string(),
                description: "Anime / manga lists and scores".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "MyAnimeList Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.mal_username.clone()),
                            "MAL_USERNAME",
                        ),
                        placeholder: "your MAL username (required)".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "client_id".to_string(),
                        label: "Client ID (optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.mal_client_id.clone()),
                            "MAL_CLIENT_ID",
                        )),
                        placeholder:
                            "Optional — leave empty for public list (load.json); fill for official API (myanimelist.net/apiconfig)"
                                .to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "Xbox".to_string(),
                enabled: xbox_enabled,
                has_token: has_openxbl_key,
                icon: "".to_string(),
                description: "Achievements, Gamerscore, and recent games".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "gamertag".to_string(),
                        label: "Gamertag".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.xbox_gamertag.clone()),
                            "XBOX_GAMERTAG",
                        ),
                        placeholder: "Major Nelson 或 名字#1234".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "openxbl_api_key".to_string(),
                        label: "OpenXBL API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.openxbl_api_key.clone()),
                            "OPENXBL_API_KEY",
                        )),
                        placeholder: "From xbl.io profile".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "PlayStation".to_string(),
                enabled: psn_enabled,
                has_token: has_psn_npsso,
                icon: "".to_string(),
                description: "Trophies, trophy level, and recent games".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "online_id".to_string(),
                        label: "Online ID".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.psn_online_id.clone()),
                            "PSN_ONLINE_ID",
                        ),
                        placeholder: "Your PSN Online ID".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "npsso".to_string(),
                        label: "NPSSO Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.psn_npsso.clone()),
                            "PSN_NPSSO",
                        )),
                        placeholder: "64-char token from ca.account.sony.com".to_string(),
                        required: true,
                    },
                ],
            },
        ],
        auto_fetch: Some(PlatformAutoFetchConfig {
            enabled: db_config
                .as_ref()
                .is_some_and(|config| config.enable_auto_fetch),
            interval_hours: crate::services::platform_auto_refresh::clamp_interval_hours(
                db_config
                    .as_ref()
                    .map(|config| config.fetch_interval_hours)
                    .unwrap_or(24),
            ),
        }),
        ai_config: AiConfig {
            provider: db_config
                .as_ref()
                .map(|c| c.ai_provider.clone())
                .unwrap_or_else(|| {
                    std::env::var("AI_PROVIDER").unwrap_or_else(|_| "gemini".to_string())
                }),
            model: db_config
                .as_ref()
                .map(|c| c.gemini_model.clone())
                .unwrap_or_else(|| {
                    std::env::var("GEMINI_MODEL")
                        .unwrap_or_else(|_| "gemini-3.6-flash".to_string())
                }),
            api_key: get_value(
                db_config.as_ref().and_then(|c| c.gemini_api_key.clone()),
                "GEMINI_API_KEY",
            ),
            enabled: db_config
                .as_ref()
                .and_then(|c| c.gemini_api_key.as_ref())
                .is_some()
                || db_config
                    .as_ref()
                    .and_then(|c| c.openai_api_key.as_ref())
                    .is_some()
                || std::env::var("GEMINI_API_KEY").is_ok()
                || std::env::var("OPENAI_API_KEY").is_ok(),
            config_fields: vec![
                ConfigField {
                    key: "provider".to_string(),
                    label: "AI Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_PROVIDER").unwrap_or_else(|_| "openai".to_string())
                        }),
                    placeholder: "openai".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "gemini_api_key".to_string(),
                    label: "Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.gemini_api_key.clone()),
                        "GEMINI_API_KEY",
                    )),
                    placeholder: "Get from https://makersuite.google.com/app/apikey".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "gemini_model".to_string(),
                    label: "Gemini Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.gemini_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("GEMINI_MODEL")
                                .unwrap_or_else(|_| "gemini-3.6-flash".to_string())
                        }),
                    placeholder: "gemini-3.6-flash, gemini-3.1-pro-preview, gemini-2.5-flash, etc."
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "openai_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.openai_api_key.clone()),
                        "OPENAI_API_KEY",
                    )),
                    placeholder: "OpenAI API Key or compatible service key".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "openai_model".to_string(),
                    label: "OpenAI Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("OPENAI_MODEL")
                                .unwrap_or_else(|_| "minimax/minimax-m3".to_string())
                        }),
                    placeholder: "minimax/minimax-m3, gpt-5.6-terra, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "openai_base_url".to_string(),
                    label: "OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("OPENAI_BASE_URL")
                                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string())
                        }),
                    placeholder:
                        "https://openrouter.ai/api/v1 (base URL only, no /chat/completions)"
                            .to_string(),
                    required: false,
                },
                // Pro 模型配置
                ConfigField {
                    key: "pro_enabled".to_string(),
                    label: "Enable Pro Model".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_provider".to_string(),
                    label: "【Pro Model】AI Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_ai_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_AI_PROVIDER")
                                .unwrap_or_else(|_| "openai".to_string())
                        }),
                    placeholder: "openai".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "pro_gemini_api_key".to_string(),
                    label: "【Pro Model】Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.pro_gemini_api_key.clone()),
                        "PRO_GEMINI_API_KEY",
                    )),
                    placeholder: "Pro model Gemini API Key (leave empty to reuse standard)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_gemini_model".to_string(),
                    label: "【Pro Model】Gemini Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_gemini_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_GEMINI_MODEL")
                                .unwrap_or_else(|_| "gemini-3.1-pro-preview".to_string())
                        }),
                    placeholder: "gemini-3.1-pro-preview, gemini-3-flash-preview, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_openai_api_key".to_string(),
                    label: "【Pro Model】OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.pro_openai_api_key.clone()),
                        "PRO_OPENAI_API_KEY",
                    )),
                    placeholder: "Pro model OpenAI API Key (leave empty to reuse standard)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_openai_model".to_string(),
                    label: "【Pro Model】OpenAI Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_OPENAI_MODEL")
                                .unwrap_or_else(|_| "anthropic/claude-opus-5".to_string())
                        }),
                    placeholder: "anthropic/claude-opus-5, gpt-5.6-sol, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_openai_base_url".to_string(),
                    label: "【Pro Model】OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_OPENAI_BASE_URL")
                                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string())
                        }),
                    placeholder: "https://api.openai.com/v1 (leave empty to reuse standard)"
                        .to_string(),
                    required: false,
                },
                // AI 图片生成配置
                ConfigField {
                    key: "ai_image_provider".to_string(),
                    label: "Image Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_PROVIDER")
                                .unwrap_or_else(|_| "openrouter".to_string())
                        }),
                    placeholder: "openai (compatible), openrouter, or volcengine"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_model".to_string(),
                    label: "OpenAI Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_MODEL")
                                .unwrap_or_else(|_| "openai/gpt-image-2".to_string())
                        }),
                    placeholder: "openai/gpt-image-2".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_openai_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ai_image_openai_api_key.clone()),
                        "AI_IMAGE_OPENAI_API_KEY",
                    )),
                    placeholder: "Falls back to the standard OpenAI key when empty".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_openai_base_url".to_string(),
                    label: "OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_OPENAI_BASE_URL")
                                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
                        }),
                    placeholder: "https://api.openai.com/v1".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_openrouter_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ai_image_openrouter_api_key.clone()),
                        "AI_IMAGE_OPENROUTER_API_KEY",
                    )),
                    placeholder: "Falls back to the standard OpenRouter key when compatible".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_volcengine_api_key".to_string(),
                    label: "Volcengine Ark API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ai_image_volcengine_api_key.clone()),
                        "AI_IMAGE_VOLCENGINE_API_KEY",
                    )),
                    placeholder: "Ark API key".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_volcengine_base_url".to_string(),
                    label: "Volcengine Ark Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_volcengine_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_VOLCENGINE_BASE_URL").unwrap_or_else(|_| {
                                "https://ark.cn-beijing.volces.com/api/v3".to_string()
                            })
                        }),
                    placeholder: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
                    required: false,
                },
                // Lite 模型配置（与 Pro 使用同一字段协议：开关 + 字段留空回退 Standard）
                ConfigField {
                    key: "lite_enabled".to_string(),
                    label: "Enable Lite Model".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_provider".to_string(),
                    label: "【Lite Model】AI Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_ai_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_AI_PROVIDER")
                                .unwrap_or_else(|_| "openai".to_string())
                        }),
                    placeholder: "openai".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "lite_gemini_api_key".to_string(),
                    label: "【Lite Model】Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.lite_gemini_api_key.clone()),
                        "LITE_GEMINI_API_KEY",
                    )),
                    placeholder: "Leave empty to reuse Standard".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_gemini_model".to_string(),
                    label: "【Lite Model】Gemini Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_gemini_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_GEMINI_MODEL")
                                .unwrap_or_else(|_| "gemini-3.5-flash-lite".to_string())
                        }),
                    placeholder: "gemini-3.5-flash-lite".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_openai_api_key".to_string(),
                    label: "【Lite Model】OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.lite_openai_api_key.clone()),
                        "LITE_OPENAI_API_KEY",
                    )),
                    placeholder: "Leave empty to reuse Standard".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_openai_model".to_string(),
                    label: "【Lite Model】OpenAI Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_OPENAI_MODEL")
                                .unwrap_or_else(|_| "openai/gpt-oss-20b:free".to_string())
                        }),
                    placeholder: "openai/gpt-oss-20b:free, gpt-5.6-luna, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_openai_base_url".to_string(),
                    label: "【Lite Model】OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_OPENAI_BASE_URL").unwrap_or_else(|_| {
                                "https://openrouter.ai/api/v1".to_string()
                            })
                        }),
                    placeholder: "https://openrouter.ai/api/v1".to_string(),
                    required: false,
                },
                // 腾讯云语音服务配置 (TTS/ASR)
                ConfigField {
                    key: "tencent_secret_id".to_string(),
                    label: "Tencent Cloud Secret ID".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.tencent_secret_id.clone()),
                        "TENCENT_SECRET_ID",
                    )),
                    placeholder: "Get from https://console.cloud.tencent.com/cam/capi".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tencent_secret_key".to_string(),
                    label: "Tencent Cloud Secret Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.tencent_secret_key.clone()),
                        "TENCENT_SECRET_KEY",
                    )),
                    placeholder: "Keep this secret secure".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tencent_region".to_string(),
                    label: "Tencent Cloud Region".to_string(),
                    field_type: "select".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.tencent_region.clone()),
                        "TENCENT_REGION",
                    ),
                    placeholder: "ap-guangzhou".to_string(),
                    required: false,
                },
            ],
            image_provider: db_config
                .as_ref()
                .map(|c| c.ai_image_provider.clone())
                .unwrap_or_else(|| {
                    std::env::var("AI_IMAGE_PROVIDER")
                        .unwrap_or_else(|_| "openrouter".to_string())
                }),
        },
        tripo_config: TripoConfig {
            enabled: db_config
                .as_ref()
                .map(|c| c.tripo_enabled)
                .unwrap_or_else(|| {
                    std::env::var("TRIPO_ENABLED")
                        .ok()
                        .is_some_and(|v| v == "true" || v == "1")
                }),
            configured: nonempty_db(
                db_config.as_ref().and_then(|c| c.tripo_api_key.as_ref()),
            ) || nonempty_env("TRIPO_API_KEY"),
            config_fields: vec![
                ConfigField {
                    key: "tripo_enabled".to_string(),
                    label: "Enable Tripo 3D".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_enabled.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_ENABLED")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tripo_api_key".to_string(),
                    label: "Tripo API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.tripo_api_key.clone()),
                        "TRIPO_API_KEY",
                    )),
                    placeholder: "Get from platform.tripo3d.ai".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tripo_base_url".to_string(),
                    label: "Tripo API Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_BASE_URL").unwrap_or_else(|_| {
                                "https://openapi.tripo3d.ai/v3".to_string()
                            })
                        }),
                    placeholder: "https://openapi.tripo3d.ai/v3".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_model".to_string(),
                    label: "Default low-poly model".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_MODEL")
                                .unwrap_or_else(|_| "P1-20260311".to_string())
                        }),
                    placeholder: "P1-20260311".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_face_limit".to_string(),
                    label: "Default face limit".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_face_limit.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_FACE_LIMIT")
                                .unwrap_or_else(|_| "5000".to_string())
                        }),
                    placeholder: "5000".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_poll_interval_seconds".to_string(),
                    label: "Polling interval (seconds)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_poll_interval_seconds.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_POLL_INTERVAL_SECONDS")
                                .unwrap_or_else(|_| "2".to_string())
                        }),
                    placeholder: "2".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_task_timeout_seconds".to_string(),
                    label: "Task timeout (seconds)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_task_timeout_seconds.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_TASK_TIMEOUT_SECONDS")
                                .unwrap_or_else(|_| "900".to_string())
                        }),
                    placeholder: "900".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_max_download_mb".to_string(),
                    label: "Maximum stored model size (MB)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_max_download_mb.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_MAX_DOWNLOAD_MB")
                                .unwrap_or_else(|_| "64".to_string())
                        }),
                    placeholder: "64".to_string(),
                    required: true,
                },
            ],
        },
        report_config: ReportConfig {
            topic_style: db_config
                .as_ref()
                .map(|c| c.topic_style.clone())
                .unwrap_or_else(|| {
                    std::env::var("TOPIC_STYLE").unwrap_or_else(|_| "balanced".to_string())
                }),
            config_fields: vec![ConfigField {
                key: "topic_style".to_string(),
                label: "Report Topic Style".to_string(),
                field_type: "select".to_string(),
                value: db_config
                    .as_ref()
                    .map(|c| c.topic_style.clone())
                    .unwrap_or_else(|| {
                        std::env::var("TOPIC_STYLE").unwrap_or_else(|_| "balanced".to_string())
                    }),
                placeholder: "balanced".to_string(),
                required: true,
            }],
        },
        // 管理端 ui_config 仅 bag；typed 镜像已删除（见 UiConfig 注释）
        ui_config: UiConfig {
            // ui_config.config_fields 跨页共享大袋子；按设置 Section 归属 emit。
            // 死字段（无设置页入口）勿再 emit：
            // pet_*、wallpaper_parallax（legacy 仅 DB；公开 API 亦不再返回）
            // github_client_*（走 OAuth 专用端点 + legacy 平铺字段，勿进 admin bag）
            // 归属：
            // UI        → wallpaper_*, evocative_*, site_*, cloud_sponsors, pwa_enabled, base_url
            // Platforms → analytics_enabled
            // Modules   → music_*
            // Advanced  → proxy_*, gemini_base_url, github_api_base_url
            // OAuth     → 只读 base_url（编辑走 SiteUrlField 独立 API）
            config_fields: vec![
                ConfigField {
                    key: "wallpaper_url".to_string(),
                    label: "Wallpaper URL".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.ui_wallpaper_url.clone()),
                        "UI_WALLPAPER_URL",
                    ),
                    placeholder: "URL to wallpaper image or API endpoint".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "wallpaper_blur".to_string(),
                    label: "Wallpaper Blur (0-10)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_wallpaper_blur.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_WALLPAPER_BLUR").unwrap_or_else(|_| "3".to_string())
                        }),
                    placeholder: "3".to_string(),
                    required: false,
                },
                // Evocative 壁纸动效配置
                ConfigField {
                    key: "evocative_parallax".to_string(),
                    label: "微动效果".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_parallax.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_PARALLAX")
                                .unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_dynamic_blur".to_string(),
                    label: "动态模糊".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_dynamic_blur.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_DYNAMIC_BLUR")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_ripple".to_string(),
                    label: "涟漪效果".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_ripple.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_RIPPLE")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_fps".to_string(),
                    label: "动效帧率".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_fps.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_FPS").unwrap_or_else(|_| "30".to_string())
                        }),
                    placeholder: "30".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_ripple_quality".to_string(),
                    label: "涟漪画质".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_ripple_quality.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_RIPPLE_QUALITY")
                                .unwrap_or_else(|_| "0.85".to_string())
                        }),
                    placeholder: "0.85".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "analytics_enabled".to_string(),
                    label: "Enable visitor analytics".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.analytics_enabled.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("ANALYTICS_ENABLED")
                                .unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pwa_enabled".to_string(),
                    label: "Enable PWA".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pwa_enabled.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("PWA_ENABLED")
                                .unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_title".to_string(),
                    label: "网站标题".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.site_title.clone()),
                        "SITE_TITLE",
                    ),
                    placeholder: "Myriad - A myriad of lights, in one place.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_description".to_string(),
                    label: "网站描述".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.site_description.clone()),
                        "SITE_DESCRIPTION",
                    ),
                    placeholder: "A myriad of lights, in one place.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_favicon".to_string(),
                    label: "网站图标 URL".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.site_favicon.clone()),
                        "SITE_FAVICON",
                    ),
                    placeholder: "/favicon.webp 或 https://example.com/icon.png（支持站外链接）"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_keywords".to_string(),
                    label: "SEO 关键词".to_string(),
                    field_type: "text".to_string(),
                    // Clearable: empty DB wins over env (see db_or_env_clearable).
                    value: db_or_env_clearable(
                        db_config.as_ref().and_then(|c| c.site_keywords.clone()),
                        "SITE_KEYWORDS",
                        "",
                    ),
                    placeholder: "个人主页, 博客, 数字生活（逗号分隔）".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_og_image".to_string(),
                    label: "分享预览图".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config.as_ref().and_then(|c| c.site_og_image.clone()),
                        "SITE_OG_IMAGE",
                        "",
                    ),
                    placeholder: "https://example.com/og.png 或上传本地图片".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_noindex".to_string(),
                    label: "禁止搜索引擎收录".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.site_noindex.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("SITE_NOINDEX")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_visibility_policy".to_string(),
                    label: "搜索与 AI 可见性".to_string(),
                    field_type: "select".to_string(),
                    value: {
                        let noindex = db_config
                            .as_ref()
                            .map(|c| c.site_noindex)
                            .unwrap_or_else(|| {
                                std::env::var("SITE_NOINDEX")
                                    .map(|v| v == "true" || v == "1")
                                    .unwrap_or(false)
                            });
                        let raw = db_config
                            .as_ref()
                            .map(|c| c.site_visibility_policy.clone())
                            .filter(|s| !s.trim().is_empty())
                            .or_else(|| std::env::var("SITE_VISIBILITY_POLICY").ok())
                            .unwrap_or_default();
                        crate::api::seo_policy::normalize_visibility_policy(&raw, noindex).to_string()
                    },
                    placeholder: "ai_citation".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_ai_intro".to_string(),
                    label: "AI 站点简介".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config.as_ref().and_then(|c| c.site_ai_intro.clone()),
                        "SITE_AI_INTRO",
                        "",
                    ),
                    placeholder: "用 2～4 句话向 AI 说明本站是谁、有什么内容（写入 llms.txt）"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ga_measurement_id".to_string(),
                    label: "Google Analytics".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ga_measurement_id.clone()),
                        "GA_MEASUREMENT_ID",
                        "",
                    ),
                    placeholder: "G-XXXXXXXXXX".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "umami_website_id".to_string(),
                    label: "Umami Website ID".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.umami_website_id.clone()),
                        "UMAMI_WEBSITE_ID",
                        "",
                    ),
                    placeholder: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "umami_script_url".to_string(),
                    label: "Umami Script URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.umami_script_url.clone()),
                        "UMAMI_SCRIPT_URL",
                        "",
                    ),
                    placeholder: "https://cloud.umami.is/script.js".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_icp".to_string(),
                    label: "ICP 备案号".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.site_icp.clone())
                        .unwrap_or_default(),
                    placeholder: "如：京ICP备12345678号".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_gongan".to_string(),
                    label: "公安备案号".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.site_gongan.clone())
                        .unwrap_or_default(),
                    placeholder: "如：京公网安备11010802012345号".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "cloud_sponsors".to_string(),
                    label: "云赞助商".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.cloud_sponsors.clone())
                        .unwrap_or_default(),
                    placeholder: "cloudflare,edgeone,upyun（多个用逗号分隔）".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_footer_custom".to_string(),
                    label: "页脚自定义项".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.site_footer_custom.clone()),
                        "SITE_FOOTER_CUSTOM",
                        "",
                    ),
                    placeholder: r#"[{"text":"示例","icon":"/logo.webp","url":"https://example.com"}]"#
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "base_url".to_string(),
                    label: "Site Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.base_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://yourdomain.com (用于生成 OAuth 回调 URL)".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_enabled".to_string(),
                    label: "Enable Music Player".to_string(),
                    field_type: "checkbox".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.music_enabled.clone()),
                        "MUSIC_ENABLED",
                    ),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_source".to_string(),
                    label: "Music Source".to_string(),
                    field_type: "select".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.music_source.clone()),
                        "MUSIC_SOURCE",
                    ),
                    placeholder: "netease or qq".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_playlist_id".to_string(),
                    label: "Playlist ID".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.music_playlist_id.clone()),
                        "MUSIC_PLAYLIST_ID",
                    ),
                    placeholder: "Playlist ID from music platform".to_string(),
                    required: false,
                },
                // 内存节约（高级设置）
                ConfigField {
                    key: "memory_saver_enabled".to_string(),
                    label: "内存节约".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.memory_saver_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                // 网络代理配置
                ConfigField {
                    key: "proxy_enabled".to_string(),
                    label: "启用网络代理".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.proxy_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "proxy_url".to_string(),
                    label: "代理服务器地址".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.proxy_url.clone())
                        .unwrap_or_default(),
                    placeholder: "http://127.0.0.1:7890 或 socks5://127.0.0.1:1080".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "proxy_bypass".to_string(),
                    label: "代理绕过列表".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.proxy_bypass.clone())
                        .unwrap_or_default(),
                    placeholder: "localhost,127.0.0.1,.local".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "gemini_base_url".to_string(),
                    label: "Gemini API 基础 URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.gemini_base_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://generativelanguage.googleapis.com (留空使用默认)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "github_api_base_url".to_string(),
                    label: "GitHub API 基础 URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.github_api_base_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://api.github.com (留空使用默认)".to_string(),
                    required: false,
                },
            ],
        },
    };

    let mut config = config;
    sort_platforms_by_order(
        &mut config.platforms,
        db_config.as_ref().and_then(|c| c.platform_order.as_ref()),
    );

    config
}

pub async fn get_config(crate::extract::Db(db): crate::extract::Db) -> (StatusCode, Json<Value>) {
    let config = build_config(&db, false).await;
    (StatusCode::OK, Json(json!(config)))
}

/// 平台是否已配置好可同步的凭证（与报告页 enabled 开关无关）。
pub(crate) fn platform_config_is_ready(platform: &PlatformConfig) -> bool {
    let name = platform.name.trim().to_ascii_lowercase();
    let field_filled = |key: &str| {
        platform
            .config_fields
            .iter()
            .find(|f| f.key == key)
            .is_some_and(|f| !f.value.trim().is_empty())
    };

    match name.as_str() {
        // Bangumi：用户名或访问令牌任一即可
        "bangumi" => field_filled("username") || field_filled("access_token"),
        // Discord：OAuth 后 has_token，或字段里已有 token
        "discord" => platform.has_token || field_filled("access_token") || field_filled("token"),
        _ => {
            // 所有必填字段均有值（掩码也算已配置）
            let required: Vec<_> = platform
                .config_fields
                .iter()
                .filter(|f| f.required)
                .collect();
            if required.is_empty() {
                // 无必填时：任意字段有值，或 has_token
                platform.has_token
                    || platform
                        .config_fields
                        .iter()
                        .any(|f| !f.value.trim().is_empty())
            } else {
                required.iter().all(|f| !f.value.trim().is_empty())
            }
        }
    }
}

/// Clearable optional string: `Some` (including empty) is intentional DB state and
/// wins over env. `None` means never set → env → `default`.
///
/// Used for SEO/analytics fields so clearing the admin UI cannot be undone by a
/// leftover env var (e.g. `GA_MEASUREMENT_ID` in process environment).
pub(crate) fn db_or_env_clearable(
    db_val: Option<String>,
    env_key: &str,
    default: &str,
) -> String {
    match db_val {
        Some(v) => v,
        None => std::env::var(env_key).unwrap_or_else(|_| default.to_string()),
    }
}

pub(crate) async fn reconcile_platform_auto_refresh_with_config(
    db: &DatabaseConnection,
    config: &ConfigResponse,
) -> Result<crate::services::platform_auto_refresh::PlatformAutoRefreshSummary, String> {
    let auto_fetch = config.auto_fetch.clone().unwrap_or_default();
    let user_id = crate::api::profile::site_owner_user_id(db).await?;
    // 自动刷新覆盖所有「已配置」平台；报告页 enabled 不参与
    let platforms: Vec<String> = config
        .platforms
        .iter()
        .filter(|platform| platform_config_is_ready(platform))
        .map(|platform| platform.name.clone())
        .collect();
    crate::services::platform_auto_refresh::reconcile_platform_auto_refresh(
        db,
        user_id,
        auto_fetch.enabled,
        auto_fetch.interval_hours,
        &platforms,
    )
    .await
}

/// Rebuild core platform refresh tasks from persisted configuration.
/// Called on backend startup so the database configuration remains the source
/// of truth even after a restart or interrupted settings save.
pub async fn reconcile_platform_auto_refresh(
    db: &DatabaseConnection,
) -> Result<crate::services::platform_auto_refresh::PlatformAutoRefreshSummary, String> {
    let config = build_config(db, false).await;
    reconcile_platform_auto_refresh_with_config(db, &config).await
}

pub(crate) const SETTINGS_BACKUP_FORMAT: &str = "myriad-settings-backup";
pub(crate) const SETTINGS_BACKUP_VERSION: u32 = 2;
pub(crate) const MIN_SETTINGS_BACKUP_VERSION: u32 = 1;
pub(crate) const MAX_SETTINGS_BACKUP_ENTRIES: usize = 10_000;

pub(crate) fn default_setting_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingDescriptor {
    schema_version: u32,
    introduced_in_backup_version: u32,
}

// 这是配置备份唯一的后端注册表。新增或删除非 ConfigResponse 设置时只需要改这里；
// 恢复、预检和导出过滤全部从该注册表派生。
/// 备份/恢复 registry：含 legacy 键（`pet_*` / `ui_wallpaper_parallax` / `github_client_*` 等）。
/// 这些键仍可从旧备份还原到 DB，但**不再**进入管理端 `ui_config.config_fields` emit。
pub(crate) const REGISTERED_CONFIGURATION_KEYS_V1: &[&str] = &[
    "ai_image_model",
    "ai_image_openai_api_key",
    "ai_image_openai_base_url",
    "ai_image_openrouter_api_key",
    "ai_image_provider",
    "ai_image_volcengine_api_key",
    "ai_image_volcengine_base_url",
    "ai_provider",
    "allow_local_registration",
    "analytics_enabled",
    "tapp_private_install_cleanup",
    "tapp_private_install_inactivity_days",
    "bangumi_access_token",
    "bangumi_enabled",
    "bangumi_user_agent",
    "bangumi_username",
    "base_url",
    "bilibili_enabled",
    "bilibili_uid",
    "cloud_sponsors",
    "control_panel_layout",
    "control_panel_rows",
    "custom_platforms",
    "dashboard_layout",
    "dashboard_title",
    "discord_access_token",
    "discord_enabled",
    "discord_refresh_token",
    "discord_token_expires_at",
    "discord_user_id",
    "enable_auto_fetch",
    "fetch_interval_hours",
    "ga_measurement_id",
    "gemini_api_key",
    "gemini_base_url",
    "gemini_model",
    "github_api_base_url",
    "github_client_id",
    "github_client_secret",
    "github_enabled",
    "github_redirect_url",
    "github_token",
    "github_username",
    "guest_ai_cooldown_seconds",
    "guest_ai_daily_calls",
    "guest_ai_daily_tokens",
    "guest_perm_ai_analyze",
    "guest_perm_ai_chat",
    "guest_perm_ai_generate",
    "guest_perm_ai_image",
    "guest_perm_component_theme",
    "guest_perm_event_publish",
    "guest_perm_network_fetch",
    "lite_ai_provider",
    "lite_enabled",
    "lite_gemini_api_key",
    "lite_gemini_model",
    "lite_openai_api_key",
    "lite_openai_base_url",
    "lite_openai_model",
    "guest_perm_report_write",
    "guest_perm_scheduler_register",
    "guest_perm_shortcut_register",
    "guest_perm_speech_asr",
    "guest_perm_speech_tts",
    "hitokoto_config",
    "library_source_preferences",
    "mal_client_id",
    "mal_enabled",
    "mal_username",
    "module_visibility_preferences",
    "music_enabled",
    "music_playlist_id",
    "music_source",
    "netease_enabled",
    "netease_user_id",
    "oauth_providers",
    "openai_api_key",
    "openai_base_url",
    "openai_max_tokens",
    "openai_model",
    "openxbl_api_key",
    "pet_enabled",
    "pet_image_url",
    "platform_order",
    "pro_ai_provider",
    "pro_enabled",
    "pro_gemini_api_key",
    "pro_gemini_model",
    "pro_openai_api_key",
    "pro_openai_base_url",
    "pro_openai_model",
    "memory_saver_enabled",
    "proxy_bypass",
    "proxy_enabled",
    "proxy_url",
    "psn_enabled",
    "psn_npsso",
    "psn_online_id",
    "pwa_enabled",
    "report_settings",
    "site_description",
    "site_favicon",
    "site_footer_custom",
    "site_gongan",
    "site_icp",
    "site_ai_intro",
    "site_keywords",
    "site_noindex",
    "site_og_image",
    "site_title",
    "site_visibility_policy",
    "steam_api_key",
    "steam_enabled",
    "steam_id",
    "youtube_api_key",
    "youtube_channel_id",
    "youtube_enabled",
    "tapp_window_schemes",
    "tencent_region",
    "tencent_secret_id",
    "tencent_secret_key",
    "title_color",
    "title_font",
    "title_font_size",
    "topic_style",
    "tripo_api_key",
    "tripo_base_url",
    "tripo_enabled",
    "tripo_face_limit",
    "tripo_max_download_mb",
    "tripo_model",
    "tripo_poll_interval_seconds",
    "tripo_task_timeout_seconds",
    "ui_evocative_dynamic_blur",
    "ui_evocative_fps",
    "ui_evocative_parallax",
    "ui_evocative_ripple",
    "ui_evocative_ripple_quality",
    "ui_primary_color",
    "ui_secondary_color",
    "ui_theme",
    "ui_wallpaper_blur",
    "ui_wallpaper_parallax",
    "ui_wallpaper_url",
    "umami_script_url",
    "umami_website_id",
    "user_ai_cooldown_seconds",
    "user_ai_daily_calls",
    "user_ai_daily_tokens",
    "user_perm_ai_analyze",
    "user_perm_ai_chat",
    "user_perm_ai_generate",
    "user_perm_ai_image",
    "user_perm_component_theme",
    "user_perm_event_publish",
    "user_perm_network_fetch",
    "user_perm_report_write",
    "user_perm_scheduler_register",
    "user_perm_shortcut_register",
    "user_perm_speech_asr",
    "user_perm_speech_tts",
    "widget_theme",
    "x_bearer_token",
    "x_enabled",
    "x_username",
    "xbox_enabled",
    "xbox_gamertag",
];

pub(crate) fn settings_registry() -> std::collections::HashMap<&'static str, SettingDescriptor> {
    REGISTERED_CONFIGURATION_KEYS_V1
        .iter()
        .map(|key| {
            (
                *key,
                SettingDescriptor {
                    schema_version: 1,
                    introduced_in_backup_version: 1,
                },
            )
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsBackupEntry {
    pub key: String,
    pub value: Value,
    #[serde(default = "default_setting_schema_version")]
    pub schema_version: u32,
    pub description: Option<String>,
    pub category: Option<String>,
    pub is_encrypted: Option<bool>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsBackupUserPreferences {
    pub notification_preferences:
        crate::services::agent::notification_preferences::NotificationPreferences,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsBackup {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub contains_secrets: bool,
    pub configurations: Vec<SettingsBackupEntry>,
    pub effective_config: ConfigResponse,
    pub user_preferences: SettingsBackupUserPreferences,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsRestorePreview {
    pub backup_version: u32,
    pub current_version: u32,
    pub restore_count: usize,
    pub preserve_count: usize,
    pub ignored_count: usize,
    pub migrated_count: usize,
    pub invalid_count: usize,
    pub ignored_keys: Vec<String>,
    pub invalid_keys: Vec<String>,
}

pub(crate) struct SettingsRestorePlan {
    entries: Vec<SettingsBackupEntry>,
    preview: SettingsRestorePreview,
}

pub(crate) fn validate_settings_backup(backup: &SettingsBackup) -> Result<(), String> {
    if backup.format != SETTINGS_BACKUP_FORMAT {
        return Err("Unsupported settings backup format".to_string());
    }
    if !(MIN_SETTINGS_BACKUP_VERSION..=SETTINGS_BACKUP_VERSION).contains(&backup.version) {
        return Err(format!(
            "Unsupported settings backup version: {}",
            backup.version
        ));
    }
    if backup.configurations.len() > MAX_SETTINGS_BACKUP_ENTRIES {
        return Err("Settings backup contains too many configuration entries".to_string());
    }

    let mut keys = std::collections::HashSet::new();
    for entry in &backup.configurations {
        if entry.key.is_empty() || entry.key.len() > 255 {
            return Err("Settings backup contains an invalid configuration key".to_string());
        }
        if !keys.insert(entry.key.as_str()) {
            return Err(format!(
                "Settings backup contains duplicate key: {}",
                entry.key
            ));
        }
    }

    Ok(())
}

/// 敏感 key 判定。
///
/// 单一定义放在 `data_key`，这样"标记为已加密"和"实际加密"永远同源 ——
/// 修复前这个列只是个标签，值仍然明文落库。
fn is_sensitive_configuration_key(key: &str) -> bool {
    crate::services::data_key::is_sensitive_config_key(key)
}

fn merge_settings_backup_entries(
    backup: &SettingsBackup,
) -> std::collections::HashMap<String, SettingsBackupEntry> {
    let registry = settings_registry();
    let mut entries: std::collections::HashMap<String, SettingsBackupEntry> = backup
        .configurations
        .iter()
        .cloned()
        .map(|entry| (entry.key.clone(), entry))
        .collect();

    // v1 deployments may source values from environment variables. Only settings which already
    // existed in that backup version may be filled from its effective legacy snapshot; settings
    // introduced later must keep the current installation's value/default.
    for (key, value) in collect_database_updates(&backup.effective_config) {
        let Some(descriptor) = registry.get(key.as_str()) else {
            continue;
        };
        if descriptor.introduced_in_backup_version > backup.version {
            continue;
        }
        let is_encrypted = is_sensitive_configuration_key(&key);
        entries.entry(key.clone()).or_insert(SettingsBackupEntry {
            key,
            value,
            schema_version: 1,
            description: None,
            category: Some("general".to_string()),
            is_encrypted: Some(is_encrypted),
            is_public: Some(false),
        });
    }

    entries
}

fn migrate_setting_entry(
    mut entry: SettingsBackupEntry,
    descriptor: SettingDescriptor,
) -> Result<(SettingsBackupEntry, bool), String> {
    if entry.schema_version == 0 || entry.schema_version > descriptor.schema_version {
        return Err(format!(
            "unsupported schema version {} (current {})",
            entry.schema_version, descriptor.schema_version
        ));
    }

    let migrated = entry.schema_version < descriptor.schema_version;
    if migrated {
        // Per-setting migrations are intentionally centralized here. Add explicit transforms
        // before increasing a descriptor's schema_version; silent shape guessing is forbidden.
        return Err(format!(
            "missing migration from schema version {} to {}",
            entry.schema_version, descriptor.schema_version
        ));
    }

    entry.value = normalize_registered_setting_value(&entry.key, entry.value)?;
    entry.schema_version = descriptor.schema_version;
    Ok((entry, migrated))
}

fn normalize_registered_setting_value(key: &str, value: Value) -> Result<Value, String> {
    fn normalize<T: serde::de::DeserializeOwned + Serialize>(
        value: Value,
        transform: impl FnOnce(T) -> T,
    ) -> Result<Value, String> {
        let parsed = serde_json::from_value::<T>(value).map_err(|error| error.to_string())?;
        serde_json::to_value(transform(parsed)).map_err(|error| error.to_string())
    }

    match key {
        MODULE_VISIBILITY_PREFERENCES_KEY => {
            normalize::<ModuleVisibilityPreferences>(value, ModuleVisibilityPreferences::normalized)
        }
        HITOKOTO_CONFIG_KEY => normalize::<HitokotoConfig>(value, HitokotoConfig::normalized),
        REPORT_SETTINGS_KEY => normalize::<ReportSettings>(value, ReportSettings::normalized),
        "library_source_preferences" => normalize::<crate::api::profile::LibrarySourcePreferences>(
            value,
            crate::api::profile::LibrarySourcePreferences::normalized,
        ),
        "oauth_providers" => {
            normalize::<Vec<crate::config::OAuthProviderEntry>>(value, |providers| providers)
        }
        _ => Ok(value),
    }
}

pub(crate) fn build_settings_restore_plan(backup: &SettingsBackup) -> SettingsRestorePlan {
    let registry = settings_registry();
    let merged = merge_settings_backup_entries(backup);
    let mut entries = Vec::new();
    let mut ignored_keys = Vec::new();
    let mut invalid_keys = Vec::new();
    let mut migrated_count = 0;

    for (_, entry) in merged {
        let Some(descriptor) = registry.get(entry.key.as_str()).copied() else {
            ignored_keys.push(entry.key);
            continue;
        };
        let entry_key = entry.key.clone();
        match migrate_setting_entry(entry, descriptor) {
            Ok((entry, migrated)) => {
                migrated_count += usize::from(migrated);
                entries.push(entry);
            }
            Err(_) => invalid_keys.push(entry_key),
        }
    }

    entries.sort_by(|left, right| left.key.cmp(&right.key));
    ignored_keys.sort();
    invalid_keys.sort();
    let preserve_count = registry.len().saturating_sub(entries.len());
    let preview = SettingsRestorePreview {
        backup_version: backup.version,
        current_version: SETTINGS_BACKUP_VERSION,
        // Notification preferences are normalized and restored as one registered user setting.
        restore_count: entries.len() + 1,
        preserve_count,
        ignored_count: ignored_keys.len(),
        migrated_count,
        invalid_count: invalid_keys.len(),
        ignored_keys,
        invalid_keys,
    };

    SettingsRestorePlan { entries, preview }
}

// merged from settings.rs

pub async fn export_settings(
    State(db): State<DatabaseConnection>,
    user_id: i32,
) -> (StatusCode, Json<Value>) {
    let registry = settings_registry();
    let effective_config = build_config(&db, true).await;
    let rows = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT key, value, description, category, is_encrypted, is_public FROM configurations ORDER BY key"
                .to_string(),
        ))
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!("Failed to export settings: {}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to read settings"})),
            );
        }
    };

    let mut configurations = Vec::with_capacity(rows.len());
    for row in rows {
        let key: String = match row.try_get("", "key") {
            Ok(value) => value,
            Err(error) => {
                tracing::error!("Failed to decode configuration key: {}", error);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Failed to decode settings"})),
                );
            }
        };
        let Some(descriptor) = registry.get(key.as_str()) else {
            // Stale rows from removed settings are intentionally not re-exported.
            continue;
        };
        let entry = SettingsBackupEntry {
            value: match row.try_get("", "value") {
                // 备份**明文**导出：这样导出的文件可以恢复到任意新实例，不必
                // 同时带上密钥文件。这是有意的取舍 —— 导出是管理员主动执行的
                // 认证操作（双重 admin 校验），同一个管理员在设置页本来就能看到
                // 这些值；而加密要挡的是数据库副本泄露，那条路径上库里仍是密文。
                Ok(value) => crate::services::data_key::open_config_value(&key, value),
                Err(error) => {
                    tracing::error!("Failed to decode configuration value: {}", error);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Failed to decode settings"})),
                    );
                }
            },
            key,
            schema_version: descriptor.schema_version,
            description: row.try_get("", "description").ok().flatten(),
            category: row.try_get("", "category").ok().flatten(),
            is_encrypted: row.try_get("", "is_encrypted").ok().flatten(),
            is_public: row.try_get("", "is_public").ok().flatten(),
        };
        configurations.push(entry);
    }

    let mut exported_keys: std::collections::HashSet<String> = configurations
        .iter()
        .map(|entry| entry.key.clone())
        .collect();
    for (key, value) in collect_database_updates(&effective_config) {
        let Some(descriptor) = registry.get(key.as_str()) else {
            continue;
        };
        if !exported_keys.insert(key.clone()) {
            continue;
        }
        configurations.push(SettingsBackupEntry {
            schema_version: descriptor.schema_version,
            description: None,
            category: Some("general".to_string()),
            is_encrypted: Some(is_sensitive_configuration_key(&key)),
            is_public: Some(false),
            key,
            value,
        });
    }
    configurations.sort_by(|left, right| left.key.cmp(&right.key));

    let notification_preferences =
        crate::services::agent::notification_preferences::load(Some(&db), user_id).await;
    let backup = SettingsBackup {
        format: SETTINGS_BACKUP_FORMAT.to_string(),
        version: SETTINGS_BACKUP_VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        contains_secrets: true,
        configurations,
        effective_config,
        user_preferences: SettingsBackupUserPreferences {
            notification_preferences,
        },
    };

    (StatusCode::OK, Json(json!(backup)))
}

pub async fn preview_settings_restore(
    Json(backup): Json<SettingsBackup>,
) -> (StatusCode, Json<Value>) {
    if let Err(message) = validate_settings_backup(&backup) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": message})));
    }

    let plan = build_settings_restore_plan(&backup);
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "preview": plan.preview,
        })),
    )
}

pub async fn restore_settings(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    user_id: i32,
    Json(backup): Json<SettingsBackup>,
) -> (StatusCode, Json<Value>) {
    if let Err(message) = validate_settings_backup(&backup) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": message})));
    }

    let plan = build_settings_restore_plan(&backup);
    let preview = plan.preview.clone();
    let entries = plan.entries;
    let notification_preferences = backup
        .user_preferences
        .notification_preferences
        .normalized();

    let transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            tracing::error!("Failed to start settings restore transaction: {}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to start settings restore"})),
            );
        }
    };

    let restore_result: Result<(), sea_orm::DbErr> = async {
        for entry in entries {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"
                        INSERT INTO configurations
                            (key, value, description, category, is_encrypted, is_public, created_at, updated_at)
                        VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                        ON CONFLICT (key) DO UPDATE SET
                            value = EXCLUDED.value,
                            description = COALESCE(EXCLUDED.description, configurations.description),
                            category = COALESCE(EXCLUDED.category, configurations.category),
                            is_encrypted = COALESCE(EXCLUDED.is_encrypted, configurations.is_encrypted),
                            is_public = COALESCE(EXCLUDED.is_public, configurations.is_public),
                            updated_at = CURRENT_TIMESTAMP
                    "#,
                    vec![
                        entry.key.clone().into(),
                        // 备份里是明文（见 export_settings），落库前重新加密。
                        crate::services::data_key::seal_config_value(&entry.key, entry.value)
                            .into(),
                        entry.description.into(),
                        entry.category.into(),
                        entry.is_encrypted.into(),
                        entry.is_public.into(),
                    ],
                ))
                .await?;
        }

        let notification_value = serde_json::to_value(&notification_preferences)
            .map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
        let update_result = transaction
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET notification_preferences = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
                vec![notification_value.into(), user_id.into()],
            ))
            .await?;
        if update_result.rows_affected() == 0 {
            return Err(sea_orm::DbErr::Custom(
                "Authenticated user no longer exists".to_string(),
            ));
        }

        transaction.commit().await?;
        Ok(())
    }
    .await;

    if let Err(error) = restore_result {
        tracing::error!("Failed to restore settings: {}", error);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to restore settings: {}", error)})),
        );
    }

    crate::services::agent::notification_preferences::cache_restored(
        user_id,
        notification_preferences,
    )
    .await;

    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    match config_service.load_config().await {
        Ok(new_config) => {
            // Same Arc as AppState.dynamic_config after from_shared — write via State.
            *dynamic_config.write().await = new_config;
            crate::services::http_client::reload_global_client().await;
            crate::services::oauth::registry::REGISTRY.reload().await;
        }
        Err(error) => {
            tracing::error!("Settings restored but runtime reload failed: {}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Settings restored, but runtime reload failed"})),
            );
        }
    }

    if let Err(error) = reconcile_platform_auto_refresh(&db).await {
        tracing::error!(
            "Settings restored but platform auto-refresh reconciliation failed: {}",
            error
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Settings restored, but platform auto-refresh could not be updated"
            })),
        );
    }

    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Settings restored successfully",
            "requires_reload": true,
            "preview": preview
        })),
    )
}

#[cfg(test)]
mod settings_backup_tests {
    use super::*;

    fn empty_config() -> ConfigResponse {
        ConfigResponse::default()
    }

    fn backup_with_entries(configurations: Vec<SettingsBackupEntry>) -> SettingsBackup {
        SettingsBackup {
            format: SETTINGS_BACKUP_FORMAT.to_string(),
            version: SETTINGS_BACKUP_VERSION,
            exported_at: "2026-01-01T00:00:00Z".to_string(),
            contains_secrets: true,
            configurations,
            effective_config: empty_config(),
            user_preferences: SettingsBackupUserPreferences {
                notification_preferences: Default::default(),
            },
        }
    }

    fn entry(key: &str) -> SettingsBackupEntry {
        SettingsBackupEntry {
            key: key.to_string(),
            value: json!(true),
            schema_version: 1,
            description: None,
            category: None,
            is_encrypted: None,
            is_public: None,
        }
    }

    #[test]
    fn validates_versioned_backup_and_rejects_duplicate_keys() {
        let valid = backup_with_entries(vec![entry("oauth_providers"), entry("report_settings")]);
        assert!(validate_settings_backup(&valid).is_ok());

        let duplicate =
            backup_with_entries(vec![entry("report_settings"), entry("report_settings")]);
        assert!(validate_settings_backup(&duplicate)
            .unwrap_err()
            .contains("duplicate key"));
    }

    #[test]
    fn rejects_unknown_format_and_version() {
        let mut backup = backup_with_entries(Vec::new());
        backup.format = "legacy".to_string();
        assert!(validate_settings_backup(&backup).is_err());

        backup.format = SETTINGS_BACKUP_FORMAT.to_string();
        backup.version = MIN_SETTINGS_BACKUP_VERSION;
        assert!(validate_settings_backup(&backup).is_ok());

        backup.version = SETTINGS_BACKUP_VERSION + 1;
        assert!(validate_settings_backup(&backup).is_err());
    }

    #[test]
    fn deserializes_v1_entries_without_per_setting_schema_version() {
        let mut value = serde_json::to_value(backup_with_entries(vec![entry("github_enabled")]))
            .expect("backup should serialize");
        value["version"] = json!(MIN_SETTINGS_BACKUP_VERSION);
        value["configurations"][0]
            .as_object_mut()
            .expect("entry should be an object")
            .remove("schema_version");

        let backup: SettingsBackup =
            serde_json::from_value(value).expect("v1 backup should deserialize");
        assert_eq!(backup.configurations[0].schema_version, 1);
        assert!(validate_settings_backup(&backup).is_ok());
    }

    #[test]
    fn effective_config_collects_unmasked_credentials_for_migration() {
        let mut config = empty_config();
        config.platforms.push(PlatformConfig {
            name: "GitHub".to_string(),
            enabled: true,
            has_token: true,
            config_fields: vec![ConfigField {
                key: "token".to_string(),
                label: String::new(),
                field_type: "password".to_string(),
                value: "secret-token".to_string(),
                placeholder: String::new(),
                required: false,
            }],
            description: String::new(),
            icon: String::new(),
        });

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("github_token"), Some(&json!("secret-token")));
        assert_eq!(updates.get("github_enabled"), Some(&json!(true)));

        let raw_entry = SettingsBackupEntry {
            key: "github_token".to_string(),
            value: json!("database-token"),
            schema_version: 1,
            description: Some("credential".to_string()),
            category: Some("platform".to_string()),
            is_encrypted: Some(true),
            is_public: Some(false),
        };
        let mut backup = backup_with_entries(vec![raw_entry]);
        backup.version = MIN_SETTINGS_BACKUP_VERSION;
        backup.effective_config = config;
        let merged = merge_settings_backup_entries(&backup);
        assert_eq!(
            merged.get("github_token").map(|entry| &entry.value),
            Some(&json!("database-token"))
        );
        assert_eq!(
            merged
                .get("github_enabled")
                .and_then(|entry| entry.is_encrypted),
            Some(false)
        );
    }

    #[test]
    fn restore_plan_ignores_removed_keys_and_preserves_missing_current_keys() {
        let backup = backup_with_entries(vec![entry("github_enabled"), entry("removed_setting")]);
        let plan = build_settings_restore_plan(&backup);

        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].key, "github_enabled");
        assert_eq!(plan.preview.ignored_keys, vec!["removed_setting"]);
        assert!(plan.preview.preserve_count > 0);
    }

    #[test]
    fn restore_plan_rejects_future_per_setting_schema_without_failing_file() {
        let mut future = entry("report_settings");
        future.schema_version = 99;
        let plan = build_settings_restore_plan(&backup_with_entries(vec![future]));

        assert!(plan.entries.is_empty());
        assert_eq!(plan.preview.invalid_keys, vec!["report_settings"]);
    }

    #[test]
    fn restore_plan_normalizes_structured_options_against_current_schema() {
        let mut module_entry = entry(MODULE_VISIBILITY_PREFERENCES_KEY);
        module_entry.value = json!({
            "modules": {
                "library": "admin",
                "removed_module": "all"
            }
        });

        let plan = build_settings_restore_plan(&backup_with_entries(vec![module_entry]));
        let modules = plan.entries[0]
            .value
            .get("modules")
            .and_then(Value::as_object)
            .unwrap();
        assert_eq!(modules.get("library"), Some(&json!("admin")));
        assert!(modules.contains_key("brew"));
        assert!(!modules.contains_key("removed_module"));
    }

    #[test]
    fn auto_fetch_settings_are_persisted_and_interval_is_clamped() {
        let mut config = empty_config();
        config.auto_fetch = Some(PlatformAutoFetchConfig {
            enabled: true,
            interval_hours: 0,
        });

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("enable_auto_fetch"), Some(&json!(true)));
        assert_eq!(updates.get("fetch_interval_hours"), Some(&json!(1)));
    }

    #[test]
    fn missing_auto_fetch_settings_preserve_existing_values() {
        let config = empty_config();
        let updates = collect_database_updates(&config);

        assert!(!updates.contains_key("enable_auto_fetch"));
        assert!(!updates.contains_key("fetch_interval_hours"));
    }

    fn ui_field(key: &str, value: &str) -> ConfigField {
        ConfigField {
            key: key.to_string(),
            label: String::new(),
            field_type: "text".to_string(),
            value: value.to_string(),
            placeholder: String::new(),
            required: false,
        }
    }

    #[test]
    fn tripo_config_is_independent_clamped_and_keeps_masked_key() {
        let mut config = empty_config();
        config.tripo_config.config_fields = vec![
            ui_field("tripo_enabled", "true"),
            ui_field("tripo_api_key", "••••••••"),
            ui_field("tripo_model", "P1-20260311"),
            ui_field("tripo_face_limit", "99999"),
            ui_field("tripo_poll_interval_seconds", "1"),
            ui_field("tripo_task_timeout_seconds", "99999"),
            ui_field("tripo_max_download_mb", "999"),
        ];

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("tripo_enabled"), Some(&json!(true)));
        assert!(!updates.contains_key("tripo_api_key"));
        assert_eq!(updates.get("tripo_model"), Some(&json!("P1-20260311")));
        assert_eq!(updates.get("tripo_face_limit"), Some(&json!(20_000)));
        assert_eq!(
            updates.get("tripo_poll_interval_seconds"),
            Some(&json!(2))
        );
        assert_eq!(
            updates.get("tripo_task_timeout_seconds"),
            Some(&json!(3_600))
        );
        assert_eq!(updates.get("tripo_max_download_mb"), Some(&json!(150)));
    }

    #[test]
    fn ui_network_proxy_and_mirror_fields_can_be_cleared() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("proxy_url", ""),
            ui_field("proxy_bypass", ""),
            ui_field("gemini_base_url", ""),
            ui_field("github_api_base_url", ""),
            ui_field("proxy_enabled", "false"),
        ];

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("proxy_url"), Some(&json!("")));
        assert_eq!(updates.get("proxy_bypass"), Some(&json!("")));
        assert_eq!(updates.get("gemini_base_url"), Some(&json!("")));
        assert_eq!(updates.get("github_api_base_url"), Some(&json!("")));
        assert_eq!(updates.get("proxy_enabled"), Some(&json!(false)));
    }

    #[test]
    fn ui_memory_saver_flag_persists_bool() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("memory_saver_enabled", "true")];
        let on = collect_database_updates(&config);
        assert_eq!(on.get("memory_saver_enabled"), Some(&json!(true)));

        config.ui_config.config_fields = vec![ui_field("memory_saver_enabled", "false")];
        let off = collect_database_updates(&config);
        assert_eq!(off.get("memory_saver_enabled"), Some(&json!(false)));
    }

    #[test]
    fn ui_network_proxy_and_mirror_fields_persist_non_empty() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("proxy_url", "http://127.0.0.1:7890"),
            ui_field("proxy_bypass", "localhost,127.0.0.1"),
            ui_field("gemini_base_url", "https://gemini.example.com"),
            ui_field("github_api_base_url", "https://gh.example.com"),
        ];

        let updates = collect_database_updates(&config);
        assert_eq!(
            updates.get("proxy_url"),
            Some(&json!("http://127.0.0.1:7890"))
        );
        assert_eq!(
            updates.get("proxy_bypass"),
            Some(&json!("localhost,127.0.0.1"))
        );
        // Bare origins: strip Url's forced trailing `/` (joiners use `{base}/v1/...`)
        assert_eq!(
            updates.get("gemini_base_url"),
            Some(&json!("https://gemini.example.com"))
        );
        assert_eq!(
            updates.get("github_api_base_url"),
            Some(&json!("https://gh.example.com"))
        );
    }

    #[test]
    fn ui_secret_fields_skip_empty_and_masked_values() {
        let mut config = empty_config();

        // Empty secret must not overwrite
        config.ui_config.config_fields = vec![ui_field("github_client_secret", "")];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("github_client_secret"));

        // Masked secret must not overwrite
        config.ui_config.config_fields = vec![ui_field("github_client_secret", "••••••••")];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("github_client_secret"));

        // Real secret is persisted
        config.ui_config.config_fields = vec![ui_field("github_client_secret", "real-secret")];
        let updates = collect_database_updates(&config);
        assert_eq!(
            updates.get("github_client_secret"),
            Some(&json!("real-secret"))
        );
    }

    #[test]
    fn ui_clearable_fields_persist_empty_strings() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("site_title", ""),
            ui_field("site_description", ""),
            ui_field("site_favicon", ""),
            ui_field("site_keywords", ""),
            ui_field("site_og_image", ""),
            ui_field("site_noindex", "false"),
            ui_field("site_visibility_policy", "ai_full"),
            ui_field("site_ai_intro", ""),
            ui_field("ga_measurement_id", ""),
            ui_field("umami_website_id", ""),
            ui_field("umami_script_url", ""),
            ui_field("wallpaper_url", ""),
            ui_field("music_playlist_id", ""),
            ui_field("site_icp", ""),
            ui_field("site_footer_custom", ""),
        ];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_title"), Some(&json!("")));
        assert_eq!(updates.get("site_description"), Some(&json!("")));
        assert_eq!(updates.get("site_favicon"), Some(&json!("")));
        assert_eq!(updates.get("site_keywords"), Some(&json!("")));
        assert_eq!(updates.get("site_og_image"), Some(&json!("")));
        assert_eq!(updates.get("site_noindex"), Some(&json!(false)));
        // Both fields present: policy is authoritative; raw noindex alone must not
        // rewrite policy away from the explicit site_visibility_policy value.
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("ai_full"))
        );
        assert_eq!(updates.get("ga_measurement_id"), Some(&json!("")));
        assert_eq!(updates.get("umami_website_id"), Some(&json!("")));
        assert_eq!(updates.get("umami_script_url"), Some(&json!("")));
        assert_eq!(updates.get("ui_wallpaper_url"), Some(&json!("")));
        assert_eq!(updates.get("music_playlist_id"), Some(&json!("")));
        assert_eq!(updates.get("site_icp"), Some(&json!("")));
        assert_eq!(updates.get("site_footer_custom"), Some(&json!("")));
    }

    #[test]
    fn site_noindex_alone_syncs_visibility_policy() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("site_noindex", "true")];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_noindex"), Some(&json!(true)));
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("private"))
        );

        config.ui_config.config_fields = vec![ui_field("site_noindex", "false")];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_noindex"), Some(&json!(false)));
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("ai_full"))
        );
    }

    #[test]
    fn site_visibility_policy_overrides_raw_noindex_in_same_payload() {
        let mut config = empty_config();
        // Conflicting legacy bit + explicit policy: policy wins, noindex follows policy.
        config.ui_config.config_fields = vec![
            ui_field("site_noindex", "true"),
            ui_field("site_visibility_policy", "ai_full"),
        ];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_noindex"), Some(&json!(false)));
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("ai_full"))
        );
    }

    #[test]
    fn sanitize_wallpaper_url_allows_http_https_and_paths() {
        assert_eq!(sanitize_wallpaper_url(""), Some(String::new()));
        assert_eq!(
            sanitize_wallpaper_url("https://images.unsplash.com/photo-1"),
            Some("https://images.unsplash.com/photo-1".to_string())
        );
        assert_eq!(
            sanitize_wallpaper_url("/uploads/wall.jpg"),
            Some("/uploads/wall.jpg".to_string())
        );
        assert_eq!(
            sanitize_wallpaper_url("//cdn.example.com/a.jpg"),
            Some("https://cdn.example.com/a.jpg".to_string())
        );
    }

    #[test]
    fn sanitize_wallpaper_url_rejects_schemes_and_private_hosts() {
        assert_eq!(sanitize_wallpaper_url("javascript:alert(1)"), None);
        assert_eq!(sanitize_wallpaper_url("data:image/png;base64,aaa"), None);
        assert_eq!(sanitize_wallpaper_url("http://127.0.0.1/a.jpg"), None);
        assert_eq!(sanitize_wallpaper_url("http://192.168.1.1/a.jpg"), None);
        assert_eq!(sanitize_wallpaper_url("http://localhost/a.jpg"), None);
        assert_eq!(
            sanitize_wallpaper_url("https://user:pass@cdn.example.com/a.jpg"),
            None
        );
    }

    #[test]
    fn collect_rejects_unsafe_wallpaper_url() {
        let mut config = empty_config();
        config.ui_config.config_fields =
            vec![ui_field("wallpaper_url", "javascript:alert(1)")];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("ui_wallpaper_url"));

        config.ui_config.config_fields =
            vec![ui_field("wallpaper_url", "https://cdn.example.com/w.jpg")];
        let updates = collect_database_updates(&config);
        assert_eq!(
            updates.get("ui_wallpaper_url"),
            Some(&json!("https://cdn.example.com/w.jpg"))
        );
    }

    #[test]
    fn soft_url_fields_allow_normal_self_host_usage() {
        // Favicon: path, public, LAN, data:image
        assert_eq!(
            sanitize_site_favicon_url("/favicon.webp"),
            Some("/favicon.webp".to_string())
        );
        assert_eq!(
            sanitize_site_favicon_url("https://cdn.example.com/icon.png"),
            Some("https://cdn.example.com/icon.png".to_string())
        );
        assert_eq!(
            sanitize_site_favicon_url("http://192.168.1.5/logo.png"),
            Some("http://192.168.1.5/logo.png".to_string())
        );
        assert!(sanitize_site_favicon_url("data:image/png;base64,aaa")
            .unwrap()
            .starts_with("data:image/png"));
        assert_eq!(sanitize_site_favicon_url("javascript:alert(1)"), None);
        assert_eq!(sanitize_site_favicon_url("data:text/html,x"), None);

        // OG: no data:
        assert_eq!(
            sanitize_site_og_image_url("/og.png"),
            Some("/og.png".to_string())
        );
        assert_eq!(sanitize_site_og_image_url("data:image/png;base64,x"), None);

        // Umami + API base: http(s), private OK
        assert_eq!(
            sanitize_umami_script_url("http://10.0.0.2:3000/script.js"),
            Some("http://10.0.0.2:3000/script.js".to_string())
        );
        assert_eq!(sanitize_umami_script_url("javascript:x"), None);
        assert_eq!(
            sanitize_http_base_url("http://127.0.0.1:11434/v1"),
            Some("http://127.0.0.1:11434/v1".to_string())
        );
        // Bare origin must not keep the slash that Url::to_string() adds.
        assert_eq!(
            sanitize_http_base_url("https://gemini.example.com"),
            Some("https://gemini.example.com".to_string())
        );
        assert_eq!(
            sanitize_http_base_url("https://gemini.example.com/"),
            Some("https://gemini.example.com".to_string())
        );

        // Proxy: socks + localhost OK
        assert_eq!(
            sanitize_proxy_url("http://127.0.0.1:7890"),
            Some("http://127.0.0.1:7890".to_string())
        );
        assert_eq!(
            sanitize_proxy_url("socks5://127.0.0.1:1080"),
            Some("socks5://127.0.0.1:1080".to_string())
        );
        assert_eq!(sanitize_proxy_url("javascript:x"), None);
    }

    #[test]
    fn collect_soft_url_fields_reject_only_dangerous_schemes() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("site_favicon", "javascript:x"),
            ui_field("proxy_url", "http://127.0.0.1:7890"),
            ui_field("umami_script_url", "https://cloud.umami.is/script.js"),
        ];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("site_favicon"));
        assert_eq!(
            updates.get("proxy_url"),
            Some(&json!("http://127.0.0.1:7890"))
        );
        assert_eq!(
            updates.get("umami_script_url"),
            Some(&json!("https://cloud.umami.is/script.js"))
        );
    }

    #[test]
    fn clearable_db_empty_wins_over_env_fallback() {
        // UI clear writes Some(""); that must not be treated as "missing → env".
        assert_eq!(
            db_or_env_clearable(Some(String::new()), "GA_MEASUREMENT_ID", ""),
            ""
        );
        assert_eq!(
            db_or_env_clearable(Some("G-ABC".into()), "GA_MEASUREMENT_ID", ""),
            "G-ABC"
        );
        // None = never set; may use env (unset here → default).
        assert_eq!(
            db_or_env_clearable(None, "MYRIAD_TEST_UNSET_ENV_KEY_XYZ", "fallback"),
            "fallback"
        );
    }

    #[test]
    fn ui_empty_base_url_does_not_overwrite() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("base_url", "")];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("base_url"));

        config.ui_config.config_fields = vec![ui_field("base_url", "https://example.com")];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("base_url"), Some(&json!("https://example.com")));
    }

    #[test]
    fn normalize_music_playlist_id_accepts_urls() {
        assert_eq!(normalize_music_playlist_id("2884035"), "2884035");
        assert_eq!(
            normalize_music_playlist_id("https://music.163.com/#/playlist?id=2884035"),
            "2884035"
        );
        assert_eq!(
            normalize_music_playlist_id("https://y.qq.com/n/ryqq/playlist/8039305244"),
            "8039305244"
        );
        assert_eq!(normalize_music_playlist_id("  42  "), "42");
    }

    #[test]
    fn secret_env_fields_skip_masks() {
        assert!(!should_write_env_field("token", "••••••••"));
        assert!(!should_write_env_field("api_key", "********"));
        // AI/OAuth secrets still treat empty as "keep" (omit write).
        assert!(!should_write_env_field("gemini_api_key", ""));
        assert!(should_write_env_field("token", "ghp_real_token"));
        assert!(should_write_env_field("username", "octocat"));
        assert!(should_write_env_field("username", ""));
    }

    #[test]
    fn platform_env_fields_write_empty_but_skip_masks() {
        assert!(!should_write_platform_env_field("••••••••"));
        assert!(!should_write_platform_env_field("********"));
        // Empty platform secrets/usernames must clear .env (not keep).
        assert!(should_write_platform_env_field(""));
        assert!(should_write_platform_env_field("ghp_real_token"));
        assert!(should_write_platform_env_field("octocat"));
    }

    #[test]
    fn platform_credentials_clear_empty_username_and_token() {
        let mut config = empty_config();
        config.platforms.push(PlatformConfig {
            name: "GitHub".to_string(),
            enabled: true,
            has_token: true,
            config_fields: vec![
                ui_field("username", "octocat"),
                ConfigField {
                    key: "token".to_string(),
                    label: String::new(),
                    field_type: "password".to_string(),
                    value: "ghp_set".to_string(),
                    placeholder: String::new(),
                    required: false,
                },
            ],
            description: String::new(),
            icon: String::new(),
        });
        let set = collect_database_updates(&config);
        assert_eq!(set.get("github_username"), Some(&json!("octocat")));
        assert_eq!(set.get("github_token"), Some(&json!("ghp_set")));

        // Clear both with empty strings (not masks).
        config.platforms[0].config_fields = vec![
            ui_field("username", ""),
            ConfigField {
                key: "token".to_string(),
                label: String::new(),
                field_type: "password".to_string(),
                value: String::new(),
                placeholder: String::new(),
                required: false,
            },
        ];
        let cleared = collect_database_updates(&config);
        assert_eq!(cleared.get("github_username"), Some(&json!("")));
        assert_eq!(cleared.get("github_token"), Some(&json!("")));
    }

    #[test]
    fn platform_credentials_mask_keeps_secret() {
        let mut config = empty_config();
        config.platforms.push(PlatformConfig {
            name: "GitHub".to_string(),
            enabled: true,
            has_token: true,
            config_fields: vec![
                ui_field("username", "octocat"),
                ConfigField {
                    key: "token".to_string(),
                    label: String::new(),
                    field_type: "password".to_string(),
                    value: "••••••••".to_string(),
                    placeholder: String::new(),
                    required: false,
                },
            ],
            description: String::new(),
            icon: String::new(),
        });
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("github_username"), Some(&json!("octocat")));
        assert!(!updates.contains_key("github_token"));
    }

    #[test]
    fn platform_clear_covers_bangumi_x_steam_and_psn() {
        let mut config = empty_config();
        for (name, fields) in [
            (
                "Bangumi",
                vec![
                    ("username", ""),
                    ("access_token", ""),
                    ("user_agent", ""),
                ],
            ),
            ("X", vec![("username", ""), ("bearer_token", "")]),
            ("Steam", vec![("api_key", ""), ("steam_id", "")]),
            ("PlayStation", vec![("online_id", ""), ("npsso", "")]),
        ] {
            config.platforms.push(PlatformConfig {
                name: name.to_string(),
                enabled: false,
                has_token: false,
                config_fields: fields
                    .into_iter()
                    .map(|(k, v)| ui_field(k, v))
                    .collect(),
                description: String::new(),
                icon: String::new(),
            });
        }
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("bangumi_username"), Some(&json!("")));
        assert_eq!(updates.get("bangumi_access_token"), Some(&json!("")));
        assert_eq!(updates.get("x_username"), Some(&json!("")));
        assert_eq!(updates.get("x_bearer_token"), Some(&json!("")));
        assert_eq!(updates.get("steam_api_key"), Some(&json!("")));
        assert_eq!(updates.get("steam_id"), Some(&json!("")));
        assert_eq!(updates.get("psn_online_id"), Some(&json!("")));
        assert_eq!(updates.get("psn_npsso"), Some(&json!("")));
    }

    #[test]
    fn platform_resolve_prefers_explicit_empty_db_over_env() {
        let env_key = "MYRIAD_TEST_PLATFORM_RESOLVE_EMPTY";
        std::env::set_var(env_key, "stale-from-env");
        assert_eq!(db_or_env_clearable(Some(String::new()), env_key, ""), "");
        assert_eq!(
            db_or_env_clearable(None, env_key, ""),
            "stale-from-env"
        );
        assert_eq!(
            db_or_env_clearable(Some("from-db".to_string()), env_key, ""),
            "from-db"
        );
        std::env::remove_var(env_key);
    }

    #[test]
    fn update_env_var_clears_platform_secret_line() {
        let content = "GITHUB_TOKEN=ghp_old\nGITHUB_USERNAME=octocat\n";
        let next = update_env_var(content, "GITHUB_TOKEN", "");
        assert!(
            next.lines().any(|l| l == "# GITHUB_TOKEN="),
            "empty secret should comment out env key, got:\n{next}"
        );
        assert!(next.contains("GITHUB_USERNAME=octocat"));
        let next = update_env_var(&next, "GITHUB_USERNAME", "");
        assert!(next.lines().any(|l| l == "# GITHUB_USERNAME="));
    }

    #[test]
    fn remove_env_keys_strips_active_and_commented_db_only_lines() {
        let content = "\
DATABASE_URL=postgres://x\n\
UI_WALLPAPER_URL=https://example.com/a.jpg\n\
# GEMINI_API_KEY=old\n\
GITHUB_TOKEN=ghp_x\n\
BASE_URL=https://site.example\n\
PROXY_ENABLED=true\n\
";
        let next = remove_env_keys(content, DB_ONLY_ENV_KEYS);
        assert!(next.contains("DATABASE_URL=postgres://x"));
        assert!(next.contains("BASE_URL=https://site.example"));
        assert!(next.contains("PROXY_ENABLED=true"));
        assert!(!next.contains("UI_WALLPAPER_URL"));
        assert!(!next.contains("GEMINI_API_KEY"));
        assert!(!next.contains("GITHUB_TOKEN"));
    }

    #[test]
    fn db_only_env_keys_covers_wallpaper_and_ai() {
        assert!(DB_ONLY_ENV_KEYS.contains(&"UI_WALLPAPER_URL"));
        assert!(DB_ONLY_ENV_KEYS.contains(&"GEMINI_API_KEY"));
        assert!(DB_ONLY_ENV_KEYS.contains(&"GITHUB_TOKEN"));
        // Deploy keys must NOT be purged
        assert!(!DB_ONLY_ENV_KEYS.contains(&"BASE_URL"));
        assert!(!DB_ONLY_ENV_KEYS.contains(&"PROXY_URL"));
        assert!(!DB_ONLY_ENV_KEYS.contains(&"GITHUB_CLIENT_ID"));
    }

    #[test]
    fn music_playlist_id_normalized_in_db_updates() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field(
            "music_playlist_id",
            "https://music.163.com/#/playlist?id=2884035",
        )];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("music_playlist_id"), Some(&json!("2884035")));
    }
}

// merged from save.rs

pub async fn update_config(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Json(payload): Json<ConfigResponse>,
) -> Result<Json<Value>, crate::error::HttpError> {
    use crate::error::HttpError;
    use axum::http::StatusCode;

    tracing::info!("Updating configuration");

    // 1. 保存到数据库
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    if let Err(e) = save_to_database(&config_service, &payload).await {
        tracing::error!("Failed to save configuration to database: {}", e);
        return Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to save configuration to database: {}", e)
            })),
        )));
    }
    tracing::info!("✅ Configuration saved to database");

    // 2. Sync deploy keys to .env (BASE_URL / OAuth client / proxy); purge DB-only dual-writes
    let body = match save_all_configs(&payload).await {
        Ok(_) => {
            tracing::info!("✅ Deploy env synced; DB-only app keys purged from .env");
            json!({
                "success": true,
                "message": "Configuration saved successfully! Changes will be applied automatically within a few seconds."
            })
        }
        Err(e) => {
            tracing::error!("Failed to save configuration to .env: {}", e);
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to save configuration: {}", e)
                })),
            )));
        }
    };

    // 3. 更新全局动态配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            // 内存节约档：立即收紧并发/缓存/Argon2；DB 池在下次建连/重启后生效
            crate::services::memory_profile::apply_from_saver_flag(
                new_config.memory_saver_enabled,
            );
            *dynamic_config.write().await = new_config;
            tracing::info!("✅ Dynamic configuration cache updated");

            // 3.1 重载全局 HTTP 客户端（以应用新的代理配置）
            crate::services::http_client::reload_global_client().await;
            tracing::info!("✅ Global HTTP client reloaded with new proxy settings");

            // 3.2 兼容旧配置保存路径：如果 github_client_id/secret 仍由
            // /api/config 写入，也要让 OAuth provider 列表立即生效。
            crate::services::oauth::registry::REGISTRY.reload().await;
            tracing::info!("✅ OAuth provider registry reloaded");
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to reload dynamic config into cache: {}", e);
        }
    }

    // 4. The platform settings are the source of truth for Core scheduler
    // tasks. Reconcile immediately so the saved switch/frequency takes effect
    // without waiting for a backend restart.
    if let Err(error) = reconcile_platform_auto_refresh(&db).await {
        tracing::error!("Failed to reconcile platform auto-refresh tasks: {}", error);
        return Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!(
                    "Configuration was saved, but platform auto-refresh could not be updated: {}",
                    error
                )
            })),
        )));
    }

    // 5. 触发配置重载标志(虽然数据库连接可能不变,但确保其他服务知道配置已更新)
    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    tracing::info!(
        "🔄 Configuration reload flag set - changes will be picked up within 2-3 seconds"
    );

    Ok(Json(body))
}

/// 保存配置到数据库
async fn save_to_database(
    config_service: &crate::services::config_service::ConfigService,
    config: &ConfigResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    config_service
        .update_configs(collect_database_updates(config))
        .await?;
    Ok(())
}

fn collect_database_updates(config: &ConfigResponse) -> std::collections::HashMap<String, Value> {
    use serde_json::Value as JsonValue;
    use std::collections::HashMap;

    let mut updates: HashMap<String, JsonValue> = HashMap::new();

    if let Some(auto_fetch) = &config.auto_fetch {
        updates.insert(
            "enable_auto_fetch".to_string(),
            JsonValue::Bool(auto_fetch.enabled),
        );
        updates.insert(
            "fetch_interval_hours".to_string(),
            JsonValue::Number(
                crate::services::platform_auto_refresh::clamp_interval_hours(
                    auto_fetch.interval_hours,
                )
                .into(),
            ),
        );
    }

    // Shared with types_build / platform_test — see `is_masked_secret_value`.
    let is_masked = is_masked_secret_value;

    // 保存平台配置（空串 = 清除；掩码 = 保留；明文 = 写入）
    for platform in &config.platforms {
        match platform.name.as_str() {
            "GitHub" => {
                updates.insert(
                    "github_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "github_username",
                        "token" => "github_token",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "Bilibili" => {
                updates.insert(
                    "bilibili_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    if field.key == "uid" {
                        insert_platform_field(&mut updates, "bilibili_uid", &field.value);
                    }
                }
            }
            "Steam" => {
                updates.insert(
                    "steam_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "api_key" => "steam_api_key",
                        "steam_id" => "steam_id",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "YouTube" => {
                updates.insert(
                    "youtube_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "api_key" => "youtube_api_key",
                        "channel_id" => "youtube_channel_id",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "Netease Music" => {
                updates.insert(
                    "netease_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    if field.key == "user_id" {
                        insert_platform_field(&mut updates, "netease_user_id", &field.value);
                    }
                }
            }
            "Bangumi" => {
                updates.insert(
                    "bangumi_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "bangumi_username",
                        "access_token" => "bangumi_access_token",
                        "user_agent" => "bangumi_user_agent",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "X" => {
                updates.insert("x_enabled".to_string(), JsonValue::Bool(platform.enabled));
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "x_username",
                        "bearer_token" => "x_bearer_token",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "Discord" => {
                updates.insert(
                    "discord_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "access_token" => "discord_access_token",
                        "refresh_token" => "discord_refresh_token",
                        "user_id" => "discord_user_id",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "MyAnimeList" => {
                updates.insert("mal_enabled".to_string(), JsonValue::Bool(platform.enabled));
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "mal_username",
                        "client_id" => "mal_client_id",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "Xbox" => {
                updates.insert(
                    "xbox_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "gamertag" => "xbox_gamertag",
                        "openxbl_api_key" => "openxbl_api_key",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "PlayStation" => {
                updates.insert("psn_enabled".to_string(), JsonValue::Bool(platform.enabled));
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "online_id" => "psn_online_id",
                        "npsso" => "psn_npsso",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            _ => {}
        }
    }

    // 保存平台展示顺序（按前端提交的平台数组顺序）
    if !config.platforms.is_empty() {
        let platform_order: Vec<String> = config.platforms.iter().map(|p| p.name.clone()).collect();
        if let Ok(order_value) = serde_json::to_value(&platform_order) {
            updates.insert("platform_order".to_string(), order_value);
        }
    }

    // 保存 AI 配置
    for field in &config.ai_config.config_fields {
        let (key, json_value) = match field.key.as_str() {
            "provider" => ("ai_provider", JsonValue::String(field.value.clone())),
            "gemini_api_key" => ("gemini_api_key", JsonValue::String(field.value.clone())),
            "gemini_model" => ("gemini_model", JsonValue::String(field.value.clone())),
            "openai_api_key" => ("openai_api_key", JsonValue::String(field.value.clone())),
            "openai_model" => ("openai_model", JsonValue::String(field.value.clone())),
            "openai_base_url" => ("openai_base_url", JsonValue::String(field.value.clone())),
            // Pro 模型配置
            "pro_enabled" => ("pro_enabled", JsonValue::Bool(field.value == "true")),
            "pro_provider" => ("pro_ai_provider", JsonValue::String(field.value.clone())),
            "pro_gemini_api_key" => ("pro_gemini_api_key", JsonValue::String(field.value.clone())),
            "pro_gemini_model" => ("pro_gemini_model", JsonValue::String(field.value.clone())),
            "pro_openai_api_key" => ("pro_openai_api_key", JsonValue::String(field.value.clone())),
            "pro_openai_model" => ("pro_openai_model", JsonValue::String(field.value.clone())),
            "pro_openai_base_url" => (
                "pro_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            // AI Lite 模型配置
            "lite_enabled" => ("lite_enabled", JsonValue::Bool(field.value == "true")),
            "lite_provider" => ("lite_ai_provider", JsonValue::String(field.value.clone())),
            "lite_gemini_api_key" => (
                "lite_gemini_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "lite_gemini_model" => ("lite_gemini_model", JsonValue::String(field.value.clone())),
            "lite_openai_api_key" => (
                "lite_openai_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "lite_openai_model" => ("lite_openai_model", JsonValue::String(field.value.clone())),
            "lite_openai_base_url" => (
                "lite_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            // AI 图片生成配置
            "ai_image_provider" => ("ai_image_provider", JsonValue::String(field.value.clone())),
            "ai_image_model" => ("ai_image_model", JsonValue::String(field.value.clone())),
            "ai_image_openai_api_key" => (
                "ai_image_openai_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_openai_base_url" => (
                "ai_image_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_openrouter_api_key" => (
                "ai_image_openrouter_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_volcengine_api_key" => (
                "ai_image_volcengine_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_volcengine_base_url" => (
                "ai_image_volcengine_base_url",
                JsonValue::String(field.value.clone()),
            ),
            // 腾讯云语音服务配置 (TTS/ASR)
            "tencent_secret_id" => ("tencent_secret_id", JsonValue::String(field.value.clone())),
            "tencent_secret_key" => ("tencent_secret_key", JsonValue::String(field.value.clone())),
            "tencent_region" => ("tencent_region", JsonValue::String(field.value.clone())),
            _ => continue,
        };
        // 忽略屏蔽值（前端返回的掩码）- 保持数据库原值不变
        if !field.value.is_empty() && !is_masked(&field.value) {
            updates.insert(key.to_string(), json_value);
        }
    }

    // 保存独立 Tripo 3D 配置。密钥掩码必须保留库中原值。
    for field in &config.tripo_config.config_fields {
        let value = field.value.trim();
        match field.key.as_str() {
            "tripo_enabled" => {
                updates.insert(
                    "tripo_enabled".to_string(),
                    JsonValue::Bool(value == "true" || value == "1"),
                );
            }
            "tripo_api_key" if !value.is_empty() && !is_masked(value) => {
                updates.insert(
                    "tripo_api_key".to_string(),
                    JsonValue::String(value.to_string()),
                );
            }
            "tripo_base_url" | "tripo_model" if !value.is_empty() => {
                updates.insert(field.key.clone(), JsonValue::String(value.to_string()));
            }
            "tripo_face_limit" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(50, 20_000).into()),
                    );
                }
            }
            "tripo_poll_interval_seconds" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(2, 60).into()),
                    );
                }
            }
            "tripo_task_timeout_seconds" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(60, 3_600).into()),
                    );
                }
            }
            "tripo_max_download_mb" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(1, 150).into()),
                    );
                }
            }
            _ => {}
        }
    }

    // 保存报告配置
    for field in &config.report_config.config_fields {
        if field.key == "topic_style" && !field.value.is_empty() {
            updates.insert(
                "topic_style".to_string(),
                JsonValue::String(field.value.clone()),
            );
        }
    }

    // 保存 UI 配置
    for field in &config.ui_config.config_fields {
        let (key, json_value) = match field.key.as_str() {
            // 可清空非敏感串：空串也写库，否则「重置本页」会被下方 is_empty 守卫吞掉
            // 仅持久化策略允许的 URL（http(s)/同站路径）；非法值跳过以免写入危险 scheme/内网
            "wallpaper_url" => {
                match sanitize_wallpaper_url(&field.value) {
                    Some(safe) => {
                        updates.insert(
                            "ui_wallpaper_url".to_string(),
                            JsonValue::String(safe),
                        );
                    }
                    None => {
                        tracing::warn!(
                            wallpaper_url = %field.value,
                            "Rejecting wallpaper_url that failed scheme/host policy"
                        );
                    }
                }
                continue;
            }
            // Soft URL policy (scheme/format only — private hosts allowed for self-host)
            "site_favicon" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "site_favicon",
                    &field.value,
                    sanitize_site_favicon_url,
                );
                continue;
            }
            "site_og_image" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "site_og_image",
                    &field.value,
                    sanitize_site_og_image_url,
                );
                continue;
            }
            "umami_script_url" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "umami_script_url",
                    &field.value,
                    sanitize_umami_script_url,
                );
                continue;
            }
            "proxy_url" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "proxy_url",
                    &field.value,
                    sanitize_proxy_url,
                );
                continue;
            }
            "gemini_base_url" | "github_api_base_url" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    &field.key,
                    &field.value,
                    sanitize_http_base_url,
                );
                continue;
            }
            "site_ai_intro" => {
                // Cap to match SEO AI generate path (~500 chars) so public
                // /llms.txt cannot be bloated via the config bag.
                let capped: String = field.value.chars().take(500).collect();
                updates.insert(
                    "site_ai_intro".to_string(),
                    JsonValue::String(capped),
                );
                continue;
            }
            "site_title" | "site_description" | "site_keywords"
            | "ga_measurement_id" | "umami_website_id"
            | "music_source" | "site_icp" | "site_gongan"
            | "cloud_sponsors" | "site_footer_custom" | "proxy_bypass" => {
                updates.insert(field.key.clone(), JsonValue::String(field.value.clone()));
                continue;
            }
            "site_visibility_policy" => {
                let pol = crate::api::seo_policy::normalize_visibility_policy(&field.value, false);
                updates.insert(
                    "site_visibility_policy".to_string(),
                    JsonValue::String(pol.to_string()),
                );
                // Keep legacy noindex bit in lockstep
                updates.insert(
                    "site_noindex".to_string(),
                    JsonValue::Bool(pol == "private"),
                );
                continue;
            }
            "site_noindex" => {
                // When `site_visibility_policy` is also in this payload, policy is
                // authoritative (and already wrote `site_noindex`). Ignore raw bit.
                let has_policy = config
                    .ui_config
                    .config_fields
                    .iter()
                    .any(|f| f.key == "site_visibility_policy");
                if has_policy {
                    continue;
                }
                let enabled = field.value == "true";
                updates.insert(field.key.clone(), JsonValue::Bool(enabled));
                // Legacy-only flip: keep visibility policy in lockstep.
                updates.insert(
                    "site_visibility_policy".to_string(),
                    JsonValue::String(
                        if enabled {
                            "private".to_string()
                        } else {
                            "ai_full".to_string()
                        },
                    ),
                );
                continue;
            }
            "music_playlist_id" => {
                // Accept full NetEase/QQ playlist URLs from the config UI and store numeric id.
                updates.insert(
                    field.key.clone(),
                    JsonValue::String(normalize_music_playlist_id(&field.value)),
                );
                continue;
            }
            "wallpaper_blur" => {
                if let Ok(n) = field.value.parse::<i64>() {
                    ("ui_wallpaper_blur", JsonValue::Number(n.into()))
                } else {
                    continue;
                }
            }
            // legacy：已不再 emit，保留写入兼容旧客户端 payload
            "wallpaper_parallax" => {
                let enabled = field.value == "true";
                ("ui_wallpaper_parallax", JsonValue::Bool(enabled))
            }
            // Evocative 壁纸动效
            "evocative_parallax" => {
                let enabled = field.value == "true";
                ("ui_evocative_parallax", JsonValue::Bool(enabled))
            }
            "evocative_dynamic_blur" => {
                let enabled = field.value == "true";
                ("ui_evocative_dynamic_blur", JsonValue::Bool(enabled))
            }
            "evocative_ripple" => {
                let enabled = field.value == "true";
                ("ui_evocative_ripple", JsonValue::Bool(enabled))
            }
            "evocative_fps" => {
                if let Ok(n) = field.value.parse::<i64>() {
                    ("ui_evocative_fps", JsonValue::Number(n.into()))
                } else {
                    continue;
                }
            }
            "evocative_ripple_quality" => {
                if let Ok(n) = field.value.parse::<f64>() {
                    ("ui_evocative_ripple_quality", JsonValue::from(n))
                } else {
                    continue;
                }
            }
            // legacy：pet_* 已不再 emit
            "pet_enabled" => {
                let enabled = field.value == "true";
                ("pet_enabled", JsonValue::Bool(enabled))
            }
            "pet_image_url" => ("pet_image_url", JsonValue::String(field.value.clone())),
            "analytics_enabled" => {
                let enabled = field.value != "false" && field.value != "0";
                ("analytics_enabled", JsonValue::Bool(enabled))
            }
            "pwa_enabled" => {
                let enabled = field.value != "false" && field.value != "0";
                ("pwa_enabled", JsonValue::Bool(enabled))
            }
            // legacy：github OAuth 凭证走专用端点；bag 写入仍兼容
            "github_client_id" => ("github_client_id", JsonValue::String(field.value.clone())),
            "github_client_secret" => (
                "github_client_secret",
                JsonValue::String(field.value.clone()),
            ),
            // base_url 经独立域名 API 改写；bag 若带空串勿覆盖已生效域名
            "base_url" => {
                if field.value.trim().is_empty() {
                    continue;
                }
                ("base_url", JsonValue::String(field.value.clone()))
            }
            "music_enabled" => {
                let enabled = field.value == "true";
                ("music_enabled", JsonValue::Bool(enabled))
            }
            "proxy_enabled" => {
                let enabled = field.value == "true";
                ("proxy_enabled", JsonValue::Bool(enabled))
            }
            "memory_saver_enabled" => {
                let enabled = field.value == "true";
                ("memory_saver_enabled", JsonValue::Bool(enabled))
            }
            _ => continue,
        };
        // 忽略屏蔽值（前端返回的掩码）与空敏感字段，避免覆盖已保存的密钥
        // 非敏感字符串若需允许清空，应在上方 match 中 early-insert（见 proxy_* / site_*）
        if !field.value.is_empty() && !is_masked(&field.value) {
            updates.insert(key.to_string(), json_value);
        }
    }

    updates
}

/// Whether a config form field key holds a secret (must not write mask/empty to .env).
fn is_secret_config_field_key(field_key: &str) -> bool {
    let k = field_key.to_ascii_lowercase();
    k.contains("token")
        || k.contains("secret")
        || k.contains("api_key")
        || k.contains("npsso")
        || k.contains("password")
        || k.ends_with("_key")
        || k == "key"
}

/// Skip empty or masked secrets so save does not clobber real .env/DB values with ••••.
fn should_write_env_field(field_key: &str, value: &str) -> bool {
    if is_secret_config_field_key(field_key) {
        !value.trim().is_empty() && !is_masked_secret_value(value)
    } else {
        true
    }
}

/// App config that lives only in the database (groups A/B/C).
///
/// Never dual-write these to `.env` / process env. On every config save we also
/// strip any legacy lines and `remove_var` so stale process env cannot resurrect
/// cleared wallpaper / secrets / AI settings.
///
/// Kept in env (not in this list):
/// - infra: DATABASE_URL, SERVER_*, JWT_SECRET, CORS_ORIGINS, FRONTEND_*, RUST_LOG
/// - site origin: BASE_URL (+ site-domain FRONTEND_URL / CORS adapt)
/// - OAuth deploy: GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET
/// - outbound runtime: PROXY_*, GEMINI_BASE_URL, GITHUB_API_BASE_URL
///
/// ## TECH DEBT — remove after 5 releases
///
/// Introduced in **v0.3.26** (stop dual-writing A/B/C + one-time purge of legacy
/// `.env` / process env). The **purge path** (`DB_ONLY_ENV_KEYS`,
/// `remove_env_keys` on save, `std::env::remove_var` loop for this list) exists
/// only so old installs clean themselves on the next config save.
///
/// **Remove target: ≥ v0.3.31** (5 versions after 0.3.26). By then all active
/// deployments should have purged; keep only the “do not write A/B/C to env”
/// contract (i.e. never re-add dual-write). Delete:
/// - this constant (or shrink to empty if unused)
/// - `remove_env_keys` + its unit tests (if only used for this purge)
/// - the `remove_env_keys(...)` call and `for key in DB_ONLY_ENV_KEYS { remove_var }`
///   in `save_all_configs`
///
/// Tracked: https://github.com/Myriad-You/Myriad/issues/301
const DB_ONLY_ENV_KEYS: &[&str] = &[
    // A — pure UI / site bag / music / report topic / dead pet
    "UI_WALLPAPER_URL",
    "UI_WALLPAPER_BLUR",
    "UI_WALLPAPER_PARALLAX",
    "UI_EVOCATIVE_PARALLAX",
    "UI_EVOCATIVE_DYNAMIC_BLUR",
    "UI_EVOCATIVE_RIPPLE",
    "UI_EVOCATIVE_FPS",
    "UI_EVOCATIVE_RIPPLE_QUALITY",
    "PET_ENABLED",
    "PET_IMAGE_URL",
    "ANALYTICS_ENABLED",
    "PWA_ENABLED",
    "SITE_TITLE",
    "SITE_DESCRIPTION",
    "SITE_FAVICON",
    "SITE_KEYWORDS",
    "SITE_OG_IMAGE",
    "SITE_NOINDEX",
    "SITE_VISIBILITY_POLICY",
    "SITE_AI_INTRO",
    "SITE_FOOTER_CUSTOM",
    "SITE_ICP",
    "SITE_GONGAN",
    "GA_MEASUREMENT_ID",
    "UMAMI_WEBSITE_ID",
    "UMAMI_SCRIPT_URL",
    "MUSIC_ENABLED",
    "MUSIC_SOURCE",
    "MUSIC_PLAYLIST_ID",
    "TOPIC_STYLE",
    // B — platform credentials
    "GITHUB_USERNAME",
    "GITHUB_TOKEN",
    "BILIBILI_UID",
    "STEAM_API_KEY",
    "STEAM_ID",
    "YOUTUBE_API_KEY",
    "YOUTUBE_CHANNEL_ID",
    "NETEASE_USER_ID",
    "BANGUMI_USERNAME",
    "BANGUMI_ACCESS_TOKEN",
    "BANGUMI_USER_AGENT",
    "X_USERNAME",
    "X_BEARER_TOKEN",
    "MAL_USERNAME",
    "MAL_CLIENT_ID",
    "XBOX_GAMERTAG",
    "OPENXBL_API_KEY",
    "PSN_ONLINE_ID",
    "PSN_NPSSO",
    "DISCORD_ACCESS_TOKEN",
    "DISCORD_REFRESH_TOKEN",
    "DISCORD_TOKEN_EXPIRES_AT",
    "DISCORD_USER_ID",
    // C — AI / Lite / Pro / image / Tripo
    "AI_PROVIDER",
    "GEMINI_API_KEY",
    "GEMINI_MODEL",
    "OPENAI_API_KEY",
    "OPENAI_MODEL",
    "OPENAI_BASE_URL",
    "OPENAI_MAX_TOKENS",
    "PRO_ENABLED",
    "PRO_AI_PROVIDER",
    "PRO_GEMINI_API_KEY",
    "PRO_GEMINI_MODEL",
    "PRO_OPENAI_API_KEY",
    "PRO_OPENAI_MODEL",
    "PRO_OPENAI_BASE_URL",
    "AI_IMAGE_PROVIDER",
    "AI_IMAGE_MODEL",
    "AI_IMAGE_OPENAI_API_KEY",
    "AI_IMAGE_OPENAI_BASE_URL",
    "AI_IMAGE_OPENROUTER_API_KEY",
    "AI_IMAGE_VOLCENGINE_API_KEY",
    "AI_IMAGE_VOLCENGINE_BASE_URL",
    "AI_IMAGE_WIDTH",
    "AI_IMAGE_HEIGHT",
    "LITE_ENABLED",
    "LITE_AI_PROVIDER",
    "LITE_GEMINI_API_KEY",
    "LITE_GEMINI_MODEL",
    "LITE_OPENAI_API_KEY",
    "LITE_OPENAI_MODEL",
    "LITE_OPENAI_BASE_URL",
    "TRIPO_ENABLED",
    "TRIPO_API_KEY",
    "TRIPO_BASE_URL",
    "TRIPO_MODEL",
    "TRIPO_FACE_LIMIT",
    "TRIPO_POLL_INTERVAL_SECONDS",
    "TRIPO_TASK_TIMEOUT_SECONDS",
    "TRIPO_MAX_DOWNLOAD_MB",
];

/// Drop `KEY=...` and `# KEY=...` lines from .env content.
pub(crate) fn remove_env_keys(content: &str, keys: &[&str]) -> String {
    if keys.is_empty() {
        return content.to_string();
    }
    let prefixes: Vec<(String, String)> = keys
        .iter()
        .map(|k| (format!("{k}="), format!("# {k}=")))
        .collect();
    let mut out: Vec<&str> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        let drop = prefixes.iter().any(|(eq, hash)| {
            trimmed.starts_with(eq.as_str()) || trimmed.starts_with(hash.as_str())
        });
        if !drop {
            out.push(line);
        }
    }
    let mut s = out.join("\n");
    if content.ends_with('\n') && !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// 保存部署相关配置到 .env；应用配置（A/B/C）只在 DB，绝不 dual-write。
async fn save_all_configs(config: &ConfigResponse) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::Path;

    // 读取现有的 .env 文件（如果存在）
    let env_path = Path::new(".env");
    let mut env_content = if env_path.exists() {
        // 尝试读取文件，如果失败则从字节读取并替换非 UTF-8 字符
        match fs::read_to_string(env_path) {
            Ok(content) => content,
            Err(e) => {
                tracing::warn!("Failed to read .env as UTF-8: {}, attempting to recover", e);
                // 读取字节并尝试转换，替换无效字符
                let bytes = fs::read(env_path)?;
                String::from_utf8_lossy(&bytes).into_owned()
            }
        }
    } else {
        String::new()
    };

    // Purge legacy dual-written A/B/C keys from the file before writing deploy keys.
    env_content = remove_env_keys(&env_content, DB_ONLY_ENV_KEYS);

    // Keys emptied on this save (commented `# KEY=`) — must remove_var after dotenv.
    let mut env_keys_to_clear: Vec<&'static str> = Vec::new();

    // Capture previous public origin before rewriting BASE_URL so CORS replace
    // can swap the old entry instead of treating the new value as previous.
    let previous_base_url = crate::api::site_domain::read_env_key(&env_content, "BASE_URL")
        .or_else(|| std::env::var("BASE_URL").ok().filter(|s| !s.is_empty()));

    // Only deploy / outbound keys still dual-write to .env:
    // BASE_URL, GitHub OAuth client, proxy, API base mirrors.
    let mut saved_base_url: Option<String> = None;
    for field in &config.ui_config.config_fields {
        let key = match field.key.as_str() {
            "github_client_id" => "GITHUB_CLIENT_ID",
            "github_client_secret" => "GITHUB_CLIENT_SECRET",
            "base_url" => "BASE_URL",
            "proxy_enabled" => "PROXY_ENABLED",
            "proxy_url" => "PROXY_URL",
            "proxy_bypass" => "PROXY_BYPASS",
            "gemini_base_url" => "GEMINI_BASE_URL",
            "github_api_base_url" => "GITHUB_API_BASE_URL",
            _ => continue,
        };
        if field.key == "base_url" {
            saved_base_url = Some(field.value.clone());
        }
        // github_client_secret must not write masks
        if !should_write_env_field(&field.key, &field.value) {
            continue;
        }
        let env_value = match field.key.as_str() {
            "proxy_url" => match sanitize_proxy_url(&field.value) {
                Some(safe) => safe,
                None => {
                    tracing::warn!("Skipping PROXY_URL env write: failed scheme/format policy");
                    continue;
                }
            },
            "gemini_base_url" | "github_api_base_url" => {
                match sanitize_http_base_url(&field.value) {
                    Some(safe) => safe,
                    None => {
                        tracing::warn!(
                            key = %field.key,
                            "Skipping API base URL env write: failed scheme/format policy"
                        );
                        continue;
                    }
                }
            }
            _ => field.value.clone(),
        };
        env_content = update_env_var(&env_content, key, &env_value);
        // Commented `# KEY=` lines do not unset process env after dotenv reload.
        if env_value.trim().is_empty() {
            env_keys_to_clear.push(key);
        }
    }

    // When site base_url changes to a valid origin, also adapt FRONTEND_URL +
    // CORS_ORIGINS for the same origin (site access only — not federation Move).
    // Empty base_url must not wipe CORS_ORIGINS (production panics without it).
    if let Some(ref base) = saved_base_url {
        let base_trim = base.trim();
        if !base_trim.is_empty() {
            match crate::api::site_domain::apply_site_domain_to_env_content(
                &env_content,
                base_trim,
                previous_base_url.as_deref(),
            ) {
                Ok((adapted, _)) => env_content = adapted,
                Err(e) => {
                    // Invalid base_url: leave generic BASE_URL write as-is.
                    tracing::warn!(
                        "Skipping FRONTEND_URL/CORS_ORIGINS adapt for base_url={:?}: {}",
                        base_trim,
                        e
                    );
                }
            }
        }
    }

    // 写回 .env 文件，确保使用 UTF-8 编码
    // 在 Windows 上，确保换行符为 LF，避免编码问题
    let env_content_normalized = env_content.replace("\r\n", "\n");

    // 验证内容是否为有效的 UTF-8
    if !env_content_normalized.is_ascii() {
        tracing::debug!("Config contains non-ASCII characters, ensuring UTF-8 validity");
    }

    fs::write(env_path, env_content_normalized.as_bytes())?;
    tracing::info!("✅ Configuration saved to .env file (DB-only keys purged)");

    // 重新加载环境变量
    if let Err(e) = dotenvy::from_path_override(env_path) {
        tracing::warn!("⚠️ Failed to reload .env file after saving config: {}", e);
    } else {
        tracing::info!("♻️ Environment variables reloaded after config save");
    }

    // Drop emptied deploy keys + all DB-only keys from process env so nothing
    // can resurrect via std::env after a purge (dotenv never unsets missing keys).
    for key in env_keys_to_clear {
        std::env::remove_var(key);
    }
    for key in DB_ONLY_ENV_KEYS {
        std::env::remove_var(key);
    }

    // 触发配置重载标志(虽然数据库连接可能不变,但确保其他服务知道配置已更新)
    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    tracing::info!(
        "🔄 Configuration reload flag set - changes will be picked up within 2-3 seconds"
    );

    Ok(())
}

/// 更新或添加环境变量
///
/// `pub(crate)` so site-domain migration can rewrite BASE_URL / FRONTEND_URL /
/// CORS_ORIGINS with the same quoting rules as the general config save path.
pub fn update_env_var(content: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let key_prefix = format!("{}=", key);

    // 处理值：如果包含空格、特殊字符或中文，用引号包裹
    let sanitized_value = if value.is_empty() {
        String::new()
    } else if value.contains(' ')
        || value.contains('#')
        || value.contains('\n')
        || value.chars().any(|c| c > '\u{007F}')
    // 包含非 ASCII 字符
    {
        // 转义内部的引号和反斜杠
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    };

    let new_line = if value.is_empty() {
        format!("# {}=", key) // 空值时注释掉
    } else {
        format!("{}={}", key, sanitized_value)
    };

    // 查找是否已存在该键
    let mut found = false;
    for line in &mut lines {
        if line.starts_with(&key_prefix) || line.starts_with(&format!("# {}", key_prefix)) {
            *line = new_line.clone();
            found = true;
            break;
        }
    }

    // 如果不存在，添加到末尾
    if !found {
        lines.push(new_line);
    }

    lines.join("\n") + "\n"
}

// merged from public_ui.rs

/// 获取公开的网站元数据（不需要认证）
/// 直接从环境变量读取（配置保存时已经写入 .env 并重新加载）
/// 优先级：.env 文件（通过 save_all_configs 保存） > 默认值
pub async fn get_site_metadata(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取站点元数据配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Branding fields: empty DB still falls through to env/default (legacy UX).
    let get_branding = |db_val: Option<String>, env_key: &str, default: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var(env_key).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| default.to_string())
    };

    let site_noindex = db_config
        .as_ref()
        .map(|c| c.site_noindex)
        .unwrap_or_else(|| {
            std::env::var("SITE_NOINDEX")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
        });

    let site_visibility_policy = {
        let raw = db_config
            .as_ref()
            .map(|c| c.site_visibility_policy.clone())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("SITE_VISIBILITY_POLICY").ok())
            .unwrap_or_default();
        crate::api::seo_policy::normalize_visibility_policy(&raw, site_noindex)
    };
    let site_ai_intro = db_or_env_clearable(
        db_config.as_ref().and_then(|c| c.site_ai_intro.clone()),
        "SITE_AI_INTRO",
        "",
    );

    let metadata = json!({
        "site_title": get_branding(
            db_config.as_ref().and_then(|c| c.site_title.clone()),
            "SITE_TITLE",
            "Myriad - A myriad of lights, in one place."
        ),
        "site_description": get_branding(
            db_config.as_ref().and_then(|c| c.site_description.clone()),
            "SITE_DESCRIPTION",
            "A myriad of lights, in one place."
        ),
        "site_favicon": get_branding(
            db_config.as_ref().and_then(|c| c.site_favicon.clone()),
            "SITE_FAVICON",
            "/favicon.webp"
        ),
        // Clearable SEO / third-party analytics: explicit empty DB disables (no env re-fill).
        "site_keywords": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.site_keywords.clone()),
            "SITE_KEYWORDS",
            ""
        ),
        "site_og_image": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.site_og_image.clone()),
            "SITE_OG_IMAGE",
            ""
        ),
        "site_noindex": site_noindex,
        "site_visibility_policy": site_visibility_policy,
        "site_ai_intro": site_ai_intro,
        "ga_measurement_id": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.ga_measurement_id.clone()),
            "GA_MEASUREMENT_ID",
            ""
        ),
        "umami_website_id": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.umami_website_id.clone()),
            "UMAMI_WEBSITE_ID",
            ""
        ),
        "umami_script_url": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.umami_script_url.clone()),
            "UMAMI_SCRIPT_URL",
            ""
        ),
    });

    (StatusCode::OK, Json(metadata))
}

/// 获取公开的平台配置（不包含敏感信息，仅用于社交链接显示）
/// 公开端点 - 不需要认证
pub async fn get_public_config(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Prefer DB when present (including intentional empty clear); else process env.
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_or_env_clearable(db_val, env_key, "")
    };

    // Match build_config: empty compose `${VAR:-}` still sets the key — gate on nonempty.
    let github_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.github_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.github_username.as_ref()))
            || nonempty_env("GITHUB_USERNAME"),
    );
    let bilibili_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bilibili_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.bilibili_uid.as_ref()))
            || nonempty_env("BILIBILI_UID"),
    );
    let steam_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.steam_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.steam_id.as_ref()))
            || nonempty_env("STEAM_ID"),
    );
    let youtube_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.youtube_enabled),
        (nonempty_db(db_config.as_ref().and_then(|c| c.youtube_channel_id.as_ref()))
            || nonempty_env("YOUTUBE_CHANNEL_ID"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.youtube_api_key.as_ref()))
                || nonempty_env("YOUTUBE_API_KEY")),
    );
    let netease_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.netease_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.netease_user_id.as_ref()))
            || nonempty_env("NETEASE_USER_ID"),
    );
    let bangumi_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bangumi_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.bangumi_username.as_ref()))
            || nonempty_db(db_config.as_ref().and_then(|c| c.bangumi_access_token.as_ref()))
            || nonempty_env("BANGUMI_USERNAME")
            || nonempty_env("BANGUMI_ACCESS_TOKEN"),
    );
    let x_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.x_enabled),
        (nonempty_db(db_config.as_ref().and_then(|c| c.x_username.as_ref()))
            || nonempty_env("X_USERNAME"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.x_bearer_token.as_ref()))
                || nonempty_env("X_BEARER_TOKEN")),
    );
    let discord_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.discord_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.discord_access_token.as_ref()))
            || nonempty_env("DISCORD_ACCESS_TOKEN"),
    );
    let mal_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.mal_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.mal_username.as_ref()))
            || nonempty_env("MAL_USERNAME"),
    );
    let xbox_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.xbox_enabled),
        (nonempty_db(db_config.as_ref().and_then(|c| c.xbox_gamertag.as_ref()))
            || nonempty_env("XBOX_GAMERTAG"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.openxbl_api_key.as_ref()))
                || nonempty_env("OPENXBL_API_KEY")
                || nonempty_env("XBL_API_KEY")),
    );
    let psn_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.psn_enabled),
        (nonempty_db(db_config.as_ref().and_then(|c| c.psn_online_id.as_ref()))
            || nonempty_env("PSN_ONLINE_ID"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.psn_npsso.as_ref()))
                || nonempty_env("PSN_NPSSO")),
    );

    // 只返回公开可见的平台配置字段（不包含 API 密钥等敏感信息）
    let public_platforms = vec![
        PlatformConfig {
            name: "GitHub".to_string(),
            enabled: github_enabled,
            has_token: false, // 不暴露是否有 token
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.github_username.clone()),
                    "GITHUB_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Bilibili".to_string(),
            enabled: bilibili_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "uid".to_string(),
                label: "".to_string(),
                field_type: "number".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.bilibili_uid.clone()),
                    "BILIBILI_UID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Steam".to_string(),
            enabled: steam_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "steam_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.steam_id.clone()),
                    "STEAM_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "YouTube".to_string(),
            enabled: youtube_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "channel_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.youtube_channel_id.clone()),
                    "YOUTUBE_CHANNEL_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Netease Music".to_string(),
            enabled: netease_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "user_id".to_string(),
                label: "".to_string(),
                field_type: "number".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.netease_user_id.clone()),
                    "NETEASE_USER_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Bangumi".to_string(),
            enabled: bangumi_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.bangumi_username.clone()),
                    "BANGUMI_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "X".to_string(),
            enabled: x_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.x_username.clone()),
                    "X_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Discord".to_string(),
            enabled: discord_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            // 不暴露 token；仅返回 user_id 便于公开名片展示
            config_fields: vec![ConfigField {
                key: "user_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.discord_user_id.clone()),
                    "DISCORD_USER_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "MyAnimeList".to_string(),
            enabled: mal_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.mal_username.clone()),
                    "MAL_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Xbox".to_string(),
            enabled: xbox_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "gamertag".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.xbox_gamertag.clone()),
                    "XBOX_GAMERTAG",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "PlayStation".to_string(),
            enabled: psn_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "online_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.psn_online_id.clone()),
                    "PSN_ONLINE_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
    ];

    let mut public_platforms = public_platforms;
    sort_platforms_by_order(
        &mut public_platforms,
        db_config.as_ref().and_then(|c| c.platform_order.as_ref()),
    );

    let response = json!({
        "platforms": public_platforms
    });

    (StatusCode::OK, Json(response))
}

/// 获取公开的 UI 运行时配置（壁纸 / 动效 / 音乐 / 站点展示等）
/// 公开端点 - 不需要认证
/// 已剥离无前端消费的死字段：`pet_*`、`wallpaper_parallax`（动效改走 evocative_*）
pub async fn get_public_ui_config(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Prefer DB when present (including intentional empty clear); else process env.
    // Must match admin bag / SEO clearable fields — empty wallpaper must NOT fall
    // through to a stale UI_WALLPAPER_URL still sitting in process env after clear.
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_or_env_clearable(db_val, env_key, "")
    };

    let ui_config = json!({
        "analytics_enabled": db_config.as_ref().map(|c| c.analytics_enabled).unwrap_or_else(||
            std::env::var("ANALYTICS_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        "pwa_enabled": db_config.as_ref().map(|c| c.pwa_enabled).unwrap_or_else(||
            std::env::var("PWA_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        "wallpaper_url": get_value(
            db_config.as_ref().and_then(|c| c.ui_wallpaper_url.clone()),
            "UI_WALLPAPER_URL"
        ),
        "wallpaper_blur": db_config.as_ref().map(|c| c.ui_wallpaper_blur as u32).unwrap_or_else(||
            std::env::var("UI_WALLPAPER_BLUR").unwrap_or_else(|_| "3".to_string()).parse::<u32>().unwrap_or(3)
        ),
        // Evocative 壁纸动效
        "evocative_parallax": db_config.as_ref().map(|c| c.ui_evocative_parallax).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_PARALLAX").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        "evocative_dynamic_blur": db_config.as_ref().map(|c| c.ui_evocative_dynamic_blur).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_DYNAMIC_BLUR").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false)
        ),
        "evocative_ripple": db_config.as_ref().map(|c| c.ui_evocative_ripple).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_RIPPLE").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false)
        ),
        "evocative_fps": db_config.as_ref().map(|c| c.ui_evocative_fps as u32).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_FPS").unwrap_or_else(|_| "30".to_string()).parse::<u32>().unwrap_or(30)
        ),
        "evocative_ripple_quality": db_config.as_ref().map(|c| c.ui_evocative_ripple_quality).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_RIPPLE_QUALITY").unwrap_or_else(|_| "0.85".to_string()).parse::<f64>().unwrap_or(0.85)
        ),
        "music_enabled": get_value(
            db_config.as_ref().and_then(|c| c.music_enabled.clone()),
            "MUSIC_ENABLED"
        ),
        "music_source": get_value(
            db_config.as_ref().and_then(|c| c.music_source.clone()),
            "MUSIC_SOURCE"
        ),
        "music_playlist_id": get_value(
            db_config.as_ref().and_then(|c| c.music_playlist_id.clone()),
            "MUSIC_PLAYLIST_ID"
        ),
        "dashboard_layout": db_config.as_ref().and_then(|c| c.dashboard_layout.clone()),
        "dashboard_title": db_config.as_ref().and_then(|c| c.dashboard_title.clone()),
        "custom_platforms": db_config.as_ref().and_then(|c| c.custom_platforms.clone()),
        "widget_theme": db_config.as_ref().and_then(|c| c.widget_theme.clone()),
        "control_panel_layout": db_config.as_ref().and_then(|c| c.control_panel_layout.clone()),
        "control_panel_rows": db_config.as_ref().map(|c| c.control_panel_rows).unwrap_or(2),
        "tapp_window_schemes": db_config.as_ref().and_then(|c| c.tapp_window_schemes.clone()),
        // 标题字体样式设置
        "title_font": db_config.as_ref().and_then(|c| c.title_font.clone()),
        "title_font_size": db_config.as_ref().and_then(|c| c.title_font_size),
        "title_color": db_config.as_ref().and_then(|c| c.title_color.clone()),
        // 站点信息（用于底部显示）
        "site_icp": db_config.as_ref().and_then(|c| c.site_icp.clone()),
        "site_gongan": db_config.as_ref().and_then(|c| c.site_gongan.clone()),
        "cloud_sponsors": db_config.as_ref().and_then(|c| c.cloud_sponsors.clone()),
        "site_footer_custom": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.site_footer_custom.clone()),
            "SITE_FOOTER_CUSTOM",
            ""
        ),
    });

    (StatusCode::OK, Json(ui_config))
}

#[derive(Debug, Deserialize)]
pub struct DashboardConfigPayload {
    pub layout: Option<String>,
    pub title: Option<String>,
    pub custom_platforms: Option<String>,
    pub title_font: Option<String>,
    pub title_font_size: Option<f64>,
    pub title_color: Option<String>,
    pub widget_theme: Option<String>,
}

pub async fn update_dashboard_config(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<DashboardConfigPayload>,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    if let Some(layout) = payload.layout {
        updates.insert("dashboard_layout".to_string(), json!(layout));
    }

    if let Some(title) = payload.title {
        updates.insert("dashboard_title".to_string(), json!(title));
    }

    if let Some(custom_platforms) = payload.custom_platforms {
        updates.insert("custom_platforms".to_string(), json!(custom_platforms));
    }

    if let Some(title_font) = payload.title_font {
        updates.insert("title_font".to_string(), json!(title_font));
    }

    if let Some(title_font_size) = payload.title_font_size {
        updates.insert("title_font_size".to_string(), json!(title_font_size));
    }

    if let Some(title_color) = payload.title_color {
        updates.insert("title_color".to_string(), json!(title_color));
    }

    if let Some(widget_theme) = payload.widget_theme {
        updates.insert("widget_theme".to_string(), json!(widget_theme));
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update dashboard config: {}", e)
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Dashboard configuration updated successfully"
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct ControlPanelConfigPayload {
    pub control_panel_layout: Option<String>,
    pub control_panel_rows: Option<i32>,
}

pub async fn update_control_panel_config(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<ControlPanelConfigPayload>,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    if let Some(layout) = payload.control_panel_layout {
        updates.insert("control_panel_layout".to_string(), json!(layout));
    }

    if let Some(rows) = payload.control_panel_rows {
        updates.insert("control_panel_rows".to_string(), json!(rows));
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update control panel config: {}", e)
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Control panel configuration updated successfully"
        })),
    )
}

// Tapp 窗口方案 API

#[derive(Debug, Deserialize)]
pub struct TappWindowSchemesPayload {
    pub schemes: String,
}

pub async fn update_tapp_window_schemes(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<TappWindowSchemesPayload>,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    updates.insert("tapp_window_schemes".to_string(), json!(payload.schemes));

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update tapp window schemes: {}", e)
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Tapp window schemes updated successfully"
        })),
    )
}

const MODULE_VISIBILITY_PREFERENCES_KEY: &str = "module_visibility_preferences";
const MODULE_VISIBILITY_KEYS: [&str; 6] = ["library", "brew", "reports", "life", "tapp", "agent"];
const MODULE_VISIBILITY_LEVELS: [&str; 3] = ["all", "authenticated", "admin"];
/// 兼容旧配置字段（能力已迁至 Tapp 权限预设；读写仍规范化但不参与鉴权）
const AGENT_GUEST_USAGE_LEVELS: [&str; 2] = ["none", "visible"];
const AGENT_USER_USAGE_LEVELS: [&str; 4] = ["none", "chat", "standard", "elevated"];

/// 旧版 Agent 使用档位（已弃用：运行时以 Tapp `user_perm_*` / 预设模板为准）
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsagePreferences {
    #[serde(default = "default_agent_guest_usage")]
    pub guest: String,
    #[serde(default = "default_agent_user_usage")]
    pub user: String,
}

fn default_agent_guest_usage() -> String {
    "none".to_string()
}
fn default_agent_user_usage() -> String {
    "standard".to_string()
}

impl Default for AgentUsagePreferences {
    fn default() -> Self {
        Self {
            guest: default_agent_guest_usage(),
            user: default_agent_user_usage(),
        }
    }
}

impl AgentUsagePreferences {
    pub fn normalized(mut self) -> Self {
        if !AGENT_GUEST_USAGE_LEVELS.contains(&self.guest.as_str()) {
            self.guest = default_agent_guest_usage();
        }
        if !AGENT_USER_USAGE_LEVELS.contains(&self.user.as_str()) {
            self.user = default_agent_user_usage();
        }
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModuleVisibilityPreferences {
    #[serde(default = "default_module_visibility_modules")]
    pub modules: std::collections::HashMap<String, String>,
    /// 旧版 Agent 档位（兼容存储；鉴权请用 Tapp 权限）
    #[serde(default)]
    pub agent_usage: AgentUsagePreferences,
}

fn default_module_visibility_modules() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("library".to_string(), "all".to_string()),
        ("brew".to_string(), "all".to_string()),
        ("reports".to_string(), "all".to_string()),
        ("life".to_string(), "all".to_string()),
        ("tapp".to_string(), "all".to_string()),
        ("agent".to_string(), "all".to_string()),
    ])
}

impl Default for ModuleVisibilityPreferences {
    fn default() -> Self {
        Self {
            modules: default_module_visibility_modules(),
            agent_usage: AgentUsagePreferences::default(),
        }
    }
}

impl ModuleVisibilityPreferences {
    pub fn normalized(mut self) -> Self {
        let defaults = default_module_visibility_modules();
        let mut normalized = std::collections::HashMap::new();

        for key in MODULE_VISIBILITY_KEYS {
            let value = self
                .modules
                .remove(key)
                .unwrap_or_else(|| defaults.get(key).cloned().unwrap_or_else(|| "all".into()));
            let value = if MODULE_VISIBILITY_LEVELS.contains(&value.as_str()) {
                value
            } else {
                defaults.get(key).cloned().unwrap_or_else(|| "all".into())
            };
            normalized.insert(key.to_string(), value);
        }

        self.modules = normalized;
        self.agent_usage = self.agent_usage.normalized();
        self
    }

    /// Agent 模块页面可见级别
    pub fn agent_visibility(&self) -> &str {
        self.modules
            .get("agent")
            .map(String::as_str)
            .unwrap_or("all")
    }
}

/// 供 HTTP/config 层读取模块可见性。
/// Agent 服务请用 `services::module_visibility::agent_module_visibility`。
pub async fn load_module_visibility_preferences_for_agent(
    db: &DatabaseConnection,
) -> ModuleVisibilityPreferences {
    load_module_visibility_preferences(db).await
}

async fn load_module_visibility_preferences(
    db: &DatabaseConnection,
) -> ModuleVisibilityPreferences {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![MODULE_VISIBILITY_PREFERENCES_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<ModuleVisibilityPreferences>(value)
                .map(ModuleVisibilityPreferences::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "Invalid module visibility preferences, using defaults: {}",
                        e
                    );
                    ModuleVisibilityPreferences::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read module visibility preferences: {}", e);
                ModuleVisibilityPreferences::default()
            }
        },
        Ok(None) => ModuleVisibilityPreferences::default(),
        Err(e) => {
            tracing::warn!("Failed to load module visibility preferences: {}", e);
            ModuleVisibilityPreferences::default()
        }
    }
}

pub async fn get_module_visibility_preferences(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    let preferences = load_module_visibility_preferences(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "preferences": preferences
        })),
    )
}

pub async fn update_module_visibility_preferences(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<ModuleVisibilityPreferences>,
) -> (StatusCode, Json<Value>) {
    let preferences = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            MODULE_VISIBILITY_PREFERENCES_KEY,
            serde_json::to_value(&preferences).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "preferences": preferences
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save module visibility preferences: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save module visibility preferences"
                })),
            )
        }
    }
}

// 一言（Hitokoto）配置 API

const HITOKOTO_CONFIG_KEY: &str = "hitokoto_config";
/// Config UI source ids — keep in sync with frontend `BUILTIN_HITOKOTO_SOURCES`
/// (+ `"custom"`) in `frontend/src/utils/quote.ts`.
pub const HITOKOTO_SOURCE_IDS: [&str; 5] = [
    "hitokoto-cn",
    "hitokoto-anime",
    "quotable-en",
    "meigen-ja",
    "custom",
];

/// Builtin quote API hosts (no port) matching FE `BUILTIN_HITOKOTO_SOURCES` URLs.
/// Proxy SSRF policy is still `outbound_security`; this list is catalog alignment.
pub const HITOKOTO_BUILTIN_HOSTS: [&str; 3] = [
    "v1.hitokoto.cn",
    "api.quotable.io",
    "meigen.doodlenote.net",
];

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HitokotoConfig {
    #[serde(default = "default_hitokoto_source_id")]
    pub source_id: String,
    #[serde(default)]
    pub custom_url: Option<String>,
    #[serde(default)]
    pub custom_text_field: Option<String>,
    #[serde(default)]
    pub custom_author_field: Option<String>,
}

fn default_hitokoto_source_id() -> String {
    "hitokoto-cn".to_string()
}

impl Default for HitokotoConfig {
    fn default() -> Self {
        Self {
            source_id: default_hitokoto_source_id(),
            custom_url: None,
            custom_text_field: None,
            custom_author_field: None,
        }
    }
}

impl HitokotoConfig {
    fn normalized(mut self) -> Self {
        if !HITOKOTO_SOURCE_IDS.contains(&self.source_id.as_str()) {
            self.source_id = default_hitokoto_source_id();
        }
        self.custom_url = self.custom_url.filter(|s| !s.trim().is_empty());
        self.custom_text_field = self.custom_text_field.filter(|s| !s.trim().is_empty());
        self.custom_author_field = self.custom_author_field.filter(|s| !s.trim().is_empty());
        self
    }
}

async fn load_hitokoto_config(db: &DatabaseConnection) -> HitokotoConfig {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![HITOKOTO_CONFIG_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<HitokotoConfig>(value)
                .map(HitokotoConfig::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!("Invalid hitokoto config, using defaults: {}", e);
                    HitokotoConfig::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read hitokoto config: {}", e);
                HitokotoConfig::default()
            }
        },
        Ok(None) => HitokotoConfig::default(),
        Err(e) => {
            tracing::warn!("Failed to load hitokoto config: {}", e);
            HitokotoConfig::default()
        }
    }
}

pub async fn get_hitokoto_config(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    let config = load_hitokoto_config(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "config": config
        })),
    )
}

pub async fn update_hitokoto_config(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<HitokotoConfig>,
) -> (StatusCode, Json<Value>) {
    let config = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            HITOKOTO_CONFIG_KEY,
            serde_json::to_value(&config).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "config": config
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save hitokoto config: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save hitokoto config"
                })),
            )
        }
    }
}

// 报告过期设置

const REPORT_SETTINGS_KEY: &str = "report_settings";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportSettings {
    /// 是否启用报告过期（关闭时报告永不过期，保持历史行为）
    #[serde(default)]
    pub expiry_enabled: bool,
    /// 过期后读取时自动后台重新生成（只消耗 AI 调用，不重新抓平台数据）
    #[serde(default)]
    pub auto_regenerate: bool,
    #[serde(default = "default_report_expiry_days")]
    pub expiry_days: i64,
}

fn default_report_expiry_days() -> i64 {
    7
}

impl Default for ReportSettings {
    fn default() -> Self {
        Self {
            expiry_enabled: false,
            auto_regenerate: false,
            expiry_days: default_report_expiry_days(),
        }
    }
}

impl ReportSettings {
    fn normalized(mut self) -> Self {
        self.expiry_days = self.expiry_days.clamp(1, 365);
        self
    }
}

pub async fn load_report_settings(db: &DatabaseConnection) -> ReportSettings {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![REPORT_SETTINGS_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<ReportSettings>(value)
                .map(ReportSettings::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!("Invalid report settings, using defaults: {}", e);
                    ReportSettings::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read report settings: {}", e);
                ReportSettings::default()
            }
        },
        Ok(None) => ReportSettings::default(),
        Err(e) => {
            tracing::warn!("Failed to load report settings: {}", e);
            ReportSettings::default()
        }
    }
}

pub async fn get_report_settings(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    let settings = load_report_settings(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "config": settings
        })),
    )
}

pub async fn update_report_settings(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<ReportSettings>,
) -> (StatusCode, Json<Value>) {
    let settings = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            REPORT_SETTINGS_KEY,
            serde_json::to_value(&settings).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "config": settings
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save report settings: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save report settings"
                })),
            )
        }
    }
}

#[cfg(test)]
mod hitokoto_catalog_tests {
    use super::{HITOKOTO_BUILTIN_HOSTS, HITOKOTO_SOURCE_IDS};

    /// Must stay aligned with frontend `BUILTIN_HITOKOTO_SOURCES` + `custom`
    /// (`frontend/src/utils/quote.ts`).
    #[test]
    fn hitokoto_source_ids_match_frontend_catalog() {
        assert_eq!(
            HITOKOTO_SOURCE_IDS,
            [
                "hitokoto-cn",
                "hitokoto-anime",
                "quotable-en",
                "meigen-ja",
                "custom",
            ]
        );
    }

    #[test]
    fn hitokoto_builtin_hosts_match_frontend_urls() {
        assert_eq!(
            HITOKOTO_BUILTIN_HOSTS,
            [
                "v1.hitokoto.cn",
                "api.quotable.io",
                "meigen.doodlenote.net",
            ]
        );
        for host in HITOKOTO_BUILTIN_HOSTS {
            assert!(!host.is_empty());
            assert!(!host.contains('/'));
        }
    }
}
