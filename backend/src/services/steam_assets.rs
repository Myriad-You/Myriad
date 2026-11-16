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
//! can read them synchronously without network access. The game being played
//! right now is resolved in the background when the presence is read.
//!
//! The image proxy asks here when an old-path cover 404s. That endpoint is
//! public, so its lookups are kept small: the US store view only, no appdetails,
//! and nothing it learns about a missing or unreachable game reaches the disk —
//! those answers live in a bounded in-memory table that expires.
//!
//! "Not on the store" is only written down when every region and appdetails
//! actually answered without the game. A request that failed, timed out or was
//! never sent says nothing about the game; it is held back in memory for a while
//! and asked again later.

use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

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
/// Host for resolved (hashed) store art.
const CDN_BASE: &str = "https://shared.akamai.steamstatic.com";

/// Store answers are small (100 items with assets is ~150 KiB; one appdetails
/// entry rarely passes 200 KiB). Anything far past that is not what we asked for.
const STORE_BODY_LIMIT: usize = 2 * 1024 * 1024;
/// Per-request timeouts, shorter than the shared client's: one slow answer must
/// not eat a whole pass.
const STORE_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const APP_DETAILS_TIMEOUT: Duration = Duration::from_secs(8);

/// Fallback lookup budget per pass. Anything still missing after the region walk
/// is rare (delisted everywhere); do not hammer the appdetails endpoint.
const APP_DETAILS_LIMIT: usize = 50;

/// Positive art is refreshed this often (store art and its `?t=` change rarely).
const ART_TTL_SECS: i64 = 30 * 24 * 60 * 60;
/// Ids every region and appdetails answered "no" for are retried this often.
const MISS_TTL_SECS: i64 = 7 * 24 * 60 * 60;
/// Ids a pass got no answer for (failed, timed out, not reached) wait this long.
const NO_ANSWER_RETRY_SECS: i64 = 30 * 60;
/// Wall-clock budget for one pass; a hung store must not stall the refresh arm.
/// Whatever the pass learned before running out is still kept.
const REFRESH_BUDGET: Duration = Duration::from_secs(60);

/// Budget for the proxy's own lookup while a viewer waits for the picture.
const PROXY_LOOKUP_BUDGET: Duration = Duration::from_secs(8);
/// How long the proxy waits for a lookup someone else is already running.
const PROXY_WAIT: Duration = Duration::from_secs(10);
/// The proxy does not ask again for an appid the US store answered "no" for.
const PROXY_MISS_SECS: i64 = 6 * 60 * 60;
/// Nor, for a while, for one whose lookup failed.
const PROXY_FAILED_SECS: i64 = 5 * 60;
/// After any failed proxy lookup the proxy stops asking the store at all for this long.
const STORE_DOWN_SECS: i64 = 60;
/// Past this many cached appids the proxy stops adding the games it finds, so
/// enumerating real appids cannot grow the cache file without end. Platform
/// refreshes are not capped (a whole library is far below this).
const PROXY_LOOKUP_CAP: usize = 20_000;
/// Entries per in-memory hold table.
const HOLDS_CAP: usize = 20_000;

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

/// Appid → unix time before which it is not asked again. Memory only, bounded:
/// when full, expired entries go first, then everything.
#[derive(Default)]
struct Holds {
    until: HashMap<i64, i64>,
}

impl Holds {
    fn held(&self, appid: i64, now: i64) -> bool {
        self.until.get(&appid).is_some_and(|until| *until > now)
    }

    fn hold(&mut self, appid: i64, until: i64, now: i64) {
        if self.until.len() >= HOLDS_CAP && !self.until.contains_key(&appid) {
            self.until.retain(|_, until| *until > now);
            if self.until.len() >= HOLDS_CAP {
                self.until.clear();
            }
        }
        self.until.insert(appid, until);
    }

    fn clear(&mut self) {
        self.until.clear();
    }
}

