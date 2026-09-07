//! Process-local platform data cache (filtered JSON files under cache/platforms).
//!
//! Lives in services so AI context resolution and agent paths do not reach through
//! `api::tapp_runtime::common` for platform reads.

use once_cell::sync::Lazy;
use serde_json::Value;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

struct CacheEntry<V> {
    value: V,
    created_at: Instant,
}

struct TtlCache<V: Clone> {
    data: HashMap<String, CacheEntry<V>>,
    ttl: Duration,
}

impl<V: Clone> TtlCache<V> {
    fn new(ttl: Duration) -> Self {
        Self {
            data: HashMap::new(),
            ttl,
        }
    }

    fn get(&self, key: &str) -> Option<&V> {
        self.data.get(key).and_then(|entry| {
            if entry.created_at.elapsed() < self.ttl {
                Some(&entry.value)
            } else {
                None
            }
        })
    }

    fn set(&mut self, key: String, value: V) {
        self.data.insert(
            key,
            CacheEntry {
                value,
                created_at: Instant::now(),
            },
        );
    }

    fn len(&self) -> usize {
        self.data.len()
    }
}

struct SingleCache<V: Clone> {
    value: Option<V>,
    cached_at: Option<Instant>,
    ttl: Duration,
}

impl<V: Clone> SingleCache<V> {
    fn new(ttl: Duration) -> Self {
        Self {
            value: None,
            cached_at: None,
            ttl,
        }
    }

    fn get(&self) -> Option<V> {
        if let (Some(value), Some(cached_at)) = (&self.value, self.cached_at) {
            if cached_at.elapsed() < self.ttl {
                return Some(value.clone());
            }
        }
        None
    }

    fn set(&mut self, value: V) {
        self.value = Some(value);
        self.cached_at = Some(Instant::now());
    }
}

static PLATFORM_CACHE: Lazy<Arc<RwLock<TtlCache<Value>>>> =
    Lazy::new(|| Arc::new(RwLock::new(TtlCache::new(Duration::from_secs(30)))));

static PLATFORM_LOCKS: Lazy<RwLock<HashMap<String, Weak<Mutex<()>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

static PLATFORM_LIST_CACHE: Lazy<Arc<RwLock<SingleCache<Vec<String>>>>> =
    Lazy::new(|| Arc::new(RwLock::new(SingleCache::new(Duration::from_secs(60)))));

/// Validate platform name (path-traversal safe component).
pub fn validate_platform_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Invalid platform name length".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Platform name contains invalid characters".to_string());
    }
    Ok(())
}

fn platforms_cache_dir() -> &'static std::path::Path {
    crate::services::data_paths::platforms_cache_dir()
}

/// Available platforms from cache/platforms/*_filtered.json (60s TTL).
pub async fn get_available_platforms() -> Vec<String> {
    {
        let cache = PLATFORM_LIST_CACHE.read().await;
        if let Some(platforms) = cache.get() {
            return platforms;
        }
    }

    let cache_dir = platforms_cache_dir();
    let mut platforms = Vec::new();

    if let Ok(mut entries) = tokio::fs::read_dir(cache_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with("_filtered.json") {
                    let platform = name.trim_end_matches("_filtered.json");
                    platforms.push(platform.to_string());
                }
            }
        }
    }

    platforms.sort();

    {
        let mut cache = PLATFORM_LIST_CACHE.write().await;
        cache.set(platforms.clone());
    }

    platforms
}

/// Per-platform shared I/O lock for cache miss coalescing.
pub async fn acquire_platform_lock(platform: &str) -> Result<OwnedMutexGuard<()>, String> {
    validate_platform_name(platform)?;
    let key = platform.to_lowercase();
    let lock = {
        let mut locks = PLATFORM_LOCKS.write().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(Mutex::new(()));
            locks.insert(key, Arc::downgrade(&lock));
            lock
        }
    };
    Ok(lock.lock_owned().await)
}

/// Refresh in-memory cache after a successful file write.
pub async fn update_cached_platform_data(platform: &str, data: Value) -> Result<(), String> {
    validate_platform_name(platform)?;
    let key = platform.to_lowercase();
    PLATFORM_CACHE.write().await.set(key.clone(), data);

    let mut list_cache = PLATFORM_LIST_CACHE.write().await;
    if let Some(mut platforms) = list_cache.get() {
        if !platforms.iter().any(|value| value == &key) {
            platforms.push(key);
            platforms.sort();
            list_cache.set(platforms);
        }
    }
    Ok(())
}

fn platform_cache_read_failed(platform: &str, error: std::io::Error) -> String {
    tracing::error!(%error, platform, "failed to read platform cache");
    match error.kind() {
        ErrorKind::NotFound => format!("No cached {platform} data"),
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            format!("Failed to read {platform} data: storage is not writable")
        }
        ErrorKind::StorageFull => {
            format!("Failed to read {platform} data: not enough disk space")
        }
        _ => format!("Failed to read {platform} data"),
    }
}

