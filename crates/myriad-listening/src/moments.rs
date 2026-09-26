//! The moments in a song where something happens to the sound.
//!
//! These are the events that listening studies keep finding at the places
//! people are moved: a sudden rise in loudness, the sound opening up into
//! higher registers, a section entering, a long build and what comes after
//! it (see `reading` for the findings).

use crate::spectrum::Seconds;
use crate::{Moment, MomentKind, Section};

/// Louder by this much within two steps is a surge.
const SURGE_DB: f32 = 6.0;
/// Quieter by this much within two steps is a drop.
const DROP_DB: f32 = 8.0;
/// Brighter by this ratio within two steps: the sound opens up.
const OPENS_UP: f32 = 1.6;
/// A build rises at least this much, over this many steps, with no single
/// jump in it.
const BUILD_DB: f32 = 6.0;
const BUILD_SHORTEST: usize = 8;
const BUILD_LONGEST: usize = 20;
const BUILD_JUMP_DB: f32 = 4.0;
/// Candidates this close together are one moment.
const APART: usize = 3;
/// The song coming in and dying away are not moments in it.
const EDGE: usize = 4;

/// Keep the strongest of each run of nearby candidates.
fn strongest(candidates: Vec<(usize, f32)>) -> Vec<(usize, f32)> {
    let mut kept: Vec<(usize, f32)> = Vec::new();
    for (at, amount) in candidates {
        match kept.last_mut() {
            Some(last) if at - last.0 <= APART => {
                if amount > last.1 {
                    *last = (at, amount);
                }
            }
            _ => kept.push((at, amount)),
        }
    }
    kept
}

fn smoothed(values: &[f32]) -> Vec<f32> {
    (0..values.len())
        .map(|i| {
            let from = i.saturating_sub(1);
            let to = (i + 2).min(values.len());
            values[from..to].iter().sum::<f32>() / (to - from) as f32
        })
        .collect()
}

