//! Re-export of workspace crate [`myriad_outbound`].
//!
//! Call sites may use either `crate::services::outbound_security::…` or
//! `myriad_outbound::…`.

pub use myriad_outbound::*;

/// The form uses comma/whitespace-separated text; database rows and backups use
/// arrays. All entry points enforce the same CIDR policy before installation.
pub fn parse_configured_fake_ip_ranges(value: &serde_json::Value) -> Result<Vec<String>, String> {
    let ranges = match value {
        serde_json::Value::String(raw) => raw
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|entry| !entry.is_empty())
            .map(str::to_string)
            .collect(),
        serde_json::Value::Array(entries) => entries
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "Fake-IP ranges must contain only CIDR strings".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("Fake-IP ranges must be CIDR text or an array of CIDR strings".into()),
    };
    normalize_fake_ip_ranges(&ranges)
}

/// Push the outbound policy carried by platform settings into the
/// process-wide [`myriad_outbound`] state. Called by the configuration installer.
pub(super) fn apply_dynamic_config(config: &crate::config::DynamicConfig) {
    let previous = fake_ip_ranges();
    if let Err(error) = set_fake_ip_ranges(&config.outbound_fake_ip_ranges) {
        let _ = set_fake_ip_ranges(&[]);
        tracing::error!(%error, "Invalid fake-IP policy; DNS exceptions disabled");
        return;
    }
    let current = fake_ip_ranges();
    if !current.is_empty() && current != previous {
        tracing::warn!(
            ranges = ?current,
            "Outbound SSRF guard admits configured fake-IP DNS answers; \
             destination routing is trusted to the local fake-IP proxy"
        );
    }
}
