// 酷狗音乐服务层 - 逐字歌词（KRC）
//
// 酷狗的逐字歌词覆盖率是全网最好的（逐字歌词由酷狗最先普及），尤其日系/番剧歌曲
// 远好于网易云 yrc。作为逐字歌词的补充第三方源：网易云 yrc 缺失时回退到此。
//
// 流程（仅 2 次请求）：
// 1. 歌词搜索  krcs.kugou.com/search?keyword=&duration=&man=yes  -> {id, accesskey}
// 2. 歌词下载  lyrics.kugou.com/download?id=&accesskey=&fmt=krc  -> base64(KRC)
// KRC 解码：base64 -> 去掉前 4 字节 "krc1" 头 -> 逐字节 XOR 固定 key -> zlib inflate

use anyhow::{Result, anyhow};
use base64::{Engine, engine::general_purpose};
use flate2::read::ZlibDecoder;
use myriad_platform_utils::netease::get_random_user_agent;
use serde_json::Value;
use std::io::Read;
use std::time::{Duration, Instant};

use super::netease_service::{CacheEntry, MUSIC_CACHE, RATE_LIMITER};

// KRC XOR 解密密钥（酷狗固定 16 字节）
const KRC_KEY: [u8; 16] = [
    64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105,
];

/// 解码酷狗 KRC（base64 -> 去头 -> XOR -> zlib）
fn decode_krc(b64: &str) -> Result<String> {
    let raw = general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| anyhow!("krc base64 decode failed: {}", e))?;
    if raw.len() <= 4 || &raw[..4] != b"krc1" {
        return Err(anyhow!("invalid krc header"));
    }
    let body = &raw[4..];
    let xored: Vec<u8> = body
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ KRC_KEY[i % 16])
        .collect();
    let mut decoder = ZlibDecoder::new(&xored[..]);
    let mut out = String::new();
    decoder
        .read_to_string(&mut out)
        .map_err(|e| anyhow!("krc zlib inflate failed: {}", e))?;
    Ok(out)
}

/// 酷狗音乐服务
pub struct KugouService {
    client: reqwest::Client,
}

impl Default for KugouService {
    fn default() -> Self {
        Self::new()
    }
}

impl KugouService {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap(),
        }
    }

    /// 获取逐字歌词（解码后的 KRC 文本）
    ///
    /// keyword 建议为「歌名 歌手」，duration_ms 用于在候选中挑最接近的版本（0 表示不匹配时长）。
    pub async fn fetch_verbatim_lyrics(&self, keyword: &str, duration_ms: i64) -> Result<String> {
        let cache_key = format!("kugou_krc:{}:{}", keyword, duration_ms / 1000);

        // 限流（复用网易云限流器）
        {
            let mut limiter = RATE_LIMITER.write().await;
            if !limiter.check_rate_limit(&cache_key) {
                return Err(anyhow!("Rate limit exceeded for kugou lyric {}", keyword));
            }
        }

        // 缓存（存解码后的 KRC 文本，包装为 JSON 字符串）
        {
            let mut cache = MUSIC_CACHE.write().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.expires_at > Instant::now() {
                    if let Some(s) = entry.data.as_str() {
                        return Ok(s.to_string());
                    }
                }
            }
        }

        // 1) 歌词搜索
        let search_url = format!(
            "http://krcs.kugou.com/search?ver=1&man=yes&client=mobi&keyword={}&duration={}&hash=",
            urlencoding_encode(keyword),
            duration_ms
        );
        let search: Value = self
            .client
            .get(&search_url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://www.kugou.com/")
            .send()
            .await?
            .json()
            .await?;

        let candidates = search
            .get("candidates")
            .and_then(|c| c.as_array())
            .ok_or_else(|| anyhow!("no kugou lyric candidates"))?;
        if candidates.is_empty() {
            return Err(anyhow!("no kugou lyric candidates for {}", keyword));
        }

        // 挑选：若给了时长，取候选时长最接近者；否则取第一个
        let best = if duration_ms > 0 {
            candidates
                .iter()
                .min_by_key(|c| {
                    let d = c.get("duration").and_then(|v| v.as_i64()).unwrap_or(0);
                    (d - duration_ms).abs()
                })
                .unwrap()
        } else {
            &candidates[0]
        };

        let id = best
            .get("id")
            .and_then(|v| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or(v.as_str().map(String::from))
            })
            .ok_or_else(|| anyhow!("kugou candidate missing id"))?;
        let accesskey = best
            .get("accesskey")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("kugou candidate missing accesskey"))?;

        // 2) 歌词下载
        let dl_url = format!(
            "http://lyrics.kugou.com/download?ver=1&client=pc&id={}&accesskey={}&fmt=krc&charset=utf8",
            id, accesskey
        );
        let dl: Value = self
            .client
            .get(&dl_url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://www.kugou.com/")
            .send()
            .await?
            .json()
            .await?;

        let content = dl
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("kugou download missing content"))?;

        let krc = decode_krc(content)?;

        // 存缓存（24 小时）
        {
            let mut cache = MUSIC_CACHE.write().await;
            cache.insert(
                cache_key,
                CacheEntry {
                    data: Value::String(krc.clone()),
                    expires_at: Instant::now() + Duration::from_secs(86400),
                },
            );
        }

        Ok(krc)
    }
}

/// 轻量 URL 编码（只对 query 值编码，避免额外依赖）
fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