/// Read platform filtered JSON with process TTL cache.
pub async fn get_cached_platform_data(platform: &str) -> Result<Value, String> {
    validate_platform_name(platform)?;
    let key = platform.to_lowercase();

    {
        let cache = PLATFORM_CACHE.read().await;
        if let Some(data) = cache.get(&key) {
            return Ok(data.clone());
        }
    }

    let _platform_guard = acquire_platform_lock(&key).await?;

    {
        let cache = PLATFORM_CACHE.read().await;
        if let Some(data) = cache.get(&key) {
            return Ok(data.clone());
        }
    }

    let cache_file = filtered_cache_path(&key);
    let content = tokio::fs::read_to_string(&cache_file)
        .await
        .map_err(|error| platform_cache_read_failed(&key, error))?;

    let data: Value = serde_json::from_str(&content).map_err(|error| {
        tracing::error!(%error, platform = %key, "failed to parse platform cache");
        format!("Failed to parse {key} data")
    })?;

    {
        let mut cache = PLATFORM_CACHE.write().await;
        cache.set(key, data.clone());
    }

    Ok(data)
}

/// Number of entries currently held in the process platform cache (metrics).
pub async fn platform_cache_entry_count() -> usize {
    PLATFORM_CACHE.read().await.len()
}

/// Domain errors for filtered-cache IO (write path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformCacheError {
    InvalidName(String),
    Read(String),
    Parse(String),
    Write(String),
    InvalidStructure,
}

impl PlatformCacheError {
    pub fn message(&self) -> String {
        match self {
            Self::InvalidName(msg) | Self::Read(msg) | Self::Parse(msg) | Self::Write(msg) => {
                msg.clone()
            }
            Self::InvalidStructure => "Invalid cache file structure".to_string(),
        }
    }

    /// Stable HTTP-ish status class for API adapters.
    pub fn status_hint(&self) -> u16 {
        match self {
            Self::InvalidName(_) => 400,
            Self::InvalidStructure => 500,
            Self::Read(_) | Self::Parse(_) | Self::Write(_) => 500,
        }
    }
}

impl std::fmt::Display for PlatformCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for PlatformCacheError {}

fn filtered_cache_path(platform_key: &str) -> std::path::PathBuf {
    platforms_cache_dir().join(format!("{platform_key}_filtered.json"))
}

/// On-disk path for a platform's filtered cache file.
pub fn platform_filtered_cache_path(platform: &str) -> Result<std::path::PathBuf, String> {
    validate_platform_name(platform)?;
    Ok(filtered_cache_path(&platform.to_lowercase()))
}

/// Ensure `cache/platforms` exists.
pub async fn ensure_platforms_dir() -> Result<(), PlatformCacheError> {
    tokio::fs::create_dir_all(platforms_cache_dir())
        .await
        .map_err(|error| {
            tracing::error!(%error, "[PLATFORM] Failed to create platform cache directory");
            PlatformCacheError::Write("Failed to save platform data".to_string())
        })
}

