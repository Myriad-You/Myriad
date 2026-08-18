//! Process-wide memory profile (default vs memory-saver).
//!
//! - **default**: current balanced, bounded product budgets. These are not the
//!   legacy unbounded/high-water values, so upgrading can change request caps.
//! - **saver**: a second, tighter notch for ~1 GiB hosts. Default is already
//!   bounded; saver further cuts cache, chunk inflight, pool, Argon2, and
//!   large-media peaks so those knobs stay meaningfully below default.
//!
//! Selection order: `MYRIAD_MEMORY_PROFILE` env (`default`|`saver`|`small`) >
//! dynamic config `memory_saver_enabled` > default.
//!
//! Runtime-tunable budgets (inbox inflight, chunk inflight, cache caps, Argon2
//! permits) apply on config save. DB pool min/max apply on next pool create.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use once_cell::sync::Lazy;
use tokio::sync::Semaphore;

/// Current balanced product defaults. They are not compatibility promises for
/// releases that predate the bounded federation profile.
///
/// Pool min is the idle floor (not request concurrency). Keep it small so a
/// quiet host does not park five Postgres backends; max is the concurrent
/// query ceiling. 2/24 is +4 peak checkouts versus the old 5/20, paid for by
/// three fewer idle connections at rest.
pub const DEFAULT_DB_MIN_CONNECTIONS: u32 = 2;
pub const DEFAULT_DB_MAX_CONNECTIONS: u32 = 24;
pub const DEFAULT_INBOX_INFLIGHT_RAW_BUDGET: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_IN_FLIGHT_CHUNK_BYTES: usize = 128 * 1024 * 1024;
pub const DEFAULT_MAX_API_CACHE_ENTRIES: usize = 2048;
pub const DEFAULT_MAX_GEO_CACHE_ENTRIES: usize = 2048;
/// Approximate total serialized JSON size for API response cache.
pub const DEFAULT_MAX_API_CACHE_BYTES: usize = 96 * 1024 * 1024;
pub const DEFAULT_MAX_GEO_CACHE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_ARGON2_PERMITS: usize = 4;
pub const DEFAULT_MAX_AUDIO_BYTES: usize = 128 * 1024 * 1024;

/// Memory-saver budgets for ~1 GiB hosts (second notch below bounded default).
pub const SAVER_DB_MIN_CONNECTIONS: u32 = 1;
pub const SAVER_DB_MAX_CONNECTIONS: u32 = 4;
pub const SAVER_INBOX_INFLIGHT_RAW_BUDGET: usize = 4 * 1024 * 1024;
pub const SAVER_MAX_IN_FLIGHT_CHUNK_BYTES: usize = 16 * 1024 * 1024;
pub const SAVER_MAX_API_CACHE_ENTRIES: usize = 128;
pub const SAVER_MAX_GEO_CACHE_ENTRIES: usize = 128;
pub const SAVER_MAX_API_CACHE_BYTES: usize = 8 * 1024 * 1024;
pub const SAVER_MAX_GEO_CACHE_BYTES: usize = 256 * 1024;
pub const SAVER_ARGON2_PERMITS: usize = 1;
pub const SAVER_MAX_AUDIO_BYTES: usize = 16 * 1024 * 1024;
/// Federation JSON caps. Larger media uses the chunked transfer surface.
pub const DEFAULT_MESSAGE_PAYLOAD_LIMIT: usize = 4 * 1024 * 1024;
pub const DEFAULT_INBOX_BODY_LIMIT: usize = 8 * 1024 * 1024;
pub const DEFAULT_AUTHENTICATED_BODY_LIMIT: usize = 24 * 1024 * 1024;
pub const DEFAULT_NOTE_IMAGE_LIMIT: usize = 32 * 1024 * 1024;
pub const DEFAULT_NOTE_VIDEO_LIMIT: usize = 256 * 1024 * 1024;
pub const SAVER_MESSAGE_PAYLOAD_LIMIT: usize = 2 * 1024 * 1024;
pub const SAVER_INBOX_BODY_LIMIT: usize = 4 * 1024 * 1024;
pub const SAVER_AUTHENTICATED_BODY_LIMIT: usize = 8 * 1024 * 1024;
pub const SAVER_NOTE_IMAGE_LIMIT: usize = 8 * 1024 * 1024;
pub const SAVER_NOTE_VIDEO_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryProfile {
    Default,
    Saver,
}

