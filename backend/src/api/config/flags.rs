//! Platform configured-flags and public UI value helpers.
//!
//! No Axum: Agent and other services must not import the HTTP config handlers
//! just to read these flags.

use super::types::PlatformConfig;
use crate::services::platform_id::PlatformId;

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

/// DB Option 是否有非空字符串（空串 / 纯空白不算已配置）。
pub(crate) fn nonempty_db(opt: Option<&String>) -> bool {
    opt.is_some_and(|s| !s.trim().is_empty())
}

/// 管理台平台开关：就是运行时的 [`PlatformId::enabled`]。环境变量只在启动时给从没
/// 存过的设置一次初值（`config_service::env_seed`），表单和运行时都只看库。
pub(crate) fn admin_platform_enabled(
    stored: &crate::config::DynamicConfig,
) -> Vec<(PlatformId, bool)> {
    PlatformId::ALL
        .into_iter()
        .map(|id| (id, id.enabled(stored)))
        .collect()
}
