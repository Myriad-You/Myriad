//! Ephemeral director delivery, separate from the durable run terminal.
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::watch;

use super::merope::PerformanceDirective;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectionSnapshot {
    pub version: u64,
    pub closed: bool,
    pub performance: Option<PerformanceDirective>,
}

/// One latest-value slot per existing run; never persisted or replayed as speech.
#[derive(Clone)]
pub struct PlaybackDirection(
    watch::Sender<DirectionSnapshot>,
    watch::Sender<Option<(Instant, PlaybackObservation)>>,
);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaybackObservation {
    pub upcoming_text: String,
    pub rig: serde_json::Value,
}

impl Default for PlaybackDirection {
    fn default() -> Self {
        Self(
            watch::channel(DirectionSnapshot::default()).0,
            watch::channel(None).0,
        )
    }
}

impl PlaybackDirection {
    /// Authenticated, ephemeral playback evidence. It never changes the run log.
    pub fn observe(&self, observation: PlaybackObservation) {
        if self.0.borrow().closed {
            return;
        }
        let Some(rig) = myriad_merope::sanitize_rig_state(&observation.rig) else {
            return;
        };
        self.1.send_replace(Some((
            Instant::now(),
            PlaybackObservation {
                upcoming_text: observation.upcoming_text.chars().take(900).collect(),
                rig: serde_json::to_value(rig).unwrap_or_default(),
            },
        )));
    }

    pub fn observation(&self) -> Option<PlaybackObservation> {
        self.1
            .borrow()
            .as_ref()
            .filter(|(at, _)| at.elapsed() < Duration::from_secs(2))
            .map(|(_, observation)| observation.clone())
    }

    pub fn observations(&self) -> watch::Receiver<Option<(Instant, PlaybackObservation)>> {
        self.1.subscribe()
    }

    pub fn publish(&self, performance: PerformanceDirective) -> bool {
        self.0.send_if_modified(|state| {
            if state.closed {
                return false;
            }
            state.version += 1;
            state.performance = Some(performance);
            true
        })
    }

    pub fn close(&self) {
        self.1.send_replace(None);
        self.0.send_modify(|state| {
            state.closed = true;
            state.performance = None;
        });
    }

    pub fn finish(&self) {
        self.1.send_replace(None);
        self.0.send_modify(|state| state.closed = true);
    }

    pub async fn cancelled(&self) {
        let mut rx = self.0.subscribe();
        loop {
            if rx.borrow_and_update().closed {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    pub async fn read_after(&self, after: u64) -> DirectionSnapshot {
        let mut rx = self.0.subscribe();
        let wait = async {
            loop {
                let state = rx.borrow_and_update().clone();
                if state.closed || state.version > after {
                    return state;
                }
                if rx.changed().await.is_err() {
                    return rx.borrow().clone();
                }
            }
        };
        // One bounded long-poll; polling reads never start model work.
        match tokio::time::timeout(std::time::Duration::from_secs(15), wait).await {
            Ok(state) => state,
            Err(_) => self.0.borrow().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_direction_observations_are_bounded_expire_and_do_not_publish_plans() {
        let slot = PlaybackDirection::default();
        slot.observe(PlaybackObservation {
            upcoming_text: "字".repeat(1000),
            rig: serde_json::json!({}),
        });
        assert_eq!(
            slot.observation().unwrap().upcoming_text.chars().count(),
            900
        );
        assert_eq!(slot.0.borrow().version, 0);
        let observation = slot.observation().unwrap();
        slot.1
            .send_replace(Some((Instant::now() - Duration::from_secs(3), observation)));
        assert!(slot.observation().is_none());
        slot.close();
        slot.observe(PlaybackObservation {
            upcoming_text: "不能恢复".into(),
            rig: serde_json::json!({}),
        });
        assert!(slot.observation().is_none());
    }

    #[tokio::test]
    async fn playback_direction_wakes_reader_and_cannot_reopen_after_cancel() {
        let slot = PlaybackDirection::default();
        let pending = slot.read_after(0);
        slot.close();
        let state = pending.await;
        assert!(state.closed);
        let performance = crate::services::agent::merope::local_directive(
            &crate::services::agent::motion_overlay::motion_refinement_tests::context(),
        )
        .unwrap();
        assert!(!slot.publish(performance));
        assert_eq!(slot.read_after(0).await.version, 0);
    }
}