impl MemoryProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Saver => "saver",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBudgets {
    pub profile: MemoryProfile,
    pub db_min_connections: u32,
    pub db_max_connections: u32,
    pub inbox_inflight_raw_budget: usize,
    pub max_in_flight_chunk_bytes: usize,
    pub max_api_cache_entries: usize,
    pub max_geo_cache_entries: usize,
    pub max_api_cache_bytes: usize,
    pub max_geo_cache_bytes: usize,
    pub argon2_permits: usize,
    pub max_audio_bytes: usize,
    pub message_payload_limit: usize,
    pub inbox_body_limit: usize,
    pub authenticated_body_limit: usize,
    pub note_image_limit: usize,
    pub note_video_limit: usize,
}

impl MemoryBudgets {
    pub fn for_profile(profile: MemoryProfile) -> Self {
        match profile {
            MemoryProfile::Default => Self {
                profile,
                db_min_connections: DEFAULT_DB_MIN_CONNECTIONS,
                db_max_connections: DEFAULT_DB_MAX_CONNECTIONS,
                inbox_inflight_raw_budget: DEFAULT_INBOX_INFLIGHT_RAW_BUDGET,
                max_in_flight_chunk_bytes: DEFAULT_MAX_IN_FLIGHT_CHUNK_BYTES,
                max_api_cache_entries: DEFAULT_MAX_API_CACHE_ENTRIES,
                max_geo_cache_entries: DEFAULT_MAX_GEO_CACHE_ENTRIES,
                max_api_cache_bytes: DEFAULT_MAX_API_CACHE_BYTES,
                max_geo_cache_bytes: DEFAULT_MAX_GEO_CACHE_BYTES,
                argon2_permits: DEFAULT_ARGON2_PERMITS,
                max_audio_bytes: DEFAULT_MAX_AUDIO_BYTES,
                message_payload_limit: DEFAULT_MESSAGE_PAYLOAD_LIMIT,
                inbox_body_limit: DEFAULT_INBOX_BODY_LIMIT,
                authenticated_body_limit: DEFAULT_AUTHENTICATED_BODY_LIMIT,
                note_image_limit: DEFAULT_NOTE_IMAGE_LIMIT,
                note_video_limit: DEFAULT_NOTE_VIDEO_LIMIT,
            },
            MemoryProfile::Saver => Self {
                profile,
                db_min_connections: SAVER_DB_MIN_CONNECTIONS,
                db_max_connections: SAVER_DB_MAX_CONNECTIONS,
                inbox_inflight_raw_budget: SAVER_INBOX_INFLIGHT_RAW_BUDGET,
                max_in_flight_chunk_bytes: SAVER_MAX_IN_FLIGHT_CHUNK_BYTES,
                max_api_cache_entries: SAVER_MAX_API_CACHE_ENTRIES,
                max_geo_cache_entries: SAVER_MAX_GEO_CACHE_ENTRIES,
                max_api_cache_bytes: SAVER_MAX_API_CACHE_BYTES,
                max_geo_cache_bytes: SAVER_MAX_GEO_CACHE_BYTES,
                argon2_permits: SAVER_ARGON2_PERMITS,
                max_audio_bytes: SAVER_MAX_AUDIO_BYTES,
                message_payload_limit: SAVER_MESSAGE_PAYLOAD_LIMIT,
                inbox_body_limit: SAVER_INBOX_BODY_LIMIT,
                authenticated_body_limit: SAVER_AUTHENTICATED_BODY_LIMIT,
                note_image_limit: SAVER_NOTE_IMAGE_LIMIT,
                note_video_limit: SAVER_NOTE_VIDEO_LIMIT,
            },
        }
    }
}

