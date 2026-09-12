//! 图片缓存服务
//!
//! 用于缓存外部图片（特别是 Notion 的临时 URL）
//! Notion 托管的文件 URL 是带签名的临时链接
//! 此服务会下载并本地缓存这些图片

use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

fn cache_io_error(action: &str, error: std::io::Error) -> String {
    tracing::error!(%error, action, "image cache io failed");
    match error.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            format!("{action}: storage is not writable")
        }
        ErrorKind::StorageFull => format!("{action}: not enough disk space"),
        ErrorKind::AlreadyExists => format!("{action}: already exists"),
        ErrorKind::NotFound => format!("{action}: path not found"),
        _ => format!("{action} failed"),
    }
}

/// 最大图片大小（`MAX_IMAGE_SIZE` = 10 MiB）
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// 图片缓存服务
pub struct ImageCacheService {
    cache_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredImage {
    pub url: String,
    pub created: bool,
}

impl Default for ImageCacheService {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCacheService {
    pub fn new() -> Self {
        let cache_dir = crate::services::data_paths::paths().cache_images.clone();

        Self { cache_dir }
    }

    /// 检查 URL 是否像 Notion / S3 临时文件（含 `notion.so/image` 与 `X-Amz-*`）
    pub fn is_notion_temporary_url(url: &str) -> bool {
        let patterns = [
            "prod-files-secure.s3",
            "secure.notion-static.com",
            "notion.so/image",
            "s3.us-west-2.amazonaws.com",
            "X-Amz-Algorithm",
            "X-Amz-Credential",
            "X-Amz-Signature",
        ];

        let url_lower = url.to_lowercase();
        patterns
            .iter()
            .any(|p| url_lower.contains(&p.to_lowercase()))
    }

    /// 生成 URL 的缓存文件名（基于 SHA256 哈希）
    fn generate_cache_filename(url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let hash = hasher.finalize();
        hex::encode(hash)
    }

    /// 从 URL 推断文件扩展名
    fn infer_extension(url: &str, content_type: Option<&str>) -> &'static str {
        // 优先从 Content-Type 推断
        if let Some(ct) = content_type {
            match ct {
                _ if ct.contains("jpeg") || ct.contains("jpg") => return "jpg",
                _ if ct.contains("png") => return "png",
                _ if ct.contains("gif") => return "gif",
                _ if ct.contains("webp") => return "webp",
                _ if ct.contains("svg") => return "svg",
                _ if ct.contains("avif") => return "avif",
                _ => {}
            }
        }

        // 从 URL 路径推断
        let url_lower = url.to_lowercase();
        if url_lower.contains(".jpg") || url_lower.contains(".jpeg") {
            "jpg"
        } else if url_lower.contains(".png") {
            "png"
        } else if url_lower.contains(".gif") {
            "gif"
        } else if url_lower.contains(".webp") {
            "webp"
        } else if url_lower.contains(".svg") {
            "svg"
        } else if url_lower.contains(".avif") {
            "avif"
        } else {
            // 默认 jpg
            "jpg"
        }
    }

    /// 确保缓存目录存在
    async fn ensure_cache_dir(&self) -> Result<(), String> {
        fs::create_dir_all(&self.cache_dir)
            .await
            .map_err(|e| cache_io_error("Failed to create cache directory", e))
    }

    /// 获取缓存文件路径
    fn get_cache_path(&self, filename: &str, ext: &str) -> PathBuf {
        // 使用两级目录结构避免单目录文件过多
        // 例如: cache/images/ab/abcdef1234...jpg
        let subdir = &filename[..2.min(filename.len())];
        self.cache_dir
            .join(subdir)
            .join(format!("{}.{}", filename, ext))
    }

    /// 检查缓存是否存在
    pub async fn get_cached_url(&self, original_url: &str) -> Option<String> {
        let filename = Self::generate_cache_filename(original_url);

        // 检查常见扩展名
        let extensions = ["jpg", "png", "gif", "webp", "svg", "avif"];
        for ext in extensions {
            let path = self.get_cache_path(&filename, ext);
            if path.exists() {
                // 返回相对于服务器的 URL
                let subdir = &filename[..2.min(filename.len())];
                return Some(format!(
                    "/api/brew/image-cache/{}/{}.{}",
                    subdir, filename, ext
                ));
            }
        }

        None
    }

