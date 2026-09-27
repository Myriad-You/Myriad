//! Finite Merope work owned by this process. No waiting queue: when all slots
//! are occupied, best-effort work is skipped with a named diagnostic. Shutdown
//! closes admission, drains accepted work, then aborts at the process deadline.
use crate::services::jobs::{JobRunner, SHUTDOWN_DRAIN};
use std::{future::Future, sync::LazyLock};

const MAX_TASKS: usize = 32;
static TASKS: LazyLock<JobRunner> = LazyLock::new(JobRunner::new);

pub(in crate::services::agent) fn spawn(
    name: &'static str,
    work: impl Future<Output = ()> + Send + 'static,
) -> bool {
    TASKS.try_once(name, MAX_TASKS, work)
}

pub(crate) fn stop_admission() {
    TASKS.stop_admission();
}

pub(crate) async fn shutdown() {
    TASKS.shutdown(SHUTDOWN_DRAIN).await;
}