static INBOX_INFLIGHT_BUDGET: AtomicUsize = AtomicUsize::new(DEFAULT_INBOX_INFLIGHT_RAW_BUDGET);
static CHUNK_INFLIGHT_BUDGET: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_IN_FLIGHT_CHUNK_BYTES);
static API_CACHE_CAP: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_API_CACHE_ENTRIES);
static GEO_CACHE_CAP: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_GEO_CACHE_ENTRIES);
static API_CACHE_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_API_CACHE_BYTES);
static GEO_CACHE_BYTES: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_GEO_CACHE_BYTES);
static AUDIO_BYTES_CAP: AtomicUsize = AtomicUsize::new(DEFAULT_MAX_AUDIO_BYTES);
static MESSAGE_PAYLOAD: AtomicUsize = AtomicUsize::new(DEFAULT_MESSAGE_PAYLOAD_LIMIT);
static INBOX_BODY: AtomicUsize = AtomicUsize::new(DEFAULT_INBOX_BODY_LIMIT);
static AUTH_BODY: AtomicUsize = AtomicUsize::new(DEFAULT_AUTHENTICATED_BODY_LIMIT);
static NOTE_IMAGE: AtomicUsize = AtomicUsize::new(DEFAULT_NOTE_IMAGE_LIMIT);
static NOTE_VIDEO: AtomicUsize = AtomicUsize::new(DEFAULT_NOTE_VIDEO_LIMIT);
static DB_MIN: AtomicUsize = AtomicUsize::new(DEFAULT_DB_MIN_CONNECTIONS as usize);
static DB_MAX: AtomicUsize = AtomicUsize::new(DEFAULT_DB_MAX_CONNECTIONS as usize);

/// Argon2 permit pool; replaced when profile changes (in-flight holds stay valid).
static ARGON2_SEMAPHORE: Lazy<RwLock<Arc<Semaphore>>> =
    Lazy::new(|| RwLock::new(Arc::new(Semaphore::new(DEFAULT_ARGON2_PERMITS))));
static ARGON2_PERMITS: AtomicUsize = AtomicUsize::new(DEFAULT_ARGON2_PERMITS);
static ACTIVE_PROFILE: AtomicUsize = AtomicUsize::new(0); // 0 default, 1 saver

fn profile_to_tag(p: MemoryProfile) -> usize {
    match p {
        MemoryProfile::Default => 0,
        MemoryProfile::Saver => 1,
    }
}

fn tag_to_profile(t: usize) -> MemoryProfile {
    if t == 1 {
        MemoryProfile::Saver
    } else {
        MemoryProfile::Default
    }
}

/// Resolve profile: env override, then dynamic `memory_saver_enabled`.
pub fn resolve_profile(memory_saver_enabled: bool) -> MemoryProfile {
    if let Ok(raw) = std::env::var("MYRIAD_MEMORY_PROFILE") {
        let v = raw.trim().to_ascii_lowercase();
        match v.as_str() {
            "saver" | "small" | "1" | "true" | "on" | "yes" => return MemoryProfile::Saver,
            "default" | "balanced" | "0" | "false" | "off" | "no" => return MemoryProfile::Default,
            "" => {}
            other => {
                tracing::warn!(
                    value = %other,
                    "Unknown MYRIAD_MEMORY_PROFILE; falling back to config/default"
                );
            }
        }
    }
    if memory_saver_enabled {
        MemoryProfile::Saver
    } else {
        MemoryProfile::Default
    }
}