    /// 下载并缓存图片
    /// 返回缓存后的本地 URL 路径
    pub async fn cache_image(&self, url: &str) -> Result<String, String> {
        // 先检查是否已缓存
        if let Some(cached_url) = self.get_cached_url(url).await {
            tracing::debug!(cached = %cached_url, "Image already cached");
            return Ok(cached_url);
        }

        // 确保目录存在
        self.ensure_cache_dir().await?;

        // SSRF 防护：阻止请求内网地址
        if crate::federation::types::is_internal_url(url) {
            return Err("This address is not allowed".to_string());
        }

        let (target_url, client) = crate::services::outbound_security::build_public_http_client(
            url,
            Duration::from_secs(30),
            Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
        )
        .await?;

        // 下载图片
        tracing::info!(host = %url.split('/').nth(2).unwrap_or("-"), "Caching image");
        let response = client.get(target_url).send().await.map_err(|e| {
            tracing::warn!(error = %e, "Failed to download image");
            "Failed to download image".to_string()
        })?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        // 获取 Content-Type
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // 若带 Content-Type，必须是 image/*；缺 CT 则放过
        if let Some(ref ct) = content_type {
            if !ct.starts_with("image/") {
                return Err(format!("Not an image: {}", ct));
            }
        }

        // Enforce the existing image limit before buffering the entire response.
        let data = myriad_outbound::read_limited_body(response, MAX_IMAGE_SIZE).await?;

        // 生成文件名
        let filename = Self::generate_cache_filename(url);
        let ext = Self::infer_extension(url, content_type.as_deref());
        let cache_path = self.get_cache_path(&filename, ext);

        // 确保子目录存在
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| cache_io_error("Failed to create cache subdirectory", e))?;
        }

        // 写入文件
        let mut file = fs::File::create(&cache_path)
            .await
            .map_err(|e| cache_io_error("Failed to create cache file", e))?;

        file.write_all(&data)
            .await
            .map_err(|e| cache_io_error("Failed to write cache file", e))?;

        let subdir = &filename[..2.min(filename.len())];
        let cached_url = format!("/api/brew/image-cache/{}/{}.{}", subdir, filename, ext);

        tracing::info!(cached = %cached_url, "Image cached");
        Ok(cached_url)
    }

    /// Content-addressed write with creation status for transactional callers
    /// that need to compensate a later database failure.
    pub async fn store_bytes_with_status(
        &self,
        bytes: &[u8],
        media_type: &str,
    ) -> Result<StoredImage, String> {
        if bytes.is_empty() {
            return Err("generated image is empty".to_string());
        }
        if bytes.len() > MAX_IMAGE_SIZE {
            return Err(format!("Image too large: {} bytes", bytes.len()));
        }
        self.ensure_cache_dir().await?;
        let filename = {
            let mut hasher = Sha256::new();
            hasher.update(bytes);
            hex::encode(hasher.finalize())
        };
        let ext = Self::infer_extension("", Some(media_type));
        let cache_path = self.get_cache_path(&filename, ext);
        let subdir = &filename[..2.min(filename.len())];
        let url = format!("/api/brew/image-cache/{}/{}.{}", subdir, filename, ext);
        if cache_path.exists() {
            return Ok(StoredImage {
                url,
                created: false,
            });
        }
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| cache_io_error("Failed to create cache subdirectory", e))?;
        }
        let temporary_path = cache_path.with_extension(format!("{ext}.{}.tmp", Uuid::new_v4()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .await
            .map_err(|e| cache_io_error("Failed to create cache file", e))?;
        if let Err(error) = file.write_all(bytes).await {
            drop(file);
            let _ = fs::remove_file(&temporary_path).await;
            return Err(cache_io_error("Failed to write cache file", error));
        }
        if let Err(error) = file.flush().await {
            drop(file);
            let _ = fs::remove_file(&temporary_path).await;
            return Err(cache_io_error("Failed to flush cache file", error));
        }
        drop(file);
        let created = match fs::hard_link(&temporary_path, &cache_path).await {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(error) => {
                let _ = fs::remove_file(&temporary_path).await;
                return Err(cache_io_error("Failed to publish cache file", error));
            }
        };
        let _ = fs::remove_file(&temporary_path).await;
        Ok(StoredImage { url, created })
    }

    pub async fn remove_stored_url(&self, url: &str) -> Result<(), String> {
        let path = self
            .local_path_for_public_url(url)
            .ok_or_else(|| "generated image URL is not a local cache asset".to_string())?;
        match fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(cache_io_error("Failed to remove generated image", error)),
        }
    }

    /// Resolve a public `/api/brew/image-cache/{subdir}/{sha256}.{ext}` URL to a local path.
    pub fn local_path_for_public_url(&self, url: &str) -> Option<PathBuf> {
        let path = image_cache_path(url)?;
        let rest = path.strip_prefix("/api/brew/image-cache/")?;
        let (subdir, file) = rest.split_once('/')?;
        if file.contains('/') || file.contains('\\') || file.contains("..") {
            return None;
        }
        let (stem, ext) = file.rsplit_once('.')?;
        if subdir.len() != 2 || !subdir.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        if stem.len() != 64 || !stem.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let ext = ext.to_ascii_lowercase();
        if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp") {
            return None;
        }
        let stem = stem.to_ascii_lowercase();
        if !stem.starts_with(&subdir.to_ascii_lowercase()) {
            return None;
        }
        Some(self.get_cache_path(&stem, &ext))
    }

    pub async fn read_local_public_url(&self, url: &str) -> Result<(Vec<u8>, String), String> {
        let path = self
            .local_path_for_public_url(url)
            .ok_or_else(|| "imageUrl must be a local /api/brew/image-cache path".to_string())?;
        let bytes = fs::read(&path)
            .await
            .map_err(|_| "cached image not found".to_string())?;
        if bytes.is_empty() || bytes.len() > MAX_IMAGE_SIZE {
            return Err("cached image is empty or too large".to_string());
        }
        let ext = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => "application/octet-stream",
        };
        Ok((bytes, mime.to_string()))
    }

    /// 处理图片 URL - 如果是 Notion 临时 URL 则缓存，否则返回原 URL
    pub async fn process_image_url(&self, url: Option<&str>) -> Option<String> {
        let url = url?;

        if url.is_empty() {
            return None;
        }

        // 如果是 Notion 临时 URL，尝试缓存
        if Self::is_notion_temporary_url(url) {
            match self.cache_image(url).await {
                Ok(cached_url) => Some(cached_url),
                Err(e) => {
                    tracing::warn!("Failed to cache Notion image: {} - {}", url, e);
                    // 缓存失败时返回原 URL（至少在短期内还能用）
                    Some(url.to_string())
                }
            }
        } else {
            Some(url.to_string())
        }
    }
}

