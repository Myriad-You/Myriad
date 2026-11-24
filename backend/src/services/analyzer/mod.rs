//! AI analysis service using Google Gemini API or OpenAI-compatible API.

mod cached_system;
mod client;
mod embeddings;
mod gemini;
mod openai;
mod schema;
mod sse;
pub(crate) mod text_protocol;
mod tool_protocol;
mod types;

pub(crate) mod tool_calling;
mod transport;

#[cfg(test)]
mod json_shape_tests;
#[cfg(test)]
pub(crate) mod probe;
#[cfg(test)]
mod refusal_said;
#[cfg(test)]
pub(crate) mod write_thinking;

pub use cached_system::CachedSystem;
pub use client::AiAnalyzer;
pub use openai::EmptyAtLimit;
pub use sse::StreamCut;
pub use types::*;

pub(crate) use openai::openai_chat_completions_url;

pub(crate) mod request_budget;

pub(crate) use client::cleanup_shape_memo;

pub use client::ProviderHttpError;
