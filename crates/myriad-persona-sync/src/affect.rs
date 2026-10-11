//! Her affect toward the owner across sides: an anchor and the changes
//! after it, replayed in order (E11). Every side that writes her affect
//! records what moved her as an [`AffectImpulse`] with the baseline it
//! settled toward; any side's "now" is the anchor with every later change
//! replayed and settled to now, so two sides with the same changes agree to
//! the bit.
//!
//! The rules are `myriad_merope::affect`'s, as the site applies them: before
//! each change her affect settles toward that change's baseline for the
//! whole seconds since the one before, then the impulse applies, then every
//! value is clamped to 0–100.

use myriad_merope::affect::{
    self as rules, AffectBaseline, Appraisal, apply_appraisal, apply_music_listening, settle,
};
use serde::{Deserialize, Serialize};

use crate::{
    SyncError,
    entity::{Affect, AffectAnchorBody, AffectEventBody, Baseline},
};

/// What moved her: an appraisal of what was said or done (the emotion and
/// arousal it reads as, and how strongly it lands), music she listened to,
/// or nothing (a turn that only let her settle).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum AffectImpulse {
    Appraisal {
        emotion: f64,
        arousal: f64,
        scale: f64,
    },
    #[serde(rename_all = "camelCase")]
    Music { listened_seconds: i64 },
    /// Nothing pushed her; she only settled to this moment.
    None,
}

impl AffectImpulse {
    /// An appraisal as the rules give it, landing at `scale`.
    pub fn appraisal(appraisal: Appraisal, scale: f64) -> Self {
        Self::Appraisal {
            emotion: appraisal.emotion,
            arousal: appraisal.arousal,
            scale,
        }
    }

    /// What one turn of the owner's words does: praise or scolding, landing
    /// less the more turns came before it in the session; otherwise nothing
    /// (`myriad_merope::affect::apply_user_utterance`).
    pub fn utterance(utterance_index: u32, praised: bool, scolded: bool) -> Self {
        let scale = rules::repeat_scale(utterance_index);
        if scolded {
            Self::appraisal(rules::APPRAISAL_SCOLD, scale)
        } else if praised {
            Self::appraisal(rules::APPRAISAL_PRAISE, scale)
        } else {
            Self::None
        }
    }

    /// A task done for the owner, or failed.
    pub fn task(success: bool) -> Self {
        Self::appraisal(
            if success {
                rules::APPRAISAL_TASK_OK
            } else {
                rules::APPRAISAL_TASK_FAIL
            },
            1.0,
        )
    }

    /// Applies it to `affect`, unclamped where the rules leave it so.
    pub fn apply(&self, affect: &mut rules::Affect) {
        match self {
            Self::Appraisal {
                emotion,
                arousal,
                scale,
            } => apply_appraisal(
                affect,
                Appraisal {
                    emotion: *emotion,
                    arousal: *arousal,
                },
                *scale,
            ),
            Self::Music { listened_seconds } => apply_music_listening(
                affect,
                u32::try_from((*listened_seconds).max(0)).unwrap_or(u32::MAX),
            ),
            Self::None => {}
        }
    }
}

/// Every value clamped to 0–100, as a side stores her affect after a change.
pub fn clamped(affect: rules::Affect) -> rules::Affect {
    rules::Affect {
        mood: rules::clamp(affect.mood),
        arousal: rules::clamp(affect.arousal),
        emotion: rules::clamp(affect.emotion),
        emotion_arousal: rules::clamp(affect.emotion_arousal),
    }
}

/// Hours between two moments in microseconds, in whole seconds (as the site
/// settles), never negative.
pub fn settle_hours(from_micros: i64, to_micros: i64) -> f64 {
    let seconds = to_micros.saturating_sub(from_micros).max(0) / 1_000_000;
    seconds as f64 / 3600.0
}

/// One change as replay reads it: when, toward what, and what moved her.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub id: String,
    pub at: i64,
    pub baseline: Baseline,
    pub impulse: AffectImpulse,
}

impl Change {
    pub fn of(id: impl Into<String>, event: &AffectEventBody) -> Self {
        Self {
            id: id.into(),
            at: event.at,
            baseline: event.baseline,
            impulse: event.impulse.clone(),
        }
    }
}

fn live(affect: Affect) -> rules::Affect {
    rules::Affect {
        mood: affect.mood,
        arousal: affect.arousal,
        emotion: affect.emotion,
        emotion_arousal: affect.emotion_arousal,
    }
}

