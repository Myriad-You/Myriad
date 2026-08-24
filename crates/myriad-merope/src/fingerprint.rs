use crate::{MeropePolicy, RuntimeState};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintInput<'a> {
    pub runtime: &'a RuntimeState,
    pub policy: &'a MeropePolicy,
    pub unread_proactive_count: u32,
    pub master_asset_id: Option<&'a str>,
    pub rig_updated_at: Option<&'a str>,
}

pub fn merope_fingerprint(input: &FingerprintInput<'_>) -> String {
    let encoded = serde_json::to_vec(input).unwrap_or_default();
    let digest = Sha256::digest(encoded);
    format!("sha256-{}", hex_prefix(&digest[..16]))
}

fn hex_prefix(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn fingerprint_is_stable_and_sensitive_to_unread_count() {
        let runtime = RuntimeState::new(Utc::now());
        let policy = MeropePolicy::default();
        let first = merope_fingerprint(&FingerprintInput {
            runtime: &runtime,
            policy: &policy,
            unread_proactive_count: 0,
            master_asset_id: None,
            rig_updated_at: None,
        });
        let second = merope_fingerprint(&FingerprintInput {
            runtime: &runtime,
            policy: &policy,
            unread_proactive_count: 1,
            master_asset_id: None,
            rig_updated_at: None,
        });
        assert_eq!(first.len(), 39);
        assert_ne!(first, second);
    }

    #[test]
    fn fingerprint_changes_when_rig_changes() {
        let runtime = RuntimeState::new(Utc::now());
        let policy = MeropePolicy::default();
        let first = merope_fingerprint(&FingerprintInput {
            runtime: &runtime,
            policy: &policy,
            unread_proactive_count: 0,
            master_asset_id: Some("master"),
            rig_updated_at: None,
        });
        let second = merope_fingerprint(&FingerprintInput {
            runtime: &runtime,
            policy: &policy,
            unread_proactive_count: 0,
            master_asset_id: Some("master"),
            rig_updated_at: Some("2026-08-09T12:00:00Z"),
        });
        assert_ne!(first, second);
    }
}
