//! Startup egress-location gate for the federation component.
//!
//! A server whose own public IP geolocates to mainland China must not federate.
//! The decision is made once per process from the egress-location probe in
//! [`crate::services::server_location`] and then held in memory; it is a runtime
//! fact about *this* process, not an admin-editable platform setting, so it is
//! never written to `configurations` and never read back from the database.
//!
//! ## Fail-open, by instruction
//!
//! The gate closes only on a positive "this server is in a blocked country"
//! reading. A geolocation provider that is unreachable, rate-limited or
//! unparseable leaves federation enabled — losing a third-party lookup must not
//! take a working instance's federation down.
//!
//! ## The probe window
//!
//! The probe is asynchronous so it cannot delay boot, which means there is a
//! sub-5s window where no reading exists yet. `Pending` reads as *enabled*, for
//! the same reason as above: "no answer yet" is the same epistemic state as "no
//! answer at all", and the alternative would leave federation permanently shut
//! in every context that does not run the probe (tests, one-shot binaries).
//!
//! Outbound delivery — the surface that actually reaches out to other
//! instances — does not accept that window: the delivery worker awaits
//! [`wait_until_resolved`] before its first drain, so a blocked server never
//! emits an Activity on a merely-unresolved gate.

use once_cell::sync::Lazy;
use serde::Serialize;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::RwLock;
use std::time::Duration;

use crate::services::server_location::ServerLocationAssessment;

/// ISO 3166-1 alpha-2 codes whose servers must not federate.
///
/// Mainland China only. HK / MO / TW are distinct ISO codes and distinct
/// jurisdictions — widening this list is a policy change, not a typo fix.
const BLOCKED_COUNTRY_CODES: &[&str] = &["CN"];

const PENDING: u8 = 0;
const ENABLED: u8 = 1;
const DISABLED: u8 = 2;

/// Sleep between `is_resolved` polls inside `wait_until_resolved`.
const RESOLVE_WAIT_POLL: Duration = Duration::from_millis(100);

static STATE: AtomicU8 = AtomicU8::new(PENDING);

static DETAIL: Lazy<RwLock<FederationGateStatus>> =
    Lazy::new(|| RwLock::new(FederationGateStatus::pending()));

/// Diagnostics projection of the gate. Not a config row — it is re-derived on
/// every boot, because a server can move between boots.
#[derive(Clone, Debug, Serialize)]
pub struct FederationGateStatus {
    /// `pending` | `enabled` | `disabled`
    pub state: &'static str,
    /// What the rest of the process acts on. `true` while pending.
    pub enabled: bool,
    /// Whether the probe has produced a reading yet.
    pub resolved: bool,
    /// `probe_pending` | `geolocation_unavailable` | `allowed_country` |
    /// `blocked_country`
    pub reason: &'static str,
    /// Country codes observed by the probe, one per responding source.
    pub country_codes: Vec<String>,
    /// Geolocation providers that answered.
    pub sources: Vec<String>,
}

impl FederationGateStatus {
    fn pending() -> Self {
        Self {
            state: "pending",
            enabled: true,
            resolved: false,
            reason: "probe_pending",
            country_codes: Vec::new(),
            sources: Vec::new(),
        }
    }
}

fn is_blocked_country(code: &str) -> bool {
    BLOCKED_COUNTRY_CODES
        .iter()
        .any(|blocked| blocked.eq_ignore_ascii_case(code.trim()))
}

/// Pure decision from an egress-location reading: `(enabled, reason)`.
///
/// Reads *every* observed country code, not just the primary one. Two providers
/// disagreeing with one of them naming a blocked country is exactly the
/// ambiguous case where the restrictive answer is the correct one; keying off
/// the primary source alone would make the outcome depend on which lookup
/// happened to be listed first.
pub fn decide(assessment: &ServerLocationAssessment) -> (bool, &'static str) {
    if assessment.country_codes.is_empty() {
        return (true, "geolocation_unavailable");
    }
    if assessment
        .country_codes
        .iter()
        .any(|code| is_blocked_country(code))
    {
        return (false, "blocked_country");
    }
    (true, "allowed_country")
}

/// Is the federation component live in this process?
///
/// Hot path: called per federation request and per granted-permission check.
/// Lock-free by construction — the detail struct behind [`status`] is only read
/// by diagnostics.
pub fn federation_enabled() -> bool {
    STATE.load(Ordering::Relaxed) != DISABLED
}

