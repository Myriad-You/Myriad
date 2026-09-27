//! Key and mode, and how clearly the music sits in them.
//!
//! The pitch-class profile of the whole song is correlated with the
//! Krumhansl–Kessler probe-tone profiles in all 24 keys (Krumhansl 1990,
//! "Cognitive Foundations of Musical Pitch"): how well each of the twelve
//! pitch classes was heard to fit a key, measured with listeners. The best
//! match is the key; its correlation is how clear the key is.

use crate::spectrum::{Frames, Seconds};

const MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
const NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
/// Below this the key is too unclear to name.
const LEAST_CLARITY: f32 = 0.5;
/// A profile needs about this much contrast between pitch classes (spread
/// over mean) for its best match to mean anything: all twelve at once
/// correlates with some key by chance.
const FULL_CONTRAST: f32 = 0.4;

pub struct Tonal {
    pub key: Option<String>,
    pub minor: bool,
    pub clarity: f32,
}

fn correlation(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let mean_a = a.iter().sum::<f32>() / 12.0;
    let mean_b = b.iter().sum::<f32>() / 12.0;
    let (mut cross, mut var_a, mut var_b) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let (x, y) = (a[i] - mean_a, b[i] - mean_b);
        cross += x * y;
        var_a += x * x;
        var_b += y * y;
    }
    if var_a <= f32::EPSILON || var_b <= f32::EPSILON {
        return 0.0;
    }
    cross / (var_a * var_b).sqrt()
}

/// The pitch-class profile of the audible frames.
pub fn profile(frames: &Frames) -> [f32; 12] {
    let loudest = frames.loudness_db.iter().copied().fold(f32::MIN, f32::max);
    let mut sum = [0.0f32; 12];
    for (chroma, &db) in frames.chroma.iter().zip(&frames.loudness_db) {
        if db > loudest - 30.0 {
            for (class, value) in chroma.iter().enumerate() {
                sum[class] += value;
            }
        }
    }
    sum
}

/// The best-fitting key of a pitch-class profile: tonic, minor, correlation.
pub fn best_key(profile: &[f32; 12]) -> (usize, bool, f32) {
    let mut best = (0, false, f32::MIN);
    for tonic in 0..12 {
        for (minor, template) in [(false, &MAJOR), (true, &MINOR)] {
            let rotated: [f32; 12] =
                std::array::from_fn(|class| template[(class + 12 - tonic) % 12]);
            let r = correlation(profile, &rotated);
            if r > best.2 {
                best = (tonic, minor, r);
            }
        }
    }
    best
}

/// A profile moved up by this many semitones.
fn transposed(profile: &[f32; 12], up: i32) -> [f32; 12] {
    std::array::from_fn(|class| profile[(class as i32 - up).rem_euclid(12) as usize])
}

/// Where the music lifts into a higher key and stays there, as (step,
/// semitones up). The stretch after must be the stretch before moved up as a
/// whole: its pitch content matches the earlier one transposed far better
/// than as it was, both sound clearly in a key, the new key holds, and the
/// music is not fading out. Moves down are left out: on 26 real songs they
/// were mostly a progression resting on another chord, not a new key; a
/// move between relative major and minor (the same notes) is not one.
pub fn key_changes(seconds: &Seconds) -> Vec<(usize, i32)> {
    /// Steps the old key must have held, and the new one must hold: a key
    /// has to be established before it can change.
    const HELD: usize = 20;
    const SIDE: usize = 10;
    /// How much better the transposed match must be.
    const GAIN: f32 = 0.25;
    const MATCH: f32 = 0.7;
    const CLEAR: f32 = 0.5;
    const MOVES: [i32; 3] = [1, 2, 3];
    /// The music after is not this much quieter than the song.
    const NOT_FADING_DB: f32 = 6.0;
    let song = seconds.mean_loudness_db();
    let n = seconds.chroma.len();
    if n < HELD + SIDE + 1 {
        return Vec::new();
    }
    let sum = |from: usize, to: usize| -> [f32; 12] {
        let mut total = [0.0f32; 12];
        for chroma in &seconds.chroma[from..to] {
            for (class, value) in chroma.iter().enumerate() {
                total[class] += value;
            }
        }
        total
    };
    let mut found: Vec<(usize, i32, f32)> = Vec::new();
    for i in HELD..=n - SIDE {
        let (before, after) = (sum(i - HELD, i), sum(i, i + SIDE));
        let (_, _, clear_before) = best_key(&before);
        let (_, _, clear_after) = best_key(&after);
        let after_loudness = seconds.loudness_db[i..i + SIDE].iter().sum::<f32>() / SIDE as f32;
        if clear_before < CLEAR || clear_after < CLEAR || after_loudness < song - NOT_FADING_DB {
            continue;
        }
        // Whichever shift makes the before sound most like the after; it
        // must be a lift, not staying put or moving down.
        let as_was = correlation(&before, &after);
        let (up, matched) = (-5..=6)
            .map(|up| (up, correlation(&transposed(&before, up), &after)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, as_was));
        if MOVES.contains(&up) && matched >= MATCH && matched - as_was >= GAIN {
            found.push((i, up, matched - as_was));
        }
    }
    // One change per run of candidates: where the difference is greatest.
    let mut kept: Vec<(usize, i32, f32)> = Vec::new();
    for candidate in found {
        match kept.last_mut() {
            Some(last) if candidate.0 - last.0 <= SIDE => {
                if candidate.2 > last.2 {
                    *last = candidate;
                }
            }
            _ => kept.push(candidate),
        }
    }
    kept.into_iter().map(|(at, up, _)| (at, up)).collect()
}

