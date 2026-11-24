//! Unit tests for `figure.rs`: the Space's light steps tried again, and the enlargement's way out.

use std::sync::atomic::{AtomicU32, Ordering};

use super::super::drawing::{TRANSIENT_RETRIES, retried, retry_delay};
use super::upscaled;
use crate::services::see_through::SeeThroughError;

/// How long the tries before the last wait between them when all are used.
fn all_delays() -> std::time::Duration {
    (1..=TRANSIENT_RETRIES).map(retry_delay).sum()
}

#[tokio::test(start_paused = true)]
async fn a_passing_failure_is_tried_again_and_the_step_then_succeeds() {
    let started = tokio::time::Instant::now();
    let calls = AtomicU32::new(0);
    let calls = &calls;
    let out = retried("stitch", || async move {
        match calls.fetch_add(1, Ordering::Relaxed) {
            0 => Err(SeeThroughError::Transport("connection reset".into())),
            1 => Err(SeeThroughError::Upstream {
                stage: "upload",
                status: 503,
            }),
            _ => Ok(b"stitched".to_vec()),
        }
    })
    .await;
    assert_eq!(out.ok(), Some(b"stitched".to_vec()));
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert_eq!(started.elapsed(), retry_delay(1) + retry_delay(2));
}

#[tokio::test(start_paused = true)]
async fn a_passing_failure_past_its_tries_is_the_steps_own() {
    let started = tokio::time::Instant::now();
    let calls = AtomicU32::new(0);
    let calls = &calls;
    let out = retried("plan", || async move {
        calls.fetch_add(1, Ordering::Relaxed);
        Err::<(), _>(SeeThroughError::Timeout)
    })
    .await;
    assert!(matches!(out, Err(SeeThroughError::Timeout)));
    assert_eq!(calls.load(Ordering::Relaxed), TRANSIENT_RETRIES + 1);
    assert_eq!(started.elapsed(), all_delays());
}

#[tokio::test(start_paused = true)]
async fn quota_a_refusal_and_bad_input_are_not_tried_again() {
    let started = tokio::time::Instant::now();
    let failures = [
        SeeThroughError::Quota,
        SeeThroughError::Rejected(Some("ZeroGPU quota exceeded".into())),
        SeeThroughError::InvalidInput("not a decomposition".into()),
        SeeThroughError::Upstream {
            stage: "upload",
            status: 404,
        },
    ];
    for failure in failures {
        let calls = AtomicU32::new(0);
        let calls = &calls;
        let mut failure = Some(failure);
        let out = retried("keys", || {
            let failure = failure.take();
            async move {
                calls.fetch_add(1, Ordering::Relaxed);
                Err::<(), _>(failure.expect("asked once"))
            }
        })
        .await;
        assert!(out.is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
    assert_eq!(started.elapsed(), std::time::Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn an_enlargement_that_keeps_failing_leaves_the_figure_as_drawn() {
    // A passing failure every time: tried as often as any step, then given up.
    let started = tokio::time::Instant::now();
    let calls = AtomicU32::new(0);
    let calls = &calls;
    let enlarged = upscaled(|| async move {
        calls.fetch_add(1, Ordering::Relaxed);
        Err(SeeThroughError::Transport("connection reset".into()))
    })
    .await;
    assert_eq!(enlarged, None);
    assert_eq!(calls.load(Ordering::Relaxed), TRANSIENT_RETRIES + 1);
    assert_eq!(started.elapsed(), all_delays());

    // A failure that will not pass is not tried again.
    let calls = AtomicU32::new(0);
    let calls = &calls;
    let enlarged = upscaled(|| async move {
        calls.fetch_add(1, Ordering::Relaxed);
        Err(SeeThroughError::Quota)
    })
    .await;
    assert_eq!(enlarged, None);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    // A Space without the endpoint is asked once.
    let calls = AtomicU32::new(0);
    let calls = &calls;
    let enlarged = upscaled(|| async move {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(None)
    })
    .await;
    assert_eq!(enlarged, None);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[tokio::test(start_paused = true)]
async fn an_enlargement_that_fails_in_passing_is_made_on_the_next_try() {
    let calls = AtomicU32::new(0);
    let calls = &calls;
    let enlarged = upscaled(|| async move {
        match calls.fetch_add(1, Ordering::Relaxed) {
            0 => Err(SeeThroughError::Transport("connection reset".into())),
            _ => Ok(Some(b"enlarged".to_vec())),
        }
    })
    .await;
    assert_eq!(enlarged, Some(b"enlarged".to_vec()));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[tokio::test(start_paused = true)]
async fn an_enlargement_that_timed_out_is_not_waited_out_again() {
    let started = tokio::time::Instant::now();
    let calls = AtomicU32::new(0);
    let calls = &calls;
    let enlarged = upscaled(|| async move {
        calls.fetch_add(1, Ordering::Relaxed);
        Err(SeeThroughError::Timeout)
    })
    .await;
    assert_eq!(enlarged, None);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(started.elapsed(), std::time::Duration::ZERO);
}