pub fn find(seconds: &Seconds, sections: &[Section]) -> Vec<Moment> {
    let loud = &seconds.loudness_db;
    let bright = &seconds.brightness_hz;
    let n = loud.len();
    let audible = seconds.audible();
    let is_audible = |i: usize| audible.binary_search(&i).is_ok();
    let step = seconds.step_s;
    let mut moments: Vec<Moment> = Vec::new();
    let mut push = |kind: MomentKind, found: Vec<(usize, f32)>| {
        moments.extend(found.into_iter().map(|(at, amount)| Moment {
            at_s: at as f32 * step,
            kind,
            amount,
        }));
    };

    let window = |i: usize| i.saturating_sub(2)..i;
    let surges = (EDGE.max(2)..n)
        .filter(|&i| is_audible(i))
        .filter_map(|i| {
            let before = loud[window(i)].iter().copied().fold(f32::MAX, f32::min);
            let rise = loud[i] - before;
            (rise >= SURGE_DB).then_some((i, rise))
        })
        .collect();
    push(MomentKind::Surge, strongest(surges));

    let drops = (2..n.saturating_sub(EDGE))
        .filter_map(|i| {
            let before = loud[window(i)].iter().copied().fold(f32::MIN, f32::max);
            let fall = before - loud[i];
            (fall >= DROP_DB && is_audible(i - 1)).then_some((i, fall))
        })
        .collect();
    push(MomentKind::Drop, strongest(drops));

    let opens = (EDGE.max(2)..n)
        .filter(|&i| is_audible(i) && bright[i] > 0.0)
        .filter_map(|i| {
            let before = bright[window(i)]
                .iter()
                .copied()
                .filter(|&b| b > 0.0)
                .fold(f32::MAX, f32::min);
            let ratio = bright[i] / before;
            (before < f32::MAX && ratio >= OPENS_UP).then_some((i, ratio))
        })
        .collect();
    push(MomentKind::OpensUp, strongest(opens));

    // A build: a steady rise over eight to twenty steps with no single
    // jump carrying it, ending where the rise stops.
    let level = smoothed(loud);
    let build_at = |i: usize| -> Option<usize> {
        (BUILD_SHORTEST..=BUILD_LONGEST.min(i))
            .rev()
            .find(|&length| {
                let from = i - length;
                let steady = (from + 2..=i).all(|t| loud[t] - loud[t - 2] < BUILD_JUMP_DB);
                let never_falls = (from..=i).all(|t| {
                    level[from..=t].iter().copied().fold(f32::MIN, f32::max) - level[t] <= 2.0
                });
                level[i] - level[from] >= BUILD_DB && steady && never_falls
            })
    };
    let mut builds: Vec<(usize, f32)> = Vec::new();
    let mut run: Option<(usize, usize)> = None;
    for i in 0..=n {
        match (i < n).then(|| build_at(i)).flatten() {
            Some(length) => {
                let better = run.is_none_or(|(at, _)| level[i] >= level[at]);
                if better {
                    run = Some((i, length.max(run.map_or(0, |(_, l)| l))));
                }
            }
            None => {
                if let Some((at, length)) = run.take() {
                    builds.push((at, length as f32 * step));
                }
            }
        }
    }
    push(MomentKind::Build, builds);

    for pair in sections.windows(2) {
        moments.push(Moment {
            at_s: pair[1].start_s,
            kind: MomentKind::NewSection,
            amount: pair[1].loudness_db - pair[0].loudness_db,
        });
    }
    moments.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
    moments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum;
    use crate::testing::{RATE, audio, chord};

    fn kinds_near(moments: &[Moment], kind: MomentKind, at: f32) -> bool {
        moments
            .iter()
            .any(|moment| moment.kind == kind && (moment.at_s - at).abs() <= 2.0)
    }

    #[test]
    fn a_build_a_surge_and_a_drop_are_heard_where_they_happen() {
        let tone = [220.0, 330.0];
        let mut samples = chord(&tone, 10.0, 0.02);
        // Twenty dB up over twelve seconds, evenly.
        let ramp = chord(&tone, 12.0, 1.0);
        samples.extend(ramp.iter().enumerate().map(|(i, value)| {
            let t = i as f32 / RATE as f32;
            value * 0.02 * 10f32.powf(t / 12.0)
        }));
        samples.extend(chord(&tone, 8.0, 0.8));
        samples.extend(chord(&tone, 8.0, 0.02));
        samples.extend(crate::testing::silence(3.0));
        let per_second = spectrum::per_second(&spectrum::analyze(&audio(samples)));
        let moments = find(&per_second, &[]);
        let summary: Vec<(MomentKind, f32)> = moments.iter().map(|m| (m.kind, m.at_s)).collect();
        // The build ends just before the surge breaks in.
        assert!(
            moments
                .iter()
                .any(|m| m.kind == MomentKind::Build && (18.5..=22.5).contains(&m.at_s)),
            "{summary:?}"
        );
        assert!(kinds_near(&moments, MomentKind::Surge, 22.0), "{summary:?}");
        assert!(kinds_near(&moments, MomentKind::Drop, 30.0), "{summary:?}");
        // The song ending is no drop, and its start no surge.
        assert!(
            !moments
                .iter()
                .any(|m| m.at_s > per_second.loudness_db.len() as f32 - 4.0)
        );
        assert!(!moments.iter().any(|m| m.at_s < 3.0), "{summary:?}");
        // The ramp itself is no surge.
        assert!(
            !kinds_near(&moments, MomentKind::Surge, 15.0),
            "{summary:?}"
        );
        let build = moments
            .iter()
            .find(|m| m.kind == MomentKind::Build)
            .unwrap();
        assert!(build.amount >= 8.0, "{summary:?}");
    }

    #[test]
    fn the_sound_opening_up_is_heard() {
        let mut samples = chord(&[220.0], 10.0, 0.3);
        samples.extend(chord(&[220.0, 1760.0, 3520.0], 10.0, 0.3));
        let per_second = spectrum::per_second(&spectrum::analyze(&audio(samples)));
        let moments = find(&per_second, &[]);
        assert!(kinds_near(&moments, MomentKind::OpensUp, 10.0));
        assert!(!moments.iter().any(|m| m.kind == MomentKind::Surge));
    }
}
