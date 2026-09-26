//! Limits apply to received bytes before buffering, including malformed events.
//!
//! A stream counts as complete only when the provider says it finished: an
//! OpenAI-style `[DONE]` or `finish_reason`, a Gemini `finishReason`, an
//! Anthropic `message_stop`, a Responses `response.completed`. A stream that
//! just ends (the provider or the connection dropped it mid-reply) is a
//! [`StreamCut`], never a whole reply; an error event inside the stream is an
//! error.
use super::types::StreamDelta;
use anyhow::{Context, Result, bail};
use std::future::Future;

/// The stream ended before the provider said it had finished. `partial` is
/// what arrived, for a caller that has already shown it.
#[derive(Debug)]
pub struct StreamCut {
    pub partial: String,
}

impl std::fmt::Display for StreamCut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AI stream ended before the reply was complete")
    }
}

impl std::error::Error for StreamCut {}

/// Whether this event ends the stream: finished, or failed with a message.
fn stream_end(json: &serde_json::Value) -> Option<Result<(), String>> {
    if let Some(error) = json.get("error").filter(|error| !error.is_null()) {
        let message = error
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or("unknown error");
        return Some(Err(message.chars().take(200).collect()));
    }
    match json.get("type").and_then(|kind| kind.as_str()) {
        Some("error" | "response.failed" | "response.error") => {
            return Some(Err("the provider reported a failure".into()));
        }
        Some("message_stop" | "response.completed") => return Some(Ok(())),
        _ => {}
    }
    if let Some(reason) = json
        .pointer("/choices/0/finish_reason")
        .and_then(|reason| reason.as_str())
    {
        return Some(if reason == "error" {
            Err("the provider stopped with an error".into())
        } else {
            Ok(())
        });
    }
    json.pointer("/candidates/0/finishReason")
        .and_then(|reason| reason.as_str())
        .map(|_| Ok(()))
}

const MAX_STREAM_BYTES: usize = 4 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct SseLines {
    bytes: Vec<u8>,
    consumed: usize,
    received: usize,
    pending_line_bytes: usize,
}

impl SseLines {
    fn push(&mut self, chunk: &[u8]) -> Result<()> {
        self.received = self.received.saturating_add(chunk.len());
        if self.received > MAX_STREAM_BYTES {
            bail!("AI stream exceeded its response size limit");
        }
        // Check even complete lines before copying the chunk or parsing JSON.
        for (index, part) in chunk.split(|byte| *byte == b'\n').enumerate() {
            if index > 0 {
                self.pending_line_bytes = 0;
            }
            self.pending_line_bytes += part.len();
            if self.pending_line_bytes > MAX_LINE_BYTES {
                bail!("AI stream exceeded its event size limit");
            }
        }
        if self.consumed > 0 {
            self.bytes.drain(..self.consumed);
            self.consumed = 0;
        }
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    fn next_line(&mut self) -> Option<&[u8]> {
        let start = self.consumed;
        let end = start + self.bytes[start..].iter().position(|byte| *byte == b'\n')?;
        self.consumed = end + 1;
        Some(self.bytes[start..end].trim_ascii())
    }
}

pub(super) async fn consume_text_sse<F, Fut>(
    mut response: reqwest::Response,
    mut on_delta: F,
    extract: fn(&serde_json::Value) -> Vec<StreamDelta>,
) -> Result<String>
where
    F: FnMut(StreamDelta) -> Fut + Send,
    Fut: Future<Output = bool> + Send,
{
    let mut lines = SseLines::default();
    let mut full_text = String::new();
    let mut decoded_bytes = 0usize;
    let mut finished = false;
    while let Some(chunk) = response.chunk().await.context("Stream read error")? {
        lines.push(&chunk)?;
        while let Some(line) = lines.next_line() {
            let Some(data) = line.strip_prefix(b"data:").map(<[u8]>::trim_ascii_start) else {
                continue;
            };
            if data == b"[DONE]" {
                return Ok(full_text);
            }
            // Preserve UTF-8 split across chunks; only decode a complete SSE line.
            let Ok(json) = serde_json::from_slice::<serde_json::Value>(data) else {
                continue;
            };
            match stream_end(&json) {
                Some(Err(message)) => bail!("AI stream reported an error: {message}"),
                // The last event may still carry text: take it, then finish.
                Some(Ok(())) => finished = true,
                None => {}
            }
            for delta in extract(&json) {
                let content = match &delta {
                    StreamDelta::Text(content) | StreamDelta::Reasoning(content) => content,
                };
                decoded_bytes = decoded_bytes.saturating_add(content.len());
                if decoded_bytes > MAX_STREAM_BYTES {
                    bail!("AI stream exceeded its text size limit");
                }
                if let StreamDelta::Text(content) = &delta {
                    full_text.push_str(content);
                }
                if !on_delta(delta).await {
                    return Ok(full_text);
                }
                tokio::task::yield_now().await;
            }
        }
    }
    if finished {
        Ok(full_text)
    } else {
        Err(StreamCut { partial: full_text }.into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{gemini::gemini_stream_deltas, openai::openai_stream_deltas};
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn response(body: Vec<u8>) -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 8192];
            if socket.read(&mut request).await.is_err() {
                return;
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            if socket.write_all(header.as_bytes()).await.is_ok() {
                // Early rejection/cancellation intentionally closes this connection.
                let _ = socket.write_all(&body).await;
            }
        });
        reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap()
            .get(format!("http://{addr}/"))
            .send()
            .await
            .unwrap()
    }

