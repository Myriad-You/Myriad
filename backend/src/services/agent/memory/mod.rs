//! Agent long-term memory: one table (`agent_memories`) for Chat and Work.
//!
//! [`unified`] owns storage, audience and retrieval; `lexical` scores text
//! relevance for it, [`meaning`] how close a memory is to what was asked in
//! meaning, and `association` spreads activation from what was named to what
//! it brings to mind (both, and `strength`, are the pure
//! `myriad_agent_rules::memory`); [`work_memory`] turns a finished Work run
//! into memories and imports the pre-unified JSON store.

use myriad_agent_rules::memory::association;
pub(crate) use myriad_agent_rules::memory::lexical;
pub mod manage;
pub mod meaning;
pub mod origins;
pub use myriad_agent_rules::memory::strength;
pub mod unified;

/// Now, for what memory stores and how readily it comes to mind (a
/// recording of group talk runs on its own clock, see `channel_group::g0`).
pub(crate) fn utc_now() -> chrono::DateTime<chrono::Utc> {
    #[cfg(test)]
    if let Some(at) = crate::services::channel_group::g0::now() {
        return at;
    }
    chrono::Utc::now()
}
pub(crate) mod work_memory;

