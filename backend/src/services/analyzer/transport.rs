//! Reuse transport, not model credentials or persona context. Each request
//! still resolves its current model/key; proxy changes select a different pool.

use crate::services::http_client::{ProxyConfig, apply_proxy};
use reqwest::Client;
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[derive(PartialEq, Eq)]
struct TransportKey {
    timeout: Duration,
    proxy_enabled: bool,
    proxy_url: Option<String>,
    bypass: Vec<String>,
}

type Pool = VecDeque<(TransportKey, Client)>;

pub(super) fn pooled_client(
    proxy: &ProxyConfig,
    timeout: Duration,
) -> Result<Client, reqwest::Error> {
    static POOL: OnceLock<Mutex<Pool>> = OnceLock::new();
    let key = TransportKey {
        timeout,
        proxy_enabled: proxy.enabled,
        proxy_url: proxy.proxy_url.clone(),
        bypass: proxy.bypass_list.clone(),
    };
    let mut pool = POOL
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(index) = pool.iter().position(|(existing, _)| existing == &key) {
        let entry = pool.remove(index).unwrap();
        let client = entry.1.clone();
        pool.push_back(entry);
        return Ok(client);
    }
    let client = apply_proxy(
        Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(30))
            .user_agent("Myriad/1.0"),
        proxy,
    )?
    .build()?;
    if pool.len() >= 8 {
        pool.pop_front();
    }
    pool.push_back((key, client.clone()));
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deep_thinking_reaches_messages_and_only_request_rejection_disables_it() {
        use super::super::{AiAnalyzer, AiProvider};
        use axum::{Json, Router, http::StatusCode, routing::post};
        use serde_json::{Value, json};
        use std::sync::Arc;

        let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let router = Router::new().route(
            "/v1/messages",
            post(move |Json(body): Json<Value>| {
                let observed = observed.clone();
                async move {
                    observed.lock().unwrap().push(body.clone());
                    let status = match body["model"].as_str().unwrap() {
                        "unauthorized" => StatusCode::UNAUTHORIZED,
                        "limited" => StatusCode::TOO_MANY_REQUESTS,
                        "unavailable" => StatusCode::SERVICE_UNAVAILABLE,
                        "legacy" if body.get("output_config").is_some() => StatusCode::BAD_REQUEST,
                        _ => StatusCode::OK,
                    };
                    if status != StatusCode::OK {
                        return (status, Json(json!({"error":"fixture failure"})));
                    }
                    (
                        status,
                        Json(json!({"content":[
                            {"type":"thinking","thinking":"fixture reasoning"},
                            {"type":"text","text":"{\"read\":true}"}
                        ]})),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let analyzer = |model: &str| {
            AiAnalyzer::new_with_timeout(
                AiProvider::Anthropic,
                String::new(),
                model.to_string(),
                Some(format!("http://{address}/v1")),
                Duration::from_secs(2),
            )
        };

        let ordinary = analyzer("supported").await;
        ordinary
            .analyze_json("read", "hello", "read", None)
            .await
            .unwrap();
        let thorough = ordinary.with_deep_thinking();
        assert_eq!(
            thorough
                .analyze_json("read", "hello", "read", None)
                .await
                .unwrap(),
            r#"{"read":true}"#,
            "thinking blocks must not become memory text",
        );
        {
            let bodies = requests.lock().unwrap();
            assert!(bodies[0].get("output_config").is_none());
            assert!(bodies[0].get("thinking").is_none());
            assert_eq!(bodies[0]["max_tokens"], 4096);
            assert_eq!(bodies[1]["output_config"]["effort"], "high");
            assert_eq!(bodies[1]["thinking"]["type"], "adaptive");
            assert_eq!(bodies[1]["max_tokens"], 16_000);
        }

        requests.lock().unwrap().clear();
        let legacy = analyzer("legacy").await.with_deep_thinking();
        for _ in 0..2 {
            legacy
                .analyze_json("read", "hello", "read", None)
                .await
                .unwrap();
        }
        {
            let bodies = requests.lock().unwrap();
            assert_eq!(bodies.len(), 3, "one refusal, then two ordinary requests");
            assert!(bodies[0].get("output_config").is_some());
            for body in &bodies[1..] {
                assert!(body.get("output_config").is_none());
                assert!(body.get("thinking").is_none());
            }
        }

        for (model, expected) in [
            ("unauthorized", StatusCode::UNAUTHORIZED),
            ("limited", StatusCode::TOO_MANY_REQUESTS),
            ("unavailable", StatusCode::SERVICE_UNAVAILABLE),
        ] {
            requests.lock().unwrap().clear();
            let failing = analyzer(model).await.with_deep_thinking();
            for _ in 0..2 {
                let error = failing
                    .analyze_json("read", "hello", "read", None)
                    .await
                    .unwrap_err();
                assert_eq!(
                    error
                        .downcast_ref::<super::super::client::ProviderHttpError>()
                        .unwrap()
                        .status,
                    expected,
                );
            }
            let bodies = requests.lock().unwrap();
            assert_eq!(bodies.len(), 2, "no retry for {expected}");
            assert!(
                bodies
                    .iter()
                    .all(|body| body["output_config"]["effort"] == "high")
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn light_thinking_asks_anthropic_for_low_effort_and_drops_it_once_refused() {
        use super::super::{AiAnalyzer, AiProvider};
        use axum::{Json, Router, http::StatusCode, routing::post};
        use serde_json::{Value, json};
        use std::sync::Arc;

        let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let router = Router::new().route(
            "/v1/messages",
            post(move |Json(body): Json<Value>| {
                let observed = observed.clone();
                async move {
                    observed.lock().unwrap().push(body.clone());
                    let status = match body["model"].as_str().unwrap() {
                        model
                            if model.starts_with("legacy")
                                && body.get("output_config").is_some() =>
                        {
                            StatusCode::BAD_REQUEST
                        }
                        "limited" => StatusCode::TOO_MANY_REQUESTS,
                        _ => StatusCode::OK,
                    };
                    if status != StatusCode::OK {
                        return (status, Json(json!({"error":"fixture failure"})));
                    }
                    (
                        status,
                        Json(json!({"content":[{"type":"text","text":"{\"read\":true}"}]})),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let analyzer = |model: &str| {
            AiAnalyzer::new_with_timeout(
                AiProvider::Anthropic,
                String::new(),
                model.to_string(),
                Some(format!("http://{address}/v1")),
                Duration::from_secs(2),
            )
        };
        let bodies = || std::mem::take(&mut *requests.lock().unwrap());

        // Asked to think little: effort low, in a typed judgment, a reply,
        // and a stream; not otherwise, and deep thinking keeps its own.
        let light = analyzer("supported").await.with_light_thinking();
        light
            .analyze_json("read", "hello", "read", None)
            .await
            .unwrap();
        light.analyze_with_system("read", "hello").await.unwrap();
        let _ = light.analyze_stream("hello", |_| true).await;
        analyzer("supported")
            .await
            .analyze_json("read", "hello", "read", None)
            .await
            .unwrap();
        analyzer("supported")
            .await
            .with_light_thinking()
            .with_deep_thinking()
            .analyze_json("read", "hello", "read", None)
            .await
            .unwrap();
        {
            let sent = bodies();
            assert_eq!(sent.len(), 5);
            for body in &sent[..3] {
                assert_eq!(body["output_config"], json!({"effort": "low"}));
                assert!(body.get("thinking").is_none());
            }
            assert_eq!(sent[2]["stream"], true);
            assert!(sent[3].get("output_config").is_none());
            assert_eq!(sent[4]["output_config"]["effort"], "high");
        }

        // Refused: asked once more without it, and never again.
        let legacy = analyzer("legacy").await.with_light_thinking();
        for _ in 0..2 {
            legacy
                .analyze_json("read", "hello", "read", None)
                .await
                .unwrap();
        }
        let _ = legacy.analyze_stream("hello", |_| true).await;
        {
            let sent = bodies();
            assert_eq!(sent.len(), 4, "one refusal, then three ordinary requests");
            assert!(sent[0].get("output_config").is_some());
            assert!(
                sent[1..]
                    .iter()
                    .all(|body| body.get("output_config").is_none())
            );
        }
        // Remembered per endpoint and model: another model's stream is
        // refused once on its own.
        let legacy_stream = analyzer("legacy-stream").await.with_light_thinking();
        let _ = legacy_stream.analyze_stream("hello", |_| true).await;
        {
            let sent = bodies();
            assert_eq!(sent.len(), 2, "a stream drops it the same way");
            assert!(sent[1].get("output_config").is_none());
        }

        // Any other failure is not a refusal of it.
        let limited = analyzer("limited").await.with_light_thinking();
        for _ in 0..2 {
            assert!(
                limited
                    .analyze_json("read", "hello", "read", None)
                    .await
                    .is_err()
            );
        }
        {
            let sent = bodies();
            assert!(!sent.is_empty());
            assert!(
                sent.iter()
                    .all(|body| body["output_config"]["effort"] == "low")
            );
        }
        server.abort();
    }

    #[tokio::test]
    async fn configured_text_protocol_controls_shape_and_optional_auth() {
        use axum::{Json, Router, http::HeaderMap, routing::post};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .route(
                "/v1/responses",
                post(
                    |headers: HeaderMap, Json(body): Json<serde_json::Value>| async move {
                        assert!(headers.get("authorization").is_none());
                        assert_eq!(body["store"], false);
                        assert_eq!(body["input"][0]["content"], "hello");
                        Json(serde_json::json!({"output_text":"responses-ok"}))
                    },
                ),
            )
            .route(
                "/v1/messages",
                post(
                    |headers: HeaderMap, Json(body): Json<serde_json::Value>| async move {
                        assert_eq!(headers["x-api-key"], "anthropic-key");
                        assert_eq!(headers["anthropic-version"], "2023-06-01");
                        assert_eq!(body["messages"][0]["content"], "hello");
                        Json(serde_json::json!({"content":[{"type":"text","text":"anthropic-ok"}]}))
                    },
                ),
            );
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let responses = super::super::AiAnalyzer::new_with_timeout(
            super::super::AiProvider::OpenAIResponses,
            String::new(),
            "gpt-x".into(),
            Some(format!("http://{address}/v1")),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(responses.analyze("hello").await.unwrap(), "responses-ok");

        let anthropic = super::super::AiAnalyzer::new_with_timeout(
            super::super::AiProvider::Anthropic,
            "anthropic-key".to_string(),
            "claude-x".into(),
            Some(format!("http://{address}/v1")),
            Duration::from_secs(2),
        )
        .await;
        assert_eq!(anthropic.analyze("hello").await.unwrap(), "anthropic-ok");
        server.abort();
    }

    #[tokio::test]
    async fn ordinary_json_reuses_known_format_support_without_retrying_auth_failures() {
        use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let attempts = Arc::new(AtomicUsize::new(0));
        let router = Router::new().route("/v1/chat/completions", post(
            |State(attempts): State<Arc<AtomicUsize>>, Json(body): Json<serde_json::Value>| async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                if body["model"] == "denied" { return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"test"}))); }
                if body.get("response_format").is_some() { return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error":"unsupported format"}))); }
                (StatusCode::OK, Json(serde_json::json!({"choices":[{"message":{"content":"{\"v\":1}"}}]})))
            }
        )).with_state(attempts.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let analyzer = super::super::AiAnalyzer::new_with_timeout(
            super::super::AiProvider::OpenAI,
            "test".into(),
            "memo".into(),
            Some(format!("http://{address}/v1")),
            Duration::from_secs(2),
        )
        .await;
        for _ in 0..2 {
            assert_eq!(
                analyzer
                    .analyze_json("test", "test", "test", None)
                    .await
                    .unwrap(),
                "{\"v\":1}"
            );
        }
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "only the first call pays the rejected-format round trip"
        );
        let denied = super::super::AiAnalyzer::new_with_timeout(
            super::super::AiProvider::OpenAI,
            "test".into(),
            "denied".into(),
            Some(format!("http://{address}/v1")),
            Duration::from_secs(2),
        )
        .await;
        assert!(
            denied
                .analyze_json("test", "test", "test", None)
                .await
                .is_err()
        );
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            4,
            "authentication failures must not retry"
        );
        server.abort();
    }

    #[tokio::test]
    async fn repeated_analyzers_reuse_a_connection_but_not_auth_or_timeout_policy() {
        use axum::{Router, extract::ConnectInfo, http::HeaderMap, routing::get};
        use std::net::SocketAddr;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new().route(
            "/",
            get(
                |ConnectInfo(peer): ConnectInfo<SocketAddr>, headers: HeaderMap| async move {
                    format!(
                        "{}|{}",
                        peer.port(),
                        headers
                            .get("authorization")
                            .and_then(|h| h.to_str().ok())
                            .unwrap_or("none")
                    )
                },
            ),
        );
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let proxy = ProxyConfig::default();
        let first = pooled_client(&proxy, Duration::from_millis(1753)).unwrap();
        let second = pooled_client(&proxy, Duration::from_millis(1753)).unwrap();
        let url = format!("http://{address}/");
        let a = first
            .get(&url)
            .header("Authorization", "test-a")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let b = second
            .get(&url)
            .header("Authorization", "test-b")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(
            a.split('|').next(),
            b.split('|').next(),
            "must reuse TCP connection"
        );
        assert!(a.ends_with("|test-a") && b.ends_with("|test-b"));
        let c = second.get(&url).send().await.unwrap().text().await.unwrap();
        assert!(
            c.ends_with("|none"),
            "request credentials cannot persist in the pool"
        );
        let changed = pooled_client(&proxy, Duration::from_millis(1754)).unwrap();
        let d = changed
            .get(&url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_ne!(a.split('|').next(), d.split('|').next());
        let invalid = ProxyConfig {
            enabled: true,
            proxy_url: Some("http://[invalid".into()),
            bypass_list: vec![],
        };
        assert!(
            pooled_client(&invalid, Duration::from_millis(1753)).is_err(),
            "must not reuse direct transport when required proxy fails"
        );
        server.abort();
    }

    /// A strict JSON ask holds Anthropic's own API to the schema
    /// (`output_config.format`), beside the effort it is asked to think
    /// with; an endpoint that rejects the format is asked prompt-only, and
    /// remembered.
    #[tokio::test]
    async fn a_strict_ask_holds_anthropic_to_the_schema() {
        use super::super::{AiAnalyzer, AiProvider};
        use axum::{Json, Router, http::StatusCode, routing::post};
        use serde_json::{Value, json};
        use std::sync::Arc;

        let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let router = Router::new().route(
            "/v1/messages",
            post(move |Json(body): Json<Value>| {
                let observed = observed.clone();
                async move {
                    observed.lock().unwrap().push(body.clone());
                    if body["model"] == "formless" && body["output_config"].get("format").is_some()
                    {
                        return (StatusCode::BAD_REQUEST, Json(json!({"error":"no format"})));
                    }
                    (
                        StatusCode::OK,
                        Json(json!({"content":[{"type":"text","text":"{\"read\":true}"}]})),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let analyzer = |model: &str| {
            AiAnalyzer::new_with_timeout(
                AiProvider::Anthropic,
                String::new(),
                model.to_string(),
                Some(format!("http://{address}/v1")),
                Duration::from_secs(2),
            )
        };
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["read"],
            "properties": {"read": {"type": "boolean"}}
        });

        let held = analyzer("held").await.with_light_thinking();
        held.analyze_json_strict("read", "hello", "read", &schema)
            .await
            .unwrap();
        held.analyze_json("read", "hello", "read", Some(&schema))
            .await
            .unwrap();
        {
            let bodies = requests.lock().unwrap();
            assert_eq!(bodies[0]["output_config"]["format"]["type"], "json_schema");
            assert_eq!(bodies[0]["output_config"]["format"]["schema"], schema);
            assert_eq!(bodies[0]["output_config"]["effort"], "low");
            assert_eq!(bodies[0]["messages"][0]["content"], "hello");
            // Every other ask only restates it in the prompt.
            assert!(bodies[1]["output_config"].get("format").is_none());
            assert_ne!(bodies[1]["messages"][0]["content"], "hello");
        }

        requests.lock().unwrap().clear();
        let formless = analyzer("formless").await;
        for _ in 0..2 {
            formless
                .analyze_json_strict("read", "hello", "read", &schema)
                .await
                .unwrap();
        }
        {
            let bodies = requests.lock().unwrap();
            assert_eq!(bodies.len(), 3, "one refusal, then prompt-only twice");
            for body in &bodies[1..] {
                assert!(body.get("output_config").is_none());
            }
        }
        server.abort();
    }

    /// One endpoint and model, asked for JSON under several schemas: what it
    /// refuses of one says nothing of another, and an ask it refuses
    /// whatever is left out says nothing of anything.
    #[tokio::test]
    async fn a_refused_schema_is_remembered_for_that_schema_alone() {
        use super::super::{AiAnalyzer, AiProvider, types::OutputBudget};
        use axum::{Json, Router, http::StatusCode, routing::post};
        use serde_json::{Value, json};
        use std::sync::Arc;

        let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
        let observed = requests.clone();
        let out_of_credit = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let broke = out_of_credit.clone();
        let router = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let observed = observed.clone();
                let broke = broke.load(std::sync::atomic::Ordering::SeqCst);
                async move {
                    observed.lock().unwrap().push(body.clone());
                    if broke || body["response_format"]["json_schema"]["name"] == "too_complex" {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error": {"message": "fixture failure"}})),
                        );
                    }
                    (
                        StatusCode::OK,
                        Json(json!({"choices": [{"message": {"content": "{\"read\":true}"}}]})),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let analyzer = |model: &str| {
            AiAnalyzer::new_with_timeout(
                AiProvider::OpenAI,
                String::new(),
                model.to_string(),
                Some(format!("http://{address}/v1")),
                Duration::from_secs(2),
            )
        };
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["read"],
            "properties": {"read": {"type": "boolean"}}
        });
        // The schemas each request since the last look was held to.
        let held = || -> Vec<Option<String>> {
            std::mem::take(&mut *requests.lock().unwrap())
                .iter()
                .map(|body| {
                    body["response_format"]["json_schema"]["name"]
                        .as_str()
                        .map(str::to_string)
                })
                .collect()
        };
        let named = |name: &str| Some(name.to_string());

        for short in [false, true] {
            let model = analyzer(if short { "short" } else { "whole" }).await;
            let ask = async |name: &str| {
                if short {
                    let budget = OutputBudget { max_tokens: 256 };
                    model
                        .analyze_json_short("read", "hello", name, Some(&schema), budget)
                        .await
                } else {
                    model
                        .analyze_json("read", "hello", name, Some(&schema))
                        .await
                }
            };
            ask("too_complex").await.unwrap();
            assert_eq!(held().last().unwrap(), &None, "read prompt-only in the end");
            ask("plain").await.unwrap();
            assert_eq!(held(), [named("plain")], "another schema is still held to");
            ask("too_complex").await.unwrap();
            assert_eq!(held(), [None], "it is not tried again");

            // Refused whatever is left out: nothing of it is remembered.
            out_of_credit.store(true, std::sync::atomic::Ordering::SeqCst);
            assert!(ask("plain").await.is_err());
            assert!(held().len() > 1);
            out_of_credit.store(false, std::sync::atomic::Ordering::SeqCst);
            ask("plain").await.unwrap();
            assert_eq!(held(), [named("plain")]);
            if short {
                // The output limit was not what was refused.
                ask("plain").await.unwrap();
                let sent = std::mem::take(&mut *requests.lock().unwrap());
                assert_eq!(sent[0]["max_tokens"], 256);
            }
        }
        server.abort();
    }
}
