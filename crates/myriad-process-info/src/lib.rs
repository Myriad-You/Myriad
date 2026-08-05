//! Process and build identity helpers shared by HTTP `/health`, `/api/metrics`,
//! and the Agent `system.metrics` capability.
//!
//! Workspace crate so services do not import the HTTP `api` layer for pure
//! process introspection.
//!
//! # Version fallback
//!
//! `option_env!("MYRIAD_VERSION")` is preferred (Docker/CI stamp). When unset,
//! [`set_package_version_fallback`] must be called once at binary startup with
//! the **binary** package version (`concat!("v", env!("CARGO_PKG_VERSION"))`
//! from `myriad-backend`), not this crate's version.

use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Instant;

static STARTED_AT: OnceLock<Instant> = OnceLock::new();
static PACKAGE_VERSION_FALLBACK: OnceLock<&'static str> = OnceLock::new();

/// Record the host binary's package version for use when `MYRIAD_VERSION` is unset.
///
/// Call once from the binary's `main` (idempotent).
pub fn set_package_version_fallback(version: &'static str) {
    let _ = PACKAGE_VERSION_FALLBACK.set(version);
}

/// Mark process start for uptime reporting. Call once at binary startup.
pub fn mark_startup() {
    let _ = STARTED_AT.set(Instant::now());
}

/// Process uptime in seconds since [`mark_startup`] (0 if not marked).
pub fn process_uptime_seconds() -> u64 {
    STARTED_AT.get().map(|t| t.elapsed().as_secs()).unwrap_or(0)
}

/// Build version string for `/health` and agent metrics.
pub fn build_version() -> &'static str {
    if let Some(v) = option_env!("MYRIAD_VERSION") {
        return v;
    }
    PACKAGE_VERSION_FALLBACK
        .get()
        .copied()
        .unwrap_or("v0.0.0-dev")
}

/// Optional 40-char hex commit SHA from `MYRIAD_COMMIT_SHA` at compile time.
pub fn build_commit_sha() -> Option<&'static str> {
    option_env!("MYRIAD_COMMIT_SHA")
        .map(str::trim)
        .filter(|sha| sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Host process OS / CPU architecture (binary target, not host-of-container).
///
/// Values come from `std::env::consts` / `target_pointer_width` and are the
/// same inside Docker and bare metal for the running process. Cross-arch
/// containers report the guest arch, not the hypervisor host.
pub fn process_platform_info() -> Value {
    let pointer_width = if cfg!(target_pointer_width = "64") {
        "64"
    } else if cfg!(target_pointer_width = "32") {
        "32"
    } else {
        "unknown"
    };
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "pointer_width": pointer_width,
    })
}

/// RSS (MiB) at/above which metrics and runtime diagnostics report **warning**.
///
/// ~500 MiB backend load is operationally normal on 1 GiB hosts; warn slightly above.
pub const MEMORY_WARNING_MB: u64 = 600;

/// RSS (MiB) at/above which metrics and runtime diagnostics report **critical**.
///
/// Leaves a little headroom under a 1 GiB host budget before OOM pressure.
pub const MEMORY_CRITICAL_MB: u64 = 750;

/// Process memory info (cross-platform, best-effort).
pub fn process_memory_info() -> Value {
    get_memory_info()
}

fn get_memory_info() -> Value {
    #[cfg(target_os = "linux")]
    {
        use std::fs;

        if let Ok(status) = fs::read_to_string("/proc/self/status") {
            let mut vm_rss = 0u64;
            let mut vm_size = 0u64;

            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    vm_rss = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                } else if line.starts_with("VmSize:") {
                    vm_size = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                }
            }

            json!({
                "rss_kb": vm_rss,
                "rss_mb": vm_rss / 1024,
                "virtual_kb": vm_size,
                "virtual_mb": vm_size / 1024,
                "platform": "linux",
            })
        } else {
            json!({
                "platform": "linux",
                "note": "Unable to read /proc/self/status"
            })
        }
    }

    #[cfg(target_os = "windows")]
    {
        json!({
            "platform": "windows",
            "note": "Detailed memory metrics require additional dependencies"
        })
    }

    #[cfg(target_os = "macos")]
    {
        let pid = std::process::id();
        if let Ok(output) = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
        {
            if let Ok(s) = String::from_utf8(output.stdout) {
                if let Ok(rss_kb) = s.trim().parse::<u64>() {
                    return json!({
                        "rss_kb": rss_kb,
                        "rss_mb": rss_kb / 1024,
                        "platform": "macos",
                    });
                }
            }
        }
        json!({
            "platform": "macos",
            "note": "Unable to read process RSS via ps"
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        json!({
            "platform": "unknown",
            "note": "Memory metrics not available"
        })
    }
}

/// Validate a candidate commit SHA (for unit tests and callers).
pub fn is_valid_commit_sha(sha: &str) -> bool {
    let sha = sha.trim();
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_version_uses_fallback_when_env_unset() {
        // option_env!("MYRIAD_VERSION") may or may not be set in CI; when unset,
        // we need a fallback path. Setting after first call is a no-op if already set.
        set_package_version_fallback("v9.9.9-test");
        let v = build_version();
        // Either stamped MYRIAD_VERSION or our fallback (if env unset at compile).
        assert!(
            v.starts_with('v') || v.contains('.'),
            "unexpected version: {v}"
        );
        // Fallback is only used when MYRIAD_VERSION was not compiled in.
        if option_env!("MYRIAD_VERSION").is_none() {
            assert_eq!(v, "v9.9.9-test");
        }
    }

    #[test]
    fn uptime_zero_before_mark_or_non_negative_after() {
        // Other tests / parallel crates may have already marked startup in-process.
        // Contract: never negative; after mark_startup is non-decreasing.
        let before = process_uptime_seconds();
        mark_startup();
        let after = process_uptime_seconds();
        assert!(after >= before || after == 0);
    }

    #[test]
    fn commit_sha_validator() {
        assert!(is_valid_commit_sha(
            "0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(!is_valid_commit_sha("short"));
        assert!(!is_valid_commit_sha(
            "0123456789abcdef0123456789abcdef0123456g"
        ));
        assert!(!is_valid_commit_sha(
            "0123456789abcdef0123456789abcdef012345678" // 41
        ));
    }

    #[test]
    fn memory_alert_thresholds_fit_1g_host_budget() {
        assert_eq!(MEMORY_WARNING_MB, 600);
        assert_eq!(MEMORY_CRITICAL_MB, 750);
    }

    #[test]
    fn process_memory_info_reports_platform() {
        let mem = process_memory_info();
        let platform = mem.get("platform").and_then(|v| v.as_str()).unwrap_or("");
        assert!(
            matches!(platform, "linux" | "macos" | "windows" | "unknown"),
            "platform={platform}"
        );
    }

    #[test]
    fn process_platform_info_reports_os_and_arch() {
        let info = process_platform_info();
        let os = info.get("os").and_then(|v| v.as_str()).unwrap_or("");
        let arch = info.get("arch").and_then(|v| v.as_str()).unwrap_or("");
        let family = info.get("family").and_then(|v| v.as_str()).unwrap_or("");
        let width = info
            .get("pointer_width")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(!os.is_empty(), "os empty");
        assert!(!arch.is_empty(), "arch empty");
        assert!(matches!(family, "unix" | "windows"), "family={family}");
        assert!(matches!(width, "32" | "64"), "pointer_width={width}");
        assert_eq!(os, std::env::consts::OS);
        assert_eq!(arch, std::env::consts::ARCH);
    }
}
