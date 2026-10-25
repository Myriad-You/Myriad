//! Native tool protocol. Provider continuation data stays in server checkpoints,
//! never in AgentResponse or SSE. A truncated stream cannot authorize a tool.
use super::{AiAnalyzer, AiProvider};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub(crate) enum ToolMessage {
    User { content: String },
    Assistant { turn: ToolTurn },
    Tool { call: ToolCall, content: String },
}

/// Upper-bound token estimate for text no provider has counted yet: a non-ASCII character
/// (CJK, emoji) as one token, ASCII at three characters a token. Tokenizers pack English at
/// about four, so this stays above the real count without tripling Chinese or quadrupling
/// English the way bytes or characters did.
pub(crate) fn approx_tokens(text: &str) -> u64 {
    let (ascii, other) = text.chars().fold((0u64, 0u64), |(ascii, other), ch| {
        if ch.is_ascii() {
            (ascii + 1, other)
        } else {
            (ascii, other + 1)
        }
    });
    other + ascii.div_ceil(3)
}

fn wire_text(message: &ToolMessage) -> String {
    match message {
        ToolMessage::User { content } => content.clone(),
        ToolMessage::Assistant { turn } => turn.native.to_string(),
        ToolMessage::Tool { call, content } => format!("{}{}{}", call.id, call.name, content),
    }
}

/// Tokens the next turn will send for `history`. Everything up to the last answered turn is
/// what the provider itself counted for it (its input plus what it produced); only messages
/// after that are estimated. Compacted older results only make the real count smaller.
pub(crate) fn history_input_estimate(history: &[ToolMessage]) -> u64 {
    let anchor = history.iter().rposition(
        |message| matches!(message, ToolMessage::Assistant { turn } if turn.usage.is_some()),
    );
    let (base, rest) = match anchor {
        Some(index) => {
            let ToolMessage::Assistant { turn } = &history[index] else {
                unreachable!()
            };
            let usage = turn.usage.as_ref().unwrap();
            (
                usage.input_tokens + usage.output_tokens,
                &history[index + 1..],
            )
        }
        None => (0, history),
    };
    base + rest
        .iter()
        .map(|message| approx_tokens(&wire_text(message)))
        .sum::<u64>()
}