pub fn analyze(frames: &Frames) -> Tonal {
    let profile = profile(frames);
    let (tonic, minor, r) = best_key(&profile);
    let mean = profile.iter().sum::<f32>() / 12.0;
    let contrast = if mean > 0.0 {
        (profile.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / 12.0).sqrt() / mean
    } else {
        0.0
    };
    let clarity = (r * (contrast / FULL_CONTRAST).min(1.0)).clamp(0.0, 1.0);
    Tonal {
        key: (clarity >= LEAST_CLARITY)
            .then(|| format!("{} {}", NAMES[tonic], if minor { "minor" } else { "major" })),
        minor,
        clarity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum;
    use crate::testing::{audio, chord};

    const C4: f32 = 261.63;
    const D4: f32 = 293.66;
    const E4: f32 = 329.63;
    const F4: f32 = 349.23;
    const G4: f32 = 392.00;
    const A3: f32 = 220.00;
    const B3: f32 = 246.94;

    fn key_of(chords: &[&[f32]]) -> Tonal {
        let samples: Vec<f32> = chords
            .iter()
            .flat_map(|notes| chord(notes, 2.0, 0.5))
            .collect();
        analyze(&spectrum::analyze(&audio(samples)))
    }

    #[test]
    fn a_progression_is_heard_in_its_key() {
        // I–IV–V–I in C.
        let c = key_of(&[
            &[C4, E4, G4],
            &[F4, A3 * 2.0, C4 * 2.0],
            &[G4, B3 * 2.0, D4 * 2.0],
            &[C4, E4, G4],
        ]);
        assert_eq!(c.key.as_deref(), Some("C major"));
        assert!(c.clarity > 0.6, "{}", c.clarity);
        // i–iv–V–i in A minor (with the raised seventh, G#).
        let a = key_of(&[
            &[A3, C4, E4],
            &[D4, F4, A3 * 2.0],
            &[E4, 415.30, B3 * 2.0],
            &[A3, C4, E4],
        ]);
        assert_eq!(a.key.as_deref(), Some("A minor"));
        assert!(a.minor);
    }

    #[test]
    fn a_lift_into_a_new_key_is_found_where_it_happens() {
        let progression = |shift: f32| -> Vec<f32> {
            let up = 2f32.powf(shift / 12.0);
            [
                [C4, E4, G4],
                [F4, A3 * 2.0, C4 * 2.0],
                [G4, B3 * 2.0, D4 * 2.0],
                [C4, E4, G4],
            ]
            .iter()
            .flat_map(|notes| {
                let notes: Vec<f32> = notes.iter().map(|f| f * up).collect();
                chord(&notes, 2.0, 0.5)
            })
            .collect()
        };
        let mut samples = Vec::new();
        for _ in 0..3 {
            samples.extend(progression(0.0));
        }
        for _ in 0..3 {
            samples.extend(progression(2.0));
        }
        let seconds = spectrum::per_second(&spectrum::analyze(&audio(samples)));
        let changes = key_changes(&seconds);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].1, 2);
        assert!((changes[0].0 as i64 - 24).abs() <= 2, "{changes:?}");
        // Back down again is not a lift.
        let mut down = Vec::new();
        for _ in 0..3 {
            down.extend(progression(2.0));
        }
        for _ in 0..3 {
            down.extend(progression(0.0));
        }
        let seconds = spectrum::per_second(&spectrum::analyze(&audio(down)));
        assert!(
            key_changes(&seconds).is_empty(),
            "{:?}",
            key_changes(&seconds)
        );
        // The same key throughout: no change.
        let mut same = Vec::new();
        for _ in 0..6 {
            same.extend(progression(0.0));
        }
        let seconds = spectrum::per_second(&spectrum::analyze(&audio(same)));
        assert!(key_changes(&seconds).is_empty());
    }

    #[test]
    fn all_twelve_notes_at_once_have_no_key() {
        let cluster: Vec<f32> = (0..12).map(|i| C4 * 2f32.powf(i as f32 / 12.0)).collect();
        let tonal = key_of(&[&cluster]);
        assert_eq!(tonal.key, None);
    }
}