    #[test]
    fn rejects_oversized_partial_and_complete_lines_before_retaining_chunk() {
        for ending in ["", "\n"] {
            let mut lines = SseLines::default();
            let chunk = format!("{}{}", "x".repeat(MAX_LINE_BYTES + 1), ending);
            assert!(lines.push(chunk.as_bytes()).is_err());
            assert!(lines.bytes.is_empty());
        }
        let mut lines = SseLines::default();
        lines.push(&vec![b'x'; MAX_LINE_BYTES]).unwrap();
        assert!(lines.push(b"x").is_err());
        assert_eq!(lines.bytes.len(), MAX_LINE_BYTES);
    }

    #[test]
    fn preserves_unicode_split_across_network_chunks_and_crlf() {
        let mut lines = SseLines::default();
        lines.push(b"data: \xe4\xbd").unwrap();
        assert!(lines.next_line().is_none());
        lines.push(b"\xa0\r\n\r\n").unwrap();
        assert_eq!(lines.next_line().unwrap(), "data: 你".as_bytes());
        assert_eq!(lines.next_line().unwrap(), b"");
    }

    #[tokio::test]
    async fn both_providers_reject_oversized_upstream_lines() {
        for extract in [openai_stream_deltas, gemini_stream_deltas] {
            let result = consume_text_sse(
                response(vec![b'x'; MAX_LINE_BYTES + 1]).await,
                |_| async { true },
                extract,
            )
            .await;
            assert!(result.unwrap_err().to_string().contains("event size limit"));
        }
    }

    #[tokio::test]
    async fn both_providers_limit_total_stream_despite_small_events() {
        for (extract, event) in [
            (
                openai_stream_deltas as fn(&serde_json::Value) -> Vec<StreamDelta>,
                serde_json::json!({"choices":[{"delta":{"content":"x".repeat(64*1024)}}]}),
            ),
            (
                gemini_stream_deltas as fn(&serde_json::Value) -> Vec<StreamDelta>,
                serde_json::json!({"candidates":[{"content":{"parts":[{"text":"x".repeat(64*1024)}]}}]}),
            ),
        ] {
            let body = format!("data: {event}\n\n").repeat(80).into_bytes();
            let result = consume_text_sse(response(body).await, |_| async { true }, extract).await;
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("response size limit")
            );
        }
    }

    /// A reply the provider never finished is not a reply: the half sentence
    /// comes back as a cut, and an error event inside the stream is an error.
    #[tokio::test]
    async fn a_stream_that_just_ends_is_cut_not_complete() {
        let text = serde_json::json!({"choices":[{"delta":{"content":"听完要是"}}]});
        let cut = consume_text_sse(
            response(format!("data: {text}\n\n").into_bytes()).await,
            |_| async { true },
            openai_stream_deltas,
        )
        .await
        .unwrap_err();
        assert_eq!(cut.downcast_ref::<StreamCut>().unwrap().partial, "听完要是");

        let finished = serde_json::json!({"choices":[{"delta":{"content":"。"},"finish_reason":"stop"}]});
        let whole = consume_text_sse(
            response(format!("data: {text}\n\ndata: {finished}\n\n").into_bytes()).await,
            |_| async { true },
            openai_stream_deltas,
        )
        .await
        .unwrap();
        assert_eq!(whole, "听完要是。", "finish_reason without [DONE] is finished");

        let gemini = serde_json::json!({"candidates":[{"content":{"parts":[{"text":"好"}]},"finishReason":"STOP"}]});
        let whole = consume_text_sse(
            response(format!("data: {gemini}\n\n").into_bytes()).await,
            |_| async { true },
            gemini_stream_deltas,
        )
        .await
        .unwrap();
        assert_eq!(whole, "好");

        let failed = serde_json::json!({"error":{"message":"upstream overloaded"}});
        let error = consume_text_sse(
            response(format!("data: {text}\n\ndata: {failed}\n\n").into_bytes()).await,
            |_| async { true },
            openai_stream_deltas,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("upstream overloaded"));
        assert!(error.downcast_ref::<StreamCut>().is_none());
    }

    #[tokio::test]
    async fn both_providers_return_text_and_stop_on_callback_cancellation() {
        for (extract, event) in [
            (
                openai_stream_deltas as fn(&serde_json::Value) -> Vec<StreamDelta>,
                serde_json::json!({"choices":[{"delta":{"content":"你好"}}]}),
            ),
            (
                gemini_stream_deltas as fn(&serde_json::Value) -> Vec<StreamDelta>,
                serde_json::json!({"candidates":[{"content":{"parts":[{"text":"你好"}]}}]}),
            ),
        ] {
            let body = format!("data: {event}\r\n\r\ndata: {event}\n\ndata: [DONE]\n\n");
            let full = consume_text_sse(
                response(body.clone().into_bytes()).await,
                |_| async { true },
                extract,
            )
            .await
            .unwrap();
            assert_eq!(full, "你好你好");
            let cancelled = consume_text_sse(
                response(body.into_bytes()).await,
                |_| async { false },
                extract,
            )
            .await
            .unwrap();
            assert_eq!(cancelled, "你好");
        }
    }
}