/// Characters of `history` as every provider receives it: an assistant turn goes out as its
/// native message only, a tool result as its content plus the call's id and name, never the
/// call's arguments again. Counting the stored struct instead sends each rewrite twice.
pub(crate) fn history_wire_chars(history: &[ToolMessage]) -> usize {
    history
        .iter()
        .map(|message| match message {
            ToolMessage::User { content } => content.chars().count(),
            ToolMessage::Assistant { turn } => turn.native.to_string().chars().count(),
            ToolMessage::Tool { call, content } => {
                content.chars().count() + call.id.chars().count() + call.name.chars().count()
            }
        })
        .sum()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ToolTurn {
    pub text: String,
    pub calls: Vec<ToolCall>,
    /// Complete provider assistant message (reasoning/signatures included).
    pub native: Value,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

const MAX_STREAM_BYTES: usize = 4 * 1024 * 1024;

/// The provider stopped the reply at its output allowance. Retrying the same request ends the
/// same way; the model has to do less per reply, so callers tell it rather than the user.
pub(crate) const OUTPUT_LIMIT_CUT: &str =
    "Model reply hit its output limit; no tools were executed";

/// Raw bytes one tool turn's response may take. A stream wraps every token in an event of a
/// few hundred bytes (more with reasoning details), so a fixed ceiling cut long, legitimate
/// turns off once the output allowance grew; it scales with what the turn may produce.
fn stream_limit(max_tokens: u32) -> usize {
    MAX_STREAM_BYTES.max(max_tokens as usize * 768)
}

/// A tool turn that reasons and then writes many edits can stream for longer than the
/// analyzer client's five-minute request timeout while producing the whole time. Streams are
/// ended by silence instead; the Work loop's active-time limit still bounds the task.
const TURN_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);
const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

pub(crate) fn request_body(
    provider: AiProvider,
    model: &str,
    system: &str,
    history: &[ToolMessage],
    tools: &[ToolDefinition],
    max_tokens: u32,
) -> Result<Value> {
    // A provider switch mid-run cannot silently discard required signatures.
    for message in history {
        if let ToolMessage::Assistant { turn } = message {
            if turn.provider != provider.as_str()
                || turn
                    .model
                    .as_deref()
                    .is_some_and(|previous| previous != model)
            {
                bail!(
                    "The model configuration changed during this task; restore its model and provider to continue"
                );
            }
        }
    }
    match provider {
        AiProvider::OpenAIResponses | AiProvider::Anthropic => {
            super::tool_protocol::request_body(provider, model, system, history, tools, max_tokens)
        }
        AiProvider::OpenAI => {
            let mut messages = vec![json!({"role":"system","content":system})];
            for message in history {
                messages.push(match message {
                    ToolMessage::User { content } => json!({"role":"user","content":content}),
                    ToolMessage::Assistant { turn } => turn.native.clone(),
                    ToolMessage::Tool { call, content } => {
                        json!({"role":"tool","tool_call_id":call.id,"content":content})
                    }
                });
            }
            Ok(
                json!({"model":model,"messages":messages,"stream":true,"stream_options":{"include_usage":true},"max_tokens":max_tokens,
                "tools":tools.iter().map(|tool| json!({"type":"function","function":tool})).collect::<Vec<_>>()}),
            )
        }
        AiProvider::Gemini => {
            let mut contents: Vec<Value> = Vec::new();
            for message in history {
                match message {
                    ToolMessage::User { content } => {
                        contents.push(json!({"role":"user","parts":[{"text":content}]}))
                    }
                    ToolMessage::Assistant { turn } => contents.push(turn.native.clone()),
                    ToolMessage::Tool { call, content } => {
                        let mut response = json!({"name":call.name,"response":{"result":content}});
                        // Generated ids are local correlation ids; do not invent provider ids.
                        if !call.id.starts_with("myriad_") {
                            response["id"] = json!(call.id);
                        }
                        let part = json!({"functionResponse":response});
                        if let Some(last) = contents.last_mut().filter(|v| {
                            v["role"] == "user" && v["parts"][0].get("functionResponse").is_some()
                        }) {
                            last["parts"].as_array_mut().unwrap().push(part);
                        } else {
                            contents.push(json!({"role":"user","parts":[part]}));
                        }
                    }
                }
            }
            Ok(
                json!({"systemInstruction":{"parts":[{"text":system}]},"contents":contents,
                "generationConfig":{"maxOutputTokens":max_tokens},
                "tools":[{"functionDeclarations":tools.iter().map(|t| json!({"name":t.name,"description":t.description,"parametersJsonSchema":t.parameters})).collect::<Vec<_>>()}]}),
            )
        }
    }
}

#[derive(Default, Clone)]
struct Accumulator {
    text: String,
    reasoning: String,
    reasoning_details: Vec<Value>,
    calls: BTreeMap<usize, ToolCall>,
    parts: Vec<Value>,
    finish: Option<String>,
    usage: Option<TokenUsage>,
}

impl Accumulator {
    /// What the model produced so far: text, reasoning and call arguments, without the
    /// transport's per-token event wrapping.
    fn produced_chars(&self) -> usize {
        self.text.len()
            + self.reasoning.len()
            + self
                .reasoning_details
                .iter()
                .map(|detail| detail.to_string().len())
                .sum::<usize>()
            + self
                .calls
                .values()
                .map(|call| call.name.len() + call.arguments.len())
                .sum::<usize>()
            + self
                .parts
                .iter()
                .map(|part| part.to_string().len())
                .sum::<usize>()
    }

    fn push(&mut self, provider: AiProvider, value: Value) -> Result<String> {
        if value.get("error").is_some() {
            bail!("Model provider returned an error");
        }
        let counts = match provider {
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                bail!("Native protocol requires a complete response")
            }
            AiProvider::OpenAI => value["usage"]["prompt_tokens"]
                .as_u64()
                .zip(value["usage"]["completion_tokens"].as_u64()),
            AiProvider::Gemini => {
                let usage = &value["usageMetadata"];
                usage["promptTokenCount"].as_u64().and_then(|input| {
                    let output = usage["totalTokenCount"]
                        .as_u64()
                        .map(|total| total.saturating_sub(input))
                        .or_else(|| {
                            usage["candidatesTokenCount"].as_u64().map(|count| {
                                count.saturating_add(
                                    usage["thoughtsTokenCount"].as_u64().unwrap_or(0),
                                )
                            })
                        });
                    output.map(|output| (input, output))
                })
            }
        };
        if let Some((input_tokens, output_tokens)) = counts {
            self.usage = Some(TokenUsage {
                input_tokens,
                output_tokens,
            });
        }
        let mut visible = String::new();
        match provider {
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                bail!("Native protocol requires a complete response")
            }
            AiProvider::OpenAI => {
                let Some(choice) = value["choices"].as_array().and_then(|c| c.first()) else {
                    return Ok(visible);
                };
                let delta = &choice["delta"];
                if let Some(text) = delta["content"].as_str() {
                    visible.push_str(text);
                    self.text.push_str(text);
                }
                if let Some(text) = delta["reasoning_content"]
                    .as_str()
                    .or_else(|| delta["reasoning"].as_str())
                {
                    self.reasoning.push_str(text);
                }
                if let Some(details) = delta["reasoning_details"].as_array() {
                    for detail in details {
                        // Text and summary deltas form one block. Encrypted
                        // blocks are opaque and must never be concatenated.
                        let field = match detail["type"].as_str() {
                            Some("reasoning.text") => Some("text"),
                            Some("reasoning.summary") => Some("summary"),
                            _ => None,
                        };
                        let previous = self.reasoning_details.last_mut().filter(|previous| {
                            field.is_some()
                                && previous["type"] == detail["type"]
                                && previous["index"] == detail["index"]
                                && (detail["id"].is_null()
                                    || previous["id"].is_null()
                                    || previous["id"] == detail["id"])
                        });
                        if let (Some(previous), Some(field)) = (previous, field) {
                            let text = format!(
                                "{}{}",
                                previous[field].as_str().unwrap_or_default(),
                                detail[field].as_str().unwrap_or_default()
                            );
                            for (key, value) in detail.as_object().unwrap() {
                                if !value.is_null() && key != field {
                                    previous[key] = value.clone();
                                }
                            }
                            previous[field] = json!(text);
                        } else {
                            self.reasoning_details.push(detail.clone());
                        }
                    }
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for part in calls {
                        let index = part["index"]
                            .as_u64()
                            .ok_or_else(|| anyhow::anyhow!("Tool stream has no index"))?
                            as usize;
                        if index >= 16 {
                            bail!("Too many tool calls in one model turn");
                        }
                        let call = self.calls.entry(index).or_insert(ToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: String::new(),
                        });
                        if let Some(id) = part["id"].as_str() {
                            call.id.push_str(id);
                        }
                        if let Some(name) = part["function"]["name"].as_str() {
                            call.name.push_str(name);
                        }
                        if let Some(args) = part["function"]["arguments"].as_str() {
                            call.arguments.push_str(args);
                        }
                    }
                }
                if let Some(reason) = choice["finish_reason"].as_str() {
                    self.finish = Some(reason.into());
                }
            }
            AiProvider::Gemini => {
                let Some(candidate) = value["candidates"].as_array().and_then(|c| c.first()) else {
                    return Ok(visible);
                };
                if let Some(parts) = candidate["content"]["parts"].as_array() {
                    for part in parts {
                        if part["thought"] != true {
                            if let Some(text) = part["text"].as_str() {
                                visible.push_str(text);
                                self.text.push_str(text);
                            }
                        }
                        if let Some(function) = part.get("functionCall") {
                            let index = self.calls.len();
                            if index >= 16 {
                                bail!("Too many tool calls in one model turn");
                            }
                            self.calls.insert(
                                index,
                                ToolCall {
                                    id: function["id"].as_str().map(str::to_owned).unwrap_or_else(
                                        || format!("myriad_{}", uuid::Uuid::new_v4().simple()),
                                    ),
                                    name: function["name"].as_str().unwrap_or_default().into(),
                                    arguments: function
                                        .get("args")
                                        .cloned()
                                        .unwrap_or(json!({}))
                                        .to_string(),
                                },
                            );
                        }
                        // Keep each signed part in its original position, including empty text.
                        self.parts.push(part.clone());
                    }
                }
                if let Some(reason) = candidate["finishReason"].as_str() {
                    self.finish = Some(reason.into());
                }
            }
        }
        Ok(visible)
    }

    fn finish(self, provider: AiProvider) -> Result<ToolTurn> {
        if matches!(self.finish.as_deref(), Some("length" | "MAX_TOKENS")) {
            bail!("{OUTPUT_LIMIT_CUT}");
        }
        if !matches!(self.finish.as_deref(), Some("stop" | "tool_calls" | "STOP")) {
            bail!("Model response was incomplete; no tools were executed");
        }
        let calls: Vec<_> = self.calls.into_values().collect();
        let mut ids = HashSet::new();
        for call in &calls {
            if call.id.is_empty() || call.name.is_empty() || !ids.insert(&call.id) {
                bail!("Model returned missing or duplicate tool call identifiers");
            }
        }
        if calls.is_empty() && self.text.trim().is_empty() {
            bail!("Model returned no answer or tool calls");
        }
        let native = match provider {
            AiProvider::OpenAIResponses | AiProvider::Anthropic => {
                bail!("Native protocol requires a complete response")
            }
            AiProvider::OpenAI => {
                let mut message = json!({"role":"assistant","content":self.text});
                if !calls.is_empty() {
                    message["tool_calls"] = json!(calls.iter().map(|c| json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments}})).collect::<Vec<_>>());
                }
                if !self.reasoning.is_empty() {
                    message["reasoning_content"] = json!(self.reasoning);
                }
                if !self.reasoning_details.is_empty() {
                    message["reasoning_details"] = json!(self.reasoning_details);
                }
                message
            }
            AiProvider::Gemini => json!({"role":"model","parts":self.parts}),
        };
        Ok(ToolTurn {
            text: self.text,
            calls,
            native,
            provider: provider.as_str().into(),
            model: None,
            usage: self.usage,
        })
    }
}