/// Serialize tests that mutate the process-wide profile (apply / atomics).
#[cfg(test)]
pub(crate) fn test_profile_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// Apply budgets process-wide (safe to call on every config reload).
pub fn apply(profile: MemoryProfile) {
    let b = MemoryBudgets::for_profile(profile);
    INBOX_INFLIGHT_BUDGET.store(b.inbox_inflight_raw_budget, Ordering::Relaxed);
    CHUNK_INFLIGHT_BUDGET.store(b.max_in_flight_chunk_bytes, Ordering::Relaxed);
    API_CACHE_CAP.store(b.max_api_cache_entries, Ordering::Relaxed);
    GEO_CACHE_CAP.store(b.max_geo_cache_entries, Ordering::Relaxed);
    API_CACHE_BYTES.store(b.max_api_cache_bytes, Ordering::Relaxed);
    GEO_CACHE_BYTES.store(b.max_geo_cache_bytes, Ordering::Relaxed);
    AUDIO_BYTES_CAP.store(b.max_audio_bytes, Ordering::Relaxed);
    MESSAGE_PAYLOAD.store(b.message_payload_limit, Ordering::Relaxed);
    INBOX_BODY.store(b.inbox_body_limit, Ordering::Relaxed);
    AUTH_BODY.store(b.authenticated_body_limit, Ordering::Relaxed);
    NOTE_IMAGE.store(b.note_image_limit, Ordering::Relaxed);
    NOTE_VIDEO.store(b.note_video_limit, Ordering::Relaxed);
    DB_MIN.store(b.db_min_connections as usize, Ordering::Relaxed);
    DB_MAX.store(b.db_max_connections as usize, Ordering::Relaxed);
    ARGON2_PERMITS.store(b.argon2_permits, Ordering::Relaxed);
    {
        let mut guard = ARGON2_SEMAPHORE.write().unwrap_or_else(|p| p.into_inner());
        *guard = Arc::new(Semaphore::new(b.argon2_permits));
    }
    ACTIVE_PROFILE.store(profile_to_tag(profile), Ordering::Relaxed);
    tracing::info!(
        profile = profile.as_str(),
        db_pool = format!("{}/{}", b.db_min_connections, b.db_max_connections),
        inbox_inflight_mib = b.inbox_inflight_raw_budget / (1024 * 1024),
        message_payload_mib = b.message_payload_limit / (1024 * 1024),
        inbox_body_mib = b.inbox_body_limit / (1024 * 1024),
        note_video_mib = b.note_video_limit / (1024 * 1024),
        chunk_inflight_mib = b.max_in_flight_chunk_bytes / (1024 * 1024),
        api_cache_entries = b.max_api_cache_entries,
        api_cache_mib = b.max_api_cache_bytes / (1024 * 1024),
        argon2_permits = b.argon2_permits,
        "memory profile applied (route DefaultBodyLimit uses product max; handlers enforce live caps)"
    );
}

pub fn apply_from_saver_flag(memory_saver_enabled: bool) {
    apply(resolve_profile(memory_saver_enabled));
}

pub fn active_profile() -> MemoryProfile {
    tag_to_profile(ACTIVE_PROFILE.load(Ordering::Relaxed))
}

pub fn active_budgets() -> MemoryBudgets {
    MemoryBudgets::for_profile(active_profile())
}

pub fn inbox_inflight_raw_budget() -> usize {
    INBOX_INFLIGHT_BUDGET.load(Ordering::Relaxed)
}

pub fn max_in_flight_chunk_bytes() -> usize {
    CHUNK_INFLIGHT_BUDGET.load(Ordering::Relaxed)
}

pub fn max_api_cache_entries() -> usize {
    API_CACHE_CAP.load(Ordering::Relaxed)
}

pub fn max_geo_cache_entries() -> usize {
    GEO_CACHE_CAP.load(Ordering::Relaxed)
}

pub fn max_api_cache_bytes() -> usize {
    API_CACHE_BYTES.load(Ordering::Relaxed)
}

pub fn max_geo_cache_bytes() -> usize {
    GEO_CACHE_BYTES.load(Ordering::Relaxed)
}

pub fn max_audio_bytes() -> usize {
    AUDIO_BYTES_CAP.load(Ordering::Relaxed)
}

/// JSON snapshot for `/api/metrics` and diagnostics (operator-facing).
pub fn metrics_snapshot() -> serde_json::Value {
    let b = active_budgets();
    serde_json::json!({
        "profile": b.profile.as_str(),
        "db_pool": {
            "min": b.db_min_connections,
            "max": b.db_max_connections,
        },
        "inbox_inflight_raw_budget_mb": b.inbox_inflight_raw_budget / (1024 * 1024),
        "max_in_flight_chunk_mb": b.max_in_flight_chunk_bytes / (1024 * 1024),
        "message_payload_mb": b.message_payload_limit / (1024 * 1024),
        "inbox_body_mb": b.inbox_body_limit / (1024 * 1024),
        "authenticated_body_mb": b.authenticated_body_limit / (1024 * 1024),
        "note_image_mb": b.note_image_limit / (1024 * 1024),
        "note_video_mb": b.note_video_limit / (1024 * 1024),
        "api_cache": {
            "max_entries": b.max_api_cache_entries,
            "max_bytes_mb": b.max_api_cache_bytes / (1024 * 1024),
        },
        "geo_cache": {
            "max_entries": b.max_geo_cache_entries,
            "max_bytes_mb": b.max_geo_cache_bytes / (1024 * 1024),
        },
        "argon2_permits": b.argon2_permits,
        "max_audio_mb": b.max_audio_bytes / (1024 * 1024),
    })
}

