//! Enka.Network 角色元数据（名字 / 图标 / 稀有度）
//!
//! Enka 的 UID 接口只返回 avatarId，展柜要显示角色名和头像必须查映射表。
//! 数据源（GitHub raw，磁盘缓存 + 内存缓存，7 天过期）：
//! - 原神：EnkaNetwork/API-docs `store/characters.json` + `store/loc.json`
//! - 星铁：EnkaNetwork/API-docs `store/hsr/honker_characters.json`
//! （名字哈希在该文件里精度已损坏，改用 Mar-7th/StarRailRes 的 `index_min/{lang}/characters.json`）
//! - 绝区零：EnkaNetwork/API-docs `store/zzz/avatars.json` + `store/zzz/locs.json`
//! （avatars 的 `Name` 是代号，locs 里用代号做 key 换取本地化名）
//!
//! 图标直接用 Enka 的 UI CDN（`https://enka.network/ui/...`），前端 <img> 可直载。

use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

use crate::services::outbound_security::build_public_http_client;

const CACHE_DIR: &str = "cache/enka_assets";
const DISK_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, Default)]
pub struct CharacterMeta {
    pub name: Option<String>,
    pub icon: Option<String>,
    /// 大幅立绘（原神抽卡图 / 星铁签绘 / 绝区零半身像），聚焦展示用
    pub art: Option<String>,
    pub rarity: Option<i64>,
}

struct DocStore {
    docs: RwLock<HashMap<&'static str, Arc<Value>>>,
    /// 防止同一文件被并发重复拉取
    fetch_lock: Mutex<()>,
}

static STORE: OnceLock<DocStore> = OnceLock::new();

fn store() -> &'static DocStore {
    STORE.get_or_init(|| DocStore {
        docs: RwLock::new(HashMap::new()),
        fetch_lock: Mutex::new(()),
    })
}

fn doc_url(key: &str) -> &'static str {
    match key {
        "gi_chars" => {
            "https://raw.githubusercontent.com/EnkaNetwork/API-docs/master/store/characters.json"
        }
        "gi_loc" => "https://raw.githubusercontent.com/EnkaNetwork/API-docs/master/store/loc.json",
        "gi_pfps" => {
            "https://raw.githubusercontent.com/EnkaNetwork/API-docs/master/store/pfps.json"
        }
        "hsr_chars" => {
            "https://raw.githubusercontent.com/EnkaNetwork/API-docs/master/store/hsr/honker_characters.json"
        }
        "hsr_names_cn" => {
            "https://raw.githubusercontent.com/Mar-7th/StarRailRes/master/index_min/cn/characters.json"
        }
        "hsr_names_en" => {
            "https://raw.githubusercontent.com/Mar-7th/StarRailRes/master/index_min/en/characters.json"
        }
        "hsr_names_jp" => {
            "https://raw.githubusercontent.com/Mar-7th/StarRailRes/master/index_min/jp/characters.json"
        }
        "zzz_avatars" => {
            "https://raw.githubusercontent.com/EnkaNetwork/API-docs/master/store/zzz/avatars.json"
        }
        "zzz_locs" => {
            "https://raw.githubusercontent.com/EnkaNetwork/API-docs/master/store/zzz/locs.json"
        }
        _ => unreachable!("unknown enka asset doc key"),
    }
}

async fn fetch_doc(url: &str) -> Result<Value, String> {
    let (_, client) = build_public_http_client(
        url,
        Duration::from_secs(30),
        Some("Myriad/1.0 (enka-assets)"),
    )
    .await
    .map_err(|e| e.to_string())?;

    let resp = client.get(url).send().await.map_err(|error| {
        tracing::warn!(%error, "enka asset request failed");
        "request failed".to_string()
    })?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.json::<Value>().await.map_err(|error| {
        tracing::warn!(%error, "enka asset parse failed");
        "parse failed".to_string()
    })
}

fn disk_path(key: &str) -> PathBuf {
    PathBuf::from(CACHE_DIR).join(format!("{key}.json"))
}

fn load_disk(key: &str) -> Option<Value> {
    let path = disk_path(key);
    let meta = std::fs::metadata(&path).ok()?;
    let age = meta.modified().ok()?.elapsed().ok()?;
    if age > DISK_TTL {
        return None;
    }
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_disk(key: &str, doc: &Value) {
    let path = disk_path(key);
    if let Err(e) = std::fs::create_dir_all(CACHE_DIR) {
        tracing::warn!("enka assets cache dir failed: {e}");
        return;
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(content) = serde_json::to_string(doc) {
        if std::fs::write(&tmp, content).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// 取映射文档：内存 → 磁盘（7 天内）→ GitHub。全部失败返回 None（展柜退化为无名/无图标）。
async fn get_doc(key: &'static str) -> Option<Arc<Value>> {
    {
        let docs = store().docs.read().await;
        if let Some(doc) = docs.get(key) {
            return Some(doc.clone());
        }
    }

    let _guard = store().fetch_lock.lock().await;
    // 拿到锁后再查一次，可能已被并发请求填充
    {
        let docs = store().docs.read().await;
        if let Some(doc) = docs.get(key) {
            return Some(doc.clone());
        }
    }

    let doc = if let Some(doc) = load_disk(key) {
        doc
    } else {
        match fetch_doc(doc_url(key)).await {
            Ok(doc) => {
                save_disk(key, &doc);
                doc
            }
            Err(e) => {
                tracing::warn!("enka asset {key} fetch failed: {e}");
                // 磁盘上有过期副本也比没有强
                let stale = std::fs::read_to_string(disk_path(key))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());
                match stale {
                    Some(doc) => doc,
                    None => return None,
                }
            }
        }
    };

    let doc = Arc::new(doc);
    store().docs.write().await.insert(key, doc.clone());
    Some(doc)
}

/// 语言归一化：前端 i18n locale → 各数据源的语言 key
fn gi_lang(lang: &str) -> &'static str {
    match lang {
        l if l.starts_with("zh") => "zh-cn",
        l if l.starts_with("ja") => "ja",
        _ => "en",
    }
}

fn hsr_names_key(lang: &str) -> &'static str {
    match lang {
        l if l.starts_with("zh") => "hsr_names_cn",
        l if l.starts_with("ja") => "hsr_names_jp",
        _ => "hsr_names_en",
    }
}

fn zzz_lang(lang: &str) -> &'static str {
    match lang {
        l if l.starts_with("zh") => "zh-cn",
        l if l.starts_with("ja") => "ja",
        _ => "en",
    }
}

