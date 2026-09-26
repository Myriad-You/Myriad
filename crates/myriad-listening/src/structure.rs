//! Where the song changes, and which parts come back.
//!
//! Each second is described by its pitch-class content, loudness and
//! brightness. Comparing every second with every other gives a
//! self-similarity matrix; sliding a checkerboard kernel along its diagonal
//! gives a novelty curve that peaks where one kind of music gives way to
//! another (Foote 2000, "Automatic audio segmentation using a measure of
//! audio novelty", ICME). The peaks are the section boundaries. Sections
//! that sound alike share a letter; the loudest one that comes back is most
//! likely the chorus.

use crate::Section;
use crate::spectrum::Seconds;

/// Half the kernel width, in steps (about seconds).
const KERNEL: usize = 6;
/// Sections are at least this long, in steps.
const SHORTEST: usize = 8;
/// Weight of loudness and of brightness against each pitch class.
const LEVEL_WEIGHT: f32 = 2.0;
/// Less novel than this is no boundary, however quiet the rest.
const LEAST_NOVELTY: f32 = 0.1;
/// Sections this alike (cosine) share a letter.
const ALIKE: f32 = 0.6;

/// Each second as a vector: every dimension standardized over the song.
fn features(seconds: &Seconds) -> Vec<Vec<f32>> {
    let n = seconds.loudness_db.len();
    let mut columns: Vec<Vec<f32>> = (0..12)
        .map(|class| seconds.chroma.iter().map(|chroma| chroma[class]).collect())
        .collect();
    columns.push(seconds.loudness_db.clone());
    columns.push(seconds.brightness_hz.clone());
    for (index, column) in columns.iter_mut().enumerate() {
        let mean = column.iter().sum::<f32>() / n as f32;
        let spread = (column.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n as f32).sqrt();
        // Differences smaller than these are not heard as change, however
        // steady the rest of the song is.
        let least = match index {
            12 => 1.0,
            13 => 0.1 * mean.abs(),
            _ => 0.02,
        };
        let weight = if index >= 12 { LEVEL_WEIGHT } else { 1.0 };
        for value in column.iter_mut() {
            *value = (*value - mean) / spread.max(least).max(1e-6) * weight;
        }
    }
    (0..n)
        .map(|i| columns.iter().map(|column| column[i]).collect())
        .collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = (a.iter().map(|x| x * x).sum::<f32>() * b.iter().map(|y| y * y).sum::<f32>()).sqrt();
    if norm > 1e-9 { dot / norm } else { 0.0 }
}

/// How alike two seconds sound, 0 to 1: near 1 for the same music, and
/// near e^-1 for two seconds as different as any two in a varied song.
fn similarity(a: &[f32], b: &[f32]) -> f32 {
    let distance: f32 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum();
    (-distance / (2.0 * a.len() as f32)).exp()
}

/// Foote's novelty: a Gaussian-tapered checkerboard kernel along the
/// diagonal of the self-similarity matrix, scaled so that 1 is a change
/// from one kind of music to something entirely unlike it.
fn novelty(vectors: &[Vec<f32>]) -> Vec<f32> {
    let n = vectors.len();
    let mut curve = vec![0.0f32; n];
    if n < 2 * KERNEL + 1 {
        return curve;
    }
    let taper = |offset: f32| (-0.5 * (offset / (KERNEL as f32 * 0.5)).powi(2)).exp();
    let mut same_side_weight = 0.0;
    for a in 0..2 * KERNEL {
        for b in 0..2 * KERNEL {
            if (a < KERNEL) == (b < KERNEL) {
                same_side_weight +=
                    taper(a as f32 - KERNEL as f32 + 0.5) * taper(b as f32 - KERNEL as f32 + 0.5);
            }
        }
    }
    for (center, value) in curve.iter_mut().enumerate().take(n - KERNEL).skip(KERNEL) {
        let mut sum = 0.0;
        for a in 0..2 * KERNEL {
            for b in 0..2 * KERNEL {
                let (i, j) = (center + a - KERNEL, center + b - KERNEL);
                let same_side = (a < KERNEL) == (b < KERNEL);
                let weight =
                    taper(a as f32 - KERNEL as f32 + 0.5) * taper(b as f32 - KERNEL as f32 + 0.5);
                let alike = similarity(&vectors[i], &vectors[j]);
                sum += if same_side { weight } else { -weight } * alike;
            }
        }
        *value = (sum / same_side_weight).max(0.0);
    }
    curve
}

/// Section starts: novelty peaks that stand out, far enough apart.
fn boundaries(curve: &[f32]) -> Vec<usize> {
    let n = curve.len();
    let mean = curve.iter().sum::<f32>() / n.max(1) as f32;
    let spread = (curve.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / n.max(1) as f32).sqrt();
    let threshold = (mean + spread).max(LEAST_NOVELTY);
    let mut peaks: Vec<usize> = (SHORTEST..n.saturating_sub(SHORTEST / 2))
        .filter(|&i| {
            let from = i.saturating_sub(SHORTEST / 2);
            let to = (i + SHORTEST / 2 + 1).min(n);
            curve[i] > threshold
                && curve[i] > 0.0
                && curve[from..to].iter().all(|&other| other <= curve[i])
        })
        .collect();
    // Of two peaks too close together, the stronger stays.
    peaks.sort_by(|&a, &b| curve[b].total_cmp(&curve[a]));
    let mut kept: Vec<usize> = Vec::new();
    for peak in peaks {
        if kept.iter().all(|&other| other.abs_diff(peak) >= SHORTEST) {
            kept.push(peak);
        }
    }
    kept.sort_unstable();
    kept
}

