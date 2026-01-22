// 网易云音乐统一服务层
// 提供歌单获取、用户信息查询等功能，被 proxy API 和平台数据获取共享

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::netease_utils::{
    convert_http_to_https, generate_device_id, get_random_china_ip, get_random_user_agent,
};

// 🚀 内存保护：限制单个歌单最大处理数量（避免 OOM）
const MAX_TRACKS_LIMIT: usize = 5000;
// 🚀 内存保护：限制缓存最大条目数
const MAX_CACHE_ENTRIES: usize = 50;

// 缓存结构
pub struct CacheEntry {
    pub data: Value,
    pub expires_at: Instant,
}

// 限流结构
pub struct RateLimiter {
    requests: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            requests: HashMap::new(),
        }
    }

    /// 检查是否允许请求（宽松策略：每分钟60次，每小时1000次）
    pub fn check_rate_limit(&mut self, key: &str) -> bool {
        let now = Instant::now();
        // 使用 checked_sub 避免在 Instant 值较小时发生溢出 panic
        let one_minute_ago = now.checked_sub(Duration::from_secs(60));
        let one_hour_ago = now.checked_sub(Duration::from_secs(3600));

        // 清理过期的请求记录
        let times = self.requests.entry(key.to_string()).or_default();

        // 如果无法计算一小时前的时间点，保留所有记录
        if let Some(hour_ago) = one_hour_ago {
            times.retain(|&t| t > hour_ago);
        }

        // 检查限制
        let recent_count = if let Some(minute_ago) = one_minute_ago {
            times.iter().filter(|&&t| t > minute_ago).count()
        } else {
            times.len() // 如果无法计算，视为全部都是最近的
        };
        let hourly_count = times.len();

        if recent_count >= 60 || hourly_count >= 1000 {
            return false;
        }

        // 记录本次请求
        times.push(now);
        true
    }
}

// 全局缓存和限流器
pub static MUSIC_CACHE: Lazy<Arc<RwLock<HashMap<String, CacheEntry>>>> =
    Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));
pub static RATE_LIMITER: Lazy<Arc<RwLock<RateLimiter>>> =
    Lazy::new(|| Arc::new(RwLock::new(RateLimiter::new())));

/// 网易云音乐服务
pub struct NeteaseService {
    client: reqwest::Client,
}

