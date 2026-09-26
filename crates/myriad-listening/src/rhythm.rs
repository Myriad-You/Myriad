//! Tempo, how clearly the beat can be felt, and how much happens between
//! beats.
//!
//! Tempo: the onset-strength envelope is autocorrelated over the periods of
//! 60–180 BPM, weighted toward moderate tempi by a log-Gaussian prior around
//! 120 BPM, as in Ellis (2007), "Beat tracking by dynamic programming",
//! J. New Music Research 36(1). The weight settles the usual doubling and
//! halving ambiguity toward the tempo people tap along to (Moelants 2002).
//!
//! Pulse clarity: the height of that autocorrelation peak, after Lartillot
//! et al. (2008), "Multi-feature modeling of pulse clarity", which found it
//! to track how clearly listeners feel a pulse.
//!
//! Syncopation: with the beat grid laid over the onsets, the share of onset
//! energy that falls neither on a beat nor on the half-beat. A proxy, not a
//! notated syncopation index.

use crate::spectrum::Frames;

const SLOWEST_BPM: f32 = 60.0;
const FASTEST_BPM: f32 = 180.0;
const PRIOR_BPM: f32 = 120.0;
/// Width of the tempo prior, in octaves.
const PRIOR_OCTAVES: f32 = 1.0;
/// Below this, there is no beat worth naming a tempo for.
const LEAST_CLARITY: f32 = 0.15;
/// Within this share of a beat of a grid point counts as on it.
const ON_GRID: f32 = 0.1;
/// Attacks must carry at least this share of the onset strength: a held
/// sound changes a little all the time without ever striking.
const LEAST_ATTACK: f32 = 0.15;

pub struct Rhythm {
    pub tempo_bpm: Option<f32>,
    pub pulse_clarity: f32,
    pub syncopation: f32,
}

/// Onset strength with the local average taken off, so that only the
/// attacks that stand out remain.
fn envelope(frames: &Frames) -> Vec<f32> {
    const AROUND: usize = 8;
    let onset = &frames.onset;
    (0..onset.len())
        .map(|i| {
            let from = i.saturating_sub(AROUND);
            let to = (i + AROUND + 1).min(onset.len());
            let local = onset[from..to].iter().sum::<f32>() / (to - from) as f32;
            (onset[i] - local).max(0.0)
        })
        .collect()
}

fn autocorrelation(signal: &[f32], lag: usize) -> f32 {
    signal
        .iter()
        .zip(&signal[lag.min(signal.len())..])
        .map(|(a, b)| a * b)
        .sum()
}

