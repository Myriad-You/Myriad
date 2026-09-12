//! Platform configured-flags and public UI value helpers.
//!
//! No Axum: Agent and other services must not import the HTTP config handlers
//! just to read these flags.
use serde_json::{json, Value};

use super::types::PlatformConfig;

pub(crate) fn resolve_platform_enabled(
    explicit_enabled: Option<bool>,
    fallback_enabled: bool,
) -> bool {
    explicit_enabled.unwrap_or(fallback_enabled)
}

/// 按管理员配置的平台顺序对平台列表排序。
/// `order` 中的平台按其顺序排在前面，未列出的平台保持原有默认顺序排在最后。
pub(crate) fn sort_platforms_by_order(
    platforms: &mut [PlatformConfig],
    order: Option<&Vec<String>>,
) {
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

/// DB Option 是否有非空字符串（空串 / 纯空白不算已配置）。env 见 `nonempty_env`。
pub(crate) fn nonempty_db(opt: Option<&String>) -> bool {
    opt.is_some_and(|s| !s.trim().is_empty())
}

pub(crate) fn nonempty_env(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|s| !s.trim().is_empty())
}

/// Agent `config.get` / `platform.connection` / `auth.status` 共用：平台是否按配置视为已接通。
pub(crate) fn platform_configured_flags(
    config: &crate::config::DynamicConfig,
) -> Vec<(&'static str, bool)> {
    let nonempty = nonempty_db;
    let enabled = resolve_platform_enabled;
    vec![
        (
            "steam",
            enabled(
                config.steam_enabled,
                nonempty(config.steam_api_key.as_ref()),
            ),
        ),
        (
            "bilibili",
            enabled(
                config.bilibili_enabled,
                nonempty(config.bilibili_uid.as_ref()),
            ),
        ),
        (
            "github",
            enabled(
                config.github_enabled,
                nonempty(config.github_username.as_ref()),
            ),
        ),
        (
            "youtube",
            enabled(
                config.youtube_enabled,
                nonempty(config.youtube_api_key.as_ref())
                    && nonempty(config.youtube_channel_id.as_ref()),
            ),
        ),
        (
            "netease",
            enabled(
                config.netease_enabled,
                nonempty(config.netease_user_id.as_ref()),
            ),
        ),
        (
            "bangumi",
            enabled(
                config.bangumi_enabled,
                nonempty(config.bangumi_username.as_ref())
                    || nonempty(config.bangumi_access_token.as_ref()),
            ),
        ),
        (
            "x",
            enabled(
                config.x_enabled,
                nonempty(config.x_username.as_ref()) && nonempty(config.x_bearer_token.as_ref()),
            ),
        ),
        (
            "discord",
            enabled(
                config.discord_enabled,
                nonempty(config.discord_access_token.as_ref()),
            ),
        ),
        (
            "mal",
            enabled(config.mal_enabled, nonempty(config.mal_username.as_ref())),
        ),
        (
            "xbox",
            enabled(
                config.xbox_enabled,
                nonempty(config.openxbl_api_key.as_ref()),
            ),
        ),
        (
            "psn",
            enabled(config.psn_enabled, nonempty(config.psn_npsso.as_ref())),
        ),
    ]
}

/// Agent `config.get` ui 段：与公开 UI 运行时同类的非密钥字段。
pub(crate) fn public_ui_config_value(config: &crate::config::DynamicConfig) -> Value {
    json!({
        "analytics_enabled": config.analytics_enabled,
        "pwa_enabled": config.pwa_enabled,
        "wallpaper_url": config.ui_wallpaper_url,
        "wallpaper_blur": config.ui_wallpaper_blur,
        "evocative_parallax": config.ui_evocative_parallax,
        "evocative_dynamic_blur": config.ui_evocative_dynamic_blur,
        "evocative_ripple": config.ui_evocative_ripple,
        "evocative_fps": config.ui_evocative_fps,
        "evocative_ripple_quality": config.ui_evocative_ripple_quality,
        "music_enabled": config.music_enabled,
        "music_source": config.music_source,
        "site_title": config.site_title,
        "site_description": config.site_description,
    })
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
pub(crate) fn db_or_env_clearable(db_val: Option<String>, env_key: &str, default: &str) -> String {
    match db_val {
        Some(v) => v,
        None => std::env::var(env_key).unwrap_or_else(|_| default.to_string()),
    }
}
