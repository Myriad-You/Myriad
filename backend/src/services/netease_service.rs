// 网易云音乐统一服务层
// 提供歌单获取、用户信息查询等功能，被 proxy API 和平台数据获取共享

use anyhow::{Result, anyhow};
use myriad_platform_utils::netease::{
    convert_http_to_https, ensure_https_url, generate_device_id, get_random_china_ip,
    get_random_user_agent,
};
use once_cell::sync::Lazy;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use super::music_player_view::{PlayerPlaylist, PlayerSong};

mod player;

/// A resolved CDN URL and its conservative cache deadline, including request time.
pub struct NeteaseAudioUrl {
    pub url: String,
    pub cache_until: Instant,
}

fn parse_audio_url(data: &Value, requested_at: Instant) -> Result<NeteaseAudioUrl> {
    let item = data["data"]
        .get(0)
        .ok_or_else(|| anyhow!("No audio URL found"))?;
    let resource = item
        .get("uf")
        .filter(|uf| uf["url"].is_string())
        .unwrap_or(item);
    let url = resource["url"]
        .as_str()
        .filter(|url| !url.is_empty() && *url != "null")
        .ok_or_else(|| anyhow!("Audio not available (copyright or geo-restriction)"))?;
    // Use the selected resource's expiry, with the parent as an upper bound.
    // Missing/malformed expiry gets a short fallback; explicit zero/negative
    // expiry must never turn into a fresh fallback TTL.
    let ttl_for = |value: Option<&Value>| match value.and_then(Value::as_i64) {
        Some(seconds) => Duration::from_secs(seconds.max(0) as u64)
            .saturating_sub(Duration::from_secs(30))
            .min(Duration::from_secs(300)),
        None => Duration::from_secs(60),
    };
    let mut ttl = ttl_for(resource.get("expi"));
    if !std::ptr::eq(resource, item) && item.get("expi").and_then(Value::as_i64).is_some() {
        ttl = ttl.min(ttl_for(item.get("expi")));
    }
    Ok(NeteaseAudioUrl {
        url: ensure_https_url(url),
        cache_until: requested_at + ttl,
    })
}

// 内存保护：限制单个歌单最大处理数量（避免 OOM）
const MAX_TRACKS_LIMIT: usize = 5000;
// 内存保护：限制缓存最大条目数
const MAX_CACHE_ENTRIES: usize = 50;
const MAX_MUSIC_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_RATE_LIMIT_KEYS: usize = 1024;

struct StoredMusicEntry {
    entry: MusicCacheData,
    expires_at: Instant,
    size_bytes: usize,
}

enum MusicCacheData {
    Json(CacheEntry),
    Player(Arc<PlayerPlaylist>),
}

/// Every music provider shares these limits; callers cannot bypass eviction.
#[derive(Default)]
pub struct MusicCache {
    entries: HashMap<String, StoredMusicEntry>,
    size_bytes: usize,
}

impl MusicCache {
    fn prune(&mut self, now: Instant) {
        self.entries.retain(|_, stored| {
            if stored.expires_at <= now {
                self.size_bytes -= stored.size_bytes;
                false
            } else {
                true
            }
        });
    }

    pub fn get(&mut self, key: &str) -> Option<&CacheEntry> {
        self.prune(Instant::now());
        match &self.entries.get(key)?.entry {
            MusicCacheData::Json(entry) => Some(entry),
            MusicCacheData::Player(_) => None,
        }
    }

    pub fn insert(&mut self, key: String, entry: CacheEntry) {
        let size_bytes = music_value_bytes(&entry.data);
        let expires_at = entry.expires_at;
        self.insert_data(key, MusicCacheData::Json(entry), expires_at, size_bytes);
    }

    pub fn get_player(&mut self, key: &str) -> Option<Arc<PlayerPlaylist>> {
        self.prune(Instant::now());
        match &self.entries.get(key)?.entry {
            MusicCacheData::Player(playlist) => Some(Arc::clone(playlist)),
            MusicCacheData::Json(_) => None,
        }
    }

    pub fn insert_player(
        &mut self,
        key: String,
        playlist: Arc<PlayerPlaylist>,
        expires_at: Instant,
    ) {
        let size_bytes = std::mem::size_of::<PlayerPlaylist>()
            + playlist.playlist_id.capacity()
            + playlist.songs.capacity() * std::mem::size_of::<PlayerSong>()
            + playlist
                .songs
                .iter()
                .map(|song| {
                    song.id.capacity()
                        + song.name.capacity()
                        + song.artist.capacity()
                        + song.album.capacity()
                        + song.cover.capacity()
                })
                .sum::<usize>();
        self.insert_data(
            key,
            MusicCacheData::Player(playlist),
            expires_at,
            size_bytes,
        );
    }

