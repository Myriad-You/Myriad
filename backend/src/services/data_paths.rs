//! 数据路径配置模块
//!
//! DATA_DIR / CACHE_DIR layout for brew, tapps, agent, cache, widget-fonts.
//! 支持从环境变量覆盖默认值，便于容器化部署

use once_cell::sync::Lazy;
use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// 数据路径配置
pub struct DataPaths {
    /// 应用数据根目录（默认: "data"）
    pub root: PathBuf,
    /// Brew 订阅源数据目录（默认: "data/brew"）
    pub brew: PathBuf,
    /// Brew 图标目录（默认: "data/brew/icons"）
    pub brew_icons: PathBuf,
    /// Tapp 应用数据目录（默认: "data/tapps"）
    pub tapps: PathBuf,
    /// Agent identity / skills / MCP / memory root（默认: "data/agent"）
    pub agent: PathBuf,
    /// 缓存目录（默认: "cache"）
    pub cache: PathBuf,
    /// 平台数据缓存目录（默认: "cache/platforms"）
    pub cache_platforms: PathBuf,
    /// 原始数据缓存目录（默认: "cache/raw"；可用 `CACHE_DIR` 覆盖根）
    pub cache_raw: PathBuf,
    /// 图片缓存目录（默认: "cache/images"）
    pub cache_images: PathBuf,
    /// Optional widget custom fonts (default: "data/site/widget-fonts")
    pub widget_fonts: PathBuf,
}

impl DataPaths {
    /// 从环境变量加载配置
    pub fn from_env() -> Self {
        let root = PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "data".to_string()));
        let cache_root =
            PathBuf::from(env::var("CACHE_DIR").unwrap_or_else(|_| "cache".to_string()));

        Self {
            brew: root.join("brew"),
            brew_icons: root.join("brew/icons"),
            tapps: root.join("tapps"),
            agent: root.join("agent"),
            cache_platforms: cache_root.join("platforms"),
            cache_raw: cache_root.join("raw"),
            cache_images: cache_root.join("images"),
            widget_fonts: root.join("site/widget-fonts"),
            root,
            cache: cache_root,
        }
    }

    /// 获取用户 Tapp 目录
    /// 结构: {tapps}/{user_id}/
    pub fn tapp_user_dir(&self, user_id: i32) -> PathBuf {
        self.tapps.join(user_id.to_string())
    }

    /// 获取 Agent RSSHub 路由缓存文件路径
    pub fn rsshub_routes_cache(&self) -> PathBuf {
        self.cache.join("rsshub_routes.json")
    }

    /// Filtered JSON for one platform (`{slug}_filtered.json`).
    pub fn platform_filtered_file(&self, platform: &str) -> PathBuf {
        self.cache_platforms
            .join(format!("{platform}_filtered.json"))
    }

    /// Raw fetch JSON for one platform (`{slug}.json` under cache_raw).
    pub fn platform_raw_file(&self, platform: &str) -> PathBuf {
        self.cache_raw.join(format!("{platform}.json"))
    }
}

/// Process-wide platform cache directory (`CACHE_DIR/platforms`).
pub fn platforms_cache_dir() -> &'static Path {
    paths().cache_platforms.as_path()
}

/// Filtered JSON path for one platform slug.
pub fn platform_filtered_file(platform: impl AsRef<str>) -> PathBuf {
    paths().platform_filtered_file(platform.as_ref())
}

/// Process-wide raw platform cache directory (`CACHE_DIR/raw`).
pub fn raw_cache_dir() -> &'static Path {
    paths().cache_raw.as_path()
}

/// Raw fetch JSON path for one platform slug.
pub fn platform_raw_file(platform: impl AsRef<str>) -> PathBuf {
    paths().platform_raw_file(platform.as_ref())
}

impl Default for DataPaths {
    fn default() -> Self {
        Self::from_env()
    }
}

/// 全局数据路径配置（惰性初始化）
pub static DATA_PATHS: Lazy<DataPaths> = Lazy::new(DataPaths::from_env);

/// 便捷访问函数
pub fn paths() -> &'static DataPaths {
    &DATA_PATHS
}

fn storage_error(action: &str, path: &Path, error: io::Error) -> io::Error {
    let cause = match error.kind() {
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            "storage is not writable"
        }
        io::ErrorKind::StorageFull => "not enough disk space",
        io::ErrorKind::NotFound => "path not found",
        io::ErrorKind::AlreadyExists => "already exists",
        _ => "failed",
    };
    io::Error::new(
        error.kind(),
        format!("{action} {}: {cause}", path.display()),
    )
}

/// Prove that the backend uid can create and remove a file in `directory`.
///
/// Checking metadata or `Permissions::readonly` is insufficient for ACLs,
/// read-only mounts and network filesystems. Probe is `create_new` then unlink
/// (`create_new` will not follow or truncate a pre-existing path).
fn verify_directory_writable(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)
        .map_err(|error| storage_error("create storage directory", directory, error))?;
    let probe = directory.join(format!(
        ".myriad-storage-write-probe-{}",
        uuid::Uuid::new_v4().simple()
    ));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|error| storage_error("write storage probe in", directory, error))?;
    fs::remove_file(&probe).map_err(|error| storage_error("remove storage probe", &probe, error))
}