pub fn analyze(frames: &Frames) -> Rhythm {
    let none = Rhythm {
        tempo_bpm: None,
        pulse_clarity: 0.0,
        syncopation: 0.0,
    };
    let strength = envelope(frames);
    let fps = frames.per_second;
    let attack = strength.iter().sum::<f32>() / frames.onset.iter().sum::<f32>().max(f32::EPSILON);
    if attack < LEAST_ATTACK {
        return none;
    }
    // A beat rarely falls on a frame exactly; spreading each attack over its
    // neighbours lets periods between frames still line up.
    let smooth: Vec<f32> = (0..strength.len())
        .map(|i| {
            let at = |j: usize| strength.get(j).copied().unwrap_or(0.0);
            0.25 * at(i.wrapping_sub(1)) + 0.5 * at(i) + 0.25 * at(i + 1)
        })
        .collect();
    let shortest = (60.0 / FASTEST_BPM * fps).floor() as usize;
    let longest = (60.0 / SLOWEST_BPM * fps).ceil() as usize;
    if strength.len() < longest * 4 {
        return none;
    }
    let mean = smooth.iter().sum::<f32>() / smooth.len() as f32;
    let centered: Vec<f32> = smooth.iter().map(|value| value - mean).collect();
    let zero = autocorrelation(&centered, 0);
    if zero <= f32::EPSILON {
        return none;
    }
    let correlation: Vec<f32> = (0..=longest + 1)
        .map(|lag| autocorrelation(&centered, lag) / zero)
        .collect();

    let mut best: Option<(usize, f32)> = None;
    let mut clarity = 0.0f32;
    for lag in shortest.max(1)..=longest {
        let value = correlation[lag];
        let is_peak = value >= correlation[lag - 1] && value >= correlation[lag + 1];
        if !is_peak || value <= 0.0 {
            continue;
        }
        clarity = clarity.max(value);
        let bpm = 60.0 * fps / lag as f32;
        let prior = (-0.5 * ((bpm / PRIOR_BPM).log2() / PRIOR_OCTAVES).powi(2)).exp();
        let score = value * prior;
        if best.is_none_or(|(_, top)| score > top) {
            best = Some((lag, score));
        }
    }
    let clarity = clarity.clamp(0.0, 1.0);
    let Some((lag, _)) = best else {
        return none;
    };
    if clarity < LEAST_CLARITY {
        return Rhythm {
            pulse_clarity: clarity,
            ..none
        };
    }
    // The true period falls between frames; a parabola through the peak
    // finds it.
    let (before, at, after) = (correlation[lag - 1], correlation[lag], correlation[lag + 1]);
    let bend = before - 2.0 * at + after;
    let offset = if bend.abs() > f32::EPSILON {
        (0.5 * (before - after) / bend).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let period = lag as f32 + offset;
    Rhythm {
        tempo_bpm: Some(60.0 * fps / period),
        pulse_clarity: clarity,
        syncopation: syncopation(&strength, period),
    }
}

/// Lay a beat grid of this period where it best meets the onsets, then
/// measure the onset energy off both beats and half-beats.
fn syncopation(strength: &[f32], period: f32) -> f32 {
    let at = |position: f32| -> f32 {
        let index = position.round() as usize;
        (index.saturating_sub(1)..=index + 1)
            .filter_map(|i| strength.get(i))
            .copied()
            .fold(0.0, f32::max)
    };
    let steps = period.ceil() as usize;
    let phase = (0..steps)
        .map(|phase| {
            let mut sum = 0.0;
            let mut position = phase as f32;
            while (position as usize) < strength.len() {
                sum += at(position);
                position += period;
            }
            (phase as f32, sum)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(phase, _)| phase)
        .unwrap_or(0.0);

    let mut total = 0.0f32;
    let mut off = 0.0f32;
    for (i, &value) in strength.iter().enumerate() {
        if value <= 0.0 {
            continue;
        }
        let into = ((i as f32 - phase) / period).rem_euclid(1.0);
        let from_beat = into.min(1.0 - into);
        let on = from_beat < ON_GRID || (from_beat - 0.5).abs() < ON_GRID;
        total += value;
        if !on {
            off += value;
        }
    }
    if total > 0.0 { off / total } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum;
    use crate::testing::{RATE, audio, chord, clicks};

    #[test]
    fn a_steady_beat_is_found_with_its_tempo() {
        for bpm in [96.0, 120.0, 140.0] {
            let rhythm = analyze(&spectrum::analyze(&audio(clicks(bpm, 20.0, 0.8))));
            let tempo = rhythm.tempo_bpm.unwrap();
            assert!((tempo - bpm).abs() < 3.0, "{bpm}: {tempo}");
            assert!(rhythm.pulse_clarity > 0.5, "{}", rhythm.pulse_clarity);
            assert!(rhythm.syncopation < 0.15, "{}", rhythm.syncopation);
        }
    }

    #[test]
    fn hits_between_the_beats_are_syncopation() {
        let beats = clicks(120.0, 20.0, 0.8);
        // The same hits again, a quarter of a beat late.
        let delay = (0.125 * RATE as f32) as usize;
        let mut samples = beats.clone();
        for (i, value) in beats.iter().enumerate() {
            if let Some(slot) = samples.get_mut(i + delay) {
                *slot += value * 0.9;
            }
        }
        let rhythm = analyze(&spectrum::analyze(&audio(samples)));
        assert!((rhythm.tempo_bpm.unwrap() - 120.0).abs() < 3.0);
        assert!(rhythm.syncopation > 0.3, "{}", rhythm.syncopation);
    }

    #[test]
    fn a_held_tone_has_no_beat() {
        let rhythm = analyze(&spectrum::analyze(&audio(chord(
            &[220.0, 330.0],
            20.0,
            0.5,
        ))));
        assert_eq!(rhythm.tempo_bpm, None);
    }
}
