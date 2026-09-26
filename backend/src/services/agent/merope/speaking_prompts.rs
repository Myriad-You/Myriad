//! Her speaking prompts live in `myriad_merope::speaking`: pure text, no
//! storage. Re-exported here; only reading her persona row is the backend's.

use crate::models::entities::agent_persona;

pub use myriad_merope::speaking::*;

/// [`myriad_merope::speaking::format_persona`] for her stored persona.
pub fn format_persona(persona: &agent_persona::Model) -> Option<String> {
    myriad_merope::speaking::format_persona(&persona.name, &persona.personality)
}
