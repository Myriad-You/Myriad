//! Agent long-term memory: entries, TF-IDF index, and persistence manager.
//!
//! Real submodules: [`types_index`] (types + TF-IDF) and [`manager`] (`AgentMemory` impl).

mod types_index;
mod manager;

pub use manager::{get_memory, init_memory, summarize_value_for_memory};
pub use types_index::{MemoryTier, MemoryType, RecallQuery};