fn image_cache_path(url: &str) -> Option<&str> {
    let without_query = url.split('?').next().unwrap_or(url);
    let start = without_query.find("/api/brew/image-cache/")?;
    Some(&without_query[start..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_notion_temporary_url() {
        // Notion S3 URL
        assert!(ImageCacheService::is_notion_temporary_url(
            "https://prod-files-secure.s3.us-west-2.amazonaws.com/xxx/xxx.jpg?X-Amz-Algorithm=AWS4-HMAC-SHA256"
        ));

        // Notion static URL
        assert!(ImageCacheService::is_notion_temporary_url(
            "https://s3.us-west-2.amazonaws.com/secure.notion-static.com/xxx.png"
        ));

        // Regular URL
        assert!(!ImageCacheService::is_notion_temporary_url(
            "https://example.com/image.jpg"
        ));

        // Other CDN
        assert!(!ImageCacheService::is_notion_temporary_url(
            "https://images.unsplash.com/photo-xxx"
        ));
    }

    #[test]
    fn test_generate_cache_filename() {
        let url1 = "https://example.com/image1.jpg";
        let url2 = "https://example.com/image2.jpg";

        let hash1 = ImageCacheService::generate_cache_filename(url1);
        let hash2 = ImageCacheService::generate_cache_filename(url2);

        assert_ne!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA256 hex = 64 chars
    }

    #[test]
    fn local_path_only_accepts_site_image_cache_urls() {
        let service = ImageCacheService::new();
        let hash = "a".repeat(64);
        let ok = format!("https://example.com/api/brew/image-cache/aa/{hash}.png");
        assert!(service.local_path_for_public_url(&ok).is_some());
        assert!(service
            .local_path_for_public_url("https://evil.example/secret.png")
            .is_none());
        assert!(service
            .local_path_for_public_url(&format!("/api/brew/image-cache/ab/{hash}.png"))
            .is_none());
        assert!(service
            .local_path_for_public_url("/api/brew/image-cache/aa/../passwd.png")
            .is_none());
    }

    #[test]
    fn cache_io_error_names_cause_without_os_dump() {
        let denied = cache_io_error(
            "Failed to write cache file",
            std::io::Error::new(ErrorKind::PermissionDenied, "denied (os error 13)"),
        );
        assert_eq!(
            denied,
            "Failed to write cache file: storage is not writable"
        );
        assert!(!denied.contains("os error"));
        let full = cache_io_error(
            "Failed to write cache file",
            std::io::Error::new(ErrorKind::StorageFull, "No space left on device"),
        );
        assert_eq!(full, "Failed to write cache file: not enough disk space");
    }
}