/// `std::sync::Mutex`es below: short critical sections only, never held across
/// `.await` or a disk write. Lock order: cache, then in-flight, then holds.
static CACHE: OnceLock<Mutex<HashMap<i64, SteamArt>>> = OnceLock::new();
/// Appids some lookup is resolving right now, so a second caller does not ask again.
static IN_FLIGHT: Lazy<Mutex<HashSet<i64>>> = Lazy::new(|| Mutex::new(HashSet::new()));
/// Woken whenever a lookup lets go of its appids.
static RELEASED: Lazy<tokio::sync::Notify> = Lazy::new(tokio::sync::Notify::new);
/// Refresh path: ids a pass got no answer for.
static RETRY_HOLDS: Lazy<Mutex<Holds>> = Lazy::new(|| Mutex::new(Holds::default()));
/// Proxy path: ids the US store said "no" for, or whose lookup failed.
static PROXY_HOLDS: Lazy<Mutex<Holds>> = Lazy::new(|| Mutex::new(Holds::default()));
/// Proxy path: no store lookups before this unix time.
static STORE_DOWN_UNTIL: AtomicI64 = AtomicI64::new(0);
/// Bumped under the cache lock on every change.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Serialises disk writes and holds the generation last written: a change that
/// a later write already carried does not write again.
static WRITE_LOCK: tokio::sync::Mutex<u64> = tokio::sync::Mutex::const_new(0);

fn lock<T>(mutex: &'static Mutex<T>) -> MutexGuard<'static, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_cache() -> MutexGuard<'static, HashMap<i64, SteamArt>> {
    lock(CACHE.get_or_init(|| Mutex::new(load_from_disk())))
}

fn asset_url(path: &str) -> String {
    format!("{CDN_BASE}/store_item_assets/{path}")
}

/// 460×215 header cover; falls back to the legacy unhashed path when unresolved.
pub fn cover_url(appid: i64) -> String {
    if let Some(path) = art(appid).and_then(|a| a.header) {
        return asset_url(&path);
    }
    format!("{LEGACY_CDN_BASE}/steam/apps/{appid}/header.jpg")
}

/// 600×900 portrait cover; falls back to the legacy unhashed path when unresolved.
pub fn library_cover_url(appid: i64) -> String {
    if let Some(path) = art(appid).and_then(|a| a.library) {
        return asset_url(&path);
    }
    format!("{LEGACY_CDN_BASE}/steam/apps/{appid}/library_600x900.jpg")
}

/// Appid and file name of an old-path cover (`…/steam/apps/{appid}/{file}`);
/// already hashed paths are not matched.
pub fn legacy_app_art(url: &str) -> Option<(i64, String)> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if !myriad_image_proxy::host_matches_domain(host, "steamstatic.com") {
        return None;
    }
    let rest: Vec<&str> = parsed.path_segments()?.collect();
    // `steam/apps/{appid}/{file}`, behind `store_item_assets/` on some CDNs.
    let start = rest.iter().position(|segment| *segment == "steam")?;
    match &rest[start..] {
        ["steam", "apps", appid, file] if file.ends_with(".jpg") => {
            Some((appid.parse().ok()?, (*file).to_string()))
        }
        _ => None,
    }
}

/// The resolved path for one old file name; only the two we keep are known.
fn resolved_path(art: &SteamArt, file: &str) -> Option<String> {
    match file {
        "header.jpg" => art.header.clone(),
        "library_600x900.jpg" => art.library.clone(),
        _ => None,
    }
}

fn knows_file(file: &str) -> bool {
    matches!(file, "header.jpg" | "library_600x900.jpg")
}

/// What the cache already says about one old-path file: `Some(Some(url))` a hit,
/// `Some(None)` settled (the store has no such picture, recently checked),
/// `None` worth asking.
fn cached_legacy(
    cache: &HashMap<i64, SteamArt>,
    appid: i64,
    file: &str,
    now: i64,
) -> Option<Option<String>> {
    let art = cache.get(&appid)?;
    if let Some(path) = resolved_path(art, file) {
        return Some(Some(asset_url(&path)));
    }
    (!needs_resolution(Some(art), now)).then_some(None)
}

fn read_legacy(appid: i64, file: &str) -> Option<String> {
    art(appid)
        .and_then(|art| resolved_path(&art, file))
        .map(|path| asset_url(&path))
}

