//! Asking a model on her behalf: which voice, billed to whom under what
//! name, and the answer as JSON, as text, or as something she says.
//!
//! A judgment (what to pick, whether a step is worth taking, whether a guess
//! held) goes to the fast judging model. Anything in her own words goes to
//! her voice, thinking a little as her chat replies do, or, where a call
//! needs it, as long as the provider likes. Making sense of what people
//! mean, and what she keeps of it, is thought through.
//!
//! This is the one place Merope reaches the model and the cost ledger. The
//! live appraisal keeps its own analyzer for its timing probes.

use std::sync::atomic::{AtomicI64, Ordering::Relaxed};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::services::analyzer::{AiAnalyzer, JsonShape};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Voice {
    /// The fast judging model.
    Judge,
    /// Her voice, thinking a little.
    Hers,
    /// Her voice, thinking as long as the provider likes.
    HersAtLength,
    /// Her voice, thinking it through: making sense of what people mean
    /// (who someone is, what a word or a bit means here). Slower; for calls
    /// where a wrong reading costs more than the wait.
    Thorough,
    /// Her voice, keeping what was said: writing it down, settling it after
    /// a chat, keeping her topics in order. The model thinks as it does by
    /// default (Haiku 5.5: adaptively, at effort medium); asked to think it
    /// through, the memory bench kept no better for a fifth more.
    Keeping,
    /// The stronger model, thinking it through: reading a stretch of a
    /// group's talk once, which everything she keeps of it rests on.
    Reading,
    /// The model the chat answers with (the Standard tier): a careful look
    /// at what the owner's own sessions ask of them, which they may act on.
    Chat,
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);

/// Answers cut off at their output limit since the site started, by
/// operation: counts only.
static CUT_OFF: std::sync::Mutex<std::collections::BTreeMap<&'static str, u64>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// When a judgment of hers was last answered, since the site started (0:
/// none yet).
#[cfg(not(test))]
fn judged_at<T>(of: impl FnOnce(&AtomicI64) -> T) -> T {
    static AT: AtomicI64 = AtomicI64::new(0);
    of(&AT)
}
/// Each test's own.
#[cfg(test)]
fn judged_at<T>(of: impl FnOnce(&AtomicI64) -> T) -> T {
    thread_local!(static AT: AtomicI64 = const { AtomicI64::new(0) });
    AT.with(of)
}

/// Whether a judgment of hers was answered after `at`, since the site
/// started: what is refused all the while is refused for what it says, and
/// not for who asks.
pub(super) fn judged_since(at: i64) -> bool {
    judged_at(|last| last.load(Relaxed)) > at
}

/// Judgments answered last at `at`, as a test has it.
#[cfg(test)]
pub(super) fn judged_last(at: i64) {
    judged_at(|last| last.store(at, Relaxed));
}

/// How many answers of each operation were cut off at their output limit
/// since the site started, for her status.
pub(crate) fn cut_off() -> std::collections::BTreeMap<&'static str, u64> {
    CUT_OFF
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Safe, structured outcome. Provider bodies and credentials never enter job records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Failure {
    Unavailable,
    Timeout,
    Transport,
    Upstream,
    RateLimited,
    Rejected,
    /// Refused, it may be for what was shown (a content filter, too long),
    /// it may be for who asks: its status does not say which
    /// ([`judged_since`] does).
    Refused,
    InvalidOutput,
    StreamInterrupted,
    Quota,
    /// A JSON answer that stopped at its output limit: what came is not the
    /// whole of it, and is not read.
    CutOff,
    Unknown,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "model_unavailable",
            Self::Timeout => "model_timeout",
            Self::Transport => "model_transport",
            Self::Upstream => "model_upstream",
            Self::RateLimited => "model_rate_limited",
            Self::Rejected => "model_rejected",
            Self::Refused => "model_refused",
            Self::InvalidOutput => "model_invalid_output",
            Self::StreamInterrupted => "model_stream_interrupted",
            Self::Quota => "model_quota",
            Self::CutOff => "model_cut_off",
            Self::Unknown => "model_unknown_failure",
        })
    }
}
impl std::error::Error for Failure {}