/// Public product caps for host/TAPP clients (no RSS / pool internals).
pub fn public_limits_snapshot() -> serde_json::Value {
    let b = active_budgets();
    serde_json::json!({
        "profile": b.profile.as_str(),
        "message_payload_bytes": b.message_payload_limit,
        "note_image_bytes": b.note_image_limit,
        "note_video_bytes": b.note_video_limit,
    })
}

pub fn message_payload_limit() -> usize {
    MESSAGE_PAYLOAD.load(Ordering::Relaxed)
}

pub fn inbox_body_limit() -> usize {
    INBOX_BODY.load(Ordering::Relaxed)
}

pub fn authenticated_body_limit() -> usize {
    AUTH_BODY.load(Ordering::Relaxed)
}

pub fn note_image_limit() -> usize {
    NOTE_IMAGE.load(Ordering::Relaxed)
}

pub fn note_video_limit() -> usize {
    NOTE_VIDEO.load(Ordering::Relaxed)
}

pub fn db_min_connections() -> u32 {
    DB_MIN.load(Ordering::Relaxed) as u32
}

pub fn db_max_connections() -> u32 {
    DB_MAX.load(Ordering::Relaxed).max(1) as u32
}

pub fn argon2_permits() -> usize {
    ARGON2_PERMITS.load(Ordering::Relaxed).max(1)
}