/// For the image proxy when an old-path cover 404s: the cached real address,
/// else one US store lookup for this appid. If a lookup for it is already
/// running (a refresh, the presence, another viewer), wait for that one instead.
pub async fn resolve_legacy(appid: i64, file: &str) -> Option<String> {
    if appid <= 0 || !knows_file(file) {
        return None;
    }
    let now = now_unix();
    if let Some(settled) = cached_legacy(&lock_cache(), appid, file, now) {
        return settled;
    }
    if lock(&PROXY_HOLDS).held(appid, now) || STORE_DOWN_UNTIL.load(Ordering::Relaxed) > now {
        return None;
    }
    let Some(claim) = claim_one(appid) else {
        wait_released(appid, PROXY_WAIT).await;
        return read_legacy(appid, file);
    };

    let client = crate::services::http_client::get_global_client().await;
    let answer =
        tokio::time::timeout(PROXY_LOOKUP_BUDGET, fetch_items(&client, &[appid], "US")).await;
    let now = now_unix();
    let art = match answer {
        Ok(Ok(mut items)) => match items.remove(&appid) {
            Some(art) => art,
            None => {
                lock(&PROXY_HOLDS).hold(appid, now + PROXY_MISS_SECS, now);
                return None;
            }
        },
        Ok(Err(e)) => {
            tracing::debug!("Steam store items for proxy ({appid}) failed: {e}");
            proxy_lookup_failed(appid, now);
            return None;
        }
        Err(_) => {
            proxy_lookup_failed(appid, now);
            return None;
        }
    };
    let url = resolved_path(&art, file).map(|path| asset_url(&path));
    remember_proxy_art(appid, art);
    drop(claim);
    url
}

fn proxy_lookup_failed(appid: i64, now: i64) {
    lock(&PROXY_HOLDS).hold(appid, now + PROXY_FAILED_SECS, now);
    STORE_DOWN_UNTIL.store(now + STORE_DOWN_SECS, Ordering::Relaxed);
}

/// A game the proxy found goes into the cache like a refresh result, while the
/// cache is under its cap.
fn remember_proxy_art(appid: i64, art: SteamArt) {
    let generation = {
        let mut cache = lock_cache();
        if cache.len() >= PROXY_LOOKUP_CAP && !cache.contains_key(&appid) {
            return;
        }
        cache.insert(appid, art);
        GENERATION.fetch_add(1, Ordering::Relaxed) + 1
    };
    schedule_persist(generation);
}

/// Wait (at most `limit`) until no lookup holds `appid`.
async fn wait_released(appid: i64, limit: Duration) {
    let _ = tokio::time::timeout(limit, async {
        loop {
            let mut notified = std::pin::pin!(RELEASED.notified());
            notified.as_mut().enable();
            let busy = lock(&IN_FLIGHT).contains(&appid);
            if !busy {
                return;
            }
            notified.await;
        }
    })
    .await;
}

/// Resolved art for one appid, if a refresh has seen it.
pub fn art(appid: i64) -> Option<SteamArt> {
    lock_cache().get(&appid).cloned()
}

