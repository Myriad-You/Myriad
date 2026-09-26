//! Platform configured-flags and public UI value helpers.
//!
//! No Axum: Agent and other services must not import the HTTP config handlers
//! just to read these flags.

use super::types::PlatformConfig;
pub(crate) use crate::services::config_service::public_ui_config_value;
use crate::services::platform_id::PlatformId;
pub(crate) use crate::services::platform_id::platform_configured_flags;

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

/// 管理台表单里凭据字段显示、并在保存时原样写回的值：库里存过就用库里的
/// （含有意清空的空串），从未存过才用 `env`（与 `db_or_env_clearable` 同一语义）。
pub(crate) fn form_credentials(
    stored: &crate::config::DynamicConfig,
    env: impl Fn(&str) -> Option<String>,
) -> crate::config::DynamicConfig {
    let pick = |value: &Option<String>, key: &str| {
        Some(
            value
                .clone()
                .unwrap_or_else(|| env(key).unwrap_or_default()),
        )
    };
    let mut form = stored.clone();
    form.github_username = pick(&stored.github_username, "GITHUB_USERNAME");
    form.bilibili_uid = pick(&stored.bilibili_uid, "BILIBILI_UID");
    form.steam_api_key = pick(&stored.steam_api_key, "STEAM_API_KEY");
    form.steam_id = pick(&stored.steam_id, "STEAM_ID");
    form.youtube_api_key = pick(&stored.youtube_api_key, "YOUTUBE_API_KEY");
    form.youtube_channel_id = pick(&stored.youtube_channel_id, "YOUTUBE_CHANNEL_ID");
    form.netease_user_id = pick(&stored.netease_user_id, "NETEASE_USER_ID");
    form.bangumi_username = pick(&stored.bangumi_username, "BANGUMI_USERNAME");
    form.bangumi_access_token = pick(&stored.bangumi_access_token, "BANGUMI_ACCESS_TOKEN");
    form.x_username = pick(&stored.x_username, "X_USERNAME");
    form.x_bearer_token = pick(&stored.x_bearer_token, "X_BEARER_TOKEN");
    form.discord_access_token = pick(&stored.discord_access_token, "DISCORD_ACCESS_TOKEN");
    form.mal_username = pick(&stored.mal_username, "MAL_USERNAME");
    form.xbox_gamertag = pick(&stored.xbox_gamertag, "XBOX_GAMERTAG");
    form.openxbl_api_key = pick(&stored.openxbl_api_key, "OPENXBL_API_KEY");
    form.psn_online_id = pick(&stored.psn_online_id, "PSN_ONLINE_ID");
    form.psn_npsso = pick(&stored.psn_npsso, "PSN_NPSSO");
    form
}

/// 管理台平台开关：[`PlatformId::enabled`] 作用在 [`form_credentials`] 上。
/// 没有 env 可回落时它就是运行时的启用；只靠 env 的平台显示为「保存后会启用」，
/// 这样按表单现状保存不会把开关写成关。
pub(crate) fn admin_platform_enabled(
    stored: Option<&crate::config::DynamicConfig>,
    env: impl Fn(&str) -> Option<String>,
) -> Vec<(PlatformId, bool)> {
    let form = form_credentials(&stored.cloned().unwrap_or_default(), env);
    PlatformId::ALL
        .into_iter()
        .map(|id| (id, id.enabled(&form)))
        .collect()
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