fn verify_storage_layout_writable(data_paths: &DataPaths) -> io::Result<()> {
    verify_directory_writable(&data_paths.root)?;
    verify_directory_writable(&data_paths.cache)?;
    verify_directory_writable(&data_paths.tapps)?;
    verify_directory_writable(&data_paths.widget_fonts)?;

    let entries = fs::read_dir(&data_paths.tapps)
        .map_err(|error| storage_error("list Tapp owner directories", &data_paths.tapps, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            storage_error("read Tapp owner directory", &data_paths.tapps, error)
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| storage_error("inspect Tapp owner directory", &entry.path(), error))?;
        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "refusing symlink in Tapp owner directory: {}",
                    entry.path().display()
                ),
            ));
        }
        if file_type.is_dir() {
            verify_directory_writable(&entry.path())?;
        }
    }
    Ok(())
}

/// Startup preflight for persistent storage. A backend that cannot write its
/// volumes must not report healthy, otherwise the updater would accept a
/// deployment that later fails every Tapp install with PermissionDenied.
pub fn verify_runtime_storage_writable() -> io::Result<()> {
    verify_storage_layout_writable(paths())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_paths() {
        let paths = DataPaths::from_env();
        assert_eq!(paths.root, PathBuf::from("data"));
        assert_eq!(paths.brew, PathBuf::from("data/brew"));
        assert_eq!(paths.tapps, PathBuf::from("data/tapps"));
        assert_eq!(paths.agent, PathBuf::from("data/agent"));
        // Canonical cache layout under CACHE_DIR.
        assert_eq!(paths.cache_platforms, PathBuf::from("cache/platforms"));
        assert_eq!(paths.cache_raw, PathBuf::from("cache/raw"));
        assert_eq!(paths.cache_images, PathBuf::from("cache/images"));
        assert_eq!(paths.widget_fonts, PathBuf::from("data/site/widget-fonts"));
    }

    #[test]
    fn test_tapp_user_dir() {
        let paths = DataPaths::from_env();
        let user_dir = paths.tapp_user_dir(42);
        assert_eq!(user_dir, PathBuf::from("data/tapps/42"));
    }

    #[test]
    fn test_rsshub_routes_cache() {
        let paths = DataPaths::from_env();
        assert_eq!(
            paths.rsshub_routes_cache(),
            PathBuf::from("cache/rsshub_routes.json")
        );
    }

    #[test]
    fn storage_preflight_probes_roots_and_existing_tapp_owners() {
        let base = std::env::temp_dir().join(format!(
            "myriad-storage-preflight-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let data_paths = DataPaths {
            root: base.join("data"),
            brew: base.join("data/brew"),
            brew_icons: base.join("data/brew/icons"),
            tapps: base.join("data/tapps"),
            agent: base.join("data/agent"),
            cache: base.join("cache"),
            cache_platforms: base.join("cache/platforms"),
            cache_raw: base.join("cache/raw"),
            cache_images: base.join("cache/images"),
            widget_fonts: base.join("data/site/widget-fonts"),
        };
        fs::create_dir_all(data_paths.tapps.join("1")).unwrap();
        fs::create_dir_all(data_paths.tapps.join("not-an-owner")).unwrap();

        verify_storage_layout_writable(&data_paths).unwrap();
        assert!(fs::read_dir(&data_paths.root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("write-probe")));
        assert!(fs::read_dir(data_paths.tapps.join("1"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("write-probe")));

        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn storage_preflight_reports_the_failing_path() {
        let base = std::env::temp_dir().join(format!(
            "myriad-storage-preflight-file-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&base).unwrap();
        let data_file = base.join("data-as-file");
        fs::write(&data_file, "not a directory").unwrap();
        let data_paths = DataPaths {
            root: data_file.clone(),
            brew: data_file.join("brew"),
            brew_icons: data_file.join("brew/icons"),
            tapps: data_file.join("tapps"),
            agent: data_file.join("agent"),
            cache: base.join("cache"),
            cache_platforms: base.join("cache/platforms"),
            cache_raw: base.join("cache/raw"),
            cache_images: base.join("cache/images"),
            widget_fonts: data_file.join("site/widget-fonts"),
        };

        let error = verify_storage_layout_writable(&data_paths).unwrap_err();
        let message = error.to_string();
        assert!(message.contains(&data_file.display().to_string()));
        assert!(!message.contains("os error"));
        assert!(message.contains("create storage directory"), "{message}");

        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn storage_preflight_rejects_tapp_owner_symlinks() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!(
            "myriad-storage-preflight-symlink-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let outside = base.join("outside");
        let data_paths = DataPaths {
            root: base.join("data"),
            brew: base.join("data/brew"),
            brew_icons: base.join("data/brew/icons"),
            tapps: base.join("data/tapps"),
            agent: base.join("data/agent"),
            cache: base.join("cache"),
            cache_platforms: base.join("cache/platforms"),
            cache_raw: base.join("cache/raw"),
            cache_images: base.join("cache/images"),
            widget_fonts: base.join("data/site/widget-fonts"),
        };
        fs::create_dir_all(&data_paths.tapps).unwrap();
        fs::create_dir_all(&outside).unwrap();
        symlink(&outside, data_paths.tapps.join("1")).unwrap();

        let error = verify_storage_layout_writable(&data_paths).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("refusing symlink"));

        fs::remove_dir_all(base).unwrap();
    }
}