/// Forget everything, in memory and on disk (the "clear all caches" action).
pub fn forget_all() {
    let generation = {
        let mut cache = lock_cache();
        cache.clear();
        GENERATION.fetch_add(1, Ordering::Relaxed) + 1
    };
    lock(&RETRY_HOLDS).clear();
    lock(&PROXY_HOLDS).clear();
    STORE_DOWN_UNTIL.store(0, Ordering::Relaxed);
    // Queued behind any write already running, so an older copy cannot come back.
    schedule_persist(generation);
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

/// Write the cache in its own task, so a caller that goes away (a viewer closing
/// the page) cannot cut a write short while it holds the write lock.
fn schedule_persist(generation: u64) {
    tokio::spawn(persist_latest(generation));
}

/// Write the cache as it is now, unless a write since `generation` already did.
/// The snapshot is copied under the cache lock; the write itself runs on the
/// blocking pool, so cover reads on the request path never wait on the disk.
async fn persist_latest(generation: u64) {
    let mut written = WRITE_LOCK.lock().await;
    if generation <= *written {
        return;
    }
    let (snapshot, latest) = {
        let cache = lock_cache();
        // Changes bump the generation under this lock, so `latest` is exactly this copy.
        (cache.clone(), GENERATION.load(Ordering::Relaxed))
    };
    match tokio::task::spawn_blocking(move || persist(snapshot)).await {
        Ok(true) => *written = latest,
        Ok(false) => {}
        Err(e) => tracing::warn!("Steam cover cache write task failed: {e}"),
    }
}

/// Blocking file write; `true` once the file holds `items` (an empty cache is
/// no file at all).
fn persist(items: HashMap<i64, SteamArt>) -> bool {
    let path = cache_file();
    if items.is_empty() {
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => {
                tracing::warn!("Cannot remove {}: {e}", path.display());
                false
            }
        };
    }
    let Some(dir) = path.parent() else {
        return false;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!("Cannot create {}: {e}", dir.display());
        return false;
    }
    let file = SteamAssetFile {
        version: 1,
        items: items.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    };
    let json = match serde_json::to_vec_pretty(&file) {
        Ok(json) => json,
        Err(e) => {
            tracing::warn!("Cannot serialize steam assets: {e}");
            return false;
        }
    };
    // Same-directory tmp + rename: a crash mid-write must not truncate a good cache.
    let temp = dir.join(format!(
        "steam_assets.{}.json.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let written = std::fs::write(&temp, &json)
        .and_then(|()| crate::services::platform_refresh::commit_cache_temp_file(&temp, &path));
    match written {
        Ok(()) => true,
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            tracing::warn!("Cannot write {}: {e}", path.display());
            false
        }
    }
}

/// Resolve art for `appids`, merge into the cache, and back off failures.
///
/// Ids resolved within their TTL are skipped: positive art is refreshed monthly
/// (store art and its `?t=` change rarely), ids the store said "no" for are
/// retried weekly, ids a pass got no answer for wait half an hour (in memory).
/// Ids another lookup is already resolving are skipped too.
pub async fn refresh_steam_assets(client: &reqwest::Client, appids: &[i64]) {
    if let Some(claim) = claim_pending(appids, now_unix()) {
        run_claimed(client, claim, REFRESH_BUDGET).await;
    }
}

/// Same as [`refresh_steam_assets`], in the background: for callers that must
/// not wait on the store (reading the presence). Nothing is spawned when every
/// id is fresh, held back, or already being resolved.
pub fn spawn_refresh(client: reqwest::Client, appids: &[i64]) {
    if let Some(claim) = claim_pending(appids, now_unix()) {
        tokio::spawn(async move {
            run_claimed(&client, claim, REFRESH_BUDGET).await;
        });
    }
}

/// Ids one lookup has taken; released when it ends, however it ends.
struct Claim {
    ids: Vec<i64>,
}

impl Drop for Claim {
    fn drop(&mut self) {
        {
            let mut in_flight = lock(&IN_FLIGHT);
            for id in &self.ids {
                in_flight.remove(id);
            }
        }
        RELEASED.notify_waiters();
    }
}

/// Take the ids that need resolving, are not held back, and nobody is resolving yet.
fn claim_pending(appids: &[i64], now: i64) -> Option<Claim> {
    let cache = lock_cache();
    let mut in_flight = lock(&IN_FLIGHT);
    let holds = lock(&RETRY_HOLDS);
    let ids: Vec<i64> = appids
        .iter()
        .copied()
        .filter(|id| *id > 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|id| {
            needs_resolution(cache.get(id), now) && !holds.held(*id, now) && !in_flight.contains(id)
        })
        .collect();
    if ids.is_empty() {
        return None;
    }
    in_flight.extend(ids.iter().copied());
    Some(Claim { ids })
}

/// Take one appid for the proxy's own lookup, unless someone has it.
fn claim_one(appid: i64) -> Option<Claim> {
    lock(&IN_FLIGHT)
        .insert(appid)
        .then(|| Claim { ids: vec![appid] })
}

/// The claim is held until the pass is merged, so the next caller sees the result.
async fn run_claimed(client: &reqwest::Client, claim: Claim, budget: Duration) {
    let mut state = PassState::default();
    let finished = tokio::time::timeout(budget, resolve_pass(client, &claim.ids, &mut state))
        .await
        .is_ok();
    if !finished {
        tracing::warn!(
            "Steam cover resolution exceeded {}s for {} appids; keeping what it found",
            budget.as_secs(),
            claim.ids.len()
        );
    }
    let now = now_unix();
    let (merged, generation) = {
        let mut cache = lock_cache();
        let merged = merge_pass(&mut cache, &mut lock(&RETRY_HOLDS), &claim.ids, state, now);
        let changed = merged.resolved + merged.not_on_store > 0;
        (
            merged,
            changed.then(|| GENERATION.fetch_add(1, Ordering::Relaxed) + 1),
        )
    };
    if let Some(generation) = generation {
        schedule_persist(generation);
    }
    tracing::info!(
        "✓ Steam covers resolved: {}, not on the store: {}, no answer (retry later): {}",
        merged.resolved,
        merged.not_on_store,
        merged.no_answer
    );
    drop(claim);
}

/// Skip ids already resolved recently; retry "not on the store" on a slower clock.
fn needs_resolution(art: Option<&SteamArt>, now: i64) -> bool {
    match art {
        None => true,
        Some(art) if art.header.is_none() => now - art.updated_at >= MISS_TTL_SECS,
        Some(art) => now - art.updated_at >= ART_TTL_SECS,
    }
}

/// What one pass learned, filled in as it goes so a pass cut short by its
/// budget still keeps what it found.
#[derive(Default)]
struct PassState {
    found: HashMap<i64, SteamArt>,
    /// Ids some request failed for: no verdict, whatever else answered.
    failed: HashSet<i64>,
    /// Every region answered without them and appdetails said "no".
    not_on_store: Vec<i64>,
}

struct Merged {
    resolved: usize,
    not_on_store: usize,
    no_answer: usize,
}

/// Fold one pass into the cache. Only real "no" answers are written down (and
/// never over art already known); ids without an answer are held back in memory.
fn merge_pass(
    cache: &mut HashMap<i64, SteamArt>,
    holds: &mut Holds,
    pending: &[i64],
    state: PassState,
    now: i64,
) -> Merged {
    let PassState {
        found,
        not_on_store,
        ..
    } = state;
    let mut no_answer = 0usize;
    for appid in pending {
        if !found.contains_key(appid) && !not_on_store.contains(appid) {
            holds.hold(*appid, now + NO_ANSWER_RETRY_SECS, now);
            no_answer += 1;
        }
    }
    let resolved = found.len();
    cache.extend(found);
    let mut written_off = 0usize;
    for appid in not_on_store {
        if !cache.get(&appid).is_some_and(|art| art.header.is_some()) {
            cache.insert(
                appid,
                SteamArt {
                    header: None,
                    library: None,
                    updated_at: now,
                },
            );
            written_off += 1;
        }
    }
    Merged {
        resolved,
        not_on_store: written_off,
        no_answer,
    }
}

/// Walk the region priority, then appdetails for what every region answered
/// without. Ids some request failed for never reach appdetails.
async fn resolve_pass(client: &reqwest::Client, pending: &[i64], state: &mut PassState) {
    let mut missing = pending.to_vec();
    for cc in REGION_PRIORITY {
        if missing.is_empty() {
            break;
        }
        missing = fetch_region(client, &missing, cc, state).await;
    }

    // appdetails costs one call per game; only the first `APP_DETAILS_LIMIT`
    // leftovers get a try, the rest get no verdict and wait for a later pass.
    let candidates: Vec<i64> = missing
        .into_iter()
        .filter(|appid| !state.failed.contains(appid))
        .take(APP_DETAILS_LIMIT)
        .collect();
    for appid in candidates {
        match fetch_appdetails_cover(client, appid).await {
            Ok(Some(path)) => {
                state.found.insert(
                    appid,
                    SteamArt {
                        header: Some(path),
                        library: None,
                        updated_at: now_unix(),
                    },
                );
            }
            Ok(None) => state.not_on_store.push(appid),
            Err(e) => {
                tracing::debug!("Steam appdetails ({appid}) failed: {e}");
                state.failed.insert(appid);
            }
        }
    }
}

/// One region pass. Returns the appids still without art (answered without, or failed).
async fn fetch_region(
    client: &reqwest::Client,
    appids: &[i64],
    country: &str,
    state: &mut PassState,
) -> Vec<i64> {
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

    let mut missing = Vec::new();
    for (chunk, response) in chunks.iter().zip(responses) {
        if let Err(e) = &response {
            tracing::warn!(
                "Steam store items ({country}, {} appids) failed: {e}",
                chunk.len()
            );
        }
        record_chunk(state, chunk, response, &mut missing);
    }
    missing
}

/// Sort one chunk's answer: found, answered without, or failed (no verdict).
fn record_chunk(
    state: &mut PassState,
    chunk: &[i64],
    response: Result<HashMap<i64, SteamArt>, String>,
    missing: &mut Vec<i64>,
) {
    match response {
        Ok(mut items) => {
            for appid in chunk {
                match items.remove(appid) {
                    Some(art) => {
                        state.found.insert(*appid, art);
                    }
                    None => missing.push(*appid),
                }
            }
        }
        Err(_) => {
            state.failed.extend(chunk.iter().copied());
            missing.extend_from_slice(chunk);
        }
    }
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
        .timeout(STORE_REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    let body =
        crate::services::outbound_security::read_limited_body(response, STORE_BODY_LIMIT).await?;
    let body: Value = serde_json::from_slice(&body).map_err(|e| format!("json: {e}"))?;
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
/// `Ok(None)`: appdetails answered and has no usable header; `Err`: no answer.
async fn fetch_appdetails_cover(
    client: &reqwest::Client,
    appid: i64,
) -> Result<Option<String>, String> {
    let mut url = reqwest::Url::parse(APP_DETAILS_URL).map_err(|e| format!("url: {e}"))?;
    url.query_pairs_mut()
        .append_pair("appids", &appid.to_string())
        .append_pair("cc", "US");
    let response = client
        .get(url)
        .timeout(APP_DETAILS_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("status {}", response.status()));
    }
    let body =
        crate::services::outbound_security::read_limited_body(response, STORE_BODY_LIMIT).await?;
    let body: Value = serde_json::from_slice(&body).map_err(|e| format!("json: {e}"))?;
    let entry = body
        .get(appid.to_string())
        .ok_or_else(|| "appid missing from the answer".to_string())?;
    Ok(appdetails_header(entry))
}

/// The header path inside one appdetails entry, if the store has one.
fn appdetails_header(entry: &Value) -> Option<String> {
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
        // Already hashed paths, avatars and other sites are left alone.
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
    fn legacy_files_map_to_the_two_kept_paths() {
        let art = SteamArt {
            header: Some("steam/apps/1/a/header.jpg?t=1".into()),
            library: Some("steam/apps/1/b/library_600x900.jpg?t=1".into()),
            updated_at: 0,
        };
        assert_eq!(
            resolved_path(&art, "header.jpg")
                .map(|path| asset_url(&path))
                .as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1/a/header.jpg?t=1"
            )
        );
        assert!(
            resolved_path(&art, "library_600x900.jpg")
                .unwrap()
                .contains("/b/")
        );
        assert!(!knows_file("page_bg.jpg"));
        assert_eq!(resolved_path(&art, "page_bg.jpg"), None);
    }

    #[test]
    fn one_lookup_per_appid_at_a_time() {
        // Ids no real cache holds, so they always need resolving.
        let now = 1_800_000_000;
        let first = claim_pending(&[4_000_000_001, 4_000_000_002], now).expect("claims both");
        assert_eq!(first.ids, vec![4_000_000_001, 4_000_000_002]);
        // Overlapping caller only gets what nobody is resolving.
        let second = claim_pending(&[4_000_000_002, 4_000_000_003], now).expect("claims one");
        assert_eq!(second.ids, vec![4_000_000_003]);
        assert!(claim_pending(&[4_000_000_001], now).is_none());
        drop(first);
        assert!(claim_pending(&[4_000_000_001], now).is_some());
        drop(second);
        assert!(claim_pending(&[0, -5], now).is_none());

        // The proxy's own claim and a refresh claim exclude each other.
        let proxy = claim_one(4_000_000_004).expect("free");
        assert!(claim_one(4_000_000_004).is_none());
        assert!(claim_pending(&[4_000_000_004], now).is_none());
        drop(proxy);
        assert!(claim_one(4_000_000_004).is_some());
    }

    #[tokio::test]
    async fn the_proxy_waits_for_a_running_lookup() {
        let claim = claim_one(4_000_000_010).expect("free");
        let waiter = tokio::spawn(wait_released(4_000_000_010, Duration::from_secs(30)));
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        drop(claim);
        tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .expect("released before the limit")
            .unwrap();
        // Nobody holds it: returns at once.
        tokio::time::timeout(
            Duration::from_secs(1),
            wait_released(4_000_000_010, Duration::from_secs(30)),
        )
        .await
        .expect("not held");
    }

    fn header_art(path: &str, updated_at: i64) -> SteamArt {
        SteamArt {
            header: Some(path.into()),
            library: None,
            updated_at,
        }
    }

    #[test]
    fn only_real_no_answers_are_written_off() {
        let now = 1_800_000_000;
        let mut state = PassState::default();
        let mut missing = Vec::new();
        // US: first chunk answered (1 found, 2 and 3 absent), second chunk failed.
        let mut answered = HashMap::new();
        answered.insert(1, header_art("a/header.jpg", now));
        record_chunk(&mut state, &[1, 2, 3], Ok(answered), &mut missing);
        record_chunk(&mut state, &[4, 5], Err("status 429".into()), &mut missing);
        assert_eq!(missing, vec![2, 3, 4, 5]);
        assert_eq!(state.failed, HashSet::from([4, 5]));
        // appdetails said "no" for 2 only; 3 was never asked (over the limit).
        state.not_on_store.push(2);

        let mut cache = HashMap::new();
        // 5 already has art from an earlier pass.
        cache.insert(5, header_art("old/header.jpg", now - 10));
        let mut holds = Holds::default();
        let merged = merge_pass(&mut cache, &mut holds, &[1, 2, 3, 4, 5], state, now);

        assert_eq!(merged.resolved, 1);
        assert_eq!(merged.not_on_store, 1);
        assert_eq!(merged.no_answer, 3);
        assert_eq!(cache[&1].header.as_deref(), Some("a/header.jpg"));
        assert_eq!(cache[&2].header, None);
        // Failed and untried ids are not written down, only held back in memory.
        assert!(!cache.contains_key(&3));
        assert!(!cache.contains_key(&4));
        assert_eq!(cache[&5].header.as_deref(), Some("old/header.jpg"));
        for appid in [3, 4, 5] {
            assert!(holds.held(appid, now));
            assert!(!holds.held(appid, now + NO_ANSWER_RETRY_SECS));
        }
        assert!(!holds.held(1, now) && !holds.held(2, now));
    }

    #[test]
    fn a_no_never_replaces_known_art() {
        let now = 1_800_000_000;
        let mut cache = HashMap::from([(7, header_art("old/header.jpg", now - 10))]);
        let state = PassState {
            not_on_store: vec![7],
            ..PassState::default()
        };
        let merged = merge_pass(&mut cache, &mut Holds::default(), &[7], state, now);
        assert_eq!(merged.not_on_store, 0);
        assert_eq!(cache[&7].header.as_deref(), Some("old/header.jpg"));
    }

    #[test]
    fn holds_expire_and_stay_bounded() {
        let now = 1_800_000_000;
        let mut holds = Holds::default();
        holds.hold(1, now + 60, now);
        assert!(holds.held(1, now + 59));
        assert!(!holds.held(1, now + 60));
        // Full of expired entries: they are dropped to make room.
        let mut full = Holds::default();
        for appid in 0..HOLDS_CAP as i64 {
            full.hold(appid + 10, now - 1, now - 100);
        }
        full.hold(1, now + 60, now);
        assert_eq!(full.until.len(), 1);
        // Full of live entries: cleared rather than grown.
        let mut live = Holds::default();
        for appid in 0..HOLDS_CAP as i64 {
            live.hold(appid + 10, now + 60, now);
        }
        live.hold(1, now + 60, now);
        assert!(live.until.len() <= HOLDS_CAP);
        assert!(live.held(1, now));
    }

    #[test]
    fn the_proxy_trusts_settled_cache_answers() {
        let now = 1_800_000_000;
        let mut cache = HashMap::new();
        cache.insert(1, header_art("steam/apps/1/a/header.jpg", now - 60));
        cache.insert(
            2,
            SteamArt {
                header: None,
                library: None,
                updated_at: now - 60,
            },
        );
        cache.insert(
            3,
            SteamArt {
                header: None,
                library: None,
                updated_at: now - MISS_TTL_SECS - 1,
            },
        );
        assert_eq!(
            cached_legacy(&cache, 1, "header.jpg", now),
            Some(Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1/a/header.jpg"
                    .to_string()
            ))
        );
        // Fresh header, no portrait: settled, do not ask.
        assert_eq!(
            cached_legacy(&cache, 1, "library_600x900.jpg", now),
            Some(None)
        );
        // Recent "not on the store": settled.
        assert_eq!(cached_legacy(&cache, 2, "header.jpg", now), Some(None));
        // Old "not on the store" or unknown: worth asking.
        assert_eq!(cached_legacy(&cache, 3, "header.jpg", now), None);
        assert_eq!(cached_legacy(&cache, 4, "header.jpg", now), None);
    }

    #[test]
    fn appdetails_header_needs_success_and_a_store_path() {
        assert_eq!(appdetails_header(&json!({ "success": false })), None);
        assert_eq!(
            appdetails_header(&json!({
                "success": true,
                "data": { "header_image": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/9/x/header.jpg?t=1" }
            }))
            .as_deref(),
            Some("steam/apps/9/x/header.jpg?t=1")
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