    fn insert_data(
        &mut self,
        key: String,
        entry: MusicCacheData,
        expires_at: Instant,
        size_bytes: usize,
    ) {
        let now = Instant::now();
        self.prune(now);
        if let Some(previous) = self.entries.remove(&key) {
            self.size_bytes -= previous.size_bytes;
        }
        let size_bytes = size_bytes
            .saturating_add(key.capacity())
            .saturating_add(std::mem::size_of::<StoredMusicEntry>());
        if expires_at <= now || size_bytes > MAX_MUSIC_CACHE_BYTES {
            return;
        }
        while self.entries.len() >= MAX_CACHE_ENTRIES
            || self.size_bytes.saturating_add(size_bytes) > MAX_MUSIC_CACHE_BYTES
        {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, stored)| stored.expires_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(removed) = self.entries.remove(&oldest) {
                self.size_bytes -= removed.size_bytes;
            }
        }
        self.size_bytes += size_bytes;
        self.entries.insert(
            key,
            StoredMusicEntry {
                entry,
                expires_at,
                size_bytes,
            },
        );
    }
}

// Account for JSON containers as well as string payloads, without serializing
// another full copy just to decide whether it fits. Allocator overhead is approximate.
fn music_value_bytes(value: &Value) -> usize {
    let heap = match value {
        Value::String(text) => text.capacity(),
        Value::Array(items) => items.iter().fold(
            items
                .capacity()
                .saturating_mul(std::mem::size_of::<Value>()),
            |sum, item| sum.saturating_add(music_value_bytes(item)),
        ),
        Value::Object(items) => items.iter().fold(0usize, |sum, (key, item)| {
            sum.saturating_add(key.capacity())
                .saturating_add(std::mem::size_of::<String>() + 3 * std::mem::size_of::<usize>())
                .saturating_add(music_value_bytes(item))
        }),
        _ => 0,
    };
    std::mem::size_of::<Value>().saturating_add(heap)
}

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

        // Reclaim whole idle resources, including keys never requested again.
        if let Some(hour_ago) = one_hour_ago {
            self.requests
                .retain(|_, times| times.last().is_some_and(|at| *at > hour_ago));
        }
        if !self.requests.contains_key(key) && self.requests.len() >= MAX_RATE_LIMIT_KEYS {
            return false;
        }
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
pub static MUSIC_CACHE: Lazy<Arc<RwLock<MusicCache>>> =
    Lazy::new(|| Arc::new(RwLock::new(MusicCache::default())));
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

    async fn request_playlist_json(
        &self,
        url: &str,
        timeout: Duration,
    ) -> Result<reqwest::Response> {
        // 生成随机设备ID和时间戳
        let device_id = generate_device_id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        // IP 伪装
        let client_ip = get_random_china_ip();
        let proxy_ip = get_random_china_ip();
        let forwarded_for = format!("{}, {}", client_ip, proxy_ip);

        self
            .client
            .get(url)
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
            .timeout(timeout)
            .send()
            .await.map_err(Into::into)
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
            let mut cache = MUSIC_CACHE.write().await;
            if let Some(entry) = cache.get(&cache_key) {
                if entry.expires_at > Instant::now() {
                    tracing::debug!("✅ Cache hit for playlist: {}", playlist_id);
                    return Ok(entry.data.clone());
                }
            }
        }

        let url =
            format!("https://music.163.com/api/v6/playlist/detail?id={playlist_id}&n=1000&s=0&t=0");
        let response = self
            .request_playlist_json(&url, Duration::from_secs(30))
            .await?;
        let mut data: Value = player::read_playlist_json(response).await?;

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

                    // 处理大歌单（超过1000首）：串行批量获取，限制 `MAX_TRACKS_LIMIT`。
                    if track_count > loaded_tracks && loaded_tracks >= 1000 {
                        // 内存保护：限制最大获取数量，避免 OOM
                        let effective_track_count = std::cmp::min(track_count, MAX_TRACKS_LIMIT);

                        tracing::info!(
                            "🎵 Large playlist detected ({}/{}, capped at {}), fetching remaining songs...",
                            loaded_tracks,
                            track_count,
                            effective_track_count
                        );

                        if let Some(track_ids_array) = track_ids {
                            // 不克隆已加载曲目，只按 `track_ids_array` 补齐
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

                                // 添加延迟，减轻服务器压力
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
                                match self
                                    .request_playlist_json(&track_url, Duration::from_secs(15))
                                    .await
                                {
                                    Ok(resp) => {
                                        match player::read_playlist_json::<Value>(resp).await {
                                            Ok(batch_data) => {
                                                if batch_data.get("code").and_then(|c| c.as_i64())
                                                    == Some(200)
                                                {
                                                    if let Some(songs) = batch_data
                                                        .get("songs")
                                                        .and_then(|s| s.as_array())
                                                    {
                                                        for song in songs {
                                                            // `fee` → `isVip`；整首 `clone` 进列表
                                                            let fee = song
                                                                .get("fee")
                                                                .and_then(|f| f.as_i64())
                                                                .unwrap_or(0);
                                                            let is_vip = fee == 1 || fee == 4;
                                                            if is_vip {
                                                                vip_count += 1;
                                                            }

                                                            // 克隆并添加 isVip 标记
                                                            let mut song_data = song.clone();
                                                            if let Some(obj) =
                                                                song_data.as_object_mut()
                                                            {
                                                                obj.insert(
                                                                    "isVip".to_string(),
                                                                    json!(is_vip),
                                                                );
                                                            }
                                                            new_tracks.push(song_data);
                                                        }
                                                    }
                                                } else {
                                                    tracing::warn!(
                                                        "⚠️ Batch {} returned error code",
                                                        batch_num + 1
                                                    );
                                                    failed_batches += 1;
                                                }
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "⚠️ Failed to parse batch {}: {}",
                                                    batch_num + 1,
                                                    e
                                                );
                                                failed_batches += 1;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "⚠️ Failed to fetch batch {}: {}",
                                            batch_num + 1,
                                            e
                                        );
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

        // 存入缓存（添加缓存清理和内存保护）
        {
            let mut cache = MUSIC_CACHE.write().await;

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
            let mut cache = MUSIC_CACHE.write().await;
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

    /// 获取逐字歌词（网易云 yrc 格式）
    ///
    /// 使用 `song/lyric/v1` 接口并请求 `yv`/`ytv`/`yrv` 字段，返回体在包含普通
    /// `lrc`（逐行）之外，还含 `yrc`（逐字）、`ytlrc`（逐字翻译）、`yromalrc`（逐字罗马音）。
    /// 与旧 `fetch_lyrics` 分离，避免影响已稳定的逐行歌词链路。
    pub async fn fetch_lyrics_verbatim(&self, song_id: i64) -> Result<Value> {
        let cache_key = format!("lyrics_verbatim:{}", song_id);

        // 检查限流
        {
            let mut limiter = RATE_LIMITER.write().await;
            if !limiter.check_rate_limit(&cache_key) {
                return Err(anyhow!("Rate limit exceeded for lyrics {}", song_id));
            }
        }

        // 检查缓存
        {
            let mut cache = MUSIC_CACHE.write().await;
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
            "https://music.163.com/api/song/lyric/v1?id={}&cp=false&lv=0&kv=0&tv=0&yv=0&ytv=0&yrv=0",
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
            let mut cache = MUSIC_CACHE.write().await;
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
    pub async fn fetch_audio_url(&self, song_id: i64) -> Result<NeteaseAudioUrl> {
        let requested_at = Instant::now();
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

        parse_audio_url(&data, requested_at)
    }
}

impl Default for NeteaseService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod audio_url_tests {
    use super::*;

    #[test]
    fn explicit_expiry_bounds_cache_and_missing_expiry_uses_short_fallback() {
        let start = Instant::now();
        for (expiry, seconds) in [
            (json!(1200), 300),
            (json!(120), 90),
            (json!(31), 1),
            (json!(30), 0),
            (json!(5), 0),
            (json!(0), 0),
            (json!(-1), 0),
            (Value::Null, 60),
            (json!("invalid"), 60),
            (json!(1.5), 60),
        ] {
            let data = json!({"data": [{"url": "http://music.126.net/song.mp3", "expi": expiry}]});
            let result = parse_audio_url(&data, start).unwrap();
            assert_eq!(result.cache_until, start + Duration::from_secs(seconds));
            assert_eq!(result.url, "https://music.126.net/song.mp3");
        }
        let missing = parse_audio_url(
            &json!({"data": [{"url": "https://music.126.net/song"}]}),
            start,
        )
        .unwrap();
        assert_eq!(missing.cache_until, start + Duration::from_secs(60));
    }

    #[test]
    fn selected_resource_expiry_and_request_time_are_respected() {
        let start = Instant::now() - Duration::from_secs(70);
        let data = json!({"data": [{"url": "https://music.126.net/main", "expi": 1200,
            "uf": {"url": "http://music.126.net/uf", "expi": 90}}]});
        let result = parse_audio_url(&data, start).unwrap();
        assert_eq!(result.url, "https://music.126.net/uf");
        assert_eq!(result.cache_until, start + Duration::from_secs(60));
        assert!(result.cache_until < Instant::now());
        let data = json!({"data": [{"url": "https://music.126.net/main", "expi": 40,
            "uf": {"url": "https://music.126.net/uf", "expi": 1200}}]});
        assert_eq!(
            parse_audio_url(&data, start).unwrap().cache_until,
            start + Duration::from_secs(10)
        );
        let data = json!({"data": [{"url": "https://music.126.net/main", "expi": 1200,
            "uf": {"url": "https://music.126.net/uf"}}]});
        assert_eq!(
            parse_audio_url(&data, start).unwrap().cache_until,
            start + Duration::from_secs(60)
        );
    }

    #[test]
    fn missing_playable_url_is_still_an_error() {
        for data in [
            json!({}),
            json!({"data": []}),
            json!({"data": [{"url": null}]}),
            json!({"data": [{"url": "null"}]}),
            json!({"data": [{"url": ""}]}),
        ] {
            assert!(parse_audio_url(&data, Instant::now()).is_err());
        }
    }
}

#[cfg(test)]
mod memory_budget_tests {
    use super::*;

    fn entry(data: Value, expires_at: Instant) -> CacheEntry {
        CacheEntry { data, expires_at }
    }

    #[test]
    fn music_cache_bounds_entries_bytes_and_reclaims_expired_payloads() {
        let mut cache = MusicCache::default();
        let future = Instant::now() + Duration::from_secs(60);
        for i in 0..MAX_CACHE_ENTRIES + 10 {
            cache.insert(format!("lyrics:{i}"), entry(json!(i), future));
        }
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        cache.insert(
            "huge".into(),
            entry(json!("x".repeat(MAX_MUSIC_CACHE_BYTES)), future),
        );
        assert!(cache.get("huge").is_none());
        // Isolate the byte budget from the count fixture. Equal expirations may
        // evict any entry, so retaining 50 mixed small/large values can be valid.
        let mut cache = MusicCache::default();
        for i in 0..20 {
            cache.insert(
                format!("large:{i}"),
                entry(json!("x".repeat(1024 * 1024)), future),
            );
        }
        assert!(cache.size_bytes <= MAX_MUSIC_CACHE_BYTES);
        assert!(cache.entries.len() < 20);
        cache.insert("expired".into(), entry(json!("old"), Instant::now()));
        assert!(cache.get("expired").is_none());
        cache.prune(future + Duration::from_secs(1));
        assert!(cache.entries.is_empty());
        assert_eq!(cache.size_bytes, 0);
    }

    #[test]
    fn player_snapshots_share_storage_and_the_existing_cache_budget() {
        use super::super::music_player_view::PlayerMusicSource;
        let mut cache = MusicCache::default();
        let future = Instant::now() + Duration::from_secs(60);
        let playlist = Arc::new(PlayerPlaylist {
            code: 200,
            source: PlayerMusicSource::Netease,
            playlist_id: "42".into(),
            songs: vec![],
        });
        cache.insert_player("player".into(), playlist.clone(), future);
        assert!(Arc::ptr_eq(&playlist, &cache.get_player("player").unwrap()));
        assert!(cache.get("player").is_none());
        for i in 0..MAX_CACHE_ENTRIES {
            cache.insert(
                format!("lyrics:{i}"),
                entry(json!(i), future + Duration::from_secs(1)),
            );
        }
        assert!(cache.get_player("player").is_none());
        assert_eq!(cache.entries.len(), MAX_CACHE_ENTRIES);
        let mut oversized = (*playlist).clone();
        oversized.playlist_id = "x".repeat(MAX_MUSIC_CACHE_BYTES);
        cache.insert_player("oversized".into(), Arc::new(oversized), future);
        assert!(cache.get_player("oversized").is_none());
        assert!(cache.size_bytes <= MAX_MUSIC_CACHE_BYTES);
        cache.insert_player("expired".into(), playlist.clone(), Instant::now());
        assert!(cache.get_player("expired").is_none());
        cache.insert_player("player".into(), playlist, future);
        cache.prune(future + Duration::from_secs(2));
        assert!(cache.entries.is_empty());
        assert_eq!(cache.size_bytes, 0);
    }

    #[test]
    fn resource_limiter_bounds_keys_and_reclaims_idle_resources() {
        let mut limiter = RateLimiter::new();
        for i in 0..MAX_RATE_LIMIT_KEYS {
            assert!(limiter.check_rate_limit(&format!("song:{i}")));
        }
        assert!(!limiter.check_rate_limit("overflow"));
        assert!(limiter.check_rate_limit("song:0"));
        assert_eq!(limiter.requests.len(), MAX_RATE_LIMIT_KEYS);
        let old = Instant::now() - Duration::from_secs(3601);
        limiter
            .requests
            .values_mut()
            .for_each(|times| *times = vec![old]);
        assert!(limiter.check_rate_limit("new"));
        assert_eq!(limiter.requests.len(), 1);
    }
}
