//! Steam store art resolution.
//!
//! The old art path (`{cdn}/steam/apps/{appid}/header.jpg`) is gone for games whose
//! store art filenames became hashed. Blue Archive (appid 3557620) is the canary:
//! `cdn.cloudflare.steamstatic.com/steam/apps/3557620/header.jpg` → 404, while the
//! store service reports `b723141f…/header.jpg` under
//! `store_item_assets/steam/apps/3557620/`. Region-locked games also refuse to
//! answer under their own country (CN returns `success=15`, no assets), so the
//! lookup walks `REGION_PRIORITY` and keeps whatever the first laxer region gives.
//!
//! Resolution happens once per platform refresh; results land in
//! `CACHE_DIR/platforms/steam_assets.json` and in a process cache so every cover
//! consumer (`platform_items`, `library_items`, activity, reports, smart filter)
//! can read them synchronously without network access.

use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, OnceLock};

/// `IStoreBrowseService/GetItems` appid cap per request.
const STORE_ITEMS_LIMIT: usize = 100;
/// In-flight store requests per region pass (a 1000-game library is 10 batches).
const REGION_CONCURRENCY: usize = 4;

/// Laxest → strictest. Locked games only answer under a country they are sold in.
pub const REGION_PRIORITY: [&str; 3] = ["US", "HK", "CN"];

const STORE_ITEMS_URL: &str = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/";
const APP_DETAILS_URL: &str = "https://store.steampowered.com/api/appdetails";

/// Legacy host: still serves the old unhashed path for most games.
const LEGACY_CDN_BASE: &str = "https://cdn.cloudflare.steamstatic.com";

/// Fallback lookup budget per pass. Anything still missing after the region walk
/// is rare (delisted everywhere); do not hammer the appdetails endpoint.
const APP_DETAILS_LIMIT: usize = 50;

/// Positive art is refreshed this often (store art and its `?t=` change rarely).
const ART_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Ids the store never answers for are retried this often, not every refresh.
const MISS_TTL_SECS: i64 = 7 * 24 * 60 * 60;
/// Wall-clock budget for one pass; a hung store must not stall the refresh arm.
const REFRESH_BUDGET: std::time::Duration = std::time::Duration::from_secs(60);

/// One game's resolved art paths, relative to `{cdn}/store_item_assets/`.
/// The `?t=` cache-buster from `asset_url_format` is kept inside the path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SteamArt {
    /// 460×215 header used by library cards and report covers.
    #[serde(default)]
    pub header: Option<String>,
    /// 600×900 portrait used by activity tiles.
    #[serde(default)]
    pub library: Option<String>,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SteamAssetFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: HashMap<String, SteamArt>,
}

static CACHE: OnceLock<Mutex<HashMap<i64, SteamArt>>> = OnceLock::new();

fn lock_cache() -> std::sync::MutexGuard<'static, HashMap<i64, SteamArt>> {
    CACHE
        .get_or_init(|| Mutex::new(load_from_disk()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Cover CDN base, overridable with `STEAM_COVER_CDN`:
/// `akamai` (default) | `fastly` | `cloudflare` | `china`, or any full `https://` base.
pub fn cdn_base() -> &'static str {
    static BASE: Lazy<&'static str> = Lazy::new(|| {
        let raw = std::env::var("STEAM_COVER_CDN").unwrap_or_default();
        let value = raw.trim().trim_end_matches('/');
        match value.to_ascii_lowercase().as_str() {
            "" | "akamai" => "https://shared.akamai.steamstatic.com",
            "fastly" => "https://shared.fastly.steamstatic.com",
            "cloudflare" => "https://shared.cloudflare.steamstatic.com",
            "china" => "https://shared.cdn.steamchina.queniuam.com",
            _ if value.starts_with("https://") => Box::leak(value.to_string().into_boxed_str()),
            other => {
                tracing::warn!("Unknown STEAM_COVER_CDN {:?}; using akamai", other);
                "https://shared.akamai.steamstatic.com"
            }
        }
    });
    *BASE
}

/// 460×215 header cover; falls back to the legacy unhashed path when unresolved.
pub fn cover_url(appid: i64) -> String {
    if let Some(path) = art(appid).and_then(|a| a.header) {
        return format!("{}/store_item_assets/{path}", cdn_base());
    }
    format!("{LEGACY_CDN_BASE}/steam/apps/{appid}/header.jpg")
}

/// 600×900 portrait cover; falls back to the legacy unhashed path when unresolved.
pub fn library_cover_url(appid: i64) -> String {
    if let Some(path) = art(appid).and_then(|a| a.library) {
        return format!("{}/store_item_assets/{path}", cdn_base());
    }
    format!("{LEGACY_CDN_BASE}/steam/apps/{appid}/library_600x900.jpg")
}