impl AiAnalyzer {
    pub(crate) async fn tool_turn<F, Fut>(
        &self,
        system: &str,
        history: &[ToolMessage],
        tools: &[ToolDefinition],
        max_tokens: u32,
        mut on_text: F,
    ) -> Result<ToolTurn>
    where
        F: FnMut(String) -> Fut + Send,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let body = request_body(
            self.provider,
            &self.model,
            system,
            history,
            tools,
            max_tokens,
        )?;
        let input_bytes = body.to_string().len();
        let mut acc = Accumulator::default();
        let mut received_bytes = 0usize;
        let result = async {
            if matches!(
                self.provider,
                AiProvider::OpenAIResponses | AiProvider::Anthropic
            ) {
                let url = super::text_protocol::endpoint(self.provider, self.base_url.as_deref());
                let mut request = self.authenticate(self.client.post(url));
                if self.provider == AiProvider::Anthropic {
                    request = request.header("anthropic-version", "2023-06-01");
                }
                let response = request
                    .timeout(TURN_REQUEST_TIMEOUT)
                    .json(&super::request_budget::prepare(&body, self.provider)?)
                    .send()
                    .await
                    .map_err(|_| anyhow::anyhow!("Model connection failed"))?;
                if !response.status().is_success() {
                    bail!(
                        "Model tool API returned HTTP {}",
                        response.status().as_u16()
                    );
                }
                let bytes = crate::services::outbound_security::read_limited_body(
                    response,
                    stream_limit(max_tokens),
                )
                .await
                .map_err(|_| {
                    anyhow::anyhow!("Model response could not be read within its size limit")
                })?;
                received_bytes = bytes.len();
                let value = serde_json::from_slice(&bytes)
                    .map_err(|_| anyhow::anyhow!("Invalid model response"))?;
                let turn = super::tool_protocol::response_turn(self.provider, &self.model, &value)?;
                if !turn.text.is_empty() {
                    on_text(turn.text.clone()).await;
                }
                return Ok(turn);
            }
            let url = match self.provider {
                AiProvider::OpenAI => super::openai_chat_completions_url(self.base_url.as_deref()),
                AiProvider::Gemini => self.gemini_url(true).await,
                _ => unreachable!("native protocols handled above"),
            };
            let mut response = self
                .authenticate(self.client.post(url))
                .timeout(TURN_REQUEST_TIMEOUT)
                .json(&super::request_budget::prepare(&body, self.provider)?)
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("Model connection failed"))?;
            if !response.status().is_success() {
                // Provider error bodies can echo credentials or request content.
                bail!(
                    "Model tool API returned HTTP {}",
                    response.status().as_u16()
                );
            }
            let mut bytes = Vec::new();
            let mut total = 0;
            while let Some(chunk) = tokio::time::timeout(STREAM_IDLE_TIMEOUT, response.chunk())
                .await
                .map_err(|_| anyhow::anyhow!("Model stream went silent"))?
                .map_err(|_| anyhow::anyhow!("Model stream interrupted"))?
            {
                total += chunk.len();
                received_bytes = total;
                if total > stream_limit(max_tokens) {
                    bail!("Model response exceeded the stream limit");
                }
                bytes.extend_from_slice(&chunk);
                // Decode complete lines only: UTF-8 characters can span HTTP chunks.
                while let Some(end) = bytes.iter().position(|b| *b == b'\n') {
                    let line = bytes.drain(..=end).collect::<Vec<_>>();
                    let line = std::str::from_utf8(&line)?.trim();
                    if let Some(data) = line.strip_prefix("data:") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            continue;
                        }
                        let text = acc.push(self.provider, serde_json::from_str(data)?)?;
                        if !text.is_empty() {
                            on_text(text).await;
                        }
                    }
                }
            }
            if !bytes.iter().all(u8::is_ascii_whitespace) {
                bail!("Model stream ended inside an event");
            }
            let mut turn = acc.clone().finish(self.provider)?;
            turn.model = Some(self.model.clone());
            Ok(turn)
        }
        .await;
        let usage = result
            .as_ref()
            .ok()
            .and_then(|turn| turn.usage.as_ref())
            .or(acc.usage.as_ref());
        let input_tokens = usage
            .map(|usage| usage.input_tokens)
            .unwrap_or(input_bytes.div_ceil(4) as u64);
        // Without the provider's count, estimate from what the model produced. Raw stream
        // bytes wrap every token in an event and billed a failed 5 MB stream as 1.3M tokens.
        let output_tokens = usage.map(|usage| usage.output_tokens).unwrap_or_else(|| {
            result
                .as_ref()
                .map(|turn| turn.native.to_string().len())
                .unwrap_or_else(|_| acc.produced_chars())
                .div_ceil(4) as u64
        });
        crate::services::ai_cost_ledger::record_ai_tokens_from_attribution(
            self.provider.as_str(),
            &self.model,
            input_tokens.min(i32::MAX as u64) as i32,
            output_tokens.min(i32::MAX as u64) as i32,
            if result.is_ok() {
                "completed"
            } else {
                "failed"
            },
            if result.is_ok() {
                None
            } else {
                Some("AI_PROVIDER_ERROR")
            },
        )
        .await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_usage_survives_empty_final_frames_and_counts_thinking() {
        let mut acc = Accumulator::default();
        acc.push(
            AiProvider::OpenAI,
            json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}),
        )
        .unwrap();
        acc.push(AiProvider::OpenAI, json!({"choices":[],"usage":{"prompt_tokens":123,"completion_tokens":45,"total_tokens":168}})).unwrap();
        let turn = serde_json::to_value(acc.finish(AiProvider::OpenAI).unwrap()).unwrap();
        assert_eq!(turn["usage"]["input_tokens"], 123);
        assert_eq!(turn["usage"]["output_tokens"], 45);
        let mut acc = Accumulator::default();
        acc.push(AiProvider::Gemini, json!({"candidates":[{"content":{"parts":[{"text":"done"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":10,"thoughtsTokenCount":30,"totalTokenCount":60}})).unwrap();
        let turn = serde_json::to_value(acc.finish(AiProvider::Gemini).unwrap()).unwrap();
        assert_eq!(turn["usage"]["input_tokens"], 20);
        assert_eq!(turn["usage"]["output_tokens"], 40);
    }

    /// Opt-in only: two small model calls, no application tools or user content.
    /// Reads the existing site's configuration without running migrations.
    #[tokio::test]
    #[ignore = "requires MYRIAD_WORK_LIVE_SMOKE=1 and the site's existing runtime environment"]
    async fn configured_provider_native_tool_round_trip() {
        assert_eq!(std::env::var("MYRIAD_WORK_LIVE_SMOKE").as_deref(), Ok("1"));
        crate::services::data_key::init_existing()
            .expect("existing site data key must be available; never create one for a probe");
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be configured");
        let db = sea_orm::Database::connect(url)
            .await
            .expect("configuration database must be reachable");
        let config = crate::services::config_service::ConfigService::new(db)
            .load_config()
            .await
            .expect("read existing model configuration");
        *crate::GLOBAL_DYNAMIC_CONFIG.write().await = config;
        let analyzer = crate::services::ai::create_ai_analyzer_for_tier_with_timeout(
            crate::config::ModelTier::Pro,
            Some(std::time::Duration::from_secs(60)),
        )
        .await
        .expect("Work model must be configured");
        let tools = vec![ToolDefinition {
            name: "read_probe_value".into(),
            description: "Read the current synthetic probe value. No arguments.".into(),
            parameters: json!({"type":"object","properties":{},"additionalProperties":false}),
        }];
        let system = "You are testing a native tool transport. First call read_probe_value exactly once. After its result, return only the value it provided. Do not guess that value or make additional calls.";
        let mut history = vec![ToolMessage::User {
            content: "Read the probe value.".into(),
        }];
        let first = analyzer
            .tool_turn(system, &history, &tools, 2048, |_| async {})
            .await
            .expect("configured provider must return a complete native tool call");
        assert_eq!(first.calls.len(), 1);
        assert_eq!(first.calls[0].name, "read_probe_value");
        let call = first.calls[0].clone();
        let secret_probe = uuid::Uuid::new_v4().to_string();
        history.push(ToolMessage::Assistant { turn: first });
        history.push(ToolMessage::Tool {
            call,
            content: json!({"value":secret_probe}).to_string(),
        });
        let final_turn = analyzer
            .tool_turn(system, &history, &tools, 2048, |_| async {})
            .await
            .expect("provider must accept its own signed tool continuation");
        assert!(final_turn.calls.is_empty());
        assert!(
            final_turn.text.contains(&secret_probe),
            "final answer must use the actual tool result"
        );
    }

    #[test]
    fn fragmented_calls_preserve_reasoning_and_call_identity() {
        let mut acc = Accumulator::default();
        acc.push(AiProvider::OpenAI,json!({"choices":[{"delta":{"reasoning_content":"private", "tool_calls":[{"index":0,"id":"call_1","function":{"name":"lookup","arguments":"{\"q\":"}}]}}]})).unwrap();
        acc.push(AiProvider::OpenAI,json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"東京\"}"}}]},"finish_reason":"tool_calls"}]})).unwrap();
        let turn = acc.finish(AiProvider::OpenAI).unwrap();
        assert_eq!(turn.calls[0].arguments, "{\"q\":\"東京\"}");
        let call = turn.calls[0].clone();
        let body = request_body(
            AiProvider::OpenAI,
            "model",
            "system",
            &[
                ToolMessage::Assistant { turn },
                ToolMessage::Tool {
                    call,
                    content: "found".into(),
                },
            ],
            &[],
            100,
        )
        .unwrap();
        assert_eq!(body["messages"][1]["reasoning_content"], "private");
        assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
    }

    #[test]
    fn incomplete_stream_never_produces_executable_calls() {
        let mut acc = Accumulator::default();
        acc.push(AiProvider::OpenAI,json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"write","arguments":"{}"}}]}}]})).unwrap();
        assert!(acc.finish(AiProvider::OpenAI).is_err());
    }

    #[test]
    fn signed_reasoning_deltas_merge_without_merging_encrypted_blocks() {
        let mut acc = Accumulator::default();
        for detail in [
            json!({"type":"reasoning.text","index":0,"id":"r","text":"first ","signature":null}),
            json!({"type":"reasoning.text","index":0,"id":"r","text":"second","signature":"signed"}),
            json!({"type":"reasoning.encrypted","index":1,"id":"e1","data":"opaque1"}),
            json!({"type":"reasoning.encrypted","index":2,"id":"e2","data":"opaque2"}),
        ] {
            acc.push(
                AiProvider::OpenAI,
                json!({"choices":[{"delta":{"reasoning_details":[detail]}}]}),
            )
            .unwrap();
        }
        acc.push(
            AiProvider::OpenAI,
            json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}),
        )
        .unwrap();
        let native = acc.finish(AiProvider::OpenAI).unwrap().native;
        assert_eq!(native["reasoning_details"].as_array().unwrap().len(), 3);
        assert_eq!(native["reasoning_details"][0]["text"], "first second");
        assert_eq!(native["reasoning_details"][0]["signature"], "signed");
        assert_eq!(native["reasoning_details"][1]["data"], "opaque1");
    }

    #[tokio::test]
    async fn native_http_handles_utf8_chunks_and_redacts_provider_errors() {
        use axum::{
            body::{Body, Bytes},
            http::StatusCode,
            response::IntoResponse,
            routing::post,
        };
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            post(|axum::Json(body): axum::Json<Value>| async move {
                if body["model"] == "error" {
                    return (StatusCode::UNAUTHORIZED, "secret-from-provider-body").into_response();
                }
                let sse = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"delta":{"content":"東京"},"finish_reason":"stop"}]})
                );
                let chunks: Vec<_> = sse
                    .into_bytes()
                    .into_iter()
                    .map(|byte| Ok::<_, std::io::Error>(Bytes::from(vec![byte])))
                    .collect();
                (
                    [("content-type", "text/event-stream")],
                    Body::from_stream(futures::stream::iter(chunks)),
                )
                    .into_response()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut analyzer = AiAnalyzer::new(
            AiProvider::OpenAI,
            "secret-test-key".into(),
            "ok".into(),
            Some(format!("http://{address}/v1")),
        )
        .await;
        let turn = analyzer
            .tool_turn("system", &[], &[], 100, |_| async {})
            .await
            .unwrap();
        assert_eq!(turn.text, "東京");
        analyzer.model = "error".into();
        let error = analyzer
            .tool_turn("system", &[], &[], 100, |_| async {})
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("401"));
        assert!(!error.contains("secret"));
        server.abort();
    }

    #[test]
    fn gemini_signed_parts_round_trip_without_merging() {
        let parts = json!([{"text":"","thoughtSignature":"s1"},{"functionCall":{"id":"fc1","name":"read","args":{}},"thoughtSignature":"s2"}]);
        let mut acc = Accumulator::default();
        acc.push(
            AiProvider::Gemini,
            json!({"candidates":[{"content":{"parts":parts},"finishReason":"STOP"}]}),
        )
        .unwrap();
        let turn = acc.finish(AiProvider::Gemini).unwrap();
        let call = turn.calls[0].clone();
        let history = vec![
            ToolMessage::Assistant { turn },
            ToolMessage::Tool {
                call,
                content: "ok".into(),
            },
        ];
        let body = request_body(AiProvider::Gemini, "model", "system", &history, &[], 100).unwrap();
        assert_eq!(body["contents"][0]["parts"], parts);
        assert_eq!(
            body["contents"][1]["parts"][0]["functionResponse"]["id"],
            "fc1"
        );
        assert!(request_body(AiProvider::OpenAI, "model", "system", &history, &[], 100).is_err());
    }
}

