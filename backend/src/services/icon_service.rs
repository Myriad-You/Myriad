//! 图标下载与缓存服务
//!
//! 负责下载网站图标并存储到本地，避免直接引用外链。
//! 图标存储在 data/brew/icons/ 目录下（可通过 DATA_DIR 环境变量配置）。

use super::data_paths::paths;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info, warn};

fn icon_io_failed(action: &'static str, error: std::io::Error) -> String {
    tracing::error!(%error, action, "icon io failed");
    match error.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            format!("{action}: storage is not writable")
        }
        ErrorKind::StorageFull => format!("{action}: not enough disk space"),
        ErrorKind::NotFound => format!("{action}: path not found"),
        _ => action.to_string(),
    }
}

/// 图标服务
pub struct IconService {
    icons_dir: PathBuf,
}

/// 图标文件信息
#[derive(Debug, Clone)]
pub struct IconInfo {
    /// 本地相对路径（用于API返回）
    pub local_path: String,
}

impl IconService {
    /// 创建新的图标服务实例
    pub fn new() -> Self {
        // 使用统一的数据路径配置
        let icons_dir = paths().brew_icons.clone();

        Self { icons_dir }
    }

    /// 确保图标目录存在
    async fn ensure_dir(&self) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.icons_dir).await
    }

    /// 根据 Content-Type 或 URL 推断文件扩展名
    fn get_extension(content_type: Option<&str>, url: &str) -> &'static str {
        // 首先检查 Content-Type
        if let Some(ct) = content_type {
            match ct.to_lowercase().as_str() {
                t if t.contains("image/png") => return "png",
                t if t.contains("image/jpeg") || t.contains("image/jpg") => return "jpg",
                t if t.contains("image/gif") => return "gif",
                t if t.contains("image/webp") => return "webp",
                t if t.contains("image/svg+xml") => return "svg",
                t if t.contains("image/x-icon") || t.contains("image/vnd.microsoft.icon") => {
                    return "ico"
                }
                _ => {}
            }
        }

        // 从 URL 推断
        let url_lower = url.to_lowercase();
        if url_lower.ends_with(".png") {
            "png"
        } else if url_lower.ends_with(".jpg") || url_lower.ends_with(".jpeg") {
            "jpg"
        } else if url_lower.ends_with(".gif") {
            "gif"
        } else if url_lower.ends_with(".webp") {
            "webp"
        } else if url_lower.ends_with(".svg") {
            "svg"
        } else if url_lower.ends_with(".ico") {
            "ico"
        } else {
            // 默认使用 ico（大多数 favicon 是这种格式）
            "ico"
        }
    }

    /// 下载并保存图标
    ///
    /// # Arguments
    /// * `source_id` - 订阅源 ID，用于命名文件
    /// * `icon_url` - 图标的原始 URL
    ///
    /// # Returns
    /// * `Ok(Some(IconInfo))` - 下载成功，返回本地路径信息
    /// * `Ok(None)` - 下载失败但不是错误（如 404）
    /// * `Err` - 发生错误
    pub async fn download_icon(
        &self,
        source_id: i32,
        icon_url: &str,
    ) -> Result<Option<IconInfo>, String> {
        // 确保目录存在
        self.ensure_dir()
            .await
            .map_err(|error| icon_io_failed("Failed to create icons directory", error))?;

        debug!("Downloading icon for source {}: {}", source_id, icon_url);

        // SSRF 防护：阻止请求内网地址
        if crate::federation::types::is_internal_url(icon_url) {
            warn!("Blocked SSRF attempt in icon download: {}", icon_url);
            return Ok(None);
        }

        let (target_url, client) =
            match crate::services::outbound_security::build_public_http_client(
                icon_url,
                Duration::from_secs(10),
                Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
            )
            .await
            {
                Ok(target) => target,
                Err(error) => {
                    warn!(
                        "Blocked unsafe icon URL for source {}: {}",
                        source_id, error
                    );
                    return Ok(None);
                }
            };

        // 下载图标
        let response = match client.get(target_url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Failed to download icon for source {}: {}", source_id, e);
                return Ok(None);
            }
        };

        // 检查状态码
        if !response.status().is_success() {
            warn!(
                "Icon download failed for source {} with status: {}",
                source_id,
                response.status()
            );
            return Ok(None);
        }

        // 获取 Content-Type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok());

        // 获取扩展名
        let extension = Self::get_extension(content_type, icon_url);

        // 读取内容
        let bytes = response.bytes().await.map_err(|error| {
            tracing::warn!(%error, "failed to read icon bytes");
            crate::services::agent::external_pure::classify_outbound_fetch(
                "Failed to read icon bytes",
                &error.to_string(),
            )
        })?;

        // 检查是否为有效的图片（至少有一些字节）
        if bytes.len() < 10 {
            warn!(
                "Downloaded icon for source {} is too small ({} bytes), skipping",
                source_id,
                bytes.len()
            );
            return Ok(None);
        }

        // 生成文件名：source_{id}.{ext}
        let filename = format!("source_{}.{}", source_id, extension);
        let file_path = self.icons_dir.join(&filename);

        // 写入文件
        let mut file = fs::File::create(&file_path)
            .await
            .map_err(|error| icon_io_failed("Failed to create icon file", error))?;

        file.write_all(&bytes)
            .await
            .map_err(|error| icon_io_failed("Failed to write icon file", error))?;

        info!(
            "Saved icon for source {} to {} ({} bytes)",
            source_id,
            file_path.display(),
            bytes.len()
        );

        Ok(Some(IconInfo {
            local_path: format!("/api/brew/icons/{}", filename),
        }))
    }

    /// 删除指定订阅源的图标
    pub async fn delete_icon(&self, source_id: i32) -> Result<(), String> {
        // 尝试删除所有可能的扩展名
        let extensions = ["ico", "png", "jpg", "gif", "webp", "svg"];

        for ext in extensions {
            let filename = format!("source_{}.{}", source_id, ext);
            let file_path = self.icons_dir.join(&filename);

            if file_path.exists() {
                if let Err(e) = fs::remove_file(&file_path).await {
                    error!("Failed to delete icon file {}: {}", file_path.display(), e);
                } else {
                    info!("Deleted icon file: {}", file_path.display());
                }
            }
        }

        Ok(())
    }
}

impl Default for IconService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_extension() {
        assert_eq!(IconService::get_extension(Some("image/png"), ""), "png");
        assert_eq!(IconService::get_extension(Some("image/jpeg"), ""), "jpg");
        assert_eq!(IconService::get_extension(Some("image/x-icon"), ""), "ico");
        assert_eq!(
            IconService::get_extension(None, "https://example.com/icon.png"),
            "png"
        );
        assert_eq!(
            IconService::get_extension(None, "https://example.com/favicon.ico"),
            "ico"
        );
        assert_eq!(
            IconService::get_extension(None, "https://example.com/icon"),
            "ico"
        );
    }

    #[test]
    fn icon_io_failed_keeps_action_and_disk_cause() {
        use std::io::Error;

        let full = icon_io_failed(
            "Failed to write icon file",
            Error::from(ErrorKind::StorageFull),
        );
        assert_eq!(full, "Failed to write icon file: not enough disk space");
        assert!(!full.contains("os error"));

        let create = icon_io_failed(
            "Failed to create icons directory",
            Error::from(ErrorKind::PermissionDenied),
        );
        assert_eq!(
            create,
            "Failed to create icons directory: storage is not writable"
        );
        assert_ne!(full, create);
    }
}
