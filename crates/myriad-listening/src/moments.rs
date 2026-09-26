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
/// A change counts only if it holds: the few steps after, against the few
/// before. A note struck and dying away is not the music rising.
const BEFORE: usize = 4;
const AFTER: usize = 3;
const HOLDS_DB: f32 = 4.0;
const HOLDS_RATIO: f32 = 1.3;
/// A drop is a moment only if the music comes back afterwards (to within
/// this much of where it was); falling away for good is the ending.
const COMES_BACK_DB: f32 = 6.0;
/// A listener keeps only the few that stand out most.
const MOST: usize = 6;
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
    let mean = |values: &[f32]| values.iter().sum::<f32>() / values.len().max(1) as f32;
    let held_before = |values: &[f32], i: usize| mean(&values[i.saturating_sub(BEFORE)..i]);
    let held_after = |values: &[f32], i: usize| mean(&values[i..(i + AFTER).min(n)]);
    let last = n.saturating_sub(AFTER);
    // Each found with how much it stands out, for keeping the few.
    let mut found: Vec<(MomentKind, usize, f32, f32)> = Vec::new();

    for i in EDGE.max(BEFORE)..last {
        if !is_audible(i) {
            continue;
        }
        let before = loud[window(i)].iter().copied().fold(f32::MAX, f32::min);
        let rise = loud[i] - before;
        if rise >= SURGE_DB && held_after(loud, i) - held_before(loud, i) >= HOLDS_DB {
            found.push((MomentKind::Surge, i, rise, rise / SURGE_DB));
        }
    }
    for i in BEFORE..last.min(n.saturating_sub(EDGE)) {
        let before = loud[window(i)].iter().copied().fold(f32::MIN, f32::max);
        let fall = before - loud[i];
        let was = held_before(loud, i);
        let comes_back = loud[i + AFTER..].iter().any(|&l| l >= was - COMES_BACK_DB);
        if fall >= DROP_DB
            && is_audible(i - 1)
            && was - held_after(loud, i) >= HOLDS_DB
            && comes_back
        {
            found.push((MomentKind::Drop, i, fall, fall / DROP_DB));
        }
    }
    for i in EDGE.max(BEFORE)..last {
        if !is_audible(i) || bright[i] <= 0.0 {
            continue;
        }
        let before = bright[window(i)]
            .iter()
            .copied()
            .filter(|&b| b > 0.0)
            .fold(f32::MAX, f32::min);
        let ratio = bright[i] / before;
        let held = held_after(bright, i) / held_before(bright, i).max(1.0);
        if before < f32::MAX && ratio >= OPENS_UP && held >= HOLDS_RATIO {
            found.push((
                MomentKind::OpensUp,
                i,
                ratio,
                (ratio - 1.0) / (OPENS_UP - 1.0),
            ));
        }
    }
    // Of each run of nearby candidates of a kind, the strongest; then the
    // few that stand out most in the song.
    let mut kept: Vec<(MomentKind, usize, f32, f32)> = Vec::new();
    for kind in [MomentKind::Surge, MomentKind::Drop, MomentKind::OpensUp] {
        let of_kind: Vec<(usize, f32)> = found
            .iter()
            .filter(|f| f.0 == kind)
            .map(|f| (f.1, f.3))
            .collect();
        for (at, _) in strongest(of_kind) {
            if let Some(f) = found.iter().find(|f| f.0 == kind && f.1 == at) {
                kept.push(*f);
            }
        }
    }
    kept.sort_by(|a, b| b.3.total_cmp(&a.3));
    kept.truncate(MOST);
    for (kind, at, amount, _) in kept {
        push(kind, vec![(at, amount)]);
    }

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

    for (at, up) in crate::tonal::key_changes(seconds) {
        push(MomentKind::KeyChange, vec![(at, up as f32)]);
    }

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
        samples.extend(chord(&tone, 8.0, 0.5));
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
    fn a_note_dying_away_and_the_song_ending_are_not_moments() {
        // Struck notes: loud for a moment, then decaying, over and over.
        let mut samples = Vec::new();
        for _ in 0..12 {
            let note = chord(&[220.0, 330.0], 3.0, 1.0);
            samples.extend(note.iter().enumerate().map(|(i, value)| {
                let t = i as f32 / RATE as f32;
                value * 0.6 * (-2.0 * t).exp()
            }));
        }
        // The song dies away for good.
        samples.extend(chord(&[220.0], 6.0, 0.01));
        let per_second = spectrum::per_second(&spectrum::analyze(&audio(samples)));
        let moments = find(&per_second, &[]);
        let summary: Vec<(MomentKind, f32)> = moments.iter().map(|m| (m.kind, m.at_s)).collect();
        assert!(
            !moments
                .iter()
                .any(|m| matches!(m.kind, MomentKind::Surge | MomentKind::Drop)),
            "{summary:?}"
        );
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