#[cfg(test)]
mod wire_chars_tests {
    use super::*;

    #[test]
    fn a_reply_cut_at_the_output_limit_says_so() {
        for (provider, chunk) in [
            (
                AiProvider::OpenAI,
                serde_json::json!({"choices":[{"delta":{"content":"半"},"finish_reason":"length"}]}),
            ),
            (
                AiProvider::Gemini,
                serde_json::json!({"candidates":[{"content":{"parts":[{"text":"半"}]},"finishReason":"MAX_TOKENS"}]}),
            ),
        ] {
            let mut acc = Accumulator::default();
            acc.push(provider, chunk).unwrap();
            assert_eq!(
                acc.finish(provider).unwrap_err().to_string(),
                OUTPUT_LIMIT_CUT
            );
        }
    }

    #[test]
    fn estimates_stay_above_real_counts_without_tripling() {
        assert_eq!(approx_tokens("排版"), 2);
        assert_eq!(approx_tokens("abcdef"), 2);
        assert_eq!(approx_tokens("abcdefg"), 3);
    }

    #[test]
    fn the_next_turn_is_anchored_on_what_the_provider_counted() {
        let turn = |usage: Option<TokenUsage>| ToolMessage::Assistant {
            turn: ToolTurn {
                text: String::new(),
                calls: vec![],
                native: serde_json::json!({"reasoning": "想".repeat(40_000)}),
                provider: "openai".into(),
                model: None,
                usage,
            },
        };
        let result = ToolMessage::Tool {
            call: ToolCall {
                id: "c".into(),
                name: "n".into(),
                arguments: "{}".into(),
            },
            content: "结果".into(),
        };
        let counted = vec![
            ToolMessage::User {
                content: "x".repeat(90_000),
            },
            turn(Some(TokenUsage {
                input_tokens: 9_000,
                output_tokens: 8_000,
            })),
            result.clone(),
        ];
        // The long prompt and the long reasoning are the provider's 17k, not 130k characters.
        assert_eq!(
            history_input_estimate(&counted),
            17_000 + approx_tokens("cn结果")
        );
        // Without a count yet, everything is estimated.
        let uncounted = vec![ToolMessage::User {
            content: "x".repeat(90_000),
        }];
        assert_eq!(history_input_estimate(&uncounted), 30_000);
    }

    #[test]
    fn a_rewrite_counts_once_as_sent_not_twice_as_stored() {
        let body = "排版".repeat(500);
        let arguments = serde_json::json!({ "content_md": body }).to_string();
        let call = ToolCall {
            id: "c1".into(),
            name: "update".into(),
            arguments: arguments.clone(),
        };
        let native = serde_json::json!({"role":"assistant","tool_calls":[{"function":{"arguments":arguments}}]});
        let history = vec![
            ToolMessage::Assistant {
                turn: ToolTurn {
                    text: String::new(),
                    calls: vec![call.clone()],
                    native: native.clone(),
                    provider: "openai".into(),
                    model: None,
                    usage: None,
                },
            },
            ToolMessage::Tool {
                call,
                content: "{\"success\":true}".into(),
            },
        ];
        let sent = history_wire_chars(&history);
        let stored = serde_json::to_string(&history).unwrap().chars().count();
        assert!(sent >= native.to_string().chars().count());
        assert!(sent < native.to_string().chars().count() + 40);
        assert!(
            stored > sent + 1000,
            "the stored form repeats the arguments"
        );
    }
}
