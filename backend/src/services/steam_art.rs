//! Steam 游戏封面的真实地址。
//!
//! 各处按 appid 拼 `…/steam/apps/{appid}/header.jpg` 这类老地址；新上架、换过图或锁区的
//! 游戏，图只放在带一段哈希的新路径下，老地址 404。真实文件名只能从商店接口查，
//! 图片代理在老地址取不到时用这里换成新地址再取一次。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use serde_json::Value;

const STORE_ASSETS: &str = "https://shared.akamai.steamstatic.com/store_item_assets/";
/// 换图不常发生，查到的、查不到的都记一阵，免得同一屏几十张图各问一遍。
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const CACHE_MAX: usize = 4096;

static CACHE: Lazy<Mutex<HashMap<u64, (Instant, Option<Value>)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 老地址里的 appid 和文件名；已经带哈希段的新地址不认。
pub fn legacy_app_art(url: &str) -> Option<(u64, String)> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if !myriad_image_proxy::host_matches_domain(host, "steamstatic.com") {
        return None;
    }
    let rest: Vec<&str> = parsed.path_segments()?.collect();
    // `steam/apps/{appid}/{file}`，有的 CDN 前面还有 `store_item_assets/`。
    let start = rest.iter().position(|segment| *segment == "steam")?;
    match &rest[start..] {
        ["steam", "apps", appid, file] if file.ends_with(".jpg") => {
            Some((appid.parse().ok()?, (*file).to_string()))
        }
        _ => None,
    }
}

/// 老文件名在商店接口 `assets` 里对应的键。
fn asset_key(file: &str) -> Option<&'static str> {
    Some(match file {
        "header.jpg" => "header",
        "header_2x.jpg" => "header_2x",
        "library_600x900.jpg" => "library_capsule",
        "library_600x900_2x.jpg" => "library_capsule_2x",
        "library_hero.jpg" => "library_hero",
        "capsule_616x353.jpg" => "main_capsule",
        "capsule_231x87.jpg" => "small_capsule",
        _ => return None,
    })
}

/// 从商店接口的 `assets` 拼出这张图的真实地址。
fn asset_url(assets: &Value, file: &str) -> Option<String> {
    let format = assets.get("asset_url_format")?.as_str()?;
    let name = asset_key(file)
        .and_then(|key| assets.get(key))
        .and_then(Value::as_str)
        .or_else(|| {
            assets
                .as_object()?
                .values()
                .filter_map(Value::as_str)
                .find(|name| name.rsplit('/').next() == Some(file))
        })?;
    Some(format!(
        "{STORE_ASSETS}{}",
        format.replace("${FILENAME}", name)
    ))
}

async fn fetch_assets(appid: u64) -> Option<Value> {
    let request = serde_json::json!({
        "ids": [{ "appid": appid }],
        // 不给地区就什么都不回；按美区取，图在各区是同一套。
        "context": { "language": "english", "country_code": "US" },
        "data_request": { "include_assets": true },
    });
    let mut endpoint =
        url::Url::parse("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/").ok()?;
    endpoint
        .query_pairs_mut()
        .append_pair("input_json", &request.to_string());
    let (target, client) = crate::services::outbound_security::build_public_http_client(
        endpoint.as_str(),
        Duration::from_secs(8),
        None,
    )
    .await
    .ok()?;
    let response = client.get(target).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = crate::services::outbound_security::read_limited_body(response, 256 * 1024)
        .await
        .ok()?;
    let parsed: Value = serde_json::from_slice(&body).ok()?;
    parsed
        .pointer("/response/store_items/0/assets")
        .filter(|assets| assets.is_object())
        .cloned()
}

/// 老地址取不到时，同一张图的真实地址。
pub async fn resolve(appid: u64, file: &str) -> Option<String> {
    let cached = CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.get(&appid).cloned())
        .filter(|(at, _)| at.elapsed() < CACHE_TTL);
    let assets = match cached {
        Some((_, assets)) => assets,
        None => {
            let assets = fetch_assets(appid).await;
            if let Ok(mut cache) = CACHE.lock() {
                if cache.len() >= CACHE_MAX {
                    cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
                    if cache.len() >= CACHE_MAX {
                        cache.clear();
                    }
                }
                cache.insert(appid, (Instant::now(), assets.clone()));
            }
            assets
        }
    };
    asset_url(&assets?, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_legacy_app_art_is_recognised() {
        assert_eq!(
            legacy_app_art("https://cdn.cloudflare.steamstatic.com/steam/apps/3527290/header.jpg"),
            Some((3527290, "header.jpg".to_string()))
        );
        assert_eq!(
            legacy_app_art(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/10/library_600x900.jpg"
            ),
            Some((10, "library_600x900.jpg".to_string()))
        );
        // 已经带哈希的新地址、头像、别的站都不管。
        assert_eq!(
            legacy_app_art(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1/abc/header.jpg"
            ),
            None
        );
        assert_eq!(
            legacy_app_art("https://avatars.steamstatic.com/x_full.jpg"),
            None
        );
        assert_eq!(
            legacy_app_art("https://example.com/steam/apps/1/header.jpg"),
            None
        );
    }

    #[test]
    fn hashed_names_come_from_the_store_assets() {
        let assets = json!({
            "asset_url_format": "steam/apps/3527290/${FILENAME}?t=1792684718",
            "header": "31bac6b2/header.jpg",
            "library_capsule": "480bd879/library_600x900.jpg",
            "main_capsule": "f40ef565/capsule_616x353.jpg",
        });
        assert_eq!(
            asset_url(&assets, "header.jpg").as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3527290/31bac6b2/header.jpg?t=1792684718"
            )
        );
        assert!(
            asset_url(&assets, "library_600x900.jpg")
                .unwrap()
                .ends_with("/480bd879/library_600x900.jpg?t=1792684718")
        );
        // 没有对应键时按文件名找。
        assert!(
            asset_url(&assets, "capsule_616x353.jpg")
                .unwrap()
                .contains("/f40ef565/")
        );
        assert_eq!(asset_url(&assets, "page_bg.jpg"), None);
    }
}
