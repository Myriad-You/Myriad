//! 站长贴进笔记、或从旧站导入的外站图片：按地址下载下来，存成本站的媒体资产。
//!
//! 博客的 CSP 只放行本站和 data: 图片，外站地址发布后不显示，所以在粘贴和导入时就把
//! 图片存进本站。下载走 [`myriad_outbound`] 的公网客户端：只认 http(s)、地址里不带
//! 用户信息、每一跳重定向都重新解析并拒绝内网地址、正文按上限读。下载下来的字节按内容
//! 认图片，只收媒体库收的那几种图片格式。
//!
//! 按内容去重：同一张图（字节相同）不管从哪个地址来、贴几次，都落在同一个资产上。
//! 存下来先是私有的，和上传一样，笔记发布时随引用公开。日志和错误都不带地址：
//! 查询串里常有签名。

use std::sync::LazyLock;
use std::time::Duration;

use sea_orm::DatabaseConnection;

use super::MediaService;
use super::error::MediaError;
use super::types::{
    MediaActor, MediaAsset, MediaContext, MediaExposure, MediaSource, NewMediaBytes,
};
use super::validate::{MediaClass, checksum_sha256, validate_bytes};

/// 整次下载（含重定向和读正文）的时限。
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_URL_LEN: usize = 4096;
/// 和图片缓存同一个 UA：有些图床不给不像浏览器的请求出图。
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

/// 这几句会原样回到前端：不带地址、主机和内部原因。
const ADDRESS_REFUSED: &str = "This image address cannot be used";
const UNREACHABLE: &str = "The image could not be downloaded";
const NOT_AN_IMAGE: &str = "This address is not a supported image";

/// 同一张图同时被存两次时，第二次要看到第一次存好的那份，而不是撞上还在写的一行。
/// 只锁落库这一步，下载照样并行。
static PERSISTING: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

#[derive(Debug)]
struct RemoteImage {
    bytes: Vec<u8>,
    mime: String,
    filename: String,
}

impl MediaService {
    /// 把 `url` 上的图片存成站点媒体（来源「导入」）。同样内容已经存过就直接给那一份。
    pub async fn import_remote_image(
        &self,
        db: &DatabaseConnection,
        actor: MediaActor,
        url: &str,
    ) -> Result<MediaAsset, MediaError> {
        let max_bytes = crate::services::memory_profile::note_image_limit();
        let image = fetch_remote_image(url, max_bytes).await?;
        let ctx = MediaContext::site(actor, MediaSource::Import)
            .with_producer_key(format!("remote-image:{}", checksum_sha256(&image.bytes)));
        let _one = PERSISTING.lock().await;
        let (asset, _) = self
            .persist_ready_bytes(
                db,
                ctx,
                NewMediaBytes {
                    bytes: image.bytes.into(),
                    claimed_mime: image.mime,
                    filename: image.filename,
                    max_bytes,
                    derived_from_id: None,
                    exposure: MediaExposure::Private,
                },
            )
            .await?;
        Ok(asset)
    }
}

/// 下载并认出图片。失败只记原因，不记地址。
async fn fetch_remote_image(url: &str, max_bytes: usize) -> Result<RemoteImage, MediaError> {
    let url = url.trim();
    if url.len() > MAX_URL_LEN || !crate::services::image_cache::should_cache_remote_image(url) {
        return Err(MediaError::invalid(ADDRESS_REFUSED));
    }
    let fetched =
        myriad_outbound::get_public_following_redirects(url, FETCH_TIMEOUT, Some(USER_AGENT))
            .await
            .map_err(|error| {
                tracing::warn!(%error, "remote image fetch refused or failed");
                match error {
                    myriad_outbound::PublicGetError::InvalidTarget(_) => {
                        MediaError::invalid(ADDRESS_REFUSED)
                    }
                    myriad_outbound::PublicGetError::Fetch(_) => MediaError::invalid(UNREACHABLE),
                }
            })?;
    let response = fetched.response;
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), "remote image answered without the image");
        return Err(MediaError::invalid(UNREACHABLE));
    }
    let declared = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if !may_be_image(declared) {
        return Err(MediaError::invalid(NOT_AN_IMAGE));
    }
    let bytes = myriad_outbound::read_limited_body(response, max_bytes)
        .await
        .map_err(|error| {
            if error.starts_with("Response exceeds") {
                MediaError::TooLarge { max_bytes }
            } else {
                tracing::warn!(%error, "remote image body could not be read");
                MediaError::invalid(UNREACHABLE)
            }
        })?;
    // 声称的类型不作数：按字节认，认不出是图片就不收（视频、SVG、网页都在这里挡掉）。
    let payload = validate_bytes(&bytes, "image/*", max_bytes).map_err(|error| match error {
        MediaError::TooLarge { .. } => error,
        _ => MediaError::invalid(NOT_AN_IMAGE),
    })?;
    if payload.class != MediaClass::Image {
        return Err(MediaError::invalid(NOT_AN_IMAGE));
    }
    Ok(RemoteImage {
        bytes,
        mime: payload.mime,
        filename: file_name(&fetched.url),
    })
}

