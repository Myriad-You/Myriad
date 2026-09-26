//! AI analysis service using Google Gemini API or OpenAI-compatible API.

mod client;
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
pub(crate) mod probe;

pub use client::AiAnalyzer;
pub use gemini::gemini_stream_deltas;
pub use openai::openai_stream_deltas;
pub use sse::StreamCut;
pub use types::*;

pub(crate) use openai::openai_chat_completions_url;

pub(crate) mod request_budget;

pub(crate) use client::cleanup_shape_memo;
