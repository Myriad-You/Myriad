//! Fixed one-shot handoff used only after the running Guard has independently
//! verified the signed release and immutable updater image digest.

use std::process::ExitCode;

fn main() -> ExitCode {
    myriad_updater::log::init();
    match myriad_updater::docker::self_update_helper::main_from_env() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "trusted TCB self-update handoff failed");
            ExitCode::FAILURE
        }
    }
}
