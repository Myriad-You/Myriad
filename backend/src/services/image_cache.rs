//! 图片缓存服务
//!
//! 用于缓存外部图片（特别是 Notion 的临时 URL）
//! Notion 托管的文件 URL 是带签名的临时链接，通常 1 小时后失效
//! 此服务会下载并本地缓存这些图片

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;

/// 最大图片大小 (10MB)
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// 图片缓存服务
pub struct ImageCacheService {
    cache_dir: PathBuf,
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

    /// 检查 URL 是否是 Notion 托管的临时文件
    /// Notion 文件 URL 格式：
    /// - https://prod-files-secure.s3.us-west-2.amazonaws.com/...
    /// - https://s3.us-west-2.amazonaws.com/secure.notion-static.com/...
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
            .map_err(|e| format!("Failed to create cache directory: {}", e))
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
            tracing::debug!("Image already cached: {} -> {}", url, cached_url);
            return Ok(cached_url);
        }

        // 确保目录存在
        self.ensure_cache_dir().await?;

        // SSRF 防护：阻止请求内网地址
        if crate::federation::types::is_internal_url(url) {
            return Err(format!("Blocked SSRF attempt: {}", url));
        }

        let (target_url, client) = crate::services::outbound_security::build_public_http_client(
            url,
            Duration::from_secs(30),
            Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
        )
        .await?;

        // 下载图片
        tracing::info!("Caching image from: {}", url);
        let response = client
            .get(target_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download image: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        // 获取 Content-Type
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        // 验证是图片类型
        if let Some(ref ct) = content_type {
            if !ct.starts_with("image/") {
                return Err(format!("Not an image: {}", ct));
            }
        }

        // 下载数据
        let data = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read image data: {}", e))?;

        // 检查大小
        if data.len() > MAX_IMAGE_SIZE {
            return Err(format!("Image too large: {} bytes", data.len()));
        }

        // 生成文件名
        let filename = Self::generate_cache_filename(url);
        let ext = Self::infer_extension(url, content_type.as_deref());
        let cache_path = self.get_cache_path(&filename, ext);

        // 确保子目录存在
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create cache subdirectory: {}", e))?;
        }

        // 写入文件
        let mut file = fs::File::create(&cache_path)
            .await
            .map_err(|e| format!("Failed to create cache file: {}", e))?;

        file.write_all(&data)
            .await
            .map_err(|e| format!("Failed to write cache file: {}", e))?;

        let subdir = &filename[..2.min(filename.len())];
        let cached_url = format!("/api/brew/image-cache/{}/{}.{}", subdir, filename, ext);

        tracing::info!("Image cached: {} -> {}", url, cached_url);
        Ok(cached_url)
    }

    /// Persist already-downloaded image bytes and return the local serve URL.
    pub async fn store_bytes(&self, bytes: &[u8], media_type: &str) -> Result<String, String> {
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
        if cache_path.exists() {
            let subdir = &filename[..2.min(filename.len())];
            return Ok(format!(
                "/api/brew/image-cache/{}/{}.{}",
                subdir, filename, ext
            ));
        }
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create cache subdirectory: {}", e))?;
        }
        let mut file = fs::File::create(&cache_path)
            .await
            .map_err(|e| format!("Failed to create cache file: {}", e))?;
        file.write_all(bytes)
            .await
            .map_err(|e| format!("Failed to write cache file: {}", e))?;
        let subdir = &filename[..2.min(filename.len())];
        Ok(format!(
            "/api/brew/image-cache/{}/{}.{}",
            subdir, filename, ext
        ))
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
}
