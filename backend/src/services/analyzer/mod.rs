//! AI analysis service using Google Gemini API or OpenAI-compatible API.

mod client;
mod gemini;
mod openai;
mod schema;
mod types;

mod transport;

#[cfg(test)]
pub(crate) mod probe;

pub use client::AiAnalyzer;
pub use gemini::gemini_stream_deltas;
pub use openai::openai_stream_deltas;
pub use types::*;

pub(crate) use openai::openai_chat_completions_url;