impl Failure {
    pub(super) fn retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::Transport
                | Self::Upstream
                | Self::StreamInterrupted
                | Self::CutOff
        )
    }

    /// Asked, with no answer that will do, for what it was shown: none that
    /// reads, or refused. Not that it could not be asked now (nothing there,
    /// the account refused, too many asks, no answer in time): that says
    /// nothing of what it was shown.
    pub(super) fn of_what_was_shown(self) -> bool {
        matches!(self, Self::Refused | Self::InvalidOutput | Self::CutOff)
    }

    fn status(status: reqwest::StatusCode) -> Self {
        match status.as_u16() {
            400 | 403 | 413 | 422 => Self::Refused,
            408 | 504 => Self::Timeout,
            429 => Self::RateLimited,
            500..=599 => Self::Upstream,
            _ => Self::Rejected,
        }
    }

    pub(super) fn classify(error: &anyhow::Error) -> Self {
        for cause in error.chain() {
            if let Some(error) = cause.downcast_ref::<reqwest::Error>() {
                if error.is_timeout() {
                    return Self::Timeout;
                }
                if let Some(status) = error.status() {
                    return Self::status(status);
                }
                if error.is_decode() {
                    return Self::InvalidOutput;
                }
                // A request that never got its answer is the network's as
                // a refused connection is: one held open that drops while
                // it is sent ("connection error: timed out"), or a source
                // that closes it before replying.
                if error.is_connect() || error.is_body() || error.is_request() {
                    return Self::Transport;
                }
            }
            if let Some(error) =
                cause.downcast_ref::<crate::services::analyzer::ProviderHttpError>()
            {
                return Self::status(error.status);
            }
            if cause.is::<serde_json::Error>() {
                return Self::InvalidOutput;
            }
            if cause.is::<crate::services::analyzer::StreamCut>() {
                return Self::StreamInterrupted;
            }
            if cause.is::<crate::services::ai_quota::AiQuotaError>() {
                return Self::Quota;
            }
        }
        Self::Unknown
    }
}

/// A call to make: whose voice, billed to whom, and what it is called.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ask {
    voice: Voice,
    owner: i32,
    operation: &'static str,
    source: &'static str,
    timeout: Duration,
    /// How its JSON answer is held to its schema.
    shape: JsonShape,
    /// The output limit it names, thinking included; see [`Ask::output_at_most`].
    output: Option<u32>,
    /// The part of its system prompt the provider caches; see [`Ask::cached`].
    cached: Option<crate::services::analyzer::CachedSystem>,
}

/// Writing down what someone said ([`myriad_merope::chat_remember::extract_system_prompt`]):
/// its instructions and examples, before what she knows of them.
pub(super) const REMEMBER_CACHE: crate::services::analyzer::CachedSystem =
    crate::services::analyzer::CachedSystem {
        until: Some("Known:\n"),
        session: "memory-remember",
    };
/// Going over a stretch ([`myriad_merope::chat_remember::settle_system_prompt`]):
/// its instructions, before what she knows for good.
pub(super) const SETTLE_CACHE: crate::services::analyzer::CachedSystem =
    crate::services::analyzer::CachedSystem {
        until: Some("Known for good:\n"),
        session: "memory-settle",
    };
/// How things stand on a topic: its system prompt is the same throughout.
/// Long enough to cache with the schema it is asked with, which the
/// provider puts before it; asked without one, it is not cached.
pub(super) const STANDING_CACHE: crate::services::analyzer::CachedSystem =
    crate::services::analyzer::CachedSystem {
        until: None,
        session: "memory-standing",
    };

impl Ask {
    /// Billed to `owner` under Merope, as `operation`.
    pub(super) fn new(voice: Voice, owner: i32, operation: &'static str) -> Self {
        Self {
            voice,
            owner,
            operation,
            source: "merope",
            timeout: DEFAULT_TIMEOUT,
            shape: JsonShape::ByProvider,
            output: None,
            cached: None,
        }
    }

    /// For the memory writing steps, made again and again with the same
    /// instructions first: that part of the system prompt cached by the
    /// provider. Her talk is not cached.
    pub(super) fn cached(self, cached: crate::services::analyzer::CachedSystem) -> Self {
        Self {
            cached: Some(cached),
            ..self
        }
    }

    /// Its JSON answer held to the schema by the provider itself, where the
    /// provider can (a schema every object of which says
    /// `additionalProperties: false`).
    pub(super) fn strict(self) -> Self {
        Self {
            shape: JsonShape::Strict,
            ..self
        }
    }