/// 原神：avatarId → 名字（本地化）/ 头像 / 星级
pub async fn gi_character(avatar_id: i64, lang: &str) -> CharacterMeta {
    let mut meta = CharacterMeta::default();
    let Some(chars) = get_doc("gi_chars").await else {
        return meta;
    };
    let Some(entry) = chars.get(avatar_id.to_string()) else {
        return meta;
    };

    if let Some(side_icon) = entry.get("SideIconName").and_then(|v| v.as_str()) {
        let icon = side_icon.replace("_Side", "");
        meta.icon = Some(format!("https://enka.network/ui/{icon}.png"));
        // UI_AvatarIcon_Side_Ayaka → UI_Gacha_AvatarImg_Ayaka（抽卡立绘）
        let gacha = side_icon.replace("UI_AvatarIcon_Side_", "UI_Gacha_AvatarImg_");
        meta.art = Some(format!("https://enka.network/ui/{gacha}.png"));
    }
    meta.rarity = entry.get("QualityType").and_then(|v| v.as_str()).map(|q| {
        // ORANGE / ORANGE_SP（埃洛伊）按五星，其余四星
        if q.contains("ORANGE") {
            5
        } else {
            4
        }
    });

    if let Some(hash) = entry.get("NameTextMapHash") {
        // 哈希可能是数字或字符串，统一转字符串做 key
        let hash_key = hash
            .as_i64()
            .map(|n| n.to_string())
            .or_else(|| hash.as_str().map(|s| s.to_string()));
        if let (Some(hash_key), Some(loc)) = (hash_key, get_doc("gi_loc").await) {
            meta.name = loc
                .get(gi_lang(lang))
                .and_then(|l| l.get(&hash_key))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    meta
}

/// 原神：资料头像（profilePicture）。新版接口给 pfp id，旧版给 avatarId。
pub async fn gi_profile_picture(pfp_id: Option<i64>, avatar_id: Option<i64>) -> Option<String> {
    if let Some(avatar_id) = avatar_id {
        let meta = gi_character(avatar_id, "en").await;
        if meta.icon.is_some() {
            return meta.icon;
        }
    }
    let pfp_id = pfp_id?;
    let pfps = get_doc("gi_pfps").await?;
    let icon = pfps
        .get(pfp_id.to_string())?
        .get("iconPath")?
        .as_str()?
        .to_string();
    Some(format!("https://enka.network/ui/{icon}.png"))
}

/// 星铁：avatarId → 名字 / 圆形头像 / 星级
pub async fn hsr_character(avatar_id: i64, lang: &str) -> CharacterMeta {
    let mut meta = CharacterMeta::default();
    let id_key = avatar_id.to_string();

    if let Some(chars) = get_doc("hsr_chars").await {
        if let Some(entry) = chars.get(&id_key) {
            if let Some(path) = entry.get("AvatarSideIconPath").and_then(|v| v.as_str()) {
                // AvatarRoundIcon 比 SideIcon 更适合小尺寸网格
                let round = path.replace("AvatarSideIcon", "AvatarRoundIcon");
                meta.icon = Some(format!("https://enka.network/ui/hsr/{round}"));
            }
            if let Some(path) = entry
                .get("AvatarCutinFrontImgPath")
                .and_then(|v| v.as_str())
            {
                meta.art = Some(format!("https://enka.network/ui/hsr/{path}"));
            }
            meta.rarity = entry.get("Rarity").and_then(|v| v.as_i64());
        }
    }

    if let Some(names) = get_doc(hsr_names_key(lang)).await {
        meta.name = names
            .get(&id_key)
            .and_then(|c| c.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    meta
}

/// 绝区零：角色 Id → 名字 / 圆形头像 / 稀有度（4=S 级、3=A 级，按数值原样返回）
pub async fn zzz_character(avatar_id: i64, lang: &str) -> CharacterMeta {
    let mut meta = CharacterMeta::default();
    let Some(avatars) = get_doc("zzz_avatars").await else {
        return meta;
    };
    let Some(entry) = avatars.get(avatar_id.to_string()) else {
        return meta;
    };

    if let Some(circle) = entry.get("CircleIcon").and_then(|v| v.as_str()) {
        meta.icon = Some(format!("https://enka.network{circle}"));
    }
    if let Some(image) = entry.get("Image").and_then(|v| v.as_str()) {
        meta.art = Some(format!("https://enka.network{image}"));
    }
    meta.rarity = entry.get("Rarity").and_then(|v| v.as_i64());

    if let Some(codename) = entry.get("Name").and_then(|v| v.as_str()) {
        if let Some(locs) = get_doc("zzz_locs").await {
            meta.name = locs
                .get(zzz_lang(lang))
                .and_then(|l| l.get(codename))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    meta
}
