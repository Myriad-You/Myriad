//! What a memory or a note is about, and the other names it goes by.

use serde::{Deserialize, Serialize};

/// Something a memory is about, with the other names people use for it, so
/// "喵" finds the memory about the cat. Written by the same model call that
/// wrote the memory; the aliases are that model's knowledge, not the person's
/// words, and are used only to match, never shown as something they said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl Concept {
    /// Every name this concept answers to, the canonical one first.
    pub fn surface_forms(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.aliases.iter().map(String::as_str))
    }
}