/// 回的类型明说不是图片（防盗链页、登录页）就不读正文；没写或写成二进制流的照读，按字节认。
fn may_be_image(content_type: Option<&str>) -> bool {
    let Some(value) = content_type else {
        return true;
    };
    let mime = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    mime.is_empty()
        || mime.starts_with("image/")
        || mime == "application/octet-stream"
        || mime == "binary/octet-stream"
}

/// 媒体库里显示的名字：地址最后一段，清理交给 `filename_for_mime`。
fn file_name(url: &url::Url) -> String {
    url.path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|name| !name.is_empty())
        .unwrap_or("image")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{Fixture, png};
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn declared_types_that_are_not_pictures_are_refused_before_the_body() {
        assert!(may_be_image(None));
        assert!(may_be_image(Some("image/jpeg")));
        assert!(may_be_image(Some("Image/PNG; charset=binary")));
        assert!(may_be_image(Some("application/octet-stream")));
        assert!(!may_be_image(Some("text/html; charset=utf-8")));
        assert!(!may_be_image(Some("application/json")));
        assert!(!may_be_image(Some("video/mp4")));
    }

    #[test]
    fn the_shown_name_is_the_last_path_segment() {
        let url = url::Url::parse("https://old.example/wp-content/uploads/2020/01/cat.jpg?w=300")
            .unwrap();
        assert_eq!(file_name(&url), "cat.jpg");
        assert_eq!(
            file_name(&url::Url::parse("https://old.example/").unwrap()),
            "image"
        );
    }

    #[tokio::test]
    async fn addresses_that_are_not_public_pictures_are_refused_without_a_request() {
        let _lock = myriad_outbound::tests_lab_env_lock().await;
        let lab = Lab::closed();
        for url in [
            "",
            "data:image/png;base64,aaaa",
            "javascript:alert(1)",
            "ftp://cdn.example/a.png",
            "https://user:secret@cdn.example/a.png",
            "/media/assets/x/y.png",
            "https://cdn.example/api/phantasi/image-cache/aa/bb.png",
            "http://127.0.0.1/a.png",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/a.png",
        ] {
            let error = fetch_remote_image(url, 1024 * 1024).await.unwrap_err();
            assert_eq!(error, MediaError::invalid(ADDRESS_REFUSED), "{url}");
        }
        let long = format!("https://cdn.example/{}.png", "a".repeat(MAX_URL_LEN));
        assert_eq!(
            fetch_remote_image(&long, 1024).await.unwrap_err(),
            MediaError::invalid(ADDRESS_REFUSED)
        );
        drop(lab);
    }

    #[tokio::test]
    async fn only_real_pictures_within_the_limit_come_back() {
        let _lock = myriad_outbound::tests_lab_env_lock().await;
        let lab = Lab::closed();
        let (base, served) = fake_host().await;
        // 实验开关关着：本机地址照样拒，而且一个请求都没发出去。
        assert_eq!(
            fetch_remote_image(&format!("{base}/ok.png"), 1024 * 1024)
                .await
                .unwrap_err(),
            MediaError::invalid(ADDRESS_REFUSED)
        );
        assert_eq!(hits(&served, "/ok.png"), 0);

        lab.open();
        let image = fetch_remote_image(&format!("{base}/ok.png"), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(
            (image.bytes, image.mime.as_str(), image.filename.as_str()),
            (png(), "image/png", "ok.png")
        );
        // 扩展名和声称的类型都不作数：JPEG 名字下的 PNG 按 PNG 收。
        let renamed = fetch_remote_image(&format!("{base}/renamed.jpg"), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(renamed.mime, "image/png");
        // 重定向跟着走，每一跳都过同样的检查。
        let moved = fetch_remote_image(&format!("{base}/moved"), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(moved.filename, "ok.png");
        for (path, expected) in [
            ("/gone.png", MediaError::invalid(UNREACHABLE)),
            ("/page", MediaError::invalid(NOT_AN_IMAGE)),
            ("/fake.png", MediaError::invalid(NOT_AN_IMAGE)),
            ("/svg", MediaError::invalid(NOT_AN_IMAGE)),
            ("/ok.png", MediaError::TooLarge { max_bytes: 16 }),
        ] {
            let max = if expected == (MediaError::TooLarge { max_bytes: 16 }) {
                16
            } else {
                1024 * 1024
            };
            assert_eq!(
                fetch_remote_image(&format!("{base}{path}"), max)
                    .await
                    .unwrap_err(),
                expected,
                "{path}"
            );
        }
        assert_eq!(hits(&served, "/page"), 1);
        drop(lab);
    }

    /// 同一张图从两个地址各贴一次，媒体库里只有一份；存下来是私有的「导入」。
    #[tokio::test]
    async fn the_same_picture_is_kept_once() {
        let _lock = myriad_outbound::tests_lab_env_lock().await;
        let Some(f) = Fixture::new().await else {
            return;
        };
        let lab = Lab::closed();
        lab.open();
        let (base, _) = fake_host().await;
        let actor = MediaActor::admin(1).unwrap();
        let first = f
            .service
            .import_remote_image(&f.db, actor.clone(), &format!("{base}/ok.png"))
            .await
            .unwrap();
        let again = f
            .service
            .import_remote_image(&f.db, actor.clone(), &format!("{base}/renamed.jpg"))
            .await
            .unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(first.source, MediaSource::Import);
        assert_eq!(first.exposure, MediaExposure::Private);
        assert_eq!(first.mime, "image/png");
        assert_eq!(
            f.service
                .import_remote_image(&f.db, actor, &format!("{base}/page"))
                .await
                .unwrap_err(),
            MediaError::invalid(NOT_AN_IMAGE)
        );
        drop(lab);
        f.close().await;
    }

    async fn fake_host() -> (String, Arc<Mutex<HashMap<String, usize>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
        let counted = hits.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let counted = counted.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..n]);
                    }
                    let line = String::from_utf8_lossy(&request);
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    *counted.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let (status, extra, body): (&str, &str, Vec<u8>) = match path.as_str() {
                        "/ok.png" => ("200 OK", "Content-Type: image/png\r\n", png()),
                        "/renamed.jpg" => ("200 OK", "Content-Type: image/jpeg\r\n", png()),
                        "/moved" => ("302 Found", "Location: /ok.png\r\n", Vec::new()),
                        "/page" => (
                            "200 OK",
                            "Content-Type: text/html\r\n",
                            b"<html></html>".to_vec(),
                        ),
                        "/fake.png" => (
                            "200 OK",
                            "Content-Type: image/png\r\n",
                            b"<html></html>".to_vec(),
                        ),
                        "/svg" => (
                            "200 OK",
                            "Content-Type: image/svg+xml\r\n",
                            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
                        ),
                        _ => (
                            "404 Not Found",
                            "Content-Type: text/plain\r\n",
                            b"gone".to_vec(),
                        ),
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (base, hits)
    }

    fn hits(of: &Arc<Mutex<HashMap<String, usize>>>, path: &str) -> usize {
        of.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    /// 让本机上的假主机顶替外站的实验开关，测完放回原样。
    struct Lab {
        lab: Option<String>,
        env: Option<String>,
    }

    impl Lab {
        fn closed() -> Self {
            let lab = Self {
                lab: std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND").ok(),
                env: std::env::var("ENVIRONMENT").ok(),
            };
            unsafe { std::env::remove_var("ENVIRONMENT") };
            unsafe { std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND") };
            lab
        }

        fn open(&self) {
            unsafe { std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1") };
        }
    }

    impl Drop for Lab {
        fn drop(&mut self) {
            match self.lab.take() {
                Some(value) => unsafe {
                    std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", value)
                },
                None => unsafe { std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND") },
            }
            if let Some(value) = self.env.take() {
                unsafe { std::env::set_var("ENVIRONMENT", value) };
            }
        }
    }
}