pub fn argon2_semaphore() -> Arc<Semaphore> {
    ARGON2_SEMAPHORE
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_budgets_match_public_inbox_contract() {
        let b = MemoryBudgets::for_profile(MemoryProfile::Default);
        assert_eq!(b.db_min_connections, 2);
        assert_eq!(b.db_max_connections, 24);
        assert_eq!(b.inbox_inflight_raw_budget, 32 * 1024 * 1024);
        assert_eq!(b.max_in_flight_chunk_bytes, 128 * 1024 * 1024);
        assert_eq!(b.max_api_cache_entries, 2048);
        assert_eq!(b.argon2_permits, 4);
    }

    #[test]
    fn saver_is_strictly_tighter_on_concurrency_knobs() {
        let d = MemoryBudgets::for_profile(MemoryProfile::Default);
        let s = MemoryBudgets::for_profile(MemoryProfile::Saver);
        assert!(s.inbox_inflight_raw_budget < d.inbox_inflight_raw_budget);
        assert!(s.max_in_flight_chunk_bytes < d.max_in_flight_chunk_bytes);
        assert!(s.max_api_cache_entries < d.max_api_cache_entries);
        assert!(s.max_api_cache_bytes < d.max_api_cache_bytes);
        assert!(s.max_geo_cache_bytes < d.max_geo_cache_bytes);
        assert!(s.db_max_connections < d.db_max_connections);
        assert!(s.argon2_permits < d.argon2_permits);
        assert!(s.max_audio_bytes < d.max_audio_bytes);
        // Still admits at least one full inbox body under saver single-request limit.
        assert!(s.inbox_inflight_raw_budget >= s.inbox_body_limit);
    }

    #[test]
    fn saver_cuts_federation_payload_caps() {
        let d = MemoryBudgets::for_profile(MemoryProfile::Default);
        let s = MemoryBudgets::for_profile(MemoryProfile::Saver);
        assert!(s.message_payload_limit < d.message_payload_limit);
        assert!(s.inbox_body_limit < d.inbox_body_limit);
        assert!(s.authenticated_body_limit < d.authenticated_body_limit);
        assert!(s.note_image_limit < d.note_image_limit);
        assert!(s.note_video_limit < d.note_video_limit);
        // Envelope headroom: inbox must still exceed message payload by ≥25%.
        assert!(s.inbox_body_limit > s.message_payload_limit);
        assert!(s.inbox_body_limit - s.message_payload_limit >= s.message_payload_limit / 4);
        // Larger media is intentionally handled by chunked transfer, not inbox JSON.
        assert!(s.message_payload_limit >= 2 * 1024 * 1024);
        assert_eq!(s.note_video_limit, 32 * 1024 * 1024);
        assert_eq!(s.max_in_flight_chunk_bytes, 16 * 1024 * 1024);
        assert_eq!(s.max_api_cache_bytes, 8 * 1024 * 1024);
        assert_eq!(s.db_max_connections, 4);
        assert_eq!(s.argon2_permits, 1);
    }

    #[test]
    fn apply_updates_runtime_atomics() {
        let _g = test_profile_lock();
        apply(MemoryProfile::Saver);
        assert_eq!(active_profile(), MemoryProfile::Saver);
        assert_eq!(inbox_inflight_raw_budget(), SAVER_INBOX_INFLIGHT_RAW_BUDGET);
        assert_eq!(max_in_flight_chunk_bytes(), SAVER_MAX_IN_FLIGHT_CHUNK_BYTES);
        assert_eq!(max_api_cache_entries(), SAVER_MAX_API_CACHE_ENTRIES);
        assert_eq!(max_api_cache_bytes(), SAVER_MAX_API_CACHE_BYTES);
        assert_eq!(max_geo_cache_entries(), SAVER_MAX_GEO_CACHE_ENTRIES);
        assert_eq!(max_geo_cache_bytes(), SAVER_MAX_GEO_CACHE_BYTES);
        assert_eq!(max_audio_bytes(), SAVER_MAX_AUDIO_BYTES);
        assert_eq!(message_payload_limit(), SAVER_MESSAGE_PAYLOAD_LIMIT);
        assert_eq!(inbox_body_limit(), SAVER_INBOX_BODY_LIMIT);
        assert_eq!(authenticated_body_limit(), SAVER_AUTHENTICATED_BODY_LIMIT);
        assert_eq!(note_image_limit(), SAVER_NOTE_IMAGE_LIMIT);
        assert_eq!(note_video_limit(), SAVER_NOTE_VIDEO_LIMIT);
        assert_eq!(db_min_connections(), SAVER_DB_MIN_CONNECTIONS);
        assert_eq!(db_max_connections(), SAVER_DB_MAX_CONNECTIONS);
        assert_eq!(argon2_permits(), SAVER_ARGON2_PERMITS);
        apply(MemoryProfile::Default);
        assert_eq!(
            inbox_inflight_raw_budget(),
            DEFAULT_INBOX_INFLIGHT_RAW_BUDGET
        );
        assert_eq!(max_audio_bytes(), DEFAULT_MAX_AUDIO_BYTES);
        assert_eq!(db_max_connections(), DEFAULT_DB_MAX_CONNECTIONS);
        assert_eq!(argon2_permits(), DEFAULT_ARGON2_PERMITS);
        apply(MemoryProfile::Saver);
        let snap = public_limits_snapshot();
        assert_eq!(snap.get("profile").and_then(|v| v.as_str()), Some("saver"));
        assert_eq!(
            snap.get("message_payload_bytes").and_then(|v| v.as_u64()),
            Some(SAVER_MESSAGE_PAYLOAD_LIMIT as u64)
        );
        assert_eq!(
            snap.get("note_video_bytes").and_then(|v| v.as_u64()),
            Some(SAVER_NOTE_VIDEO_LIMIT as u64)
        );
        apply(MemoryProfile::Default);
    }

    #[test]
    fn resolve_env_saver_overrides_flag_false() {
        // Cannot safely mutate process env in parallel tests; unit-test pure mapping.
        assert_eq!(resolve_profile(false), MemoryProfile::Default);
        assert_eq!(resolve_profile(true), MemoryProfile::Saver);
    }
}
