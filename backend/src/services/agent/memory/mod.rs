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
pub use myriad_agent_rules::memory::strength;
pub mod unified;
pub(crate) mod work_memory;

