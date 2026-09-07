//! Bounded retry for one background memory extraction; no queue or model fallback.
use std::{future::Future, time::Duration};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Transient,
    Permanent,
    Stale,
}

pub(super) fn classify(error: &anyhow::Error) -> Failure {
    if error.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(|error| error.is_timeout() || error.is_connect())
    }) {
        Failure::Transient
    } else {
        // No string matching on provider payloads. Auth, quota, invalid output
        // and unknown errors must not start a retry/spend loop.
        Failure::Permanent
    }
}

pub(super) async fn request<R, RF, C, CF>(
    mut call: R,
    mut current: C,
    retry_delay: Duration,
) -> Result<String, Failure>
where
    R: FnMut() -> RF,
    RF: Future<Output = Result<String, Failure>>,
    C: FnMut() -> CF,
    CF: Future<Output = bool>,
{
    for attempt in 0..2 {
        if !current().await {
            return Err(Failure::Stale);
        }
        match call().await {
            Ok(raw) => return Ok(raw),
            Err(Failure::Transient) if attempt == 0 => {
                tracing::info!(
                    attempt = 1,
                    outcome = "retrying",
                    "[Merope] memory extraction"
                );
                tokio::time::sleep(retry_delay).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("second attempt always returns")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test]
    async fn real_transport_timeout_is_retryable_but_http_auth_failure_is_not() {
        use axum::{routing::get, Router};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/slow",
                        get(|| async {
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            "ok"
                        }),
                    )
                    .route(
                        "/denied",
                        get(|| async { axum::http::StatusCode::UNAUTHORIZED }),
                    ),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(30))
            .build()
            .unwrap();
        let timeout = client
            .get(format!("http://{address}/slow"))
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            classify(&anyhow::Error::from(timeout).context("provider call")),
            Failure::Transient
        );
        let denied = reqwest::Client::new()
            .get(format!("http://{address}/denied"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap_err();
        assert_eq!(classify(&denied.into()), Failure::Permanent);
        server.abort();
    }

    #[tokio::test]
    async fn temporary_failure_retries_once_but_valid_no_change_does_not() {
        let calls = Cell::new(0);
        let result = request(
            || {
                let n = calls.get();
                calls.set(n + 1);
                std::future::ready(if n == 0 {
                    Err(Failure::Transient)
                } else {
                    Ok("null decision".into())
                })
            },
            || std::future::ready(true),
            Duration::ZERO,
        )
        .await;
        assert_eq!(result, Ok("null decision".into()));
        assert_eq!(calls.get(), 2);
        calls.set(0);
        request(
            || {
                calls.set(calls.get() + 1);
                std::future::ready(Ok("no change".into()))
            },
            || std::future::ready(true),
            Duration::ZERO,
        )
        .await
        .unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn permanent_errors_and_new_input_stop_retrying() {
        let calls = Cell::new(0);
        let result = request(
            || {
                calls.set(calls.get() + 1);
                std::future::ready(Err(Failure::Permanent))
            },
            || std::future::ready(true),
            Duration::ZERO,
        )
        .await;
        assert_eq!(result, Err(Failure::Permanent));
        assert_eq!(calls.get(), 1);
        calls.set(0);
        let result = request(
            || {
                calls.set(calls.get() + 1);
                std::future::ready(Err(Failure::Transient))
            },
            || std::future::ready(calls.get() == 0),
            Duration::ZERO,
        )
        .await;
        assert_eq!(result, Err(Failure::Stale));
        assert_eq!(calls.get(), 1);
        calls.set(0);
        let result = request(
            || {
                calls.set(calls.get() + 1);
                std::future::ready(Err(Failure::Transient))
            },
            || std::future::ready(true),
            Duration::ZERO,
        )
        .await;
        assert_eq!(result, Err(Failure::Transient));
        assert_eq!(calls.get(), 2);
        assert_eq!(
            classify(&anyhow::anyhow!("HTTP 401 or quota exceeded")),
            Failure::Permanent
        );
    }
}
