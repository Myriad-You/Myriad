//! Re-export of workspace crate [`myriad_outbound`].
//!
//! Call sites may use either `crate::services::outbound_security::…` or
//! `myriad_outbound::…`.

pub use myriad_outbound::*;

/// Push the outbound policy carried by platform settings into the
/// process-wide [`myriad_outbound`] state. Call wherever a freshly loaded
/// [`crate::config::DynamicConfig`] is installed as the process config.
pub fn apply_dynamic_config(config: &crate::config::DynamicConfig) {
    set_allow_rfc2544_benchmark_range(config.allow_rfc2544_benchmark_range);
}