impl NeteaseService {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .cookie_store(true)
                .build()
                .unwrap(),
        }
    }

    /// 获取歌单详情（核心方法，带缓存、限流、防封）
    /// 支持大歌单（1000+首）、VIP检测、HTTP→HTTPS转换
    pub async fn fetch_playlist(&self, playlist_id: i64, use_cache: bool) -> Result<Value> {
        let cache_key = format!("playlist:{}", playlist_id);

        // 检查限流
        {
            let mut limiter = RATE_LIMITER.write().await;
            if !limiter.check_rate_limit(&cache_key) {
                return Err(anyhow!("Rate limit exceeded for playlist {}", playlist_id));
            }
        }

        // 检查缓存（歌单缓存7天）
        if use_cache {
            let cache = MUSIC_CACHE.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.expires_at > Instant::now() {
                    tracing::debug!("✅ Cache hit for playlist: {}", playlist_id);
                    return Ok(entry.data.clone());
                }
            }
        }

        // 生成随机设备ID和时间戳
        let device_id = generate_device_id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let url = format!(
            "https://music.163.com/api/v6/playlist/detail?id={}&n=1000&s=0&t=0",
            playlist_id
        );

        // IP 伪装
        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let response = self
            .client
            .get(&url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://music.163.com/")
            .header("Origin", "https://music.163.com")
            .header("Accept", "*/*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Connection", "keep-alive")
            .header(
                "Cookie",
                format!(
                    "osver=android; appver=8.7.01; os=android; deviceId={}; channel=netease; requestId={}_{:04}; __remember_me=true",
                    device_id,
                    timestamp,
                    rand::random::<u16>() % 10000
                ),
            )
            .header("X-Forwarded-For", forwarded_for.clone())
            .header("X-Real-IP", client_ip.clone())
            .send()
            .await?;

        let mut data: Value = response.json().await?;

        // 检查返回码
        if let Some(code) = data.get("code").and_then(|c| c.as_i64()) {
            if code != 200 {
                return Err(anyhow!("Netease API returned error code {}", code));
            }
        }

        // 转换 HTTP 链接为 HTTPS
        convert_http_to_https(&mut data);

        // 处理 VIP 标记和大歌单
        if let Some(playlist) = data.get_mut("playlist") {
            let track_count = playlist
                .get("trackCount")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as usize;

            let track_ids = playlist
                .get("trackIds")
                .and_then(|ids| ids.as_array())
                .cloned();

            if let Some(tracks) = playlist.get_mut("tracks") {
                if let Some(tracks_array) = tracks.as_array_mut() {
                    let loaded_tracks = tracks_array.len();
                    let mut vip_count = 0;

                    // 为已加载的歌曲添加 VIP 标记 - 使用安全的方式避免 panic
                    for track in tracks_array.iter_mut() {
                        // 先获取 fee 值，再进行可变借用
                        let fee = track.get("fee").and_then(|f| f.as_i64()).unwrap_or(0);
                        let is_vip = fee == 1 || fee == 4;
                        if let Some(obj) = track.as_object_mut() {
                            obj.insert("isVip".to_string(), json!(is_vip));
                            if is_vip {
                                vip_count += 1;
                            }
                        }
                    }

                    // 处理大歌单（超过1000首）- 使用串行批量获取
                    // 🚀 优化：改为串行处理，减少内存峰值和 OOM 风险
                    if track_count > loaded_tracks && loaded_tracks >= 1000 {
                        // 🚀 内存保护：限制最大获取数量，避免 OOM
                        let effective_track_count = std::cmp::min(track_count, MAX_TRACKS_LIMIT);

                        tracing::info!(
                            "🎵 Large playlist detected ({}/{}, capped at {}), fetching remaining songs...",
                            loaded_tracks,
                            track_count,
                            effective_track_count
                        );

                        if let Some(track_ids_array) = track_ids {
                            // 🚀 改进：不再克隆 tracks_array，直接收集新歌曲
                            let batch_size = 200; // 批次大小
                            let target_count =
                                std::cmp::min(track_ids_array.len(), effective_track_count);
                            let remaining_count = target_count.saturating_sub(loaded_tracks);

                            tracing::info!(
                                "📦 Need to fetch {} more songs in batches of {} (target: {})",
                                remaining_count,
                                batch_size,
                                target_count
                            );

                            // 收集新歌曲
                            let mut new_tracks: Vec<Value> = Vec::with_capacity(
                                std::cmp::min(remaining_count, 2000), // 预分配最多 2000 首的空间
                            );
                            let mut offset = loaded_tracks;
                            let mut failed_batches = 0;
                            let max_failures = 3;
                            let mut batch_num = 0;
                            let total_batches = remaining_count.div_ceil(batch_size);

                            while offset < target_count && failed_batches < max_failures {
                                let end_idx = std::cmp::min(offset + batch_size, target_count);
                                let batch_ids: Vec<i64> = track_ids_array[offset..end_idx]
                                    .iter()
                                    .filter_map(|id_obj| id_obj.get("id").and_then(|v| v.as_i64()))
                                    .collect();

                                if batch_ids.is_empty() {
                                    offset = end_idx;
                                    continue;
                                }

                                // 🚀 添加延迟，减轻服务器压力
                                let delay = 150 + (rand::random::<u64>() % 150);
                                tokio::time::sleep(Duration::from_millis(delay)).await;

                                let ids_str = batch_ids
                                    .iter()
                                    .map(|id| id.to_string())
                                    .collect::<Vec<_>>()
                                    .join(",");

                                let track_url = format!(
                                    "https://music.163.com/api/song/detail?ids=[{}]",
                                    ids_str
                                );

                                // 串行请求，减少并发内存压力
                                match self.client
                                    .get(&track_url)
                                    .header("Referer", "https://music.163.com/")
                                    .header("Origin", "https://music.163.com")
                                    .header("Accept", "*/*")
                                    .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
                                    .header("Connection", "keep-alive")
                                    .header(
                                        "Cookie",
                                        format!(
                                            "osver=android; appver=8.7.01; os=android; deviceId={}; channel=netease; requestId={}_{:04}; __remember_me=true",
                                            device_id,
                                            timestamp,
                                            rand::random::<u16>() % 10000
                                        ),
                                    )
                                    .header("X-Forwarded-For", forwarded_for.clone())
                                    .header("X-Real-IP", client_ip.clone())
                                    .timeout(Duration::from_secs(15))
                                    .send()
                                    .await
                                {
                                    Ok(resp) => {
                                        match resp.json::<Value>().await {
                                            Ok(batch_data) => {
                                                if batch_data.get("code").and_then(|c| c.as_i64()) == Some(200) {
                                                    if let Some(songs) = batch_data.get("songs").and_then(|s| s.as_array()) {
                                                        for song in songs {
                                                            // 🚀 只提取必要字段，减少内存
                                                            let fee = song.get("fee").and_then(|f| f.as_i64()).unwrap_or(0);
                                                            let is_vip = fee == 1 || fee == 4;
                                                            if is_vip {
                                                                vip_count += 1;
                                                            }

                                                            // 克隆并添加 isVip 标记
                                                            let mut song_data = song.clone();
                                                            if let Some(obj) = song_data.as_object_mut() {
                                                                obj.insert("isVip".to_string(), json!(is_vip));
                                                            }
                                                            new_tracks.push(song_data);
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!("⚠️ Batch {} returned error code", batch_num + 1);
                                                    failed_batches += 1;
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!("⚠️ Failed to parse batch {}: {}", batch_num + 1, e);
                                                failed_batches += 1;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("⚠️ Failed to fetch batch {}: {}", batch_num + 1, e);
                                        failed_batches += 1;
                                    }
                                }

                                batch_num += 1;
                                offset = end_idx;

                                // 进度日志（每5个批次打印一次）
                                if batch_num % 5 == 0 || batch_num == total_batches {
                                    tracing::info!(
                                        "✓ Progress: {}/{} batches, {} new songs collected",
                                        batch_num,
                                        total_batches,
                                        new_tracks.len()
                                    );
                                }
                            }

                            // 将新歌曲追加到原数组
                            tracks_array.extend(new_tracks);

                            tracing::info!(
                                "✅ Playlist {} 完整加载: {} 首歌曲，{} 首VIP (失败批次: {})",
                                playlist_id,
                                tracks_array.len(),
                                vip_count,
                                failed_batches
                            );
                        }
                    } else {
                        tracing::info!(
                            "✅ Playlist {} 解析完成: {} 首歌曲，{} 首VIP",
                            playlist_id,
                            tracks_array.len(),
                            vip_count
                        );
                    }
                }
            }
        }

        // 🚀 存入缓存（添加缓存清理和内存保护）
        {
            let mut cache = MUSIC_CACHE.write().await;

            // 缓存清理：如果缓存条目过多，删除过期条目
            if cache.len() >= MAX_CACHE_ENTRIES {
                let now = Instant::now();
                cache.retain(|_, entry| entry.expires_at > now);

                // 如果仍然过多，删除最旧的条目
                if cache.len() >= MAX_CACHE_ENTRIES {
                    // 找到最旧的条目并删除
                    if let Some(oldest_key) = cache
                        .iter()
                        .min_by_key(|(_, entry)| entry.expires_at)
                        .map(|(key, _)| key.clone())
                    {
                        cache.remove(&oldest_key);
                        tracing::debug!("🧹 Removed oldest cache entry: {}", oldest_key);
                    }
                }
            }

            cache.insert(
                cache_key,
                CacheEntry {
                    data: data.clone(),
                    expires_at: Instant::now() + Duration::from_secs(604800), // 7天
                },
            );
        }

        Ok(data)
    }

    /// 获取用户"我喜欢的音乐"歌单ID
    pub async fn get_user_liked_playlist_id(&self, user_id: i64) -> Result<i64> {
        // 使用正确的API端点获取用户歌单列表
        let url = format!(
            "https://music.163.com/api/user/playlist?uid={}&limit=1&offset=0",
            user_id
        );

        let device_id = generate_device_id();
        let client_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, get_random_china_ip());

        let response: Value = self
            .client
            .get(&url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://music.163.com/")
            .header("Origin", "https://music.163.com")
            .header("Accept", "*/*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Connection", "keep-alive")
            .header(
                "Cookie",
                format!(
                    "osver=android; appver=8.7.01; os=android; deviceId={}; channel=netease; __remember_me=true",
                    device_id
                ),
            )
            .header("X-Forwarded-For", forwarded_for.clone())
            .header("X-Real-IP", client_ip.clone())
            .send()
            .await?
            .json()
            .await?;

        // 网易云API返回code=200表示成功
        if response["code"].as_i64() != Some(200) {
            return Err(anyhow!(
                "Failed to fetch user playlists (code: {}): {}",
                response["code"],
                response["message"].as_str().unwrap_or("unknown error")
            ));
        }

        // 歌单列表在 playlist 字段中
        let playlists = response["playlist"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid response: no playlist field"))?;

        if playlists.is_empty() {
            return Err(anyhow!("User has no playlists"));
        }

        // 第一个歌单就是"我喜欢的音乐"
        playlists[0]["id"]
            .as_i64()
            .ok_or_else(|| anyhow!("Failed to get playlist ID"))
    }

    /// 获取用户喜欢的歌曲列表（平台数据专用）
    pub async fn fetch_user_liked_songs(&self, user_id: i64) -> Result<Vec<Value>> {
        // 1. 获取用户的第一个歌单 ID（"我喜欢的音乐"）
        let playlist_id = self.get_user_liked_playlist_id(user_id).await?;

        // 2. 获取歌单详情（复用完整的防封逻辑）
        let data = self.fetch_playlist(playlist_id, true).await?;

        // 3. 提取歌曲列表
        let tracks = data["playlist"]["tracks"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid playlist response: no tracks field"))?;

        Ok(tracks.clone())
    }

    /// 获取用户基本信息（用于验证）
    pub async fn fetch_user_info(&self, user_id: i64) -> Result<Value> {
        let url = format!("https://music.163.com/api/v1/user/detail/{}", user_id);

        let client_ip = get_random_china_ip();

        let response: Value = self
            .client
            .get(&url)
            .header("User-Agent", get_random_user_agent())
            .header("Referer", "https://music.163.com/")
            .header("X-Forwarded-For", client_ip)
            .send()
            .await?
            .json()
            .await?;

        if response["code"].as_i64() != Some(200) {
            return Err(anyhow!("Failed to fetch user info: Invalid user ID"));
        }

        Ok(response)
    }

    /// 获取歌词
    pub async fn fetch_lyrics(&self, song_id: i64) -> Result<Value> {
        let cache_key = format!("lyrics:{}", song_id);

        // 检查限流
        {
            let mut limiter = RATE_LIMITER.write().await;
            if !limiter.check_rate_limit(&cache_key) {
                return Err(anyhow!("Rate limit exceeded for lyrics {}", song_id));
            }
        }

        // 检查缓存
        {
            let cache = MUSIC_CACHE.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.expires_at > Instant::now() {
                    return Ok(entry.data.clone());
                }
            }
        }

        let device_id = generate_device_id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let url = format!(
            "https://music.163.com/api/song/lyric?id={}&os=linux&lv=-1&kv=-1&tv=-1",
            song_id
        );

        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let response = self
            .client
            .get(&url)
            .header("Referer", "https://music.163.com/")
            .header("Accept", "*/*")
            .header("User-Agent", get_random_user_agent())
            .header(
                "Cookie",
                format!(
                    "osver=android; appver=8.7.01; os=android; deviceId={}; channel=netease; requestId={}_{}",
                    device_id,
                    timestamp,
                    rand::random::<u16>() % 10000
                ),
            )
            .header("X-Forwarded-For", forwarded_for)
            .header("X-Real-IP", client_ip)
            .send()
            .await?;

        let mut data: Value = response.json().await?;
        convert_http_to_https(&mut data);

        // 存入缓存（24小时）
        {
            let mut cache = MUSIC_CACHE.write().await;
            cache.insert(
                cache_key,
                CacheEntry {
                    data: data.clone(),
                    expires_at: Instant::now() + Duration::from_secs(86400),
                },
            );
        }

        Ok(data)
    }

    /// 获取单首歌曲详情
    pub async fn fetch_song_detail(&self, song_id: i64) -> Result<Value> {
        let cache_key = format!("song:{}", song_id);

        // 检查限流
        {
            let mut limiter = RATE_LIMITER.write().await;
            if !limiter.check_rate_limit(&cache_key) {
                return Err(anyhow!("Rate limit exceeded for song {}", song_id));
            }
        }

        // 检查缓存（歌曲详情缓存24小时）
        {
            let cache = MUSIC_CACHE.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.expires_at > Instant::now() {
                    return Ok(entry.data.clone());
                }
            }
        }

        let device_id = generate_device_id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let url = format!("https://music.163.com/api/song/detail?ids=[{}]", song_id);

        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let response = self
            .client
            .get(&url)
            .header("Referer", "https://music.163.com/")
            .header("Origin", "https://music.163.com")
            .header("Accept", "*/*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Connection", "keep-alive")
            .header("User-Agent", get_random_user_agent())
            .header(
                "Cookie",
                format!(
                    "osver=android; appver=8.7.01; os=android; deviceId={}; channel=netease; requestId={}_{:04}; __remember_me=true",
                    device_id,
                    timestamp,
                    rand::random::<u16>() % 10000
                ),
            )
            .header("X-Forwarded-For", forwarded_for)
            .header("X-Real-IP", client_ip)
            .send()
            .await?;

        let data: Value = response.json().await?;

        // 检查返回码
        if data.get("code").and_then(|c| c.as_i64()) != Some(200) {
            return Err(anyhow!("Failed to fetch song detail: API returned error"));
        }

        // 提取歌曲信息
        let song = data["songs"]
            .get(0)
            .ok_or_else(|| anyhow!("Song not found"))?
            .clone();

        // 添加 isVip 标记
        let mut song_data = song;
        let fee = song_data.get("fee").and_then(|f| f.as_i64()).unwrap_or(0);
        let is_vip = fee == 1 || fee == 4;
        if let Some(obj) = song_data.as_object_mut() {
            obj.insert("isVip".to_string(), json!(is_vip));
        }

        convert_http_to_https(&mut song_data);

        // 存入缓存（24小时）
        {
            let mut cache = MUSIC_CACHE.write().await;
            cache.insert(
                cache_key,
                CacheEntry {
                    data: song_data.clone(),
                    expires_at: Instant::now() + Duration::from_secs(86400),
                },
            );
        }

        Ok(song_data)
    }

    /// 获取音频流 URL
    pub async fn fetch_audio_url(&self, song_id: i64) -> Result<String> {
        let device_id = generate_device_id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let url = format!(
            "https://music.163.com/api/song/enhance/player/url?ids=[{}]&br=320000",
            song_id
        );

        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        let response = self
            .client
            .get(&url)
            .header("Referer", "https://music.163.com/")
            .header("Origin", "https://music.163.com")
            .header("Accept", "*/*")
            .header("User-Agent", get_random_user_agent())
            .header(
                "Cookie",
                format!(
                    "osver=android; appver=8.7.01; os=android; deviceId={}; channel=netease; requestId={}_{:04}; __remember_me=true",
                    device_id,
                    timestamp,
                    rand::random::<u16>() % 10000
                ),
            )
            .header("X-Forwarded-For", forwarded_for)
            .header("X-Real-IP", client_ip)
            .send()
            .await?;

        let data: Value = response.json().await?;

        let audio_url = data["data"]
            .get(0)
            .and_then(|item| {
                if let Some(uf_url) = item.get("uf").and_then(|uf| uf["url"].as_str()) {
                    Some(uf_url)
                } else {
                    item["url"].as_str()
                }
            })
            .ok_or_else(|| anyhow!("No audio URL found"))?;

        if audio_url.is_empty() || audio_url == "null" {
            return Err(anyhow!(
                "Audio not available (copyright or geo-restriction)"
            ));
        }

        Ok(audio_url.to_string())
    }
}

impl Default for NeteaseService {
    fn default() -> Self {
        Self::new()
    }
}