/// Has the probe produced a reading yet?
pub fn is_resolved() -> bool {
    STATE.load(Ordering::Relaxed) != PENDING
}

/// Current gate status for the admin diagnostics panel.
pub fn status() -> FederationGateStatus {
    DETAIL
        .read()
        .map(|detail| detail.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
}

/// Record a probe result. Separated from [`spawn_startup_probe`] so the
/// decision can be exercised without touching the network.
pub fn apply(assessment: &ServerLocationAssessment) {
    let (enabled, reason) = decide(assessment);
    let next = FederationGateStatus {
        state: if enabled { "enabled" } else { "disabled" },
        enabled,
        resolved: true,
        reason,
        country_codes: assessment.country_codes.clone(),
        sources: assessment.sources.clone(),
    };

    if let Ok(mut detail) = DETAIL.write() {
        *detail = next.clone();
    }
    STATE.store(if enabled { ENABLED } else { DISABLED }, Ordering::Relaxed);

    if enabled {
        tracing::info!(
            reason = next.reason,
            country_codes = ?next.country_codes,
            sources = ?next.sources,
            "✅ Federation enabled by egress-location gate"
        );
    } else {
        tracing::warn!(
            reason = next.reason,
            country_codes = ?next.country_codes,
            sources = ?next.sources,
            "⛔ Federation disabled: this server's public IP geolocates to a blocked country"
        );
    }
}

/// Kick off the one-shot location probe. Never blocks boot.
pub fn spawn_startup_probe() {
    tokio::spawn(async {
        let assessment = crate::services::server_location::inspect_server_location().await;
        apply(&assessment);
    });
}

/// Wait for the probe to settle, then report whether federation is enabled.
///
/// For consumers that must not act on an unresolved gate. On timeout the
/// pending value (enabled) is returned, keeping the fail-open contract.
pub async fn wait_until_resolved(max_wait: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + max_wait;
    while !is_resolved() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(RESOLVE_WAIT_POLL).await;
    }
    if !is_resolved() {
        tracing::warn!(
            wait_secs = max_wait.as_secs(),
            "Federation egress-location gate did not resolve in time; proceeding as enabled"
        );
    }
    federation_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::server_location::unavailable_assessment;

    fn assessment_with(codes: &[&str]) -> ServerLocationAssessment {
        ServerLocationAssessment {
            country_codes: codes.iter().map(|code| code.to_string()).collect(),
            sources: codes.iter().map(|_| "test".to_string()).collect(),
            ..unavailable_assessment(false)
        }
    }

    #[test]
    fn unreachable_provider_leaves_federation_enabled() {
        let (enabled, reason) = decide(&unavailable_assessment(false));
        assert!(enabled);
        assert_eq!(reason, "geolocation_unavailable");
    }

    #[test]
    fn blocked_country_closes_the_gate() {
        let (enabled, reason) = decide(&assessment_with(&["CN"]));
        assert!(!enabled);
        assert_eq!(reason, "blocked_country");
    }

    #[test]
    fn allowed_country_keeps_the_gate_open() {
        let (enabled, reason) = decide(&assessment_with(&["JP"]));
        assert!(enabled);
        assert_eq!(reason, "allowed_country");
    }

    /// A dissenting source must not be outvoted by ordering.
    #[test]
    fn any_blocked_source_closes_the_gate() {
        assert!(!decide(&assessment_with(&["JP", "CN"])).0);
        assert!(!decide(&assessment_with(&["CN", "JP"])).0);
    }

    #[test]
    fn country_code_match_is_case_insensitive() {
        assert!(!decide(&assessment_with(&["cn"])).0);
    }

    /// Separate ISO codes are separate jurisdictions — not covered by "CN".
    #[test]
    fn china_adjacent_codes_are_not_blocked() {
        for code in ["HK", "MO", "TW"] {
            assert!(decide(&assessment_with(&[code])).0, "{code} must federate");
        }
    }

    /// Nothing may act as if the gate were closed before a reading exists.
    #[test]
    fn pending_reads_as_enabled() {
        assert!(FederationGateStatus::pending().enabled);
        assert!(!FederationGateStatus::pending().resolved);
    }
}