/// Load filtered JSON for mutation. Missing file → `{ "items": [] }`.
///
/// Caller should hold [`acquire_platform_lock`] for the same platform when
/// coordinating with concurrent writers.
pub async fn load_filtered_document(platform: &str) -> Result<Value, PlatformCacheError> {
    validate_platform_name(platform).map_err(PlatformCacheError::InvalidName)?;
    let key = platform.to_lowercase();
    let cache_file = filtered_cache_path(&key);
    match tokio::fs::read_to_string(&cache_file).await {
        Ok(content) => serde_json::from_str::<Value>(&content).map_err(|error| {
            tracing::error!(%error, platform = %key, "[PLATFORM] Invalid platform cache file");
            PlatformCacheError::Parse("Invalid platform cache data".to_string())
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(serde_json::json!({ "items": [] }))
        }
        Err(error) => {
            tracing::error!(%error, platform = %key, "[PLATFORM] Failed to read platform cache");
            Err(PlatformCacheError::Read(
                "Failed to read platform data".to_string(),
            ))
        }
    }
}

/// Atomically write filtered JSON (tmp + rename) and refresh the process cache.
///
/// Caller should hold [`acquire_platform_lock`] for the same platform.
pub async fn write_filtered_document(
    platform: &str,
    data: &Value,
) -> Result<(), PlatformCacheError> {
    validate_platform_name(platform).map_err(PlatformCacheError::InvalidName)?;
    let key = platform.to_lowercase();
    ensure_platforms_dir().await?;
    let cache_file = filtered_cache_path(&key);
    let tmp_file = cache_file.with_extension(format!("json.{}.tmp", uuid::Uuid::new_v4()));
    let content = serde_json::to_string_pretty(data).map_err(|error| {
        tracing::error!(%error, platform = %key, "[PLATFORM] Failed to serialize platform cache");
        PlatformCacheError::Write("Failed to save platform data".to_string())
    })?;
    if let Err(e) = tokio::fs::write(&tmp_file, &content).await {
        tracing::error!(%e, platform = %key, "[PLATFORM] Failed to write temp cache file");
        return Err(PlatformCacheError::Write(
            "Failed to save platform data".to_string(),
        ));
    }
    if let Err(e) = tokio::fs::rename(&tmp_file, &cache_file).await {
        tracing::error!(%e, platform = %key, "[PLATFORM] Failed to rename cache file");
        let _ = tokio::fs::remove_file(&tmp_file).await;
        return Err(PlatformCacheError::Write(
            "Failed to save platform data".to_string(),
        ));
    }
    update_cached_platform_data(&key, data.clone())
        .await
        .map_err(|error| {
            tracing::error!(%error, platform = %key, "[PLATFORM] Failed to refresh process cache");
            PlatformCacheError::Write("Failed to refresh platform data".to_string())
        })?;
    Ok(())
}

/// Append one or more items into the filtered document's `items` array.
///
/// Acquires the platform lock for the full read-modify-write. Fails with
/// [`PlatformCacheError::InvalidStructure`] when `items` exists but is not an array.
pub async fn append_filtered_items(
    platform: &str,
    new_items: Vec<Value>,
) -> Result<(), PlatformCacheError> {
    validate_platform_name(platform).map_err(PlatformCacheError::InvalidName)?;
    let key = platform.to_lowercase();
    let _lock = acquire_platform_lock(&key)
        .await
        .map_err(PlatformCacheError::InvalidName)?;
    ensure_platforms_dir().await?;
    let mut data = load_filtered_document(&key).await?;
    if let Some(items) = data.get_mut("items").and_then(|v| v.as_array_mut()) {
        items.extend(new_items);
    } else if data.get("items").is_none() {
        data["items"] = Value::Array(new_items);
    } else {
        return Err(PlatformCacheError::InvalidStructure);
    }
    write_filtered_document(&key, &data).await
}

/// Build a Tapp-authored platform item document (write path).
pub fn build_tapp_written_item(
    item_id: &str,
    tapp_id: &str,
    item_type: &str,
    title: &str,
    cover: Option<String>,
    description: Option<String>,
    url: Option<String>,
    metadata: Option<Value>,
    created_at: Option<String>,
) -> Value {
    serde_json::json!({
        "id": item_id,
        "type": item_type,
        "title": title,
        "cover": cover,
        "description": description,
        "url": url,
        "metadata": metadata,
        "createdAt": created_at.unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        "source": format!("tapp:{tapp_id}")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_tapp_written_item, filtered_cache_path, platform_cache_read_failed,
        platform_filtered_cache_path, validate_platform_name, PlatformCacheError,
    };
    use std::io::{Error, ErrorKind};

    #[test]
    fn platform_cache_read_failed_names_platform_without_os_dump() {
        assert_eq!(
            platform_cache_read_failed("steam", Error::from(ErrorKind::NotFound)),
            "No cached steam data"
        );
        assert_eq!(
            platform_cache_read_failed("bilibili", Error::from(ErrorKind::PermissionDenied)),
            "Failed to read bilibili data: storage is not writable"
        );
        assert!(
            !platform_cache_read_failed("steam", Error::from(ErrorKind::Other))
                .contains("os error")
        );
    }
    use serde_json::json;

    #[test]
    fn platform_names_reject_path_traversal() {
        assert!(validate_platform_name("steam").is_ok());
        assert!(validate_platform_name("bili-bili").is_ok());
        assert!(validate_platform_name("../etc").is_err());
        assert!(validate_platform_name("").is_err());
        assert!(validate_platform_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn filtered_cache_path_is_platform_scoped() {
        let path = filtered_cache_path("steam");
        assert!(path.ends_with("cache/platforms/steam_filtered.json"));
        let public = platform_filtered_cache_path("steam").expect("valid name");
        assert_eq!(public, path);
        assert!(platform_filtered_cache_path("../etc").is_err());
    }

    #[test]
    fn build_tapp_written_item_sets_source_and_id() {
        let item = build_tapp_written_item(
            "tapp_abc",
            "com.example.app",
            "game",
            "Hades",
            Some("https://example.com/c.jpg".into()),
            None,
            None,
            Some(json!({ "appid": "1145360" })),
            Some("2026-01-01T00:00:00Z".into()),
        );
        assert_eq!(item["id"], "tapp_abc");
        assert_eq!(item["title"], "Hades");
        assert_eq!(item["source"], "tapp:com.example.app");
        assert_eq!(item["createdAt"], "2026-01-01T00:00:00Z");
        assert_eq!(item["metadata"]["appid"], "1145360");
    }

    #[test]
    fn cache_error_status_hints() {
        assert_eq!(
            PlatformCacheError::InvalidName("x".into()).status_hint(),
            400
        );
        assert_eq!(PlatformCacheError::InvalidStructure.status_hint(), 500);
        assert_eq!(PlatformCacheError::Write("x".into()).status_hint(), 500);
    }
}