/// Resolved art for one appid, if a refresh has seen it.
pub fn art(appid: i64) -> Option<SteamArt> {
    lock_cache().get(&appid).cloned()
}

fn cache_file() -> std::path::PathBuf {
    crate::services::data_paths::platforms_cache_dir().join("steam_assets.json")
}

fn load_from_disk() -> HashMap<i64, SteamArt> {
    let path = cache_file();
    let Ok(content) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    match serde_json::from_str::<SteamAssetFile>(&content) {
        Ok(file) => file
            .items
            .into_iter()
            .filter_map(|(k, v)| k.parse::<i64>().ok().map(|id| (id, v)))
            .collect(),
        Err(e) => {
            tracing::warn!("Invalid {}: {e}", path.display());
            HashMap::new()
        }
    }
}

fn persist(items: &HashMap<i64, SteamArt>) {
    let path = cache_file();
    let Some(dir) = path.parent() else {
        return;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("Cannot create {}: {e}", dir.display());
        return;
    }
    let file = SteamAssetFile {
        version: 1,
        items: items
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    };
    let json = match serde_json::to_vec_pretty(&file) {
        Ok(json) => json,
        Err(e) => {
            tracing::warn!("Cannot serialize steam assets: {e}");
            return;
        }
    };
    // Same-directory tmp + rename: a crash mid-write must not truncate a good cache.
    let temp = dir.join(format!(
        "steam_assets.{}.json.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let written = std::fs::write(&temp, &json)
        .and_then(|()| crate::services::platform_refresh::commit_cache_temp_file(&temp, &path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        tracing::warn!("Cannot write {}: {e}", path.display());
    }
}

/// Resolve art for `appids`, merge into the cache, and back off failures.
///
/// Ids resolved within their TTL are skipped: positive art is refreshed monthly
/// (store art and its `?t=` change rarely), ids the store never answers for are
/// retried weekly instead of on every platform refresh. A pass where no region
/// answers at all writes nothing — neither art nor backoff — so an outage never
/// poisons the cache.
pub async fn refresh_steam_assets(client: &reqwest::Client, appids: &[i64]) {
    let now = now_unix();
    let pending: Vec<i64> = {
        let cache = lock_cache();
        appids
            .iter()
            .copied()
            .filter(|id| *id > 0)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|id| needs_resolution(cache.get(id), now))
            .collect()
    };
    if pending.is_empty() {
        return;
    }

    match tokio::time::timeout(REFRESH_BUDGET, resolve_pass(client, &pending)).await {
        Ok(Some(pass)) => apply_pass(pass, now),
        Ok(None) => {} // store unreachable: keep every cover we already have
        Err(_) => tracing::warn!(
            "Steam cover resolution exceeded {}s for {} appids",
            REFRESH_BUDGET.as_secs(),
            pending.len()
        ),
    }
}

/// Skip ids already resolved recently; retry unresolved ones on a slower clock.
fn needs_resolution(art: Option<&SteamArt>, now: i64) -> bool {
    match art {
        None => true,
        Some(art) if art.header.is_none() => now - art.updated_at >= MISS_TTL_SECS,
        Some(art) => now - art.updated_at >= ART_TTL_SECS,
    }
}

struct ResolvePass {
    found: HashMap<i64, SteamArt>,
    /// Ids no region and no appdetails lookup could answer for.
    unresolved: Vec<i64>,
}

/// Walk the region priority, then appdetails for what no region carried.
/// `None` means the store service never answered at all.
async fn resolve_pass(client: &reqwest::Client, pending: &[i64]) -> Option<ResolvePass> {
    let mut found: HashMap<i64, SteamArt> = HashMap::new();
    let mut missing = pending.to_vec();
    let mut reachable_regions = 0usize;
    for cc in REGION_PRIORITY {
        if missing.is_empty() {
            break;
        }
        match fetch_region(client, &missing, cc, &mut found).await {
            Ok(still_missing) => {
                reachable_regions += 1;
                missing = still_missing;
            }
            Err(e) => tracing::warn!("Steam store items ({cc}) failed: {e}"),
        }
    }

    // The store itself is unreachable: do not spray appdetails at a dead network.
    if reachable_regions == 0 {
        return None;
    }

    // appdetails costs one call per game; only the first `APP_DETAILS_LIMIT`
    // leftovers get a try, the rest wait for the next refresh.
    let attempted = missing.len().min(APP_DETAILS_LIMIT);
    let mut unresolved = missing[attempted..].to_vec();
    for appid in &missing[..attempted] {
        match fetch_appdetails_cover(client, *appid).await {
            Some(path) => {
                found.insert(
                    *appid,
                    SteamArt {
                        header: Some(path),
                        library: None,
                        updated_at: now_unix(),
                    },
                );
            }
            None => unresolved.push(*appid),
        }
    }
    Some(ResolvePass { found, unresolved })
}

/// Merge one pass into the cache. Negatives only replace entries that had no art.
fn apply_pass(pass: ResolvePass, now: i64) {
    let resolved = pass.found.len();
    let mut deferred = 0usize;
    {
        let mut cache = lock_cache();
        for (appid, art) in pass.found {
            cache.insert(appid, art);
        }
        for appid in pass.unresolved {
            let has_art = cache.get(&appid).is_some_and(|a| a.header.is_some());
            if !has_art {
                cache.insert(
                    appid,
                    SteamArt {
                        header: None,
                        library: None,
                        updated_at: now,
                    },
                );
                deferred += 1;
            }
        }
        // Hold the lock across the write so concurrent passes cannot clobber
        // each other's freshly resolved ids.
        persist(&cache);
    }
    tracing::info!("✓ Steam covers resolved: {resolved}, deferred: {deferred}");
}

/// One region pass. Returns the appids this region could not answer for.
async fn fetch_region(
    client: &reqwest::Client,
    appids: &[i64],
    country: &str,
    found: &mut HashMap<i64, SteamArt>,
) -> Result<Vec<i64>, String> {
    let chunks: Vec<Vec<i64>> = appids
        .chunks(STORE_ITEMS_LIMIT)
        .map(<[i64]>::to_vec)
        .collect();
    // 1000-game libraries → 10 chunks; cap in-flight requests instead of a burst.
    let responses: Vec<Result<HashMap<i64, SteamArt>, String>> =
        futures::stream::iter(chunks.clone())
            .map(|chunk: Vec<i64>| async move { fetch_items(client, &chunk, country).await })
            .buffered(REGION_CONCURRENCY)
            .collect()
            .await;

    let mut any_ok = false;
    let mut missing = Vec::new();
    let mut last_err = String::new();
    for (chunk, response) in chunks.iter().zip(responses) {
        match response {
            Ok(items) => {
                any_ok = true;
                for appid in chunk {
                    match items.get(appid) {
                        Some(art) => {
                            found.insert(*appid, art.clone());
                        }
                        None => missing.push(*appid),
                    }
                }
            }
            Err(e) => {
                last_err = e;
                missing.extend_from_slice(chunk);
            }
        }
    }
    if any_ok { Ok(missing) } else { Err(last_err) }
}

async fn fetch_items(
    client: &reqwest::Client,
    appids: &[i64],
    country: &str,
) -> Result<HashMap<i64, SteamArt>, String> {
    let input = json!({
        "ids": appids.iter().map(|id| json!({ "appid": id })).collect::<Vec<_>>(),
        "context": { "country_code": country, "language": "english" },
        "data_request": { "include_assets": true },
    });
    let mut url = reqwest::Url::parse(STORE_ITEMS_URL).map_err(|e| format!("url: {e}"))?;
    url.query_pairs_mut()
        .append_pair("input_json", &input.to_string());
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    let body: Value = response.json().await.map_err(|e| format!("json: {e}"))?;
    let items = body
        .pointer("/response/store_items")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing response.store_items".to_string())?;

    let mut out = HashMap::new();
    for item in items {
        if let Some(appid) = item.get("appid").and_then(Value::as_i64)
            && let Some(art) = art_from_item(item)
        {
            out.insert(appid, art);
        }
    }
    Ok(out)
}

/// Last-resort lookup for games no region's store view carries.
async fn fetch_appdetails_cover(client: &reqwest::Client, appid: i64) -> Option<String> {
    let mut url = reqwest::Url::parse(APP_DETAILS_URL).ok()?;
    url.query_pairs_mut()
        .append_pair("appids", &appid.to_string())
        .append_pair("cc", "US");
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: Value = response.json().await.ok()?;
    let entry = body.get(appid.to_string())?;
    if entry.get("success").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let image = entry.pointer("/data/header_image")?.as_str()?;
    strip_store_item_assets(image)
}

/// Parse one `store_items[]` entry into a resolved art record.
pub fn art_from_item(item: &Value) -> Option<SteamArt> {
    // 1 = visible with assets; 15 = not sold/not visible in this country.
    if item.get("success").and_then(Value::as_i64) != Some(1) {
        return None;
    }
    let assets = item.get("assets")?;
    let format = assets.get("asset_url_format").and_then(Value::as_str)?;
    let header = assets
        .get("header")
        .and_then(Value::as_str)
        .and_then(|file| apply_filename_format(format, file))?;
    let library = assets
        .get("library_capsule")
        .and_then(Value::as_str)
        .and_then(|file| apply_filename_format(format, file));
    Some(SteamArt {
        header: Some(header),
        library,
        updated_at: now_unix(),
    })
}

/// Expand `${FILENAME}` in `asset_url_format` (`steam/apps/3557620/${FILENAME}?t=…`).
pub fn apply_filename_format(format: &str, filename: &str) -> Option<String> {
    let filename = filename.trim();
    if filename.is_empty() || !format.contains("${FILENAME}") {
        return None;
    }
    Some(format.replace("${FILENAME}", filename))
}

/// Cut an absolute store image down to its `store_item_assets/`-relative path.
pub fn strip_store_item_assets(url: &str) -> Option<String> {
    const MARKER: &str = "/store_item_assets/";
    let url = url.trim();
    let index = url.find(MARKER)?;
    let path = url[index + MARKER.len()..].trim_start_matches('/');
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn now_unix() -> i64 {
    // The site's one clock (backend/clippy.toml forbids reading the host clock here).
    crate::clock::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE_ARCHIVE_FORMAT: &str = "steam/apps/3557620/${FILENAME}?t=1779773548";

    #[test]
    fn expands_hashed_filename_and_keeps_cache_buster() {
        assert_eq!(
            apply_filename_format(
                BLUE_ARCHIVE_FORMAT,
                "b723141fa8cf93bb2bb56d03789561341a2cad62/header.jpg"
            )
            .as_deref(),
            Some(
                "steam/apps/3557620/b723141fa8cf93bb2bb56d03789561341a2cad62/header.jpg?t=1779773548"
            )
        );
    }

    #[test]
    fn rejects_formats_without_placeholder_or_empty_filename() {
        assert_eq!(
            apply_filename_format("steam/apps/1/header.jpg", "a.jpg"),
            None
        );
        assert_eq!(apply_filename_format(BLUE_ARCHIVE_FORMAT, "  "), None);
    }

    #[test]
    fn cuts_appdetails_url_to_relative_asset_path() {
        assert_eq!(
            strip_store_item_assets(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3557620/x/header.jpg?t=1"
            )
            .as_deref(),
            Some("steam/apps/3557620/x/header.jpg?t=1")
        );
        assert_eq!(
            strip_store_item_assets("https://x/steam/apps/1/a.jpg"),
            None
        );
    }

    #[test]
    fn retries_misses_weekly_and_art_monthly() {
        let now = 1_800_000_000;
        assert!(needs_resolution(None, now));

        let fresh_art = SteamArt {
            header: Some("a/header.jpg".into()),
            library: None,
            updated_at: now - 60,
        };
        assert!(!needs_resolution(Some(&fresh_art), now));

        let stale_art = SteamArt {
            header: Some("a/header.jpg".into()),
            library: None,
            updated_at: now - ART_TTL_SECS - 1,
        };
        assert!(needs_resolution(Some(&stale_art), now));

        let fresh_miss = SteamArt {
            header: None,
            library: None,
            updated_at: now - 60,
        };
        assert!(!needs_resolution(Some(&fresh_miss), now));

        let old_miss = SteamArt {
            header: None,
            library: None,
            updated_at: now - MISS_TTL_SECS - 1,
        };
        assert!(needs_resolution(Some(&old_miss), now));
    }

    #[test]
    fn skips_locked_items_and_keeps_header_plus_portrait() {
        let locked = json!({"appid": 3557620, "success": 15});
        assert!(art_from_item(&locked).is_none());

        let item = json!({
            "appid": 3557620,
            "success": 1,
            "assets": {
                "asset_url_format": BLUE_ARCHIVE_FORMAT,
                "header": "b723141fa8cf93bb2bb56d03789561341a2cad62/header.jpg",
                "library_capsule": "56639673818079bf966c8bec85d7c3b26baced24/library_600x900.jpg"
            }
        });
        let art = art_from_item(&item).expect("assets parse");
        assert_eq!(
            art.library.as_deref(),
            Some(
                "steam/apps/3557620/56639673818079bf966c8bec85d7c3b26baced24/library_600x900.jpg?t=1779773548"
            )
        );
    }

    #[test]
    fn fallback_url_uses_legacy_shape_when_unresolved() {
        // Unknown appid → no cache entry, legacy host preserved.
        assert_eq!(
            cover_url(-1),
            "https://cdn.cloudflare.steamstatic.com/steam/apps/-1/header.jpg"
        );
        assert_eq!(
            library_cover_url(-1),
            "https://cdn.cloudflare.steamstatic.com/steam/apps/-1/library_600x900.jpg"
        );
    }
}
