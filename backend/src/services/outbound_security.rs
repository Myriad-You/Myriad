//! Re-export of workspace crate [`myriad_outbound`].
//!
//! Call sites may use either `crate::services::outbound_security::…` or
//! `myriad_outbound::…`.

pub use myriad_outbound::*;

/// Push the outbound policy carried by platform settings into the
/// process-wide [`myriad_outbound`] state. Called by the configuration installer.
pub(super) fn apply_dynamic_config(config: &crate::config::DynamicConfig) {
    if config.allow_rfc2544_benchmark_range && !rfc2544_benchmark_range_allowed() {
        tracing::warn!(
            "Outbound SSRF guard admits IPv4 198.18.0.0/15 DNS answers; \
             destination routing is trusted to the local fake-IP proxy"
        );
    }
    set_allow_rfc2544_benchmark_range(config.allow_rfc2544_benchmark_range);
}