pub fn sections(seconds: &Seconds) -> Vec<Section> {
    let n = seconds.loudness_db.len();
    if n == 0 {
        return Vec::new();
    }
    let vectors = features(seconds);
    let curve = novelty(&vectors);
    let mut starts = vec![0];
    starts.extend(boundaries(&curve));
    let spans: Vec<(usize, usize)> = starts
        .iter()
        .enumerate()
        .map(|(index, &start)| (start, starts.get(index + 1).copied().unwrap_or(n)))
        .collect();

    let mean_of = |from: usize, to: usize| -> Vec<f32> {
        let width = vectors[0].len();
        let mut mean = vec![0.0; width];
        for vector in &vectors[from..to] {
            for (slot, value) in mean.iter_mut().zip(vector) {
                *slot += value / (to - from) as f32;
            }
        }
        mean
    };
    let song_loudness = seconds.mean_loudness_db();
    let song_brightness = seconds.mean_brightness().max(1.0);
    let mut labels: Vec<(char, Vec<f32>)> = Vec::new();
    let mut result: Vec<Section> = spans
        .iter()
        .map(|&(from, to)| {
            let mean = mean_of(from, to);
            let alike = labels
                .iter()
                .map(|(label, other)| (*label, cosine(&mean, other)))
                .filter(|(_, similarity)| *similarity >= ALIKE)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(label, _)| label);
            let label = alike.unwrap_or_else(|| {
                let next = (b'A' + labels.len().min(25) as u8) as char;
                labels.push((next, mean.clone()));
                next
            });
            let power = seconds.loudness_db[from..to]
                .iter()
                .map(|db| 10f32.powf(db / 10.0))
                .sum::<f32>()
                / (to - from) as f32;
            let brightness =
                seconds.brightness_hz[from..to].iter().sum::<f32>() / (to - from) as f32;
            Section {
                start_s: from as f32 * seconds.step_s,
                end_s: to as f32 * seconds.step_s,
                label,
                likely_chorus: false,
                loudness_db: 10.0 * power.max(1e-12).log10() - song_loudness,
                brightness: brightness / song_brightness,
                change: if from == 0 { 0.0 } else { curve[from].min(1.0) },
            }
        })
        .collect();

    // The chorus: of the parts that come back, the loudest.
    let returning = |label: char| result.iter().filter(|s| s.label == label).count() > 1;
    let chorus = result
        .iter()
        .filter(|section| returning(section.label))
        .map(|section| {
            let same: Vec<&Section> = result.iter().filter(|s| s.label == section.label).collect();
            let loudness = same.iter().map(|s| s.loudness_db).sum::<f32>() / same.len() as f32;
            (section.label, loudness)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(label, _)| label);
    if let Some(chorus) = chorus {
        for section in result.iter_mut().filter(|s| s.label == chorus) {
            section.likely_chorus = true;
        }
    }
    result
}

/// Share of the song spent in parts that come back.
pub fn repetition(sections: &[Section]) -> f32 {
    let total: f32 = sections.iter().map(|s| s.end_s - s.start_s).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let returning: f32 = sections
        .iter()
        .filter(|section| sections.iter().filter(|s| s.label == section.label).count() > 1)
        .map(|s| s.end_s - s.start_s)
        .sum();
    returning / total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum;
    use crate::testing::{audio, chord};

    const VERSE: [f32; 3] = [261.63, 329.63, 392.00];
    const CHORUS: [f32; 3] = [349.23, 440.00, 523.25];

    #[test]
    fn a_verse_and_a_louder_chorus_twice_over() {
        let mut samples = Vec::new();
        for _ in 0..2 {
            samples.extend(chord(&VERSE, 16.0, 0.1));
            samples.extend(chord(&CHORUS, 16.0, 0.6));
        }
        let per_second = spectrum::per_second(&spectrum::analyze(&audio(samples)));
        let sections = sections(&per_second);
        let starts: Vec<f32> = sections.iter().map(|s| s.start_s).collect();
        assert_eq!(sections.len(), 4, "{starts:?}");
        for (section, expected) in sections.iter().zip([0.0, 16.0, 32.0, 48.0]) {
            assert!((section.start_s - expected).abs() <= 2.0, "{starts:?}");
        }
        let labels: String = sections.iter().map(|s| s.label).collect();
        assert_eq!(labels, "ABAB");
        assert!(sections[1].likely_chorus && sections[3].likely_chorus);
        assert!(!sections[0].likely_chorus);
        assert!(sections[1].loudness_db > sections[0].loudness_db + 10.0);
        assert_eq!(sections[0].change, 0.0);
        assert!(sections[1].change > 0.3, "{}", sections[1].change);
        assert!((repetition(&sections) - 1.0).abs() < 0.01);
    }

    #[test]
    fn a_song_that_never_changes_is_one_section() {
        let per_second = spectrum::per_second(&spectrum::analyze(&audio(chord(&VERSE, 30.0, 0.3))));
        let sections = sections(&per_second);
        assert_eq!(sections.len(), 1);
        assert_eq!(repetition(&sections), 0.0);
    }
}
