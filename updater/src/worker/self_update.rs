//! Updater TCB upgrade boundary.
//!
//! `docker-guard` owns the raw Docker socket and is therefore host-root
//! equivalent. The lower-trust updater must not choose a Guard/helper image,
//! mutate its Compose definition, or ask it to execute a privileged helper.
//! Guard/updater TCB upgrades are consequently an explicit host-operator flow:
//! verify the signed release manifest, pin the updater image by digest in the
//! host-owned Guard deployment, and recreate the TCB outside this process.

use std::sync::Arc;

use crate::error::{Result, UpdaterError};
use crate::worker::Worker;

pub async fn run(_worker: Arc<Worker>, _actor: Option<String>) -> Result<SelfUpdateReport> {
    Err(UpdaterError::Precondition(
        "automatic updater/Guard self-update is disabled: docker-guard is a separate TCB; \
         a host operator must verify signed release identity and exact image digest, then \
         recreate the Guard deployment from host-owned configuration"
            .into(),
    ))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SelfUpdateReport {
    pub helper_container_id: String,
    pub new_updater_tag: String,
    pub previous_updater_tag: String,
    pub scheduled: bool,
}

#[cfg(test)]
mod tests {
    #[test]
    fn operator_only_error_does_not_offer_a_mutable_config_fallback() {
        let message =
            "automatic updater/Guard self-update is disabled: docker-guard is a separate TCB";
        assert!(message.contains("separate TCB"));
        assert!(!message.contains("UPDATER_IMAGE"));
        assert!(!message.contains("UPDATER_TAG"));
    }
}