fn crossed(affect: rules::Affect) -> Affect {
    Affect {
        mood: affect.mood,
        arousal: affect.arousal,
        emotion: affect.emotion,
        emotion_arousal: affect.emotion_arousal,
    }
}

fn base(baseline: Baseline) -> AffectBaseline {
    AffectBaseline {
        mood: baseline.mood,
        arousal: baseline.arousal,
    }
}

/// Whether `change` is after the anchor: later, or at the anchor's moment
/// with a greater id than the last change folded into it.
fn after_anchor(anchor: &AffectAnchorBody, change: &Change) -> bool {
    match &anchor.through_event {
        Some(through) => (change.at, change.id.as_str()) > (anchor.at, through.as_str()),
        None => change.at >= anchor.at,
    }
}

/// The changes after the anchor in replay order, `(at, id)`. A change from
/// before the anchor is in it already. A side that receives a change older
/// than its anchor (it arrived after its time was folded) stamps it with the
/// anchor's moment and marks it `late` before keeping it: it is applied
/// then, not lost.
fn in_order(anchor: &AffectAnchorBody, changes: &[Change]) -> Vec<Change> {
    let mut ordered: Vec<Change> = changes
        .iter()
        .filter(|change| after_anchor(anchor, change))
        .cloned()
        .collect();
    ordered.sort_by(|a, b| (a.at, &a.id).cmp(&(b.at, &b.id)));
    ordered
}

/// Her affect right after the last of `changes`, from `anchor`: each change
/// settles from the moment before it toward its own baseline, then moves her.
/// Returns the affect and the moment it stands at.
fn run(anchor: &AffectAnchorBody, ordered: &[Change]) -> (rules::Affect, i64) {
    let mut affect = live(anchor.affect);
    let mut at = anchor.at;
    for change in ordered {
        let hours = settle_hours(at, change.at);
        affect = settle(affect, base(change.baseline), hours, hours);
        change.impulse.apply(&mut affect);
        affect = clamped(affect);
        at = at.max(change.at);
    }
    (affect, at)
}

/// Her affect now: the anchor, every change after it in `(at, id)` order,
/// settled to `now` toward `baseline_now`. Pure: the same anchor, changes,
/// moment and baseline give the same bits on every side.
pub fn replay(
    anchor: &AffectAnchorBody,
    changes: &[Change],
    now: i64,
    baseline_now: Baseline,
) -> Result<Affect, SyncError> {
    check(anchor, changes)?;
    let (affect, at) = run(anchor, &in_order(anchor, changes));
    let hours = settle_hours(at, now);
    Ok(crossed(settle(affect, base(baseline_now), hours, hours)))
}

/// The anchor with every change up to `upto` (microseconds, inclusive)
/// folded in: it stands right after the last of them, which it names. The
/// folded changes need not be kept to replay what comes after. Unchanged
/// when nothing is that old.
pub fn fold(
    anchor: &AffectAnchorBody,
    changes: &[Change],
    upto: i64,
) -> Result<AffectAnchorBody, SyncError> {
    check(anchor, changes)?;
    let ordered: Vec<Change> = in_order(anchor, changes)
        .into_iter()
        .filter(|change| change.at <= upto)
        .collect();
    let Some(last) = ordered.last() else {
        return Ok(anchor.clone());
    };
    let through = last.id.clone();
    let (affect, at) = run(anchor, &ordered);
    Ok(AffectAnchorBody {
        affect: crossed(affect),
        at,
        through_event: Some(through),
    })
}

