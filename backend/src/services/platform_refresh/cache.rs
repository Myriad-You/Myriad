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

#[derive(Debug, Clone)]
pub struct PlatformCacheFile {
    pub platform: String,
    pub data: Value,
    pub fetched_at: DateTime<Utc>,
}

pub const PLATFORM_CACHE_HOURS: i64 = 12; // 数据缓存12小时

/// Load every on-disk platform file. This is not a freshness decision.
pub fn load_platform_cache_files() -> Vec<PlatformCacheFile> {
    let raw_dir = crate::services::data_paths::raw_cache_dir();
    let Ok(entries) = fs::read_dir(&raw_dir) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.contains('.') {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(json) = serde_json::from_str(&content) else {
            continue;
        };
        let fetched_at = fs::metadata(&path)
            .ok()
            .and_then(|meta| meta.modified().ok())
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);
        files.push(PlatformCacheFile {
            platform: stem.to_string(),
            data: json,
            fetched_at,
        });
    }
    files
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn platform_cache_from_files(files: Vec<PlatformCacheFile>) -> Option<PlatformDataCache> {
    if files.is_empty() {
        return None;
    }
    let fetched_at = files.iter().map(|file| file.fetched_at).min()?;
    let mut all_data = serde_json::Map::new();
    for file in files {
        all_data.insert(file.platform, file.data);
    }
    Some(PlatformDataCache {
        data: Value::Object(all_data),
        fetched_at,
    })
}

pub fn cache_timestamp_is_fresh(fetched_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(fetched_at) < Duration::hours(PLATFORM_CACHE_HOURS)
}

/// Bulk short-circuit only when every required platform has its own fresh file.
pub fn required_platforms_are_fresh(
    files: &[PlatformCacheFile],
    required: &[&str],
    now: DateTime<Utc>,
) -> bool {
    !required.is_empty()
        && required.iter().all(|platform| {
            files.iter().any(|file| {
                file.platform == *platform && cache_timestamp_is_fresh(file.fetched_at, now)
            })
        })
}

/// Merge base for fetches. Stale files stay available; callers decide freshness.
pub fn load_platform_data_cache() -> Option<PlatformDataCache> {
    platform_cache_from_files(load_platform_cache_files())
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
    fs::create_dir_all(&raw_dir)?;
    let Some(obj) = all_data.as_object() else {
        return Ok(());
    };
    let mut errors = Vec::new();
    for (platform, data) in obj {
        match write_one_raw_platform(&raw_dir, platform, data) {
            Ok(()) => tracing::info!(platform = %platform, "saved raw platform cache"),
            Err(error) => {
                tracing::error!(platform = %platform, %error, "failed to save raw platform cache");
                errors.push(format!("{platform}: {error}"));
            }
        }
    }
    if !errors.is_empty() {
        return Err(format!("failed to save raw platform cache: {}", errors.join("; ")).into());
    }
    Ok(())
}

fn write_one_raw_platform(
    raw_dir: &Path,
    platform: &str,
    data: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let file_path = raw_dir.join(format!("{platform}.json"));
    let temp_path = raw_dir.join(format!(
        "{platform}.{}.json.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let written = (|| {
        let file = std::fs::File::create(&temp_path)?;
        let mut writer = std::io::BufWriter::with_capacity(524288, file);
        serde_json::to_writer(&mut writer, data)?;
        use std::io::Write;
        writer.flush()?;
        commit_cache_temp_file(&temp_path, &file_path)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    written
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
    fn merged_cache_keeps_oldest_timestamp_and_last_platform_value() {
        use super::{PlatformCacheFile, platform_cache_from_files};
        use chrono::{Duration, Utc};
        use serde_json::json;
        let now = Utc::now();
        let oldest = now - Duration::hours(13);
        let files = vec![
            PlatformCacheFile {
                platform: "github".into(),
                data: json!({"repos": ["old"]}),
                fetched_at: oldest,
            },
            PlatformCacheFile {
                platform: "bilibili".into(),
                data: json!({"videos": ["video"]}),
                fetched_at: now,
            },
            PlatformCacheFile {
                platform: "github".into(),
                data: json!({"repos": ["new"]}),
                fetched_at: now,
            },
        ];
        let cache = platform_cache_from_files(files).unwrap();
        assert_eq!(cache.fetched_at, oldest);
        assert_eq!(
            cache.data,
            json!({
                "github": {"repos": ["new"]}, "bilibili": {"videos": ["video"]}
            })
        );
        assert!(platform_cache_from_files(Vec::new()).is_none());
    }

    #[cfg(feature = "hotpath")]
    #[test]
    #[ignore = "manual profiling workload; run alone with --features hotpath-alloc --nocapture"]
    fn profile_platform_cache_assembly() {
        use super::{PlatformCacheFile, platform_cache_from_files};
        use chrono::Utc;
        use serde_json::json;
        let padding = "x".repeat(2048);
        let files: Vec<_> = (0..4)
            .map(|id| PlatformCacheFile {
                platform: format!("platform-{id}"),
                data: json!({"rows": (0..1000).map(|row| json!({
                    "id": row, "title": format!("Title {row}"), "unused": padding
                })).collect::<Vec<_>>()}),
                fetched_at: Utc::now(),
            })
            .collect();
        let _profile = hotpath::HotpathGuardBuilder::new("platform_cache_assembly").build();
        for _ in 0..20 {
            // Each call gets an owned input; preparation is outside the measured function.
            let input = files.clone();
            std::hint::black_box(platform_cache_from_files(std::hint::black_box(input)));
        }
    }

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
        assert!(
            !temp.exists(),
            "failed temp file must be removed, not copied"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn freshness_is_per_required_platform() {
        use super::{PlatformCacheFile, required_platforms_are_fresh};
        use chrono::{Duration, Utc};
        use serde_json::json;
        let now = Utc::now();
        let files = vec![
            PlatformCacheFile {
                platform: "github".into(),
                data: json!({}),
                fetched_at: now - Duration::hours(1),
            },
            PlatformCacheFile {
                platform: "bilibili".into(),
                data: json!({}),
                fetched_at: now - Duration::hours(13),
            },
        ];
        assert!(
            !required_platforms_are_fresh(&files, &["github", "bilibili"], now),
            "one fresh file must not hide another platform's expiry"
        );
        assert!(required_platforms_are_fresh(&files, &["github"], now));
        assert!(
            !required_platforms_are_fresh(&files, &["github", "steam"], now),
            "a missing required platform is not fresh"
        );
        assert!(!required_platforms_are_fresh(&files, &[], now));
    }

    #[test]
    fn raw_save_uses_unique_temp_and_reports_partial_failure() {
        let src = include_str!("cache.rs");
        let save = src
            .split("pub fn save_split_raw_data")
            .nth(1)
            .and_then(|rest| rest.split("pub fn commit_cache_temp_file").next())
            .expect("save_split_raw_data");
        assert!(save.contains("uuid::Uuid::new_v4"));
        assert!(save.contains("failed to save raw platform cache"));
        assert!(!save.contains("继续处理其他平台"));
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
