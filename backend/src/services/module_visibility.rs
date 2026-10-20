//! Module visibility for Agent — re-export of workspace crate.
//!
//! Prefer this over `api::config` so services never import the HTTP layer.

pub use myriad_module_visibility::{
    agent_visibility_for_authorization, try_load_module_visibility_preferences,
};

/// Convenience: agent visibility level only (`all` / `authenticated` / `admin`).
///
/// Authorization boundary: DB/JSON errors stay errors instead of defaulting to `all`.
pub async fn agent_module_visibility(db: &sea_orm::DatabaseConnection) -> Result<String, String> {
    agent_visibility_for_authorization(try_load_module_visibility_preferences(db).await)
}

/// 模块可见性的进程内副本：博客每个请求（连样式、图片）都要判断一次能不能看，不再每次查库。
/// 缓存的只是判断的输入，判断本身（`phantasi_access_allowed`）照旧每次做。本进程写了这项
/// 配置会立刻作废；别的实例写的，最多 [`CACHE_TTL`] 后读到。读库出错不缓存，照旧当错误。
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5);

struct Cached {
    at: std::time::Instant,
    generation: u64,
    preferences: myriad_module_visibility::ModuleVisibilityPreferences,
}

static CACHE: std::sync::Mutex<Option<Cached>> = std::sync::Mutex::new(None);
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub async fn cached_module_visibility_preferences(
    db: &sea_orm::DatabaseConnection,
) -> Result<myriad_module_visibility::ModuleVisibilityPreferences, String> {
    use std::sync::atomic::Ordering;
    let generation = GENERATION.load(Ordering::SeqCst);
    if let Some(cached) = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .filter(|cached| cached.generation == generation && cached.at.elapsed() < CACHE_TTL)
    {
        return Ok(cached.preferences.clone());
    }
    let preferences = try_load_module_visibility_preferences(db).await?;
    let mut slot = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    // 读库期间有人写了配置：这份可能是旧的，不留。
    if GENERATION.load(Ordering::SeqCst) == generation {
        *slot = Some(Cached {
            at: std::time::Instant::now(),
            generation,
            preferences: preferences.clone(),
        });
    }
    Ok(preferences)
}

/// 写了配置之后调用。`key` 为 `None` 表示不知道写了哪些（整批恢复、导入）。
pub fn invalidate_module_visibility_cache(key: Option<&str>) {
    if key.is_some_and(|key| key != myriad_module_visibility::MODULE_VISIBILITY_PREFERENCES_KEY) {
        return;
    }
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    *CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}
