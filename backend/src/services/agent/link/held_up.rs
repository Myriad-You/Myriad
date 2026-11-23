//! Steps of a link that held it up: its loop does one thing at a time, so
//! while a step runs no ping goes out, and a child that misses two leaves.

use std::time::Duration;

use serde::Deserialize;
use tokio::time::Instant;

use super::contract::CONTRACT;

/// A step longer than this is logged.
const HELD_UP: Duration = Duration::from_secs(5);

/// How many milliseconds a step begun at `began` took, when it was too long.
pub(super) fn held_up(began: Instant) -> Option<u128> {
    let took = began.elapsed();
    (took > HELD_UP).then_some(took.as_millis())
}

/// The type of a frame as a log may name it: one of the contract's, else `?`
/// (never other text a side sent).
pub(super) fn frame_type(text: &str) -> &'static str {
    #[derive(Deserialize)]
    struct Typed {
        t: String,
    }
    serde_json::from_str::<Typed>(text)
        .ok()
        .and_then(|frame| CONTRACT.frames.get_key_value(&frame.t))
        .map_or("?", |(t, _)| t.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn only_a_step_over_five_seconds_held_the_link_up() {
        for (millis, held) in [(0, None), (5_000, None), (5_001, Some(5_001))] {
            let began = Instant::now();
            tokio::time::sleep(Duration::from_millis(millis)).await;
            assert_eq!(held_up(began), held, "{millis}");
        }
    }

    #[test]
    fn a_log_names_only_the_contracts_frames() {
        let typed = |t: &str| {
            frame_type(&serde_json::json!({ "v": 1, "t": t, "body": { "x": 1 } }).to_string())
        };
        assert_eq!(typed("log.vector"), "log.vector");
        assert_eq!(typed("my secret"), "?");
        assert_eq!(frame_type("{"), "?");
    }
}