    /// Its schema said in the prompt alone, whatever answers: for a step
    /// whose empty answer is a whole one, and does harm. Held to the schema
    /// by the provider, the model gives that one far more.
    pub(super) fn shape_in_prompt(self) -> Self {
        Self {
            shape: JsonShape::InPrompt,
            ..self
        }
    }

    /// For a call whose answers were measured: at most `tokens` of output,
    /// thinking included, in place of the default it would be let have
    /// (4096, or 16000 thinking it through). Set well above what was
    /// measured: an answer cut off is lost.
    pub(super) fn output_at_most(self, tokens: u32) -> Self {
        Self {
            output: Some(tokens),
            ..self
        }
    }

    pub(super) fn within(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Billed under another source than Merope.
    pub(super) fn billed_as(self, source: &'static str) -> Self {
        Self { source, ..self }
    }

    /// The configured model, or an observable configuration failure.
    pub(super) async fn model(self) -> Result<Model, Failure> {
        // Not while her mind is on another core: she thinks there.
        let Some(thought) = crate::services::her_log::pen::think() else {
            tracing::info!(
                owner = self.owner,
                operation = self.operation,
                "[Merope] model not asked: her mind is on another device"
            );
            return Err(Failure::Unavailable);
        };
        let analyzer = match self.voice {
            Voice::Judge => {
                crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(self.timeout))
                    .await
                    .ok_or_else(|| self.failed(Failure::Unavailable))?
            }
            Voice::Hers => {
                crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(self.timeout))
                    .await
                    .ok_or_else(|| self.failed(Failure::Unavailable))?
                    .with_light_thinking()
            }
            Voice::HersAtLength => {
                crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(self.timeout))
                    .await
                    .ok_or_else(|| self.failed(Failure::Unavailable))?
            }
            Voice::Keeping => {
                let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(
                    Some(self.timeout),
                )
                .await
                .ok_or_else(|| self.failed(Failure::Unavailable))?;
                keeping(analyzer)
            }
            Voice::Thorough => {
                crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(self.timeout))
                    .await
                    .ok_or_else(|| self.failed(Failure::Unavailable))?
                    .with_deep_thinking()
            }
            Voice::Reading => crate::services::ai::create_ai_analyzer_for_tier_with_timeout(
                crate::config::ModelTier::Pro,
                Some(self.timeout),
            )
            .await
            .ok_or_else(|| self.failed(Failure::Unavailable))?
            .with_deep_thinking(),
            Voice::Chat => crate::services::ai::create_ai_analyzer_for_tier_with_timeout(
                crate::config::ModelTier::Standard,
                Some(self.timeout),
            )
            .await
            .ok_or_else(|| self.failed(Failure::Unavailable))?,
        };
        let analyzer = match self.output {
            Some(tokens) => analyzer.with_output_limit(tokens),
            None => analyzer,
        };
        let analyzer = match self.cached {
            Some(cached) => analyzer.with_cached_system(cached),
            None => analyzer,
        };
        Ok(Model {
            analyzer,
            ask: self,
            _thought: thought,
        })
    }

    /// What an answer's source said of it: whether it stopped at its output
    /// limit (thinking counts against it).
    fn answered(
        &self,
        reported: &crate::services::ai_cost_ledger::Reported,
        chars: Option<usize>,
    ) -> bool {
        let cut_off = reported
            .stops
            .iter()
            .any(|stop| matches!(stop.as_str(), "max_tokens" | "length" | "MAX_TOKENS"));
        if cut_off {
            self.cut_off(Some(reported.usage.output), chars);
        }
        cut_off
    }

    /// An answer cut off at its output limit, told in one line and counted
    /// by operation for her status: counts only, never what was said.
    fn cut_off(&self, output_tokens: Option<u64>, chars: Option<usize>) {
        tracing::warn!(
            owner = self.owner,
            operation = self.operation,
            output_tokens,
            chars,
            "[Merope] answer cut off at its output limit"
        );
        let mut counts = CUT_OFF
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *counts.entry(self.operation).or_default() += 1;
    }

    /// Answered, as a judgment: noted when.
    fn judged(&self) {
        if self.voice == Voice::Judge {
            let now = crate::clock::now().timestamp();
            judged_at(|last| last.store(now, Relaxed));
        }
    }

    fn failed(self, failure: Failure) -> Failure {
        tracing::warn!(owner = self.owner, operation = self.operation, source = self.source,
            outcome = %failure, retryable = failure.retryable(), "[Merope] model call failed");
        crate::services::agent::alive::model_failed(self.operation);
        // An answer that cannot be read, or one cut off, is still an answer:
        // her status counts calls with none at all.
        if matches!(failure, Failure::InvalidOutput | Failure::CutOff) {
            crate::services::agent::alive::model_answered();
        } else {
            crate::services::agent::alive::model_unanswered();
        }
        failure
    }

    /// Raw JSON; failures retain their class for retry and observability.
    pub(super) async fn json_raw(
        self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<String, Failure> {
        #[cfg(test)]
        if let Some(answer) =
            crate::services::channel_group::g0::model(self.operation, system, input)
        {
            // A model that takes its time, as the scene has it: past the
            // ask's own limit, it timed out.
            if let Some(delay) = crate::services::channel_group::g0::delay(self.operation) {
                let delay = Duration::from_secs_f64(delay);
                if delay > self.timeout {
                    tokio::time::sleep(self.timeout).await;
                    return Err(Failure::Timeout);
                }
                tokio::time::sleep(delay).await;
            }
            // One that stops at its output limit, as the scene has it.
            if crate::services::channel_group::g0::cut_off(self.operation) {
                self.cut_off(None, None);
                return Err(self.failed(Failure::CutOff));
            }
            // One that fails otherwise, named as the answer.
            match answer.as_deref() {
                Some("!rejected") => return Err(Failure::Rejected),
                Some("!refused") => return Err(Failure::Refused),
                Some("!quota") => return Err(Failure::Quota),
                Some("!rateLimited") => return Err(Failure::RateLimited),
                _ => {}
            }
            let answer = answer.ok_or(Failure::Unavailable)?;
            self.judged();
            return Ok(answer);
        }
        self.model()
            .await?
            .json(system, input, schema_name, schema)
            .await
    }

    /// [`Ask::json_raw`], read as `T`.
    pub(super) async fn json<T: for<'de> Deserialize<'de>>(
        self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<T, Failure> {
        parse(&self.json_raw(system, input, schema_name, schema).await?)
            .ok_or_else(|| self.failed(Failure::InvalidOutput))
    }
}

/// A model ready for one kind of call, billed as its [`Ask`] says.
pub(super) struct Model {
    analyzer: AiAnalyzer,
    ask: Ask,
    /// Held while the model is in use: a handover of her mind away waits
    /// for it.
    _thought: crate::services::her_log::pen::Thought,
}

impl Model {
    /// `ask`ed of `analyzer`, as [`Ask::model`] asks its own.
    #[cfg(test)]
    pub(super) fn of(analyzer: AiAnalyzer, ask: Ask) -> Self {
        crate::services::her_log::pen::reset_for_test();
        Self {
            analyzer,
            ask,
            _thought: crate::services::her_log::pen::think().expect("her mind is here"),
        }
    }

    /// `call`'s answer, billed; `whole`: only the whole of it will do, so
    /// one cut off at its output limit is a failure ([`Failure::CutOff`]).
    async fn billed(
        &self,
        call: impl std::future::Future<Output = anyhow::Result<String>>,
        whole: bool,
    ) -> Result<String, Failure> {
        #[cfg(test)]
        let started = std::time::Instant::now();
        // Boxed: every model call of hers goes through here, and the bookkeeping
        // around it would otherwise add its depth to every caller's future,
        // past what the compiler lays out for her longest chains.
        let (result, reported) =
            crate::services::ai_cost_ledger::reporting(Box::pin(tokio::time::timeout(
                self.ask.timeout,
                crate::services::ai_cost_ledger::with_site_ai_ledger(
                    self.ask.owner,
                    self.ask.source,
                    self.ask.operation,
                    call,
                ),
            )))
            .await;
        let mut cut_off = false;
        if let Some(reported) = &reported {
            let answered = match &result {
                Ok(Ok(raw)) => Some(raw.chars().count()),
                _ => None,
            };
            cut_off = self.ask.answered(reported, answered);
            #[cfg(test)]
            kept_for_bench(&self.ask, reported, answered, started.elapsed());
        }
        match result {
            Ok(Ok(_)) if whole && cut_off => Err(self.ask.failed(Failure::CutOff)),
            Ok(Ok(raw)) => {
                crate::services::agent::alive::model_answered();
                self.ask.judged();
                Ok(raw)
            }
            Ok(Err(error)) => Err(self.ask.failed(Failure::classify(&error))),
            Err(_) => Err(self.ask.failed(Failure::Timeout)),
        }
    }

    /// A JSON answer to `schema`.
    pub(super) async fn json(
        &self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<String, Failure> {
        // One future either way: two arms each holding their own call nest
        // her chat's future past the compiler's layout depth.
        self.billed(
            self.analyzer
                .analyze_json_as(system, input, schema_name, Some(schema), self.ask.shape),
            true,
        )
        .await
    }

    /// Free text.
    pub(super) async fn text(&self, system: &str, prompt: &str) -> Result<String, Failure> {
        self.billed(self.analyzer.analyze_with_system(system, prompt), false)
            .await
    }

    /// What she says to a full speaking prompt, as her chat replies are
    /// asked for.
    pub(super) async fn say(&self, prompt: &str) -> Result<String, Failure> {
        self.billed(
            self.analyzer
                .analyze_stream_parts_with_images(prompt, &[], |_| async { true }),
            false,
        )
        .await
    }

    /// What she makes of pictures, asked in `prompt`.
    pub(super) async fn look(
        &self,
        prompt: &str,
        pictures: &[crate::services::analyzer::ImageInput],
    ) -> Result<String, Failure> {
        self.billed(
            self.analyzer
                .analyze_stream_parts_with_images(prompt, pictures, |_| async { true }),
            false,
        )
        .await
    }
}

/// Who pays for what she does on her own, with no one asking: the site
/// owner. None when the site has no owner to bill.
/// Output a keeping call may take, thinking included, where it names none:
/// the room it had when it was asked to think it through. Its own thinking
/// comes out of the same allowance, and what it writes is long (a settle
/// put out 3,284 tokens on average); left at Anthropic's own 4096, the
/// thinking used it all and no answer came.
pub(super) const KEEPING_OUTPUT: u32 = 16_000;

/// `analyzer` as [`Voice::Keeping`] asks: no thinking asked for, and
/// [`KEEPING_OUTPUT`] to answer in unless the call names its own.
pub(super) fn keeping(
    analyzer: crate::services::analyzer::AiAnalyzer,
) -> crate::services::analyzer::AiAnalyzer {
    analyzer.with_output_limit(KEEPING_OUTPUT)
}

pub(super) async fn site_owner() -> Option<i32> {
    crate::services::ai_cost_ledger::resolve_site_owner_id()
        .await
        .ok()
}

/// A model's answer as `T`: the JSON object in it, or the whole of it.
pub(super) use myriad_merope::answer::parse;

#[cfg(test)]
#[path = "call_future_sizes_tests.rs"]
mod future_sizes_tests;

#[cfg(test)]
#[path = "call_cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn forbidden(said: &'static str) -> (axum::http::StatusCode, &'static str) {
        (axum::http::StatusCode::FORBIDDEN, said)
    }

    /// What tells that it is not who asks: a judgment answered since, by the
    /// same model as judges, not one refused and not another's answer.
    #[tokio::test]
    async fn only_a_judgment_answered_is_one() {
        use crate::services::channel_group::g0;
        judged_last(0);
        let now = crate::clock::now().timestamp();
        let schema = serde_json::json!({});
        for (voice, answer, answered) in [
            (Voice::Judge, Some("!refused"), false),
            (Voice::Judge, Some("!rejected"), false),
            (Voice::Judge, None, false),
            (Voice::Keeping, Some("{}"), false),
            (Voice::Chat, Some("{}"), false),
            (Voice::Judge, Some("{}"), true),
        ] {
            let mut world = g0::World::new(0.5);
            (world.models).insert("asked".into(), [answer.map(str::to_string)].into());
            g0::enter(world);
            let _ = Ask::new(voice, 1, "asked")
                .json_raw("", "", "s", &schema)
                .await;
            g0::leave();
            assert_eq!(judged_since(now - 1), answered, "{voice:?} {answer:?}");
        }
        assert!(!judged_since(now + 60));
    }

    #[tokio::test]
    async fn transient_transport_and_provider_statuses_remain_distinct() {
        use axum::{Router, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/slow",
                        get(|| async {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                            "ok"
                        }),
                    )
                    .route("/words", get(|| async { "not json" }))
                    .route("/forbidden", get(|| async { forbidden("") }))
                    .route(
                        "/region",
                        get(|| async { forbidden("not available in your region") }),
                    )
                    .route(
                        "/key-limit",
                        get(|| async { forbidden("Key limit exceeded") }),
                    )
                    .route(
                        "/filtered",
                        get(|| async { axum::http::StatusCode::BAD_REQUEST }),
                    )
                    .route(
                        "/denied",
                        get(|| async { axum::http::StatusCode::UNAUTHORIZED }),
                    )
                    .route(
                        "/limited",
                        get(|| async { axum::http::StatusCode::TOO_MANY_REQUESTS }),
                    )
                    .route(
                        "/broken",
                        get(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
                    ),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let error = client
            .get(format!("http://{address}/slow"))
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            Failure::classify(&anyhow::Error::from(error).context("outer")),
            Failure::Timeout
        );
        for (path, expected) in [
            ("denied", Failure::Rejected),
            ("filtered", Failure::Refused),
            // Who asks, it may be, whatever is said of it.
            ("forbidden", Failure::Refused),
            ("region", Failure::Refused),
            ("key-limit", Failure::Refused),
            ("limited", Failure::RateLimited),
            ("broken", Failure::Upstream),
        ] {
            let error = client
                .get(format!("http://{address}/{path}"))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap_err();
            assert_eq!(Failure::classify(&error.into()), expected);
        }
        let error = client
            .get(format!("http://{address}/words"))
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap_err();
        assert!(error.is_decode());
        assert_eq!(Failure::classify(&error.into()), Failure::InvalidOutput);
        server.abort();
        let _ = server.await;
        // A source that takes the connection and drops it unanswered: the
        // request is never answered, and it is tried again.
        let dropping = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dropped_at = dropping.local_addr().unwrap();
        let dropper = tokio::spawn(async move {
            while let Ok((stream, _)) = dropping.accept().await {
                drop(stream);
            }
        });
        let error = client
            .get(format!("http://{dropped_at}/"))
            .send()
            .await
            .unwrap_err();
        assert!(error.is_request() && !error.is_connect() && !error.is_timeout());
        assert_eq!(
            Failure::classify(&anyhow::Error::from(error).context("outer")),
            Failure::Transport
        );
        dropper.abort();
        let _ = dropper.await;
        for failure in [
            Failure::Rejected,
            Failure::Refused,
            Failure::RateLimited,
            Failure::Quota,
            Failure::Unavailable,
            Failure::InvalidOutput,
            Failure::Unknown,
        ] {
            assert!(!failure.retryable());
        }
        assert!(Failure::Timeout.retryable());
        assert!(Failure::Transport.retryable());
        assert!(Failure::Upstream.retryable());
        assert!(Failure::StreamInterrupted.retryable());
        assert!(Failure::CutOff.retryable());
    }

    #[test]
    fn an_answer_stopped_at_its_output_limit_is_told_and_counted_by_operation() {
        let ask = Ask::new(Voice::Judge, 7, "test_cut_off_counted");
        let stopped = |stop: &str| crate::services::ai_cost_ledger::Reported {
            stops: vec![stop.to_string()],
            ..Default::default()
        };
        assert!(!ask.answered(&stopped("end_turn"), Some(10)));
        assert_eq!(cut_off().get("test_cut_off_counted"), None);
        for stop in ["max_tokens", "length", "MAX_TOKENS"] {
            assert!(ask.answered(&stopped(stop), Some(10)), "{stop}");
        }
        assert_eq!(cut_off().get("test_cut_off_counted"), Some(&3));
    }

    #[test]
    fn her_status_counts_calls_with_no_answer_and_an_unreadable_one_is_an_answer() {
        let ask = || Ask::new(Voice::Judge, 7, "test_unanswered_counted");
        for failure in [
            Failure::Unavailable,
            Failure::Timeout,
            Failure::Transport,
            Failure::Upstream,
            Failure::RateLimited,
            Failure::Rejected,
            Failure::StreamInterrupted,
            Failure::Quota,
            Failure::Unknown,
        ] {
            ask().failed(failure);
        }
        assert_eq!(crate::services::agent::alive::unanswered_in_a_row(), 9);
        ask().failed(Failure::InvalidOutput);
        assert_eq!(crate::services::agent::alive::unanswered_in_a_row(), 0);
        ask().failed(Failure::Timeout);
        ask().failed(Failure::CutOff);
        assert_eq!(crate::services::agent::alive::unanswered_in_a_row(), 0);
    }

    #[tokio::test]
    async fn a_json_answer_cut_off_is_a_failure_to_try_again() {
        use crate::services::channel_group::g0;
        let mut world = g0::World::new(0.5);
        world
            .models
            .entry("test_cut_off_json".into())
            .or_default()
            .extend([Some("{}".to_string()), Some("{}".to_string())]);
        world.cut_off.insert("test_cut_off_json".into(), 1);
        g0::enter(world);
        let ask = || Ask::new(Voice::Judge, 7, "test_cut_off_json");
        let schema = serde_json::json!({});
        assert_eq!(
            ask().json_raw("", "", "s", &schema).await,
            Err(Failure::CutOff)
        );
        assert_eq!(
            ask().json_raw("", "", "s", &schema).await,
            Ok("{}".to_string())
        );
        let _ = g0::leave();
        assert_eq!(cut_off().get("test_cut_off_json"), Some(&1));
    }

    #[test]
    fn a_call_is_billed_to_merope_unless_told_otherwise() {
        let ask = Ask::new(Voice::Judge, 7, "touch_appraise");
        assert_eq!(ask.source, "merope");
        assert_eq!(ask.timeout, DEFAULT_TIMEOUT);
        let ask = ask.billed_as("touch").within(Duration::from_secs(2));
        assert_eq!((ask.source, ask.timeout), ("touch", Duration::from_secs(2)));
    }

    /// A keeping call asks for no thinking and names 16,000 tokens to answer
    /// in: Anthropic's own 4096 left a settle no room past its thinking.
    #[tokio::test]
    async fn a_keeping_call_has_room_past_its_thinking() {
        use axum::{Router, routing::post};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (sent, mut received) = tokio::sync::mpsc::unbounded_channel::<Value>();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().fallback(post(move |body: axum::Json<Value>| {
                    let _ = sent.send(body.0);
                    async { axum::http::StatusCode::BAD_REQUEST }
                })),
            )
            .await
            .unwrap();
        });
        let analyzer = || {
            crate::services::analyzer::AiAnalyzer::new(
                crate::services::analyzer::AiProvider::Anthropic,
                "test".into(),
                "keeping-test".into(),
                Some(format!("http://{address}")),
            )
        };
        let schema = serde_json::json!({"type": "object"});
        // Every body the call sent, a request it made again included.
        let mut sent_by = async |analyzer: crate::services::analyzer::AiAnalyzer| {
            let _ = analyzer
                .analyze_json_as("s", "i", "n", Some(&schema), JsonShape::Strict)
                .await;
            let mut bodies = Vec::new();
            while let Ok(body) = received.try_recv() {
                bodies.push(body);
            }
            assert!(!bodies.is_empty());
            bodies
        };
        for body in sent_by(keeping(analyzer().await)).await {
            assert_eq!(body["max_tokens"], 16_000, "{body}");
            assert!(body.get("thinking").is_none(), "{body}");
            assert!(body["output_config"].get("effort").is_none(), "{body}");
        }
        // A call that names its own limit keeps it.
        for body in sent_by(keeping(analyzer().await).with_output_limit(6000)).await {
            assert_eq!(body["max_tokens"], 6000, "{body}");
        }
        server.abort();
        let _ = server.await;
    }

    #[test]
    fn a_call_names_no_output_limit_unless_told_one() {
        let ask = Ask::new(Voice::Judge, 7, "chat_already");
        assert_eq!(ask.output, None);
        let ask = ask.within(Duration::from_secs(2)).output_at_most(1024);
        assert_eq!(
            (ask.output, ask.timeout),
            (Some(1024), Duration::from_secs(2))
        );
    }
}

/// Each call a memory bench run makes, one line to
/// `MEROPE_BENCH_CALLS=<jsonl>`: which call, why it stopped, how many tokens
/// it put out (thinking included), how long the answer is, and how long the
/// call took, from asking to the answer read (tries again included). Counts
/// only.
#[cfg(test)]
fn kept_for_bench(
    ask: &Ask,
    reported: &crate::services::ai_cost_ledger::Reported,
    chars: Option<usize>,
    took: Duration,
) {
    let Some(path) = std::env::var_os("MEROPE_BENCH_CALLS") else {
        return;
    };
    let line = serde_json::json!({
        "operation": ask.operation,
        "voice": format!("{:?}", ask.voice),
        "stops": reported.stops,
        "outputTokens": reported.usage.output,
        "chars": chars,
        "tookMs": took.as_millis() as u64,
    })
    .to_string()
        + "\n";
    use std::io::Write;
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
}
