//! Disk cache load/save for per-platform raw JSON.

use crate::services::library_items::invalidate_library_assembly_cache;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlatformDataCache {
    pub data: Value,
    pub fetched_at: DateTime<Utc>,
}

pub const PLATFORM_CACHE_HOURS: i64 = 12; // 数据缓存12小时

/// 从磁盘加载平台数据缓存
pub fn load_platform_data_cache() -> Option<PlatformDataCache> {
    // 从分平台 raw 目录加载；缺失或过期返回 None（无第二缓存路径）
    let raw_dir = crate::services::data_paths::raw_cache_dir();
    if raw_dir.exists() {
        let mut all_data = serde_json::Map::new();
        let mut latest_time = std::time::SystemTime::UNIX_EPOCH;
        let mut found_any = false;

        if let Ok(entries) = fs::read_dir(&raw_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if let Ok(content) = fs::read_to_string(&path) {
                            if let Ok(json) = serde_json::from_str(&content) {
                                all_data.insert(stem.to_string(), json);
                                found_any = true;

                                if let Ok(metadata) = fs::metadata(&path) {
                                    if let Ok(modified) = metadata.modified() {
                                        if modified > latest_time {
                                            latest_time = modified;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if found_any {
            let fetched_at: DateTime<Utc> = latest_time.into();
            let age = Utc::now() - fetched_at;

            if age < Duration::hours(PLATFORM_CACHE_HOURS) {
                tracing::info!(
                    "✓ Loaded platform data from split raw files (age: {}h)",
                    age.num_hours()
                );
                return Some(PlatformDataCache {
                    data: Value::Object(all_data),
                    fetched_at,
                });
            } else {
                tracing::info!(
                    "⏰ Split platform data cache expired (age: {}h)",
                    age.num_hours()
                );
                return None;
            }
        }
    }

    None
}

/// 保存平台数据缓存到磁盘（优化：只保存分平台数据，不再保存完整大文件）
pub fn save_platform_data_cache(data: &Value) -> Result<(), Box<dyn std::error::Error>> {
    // 保存分平台的原始数据
    save_split_raw_data(data)?;
    invalidate_library_assembly_cache();
    Ok(())
}

/// 保存分平台的原始数据（避免读取大文件）
/// 优化：添加错误容错和大文件分块写入
pub fn save_split_raw_data(all_data: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let raw_dir = crate::services::data_paths::raw_cache_dir();
    if !raw_dir.exists() {
        fs::create_dir_all(&raw_dir)?;
    }

    if let Some(obj) = all_data.as_object() {
        for (platform, data) in obj {
            // 保存所有平台的数据，不仅仅是主要平台
            let file_path = raw_dir.join(format!("{}.json", platform));

            // 优化：先写入临时文件，然后原子性重命名，避免写入中断导致文件损坏
            let temp_path = raw_dir.join(format!("{}.json.tmp", platform));

            match std::fs::File::create(&temp_path) {
                Ok(file) => {
                    // 使用更大的缓冲区处理大文件（512KB）
                    let mut writer = std::io::BufWriter::with_capacity(524288, file);

                    match serde_json::to_writer(&mut writer, data) {
                        Ok(_) => {
                            use std::io::Write;
                            if let Err(e) = writer.flush() {
                                tracing::warn!("⚠️ Failed to flush {} data: {}", platform, e);
                                // 继续处理其他平台
                                continue;
                            }

                            commit_cache_temp_file(&temp_path, &file_path).map_err(|e| {
                                tracing::error!(
                                    "❌ Failed to atomically replace cache for {}: {}",
                                    platform,
                                    e
                                );
                                e
                            })?;

                            tracing::info!("💾 Saved raw data for {} to {:?}", platform, file_path);
                        }
                        Err(e) => {
                            tracing::error!("❌ Failed to serialize {} data: {}", platform, e);
                            let _ = std::fs::remove_file(&temp_path);
                            // 继续处理其他平台，不返回错误
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("❌ Failed to create temp file for {}: {}", platform, e);
                    // 继续处理其他平台
                }
            }
        }
    }
    Ok(())
}

/// Same-directory tmp + rename. Rename failure is an error, never a copy overlay.
pub fn commit_cache_temp_file(temp_path: &Path, dest: &Path) -> Result<(), std::io::Error> {
    commit_cache_temp_file_with(temp_path, dest, |from, to| fs::rename(from, to))
}

pub(crate) fn commit_cache_temp_file_with(
    temp_path: &Path,
    dest: &Path,
    rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), std::io::Error> {
    match rename(temp_path, dest) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(temp_path);
            Err(error)
        }
    }
}

#[cfg(test)]
mod atomic_replace_tests {
    use super::{commit_cache_temp_file, commit_cache_temp_file_with};
    use std::fs;
    use std::io::{Error, ErrorKind};

    #[test]
    fn rename_failure_is_error_and_does_not_copy_over_destination() {
        let root = std::env::temp_dir().join(format!(
            "myriad-plat-cache-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let dest = root.join("dest.json");
        fs::write(&dest, b"original").unwrap();
        let temp = root.join("dest.json.tmp");
        fs::write(&temp, b"partial-write").unwrap();

        let err = commit_cache_temp_file_with(&temp, &dest, |_from, _to| {
            Err(Error::from(ErrorKind::CrossesDevices))
        })
        .unwrap_err();
        assert_eq!(err.kind(), ErrorKind::CrossesDevices);
        assert_eq!(fs::read(&dest).unwrap(), b"original");
        assert!(!temp.exists(), "failed temp file must be removed, not copied");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn successful_rename_replaces_destination() {
        let root = std::env::temp_dir().join(format!(
            "myriad-plat-cache-ok-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let dest = root.join("dest.json");
        fs::write(&dest, b"old").unwrap();
        let temp = root.join("dest.json.tmp");
        fs::write(&temp, b"new").unwrap();
        commit_cache_temp_file(&temp, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new");
        assert!(!temp.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