fn check(anchor: &AffectAnchorBody, changes: &[Change]) -> Result<(), SyncError> {
    let finite = |values: &[f64]| values.iter().all(|value| value.is_finite());
    let a = anchor.affect;
    if !finite(&[a.mood, a.arousal, a.emotion, a.emotion_arousal]) {
        return Err(SyncError::new("the anchor's affect values are numbers"));
    }
    for change in changes {
        let fine = finite(&[change.baseline.mood, change.baseline.arousal])
            && match change.impulse {
                AffectImpulse::Appraisal {
                    emotion,
                    arousal,
                    scale,
                } => finite(&[emotion, arousal, scale]),
                _ => true,
            };
        if !fine || change.id.is_empty() {
            return Err(SyncError::new(format!(
                "change {:?} has an id and numbers",
                change.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(at: i64) -> AffectAnchorBody {
        AffectAnchorBody {
            affect: Affect {
                mood: 70.0,
                arousal: 48.0,
                emotion: 50.0,
                emotion_arousal: 50.0,
            },
            at,
            through_event: Some("e0".into()),
        }
    }

    fn change(id: &str, at: i64, impulse: AffectImpulse) -> Change {
        Change {
            id: id.into(),
            at,
            baseline: Baseline {
                mood: 70.0,
                arousal: 48.0,
            },
            impulse,
        }
    }

    const HOUR: i64 = 3_600_000_000;

    #[test]
    fn replaying_is_the_rules_applied_in_turn() {
        let start = anchor(0);
        let praise = AffectImpulse::utterance(0, true, false);
        let changes = [
            change("e1", HOUR, praise.clone()),
            change("e2", 3 * HOUR, AffectImpulse::task(false)),
        ];
        let base = Baseline {
            mood: 70.0,
            arousal: 48.0,
        };
        let now = 5 * HOUR + 1_500_000;
        let replayed = replay(&start, &changes, now, base).unwrap();
        // The same by hand.
        let rest = AffectBaseline {
            mood: 70.0,
            arousal: 48.0,
        };
        let mut affect = live(start.affect);
        affect = settle(affect, rest, 1.0, 1.0);
        praise.apply(&mut affect);
        affect = clamped(affect);
        affect = settle(affect, rest, 2.0, 2.0);
        AffectImpulse::task(false).apply(&mut affect);
        affect = clamped(affect);
        // Whole seconds only: 2 hours and 1 second.
        let hours = 7201.0 / 3600.0;
        affect = settle(affect, rest, hours, hours);
        assert_eq!(replayed, crossed(affect));
        // Order is by time, then id, whatever order they come in.
        let reversed = [changes[1].clone(), changes[0].clone()];
        assert_eq!(replay(&start, &reversed, now, base).unwrap(), replayed);
    }

    #[test]
    fn folding_then_replaying_the_rest_is_replaying_all() {
        let start = anchor(0);
        let changes: Vec<Change> = (1..=6)
            .map(|index| {
                let impulse = match index % 3 {
                    0 => AffectImpulse::Music {
                        listened_seconds: 1500,
                    },
                    1 => AffectImpulse::utterance(index, false, true),
                    _ => AffectImpulse::None,
                };
                change(&format!("e{index}"), index as i64 * HOUR, impulse)
            })
            .collect();
        let base = Baseline {
            mood: 65.0,
            arousal: 50.0,
        };
        let now = 9 * HOUR;
        let all = replay(&start, &changes, now, base).unwrap();
        let folded = fold(&start, &changes, 3 * HOUR).unwrap();
        assert_eq!(folded.at, 3 * HOUR);
        assert_eq!(folded.through_event.as_deref(), Some("e3"));
        assert_eq!(replay(&folded, &changes, now, base).unwrap(), all);
        // Folding again with nothing new changes nothing.
        assert_eq!(fold(&folded, &changes, 3 * HOUR).unwrap(), folded);
        // Already folded changes are not applied twice.
        assert_eq!(replay(&folded, &changes[3..], now, base).unwrap(), all);
    }

    #[test]
    fn what_the_anchor_holds_is_not_applied_again() {
        let start = anchor(10 * HOUR);
        let base = Baseline {
            mood: 70.0,
            arousal: 48.0,
        };
        let before = change("e0", 10 * HOUR, AffectImpulse::task(true));
        let older = change("old", HOUR, AffectImpulse::task(true));
        let alone = replay(&start, &[], 12 * HOUR, base).unwrap();
        assert_eq!(
            replay(&start, &[before, older], 12 * HOUR, base).unwrap(),
            alone
        );
        // At the anchor's moment, after what it names: applied.
        let next = change("e1", 10 * HOUR, AffectImpulse::task(true));
        assert_ne!(replay(&start, &[next], 12 * HOUR, base).unwrap(), alone);
    }

    #[test]
    fn an_impulse_crosses_in_one_shape() {
        let shapes = [
            (
                AffectImpulse::appraisal(rules::APPRAISAL_PRAISE, 0.8),
                serde_json::json!({"appraisal": {"emotion": rules::APPRAISAL_PRAISE.emotion,
                    "arousal": rules::APPRAISAL_PRAISE.arousal, "scale": 0.8}}),
            ),
            (
                AffectImpulse::Music {
                    listened_seconds: 1320,
                },
                serde_json::json!({"music": {"listenedSeconds": 1320}}),
            ),
            (AffectImpulse::None, serde_json::json!("none")),
        ];
        for (impulse, json) in shapes {
            assert_eq!(serde_json::to_value(&impulse).unwrap(), json);
            assert_eq!(
                serde_json::from_value::<AffectImpulse>(json).unwrap(),
                impulse
            );
        }
    }
}
