//! Reuse transport, not model credentials or persona context. Each request
//! still resolves its current model/key; proxy changes select a different pool.

use crate::services::http_client::{apply_proxy, ProxyConfig};
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
    async fn ordinary_json_reuses_known_format_support_without_retrying_auth_failures() {
        use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
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
        assert!(denied
            .analyze_json("test", "test", "test", None)
            .await
            .is_err());
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            4,
            "authentication failures must not retry"
        );
        server.abort();
    }

    #[tokio::test]
    async fn repeated_analyzers_reuse_a_connection_but_not_auth_or_timeout_policy() {
        use axum::{extract::ConnectInfo, http::HeaderMap, routing::get, Router};
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
}
