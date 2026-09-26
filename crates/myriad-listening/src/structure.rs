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

use std::collections::HashMap;

use crate::spectrum::Seconds;
use crate::{LyricLine, Section};

/// Half the kernel width, in steps (about seconds).
const KERNEL: usize = 6;
/// Sections are at least this long, in steps.
const SHORTEST: usize = 8;
/// Weight of loudness and of brightness against each pitch class.
const LEVEL_WEIGHT: f32 = 2.0;
/// Less novel than this is no boundary, however quiet the rest.
const LEAST_NOVELTY: f32 = 0.1;
/// Sections this alike share a letter (see `alike`).
const ALIKE: f32 = 0.45;
/// A part longer than this, in steps, is split where it changes most
/// inside: a whole verse and pre-chorus rarely run this long unchanged.
const LONGEST: usize = 40;

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
    split_long(&mut starts, n, &curve);
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
    // Each part takes the letter of the earlier part it sounds most like,
    // if any sounds alike enough; otherwise a new letter.
    let mut heard: Vec<(char, (usize, usize), Vec<f32>)> = Vec::new();
    let mut letters = 0u8;
    let result: Vec<Section> = spans
        .iter()
        .map(|&(from, to)| {
            let mean = mean_of(from, to);
            let alike = heard
                .iter()
                .map(|(label, span, other)| {
                    let overall = cosine(&mean, other);
                    let unfolding = unfolds_alike(&vectors, (from, to), *span);
                    (*label, (overall + unfolding) / 2.0)
                })
                .filter(|(_, similarity)| *similarity >= ALIKE)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(label, _)| label);
            let label = alike.unwrap_or_else(|| {
                let next = (b'A' + letters.min(25)) as char;
                letters += 1;
                next
            });
            heard.push((label, (from, to), mean.clone()));
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

    let mut result = merge_neighbours(result);
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

/// How alike two parts unfold: the shorter laid along the longer where
/// they match best, second by second, by pitch content (the same chords in
/// the same order), 1 for the same.
fn unfolds_alike(vectors: &[Vec<f32>], a: (usize, usize), b: (usize, usize)) -> f32 {
    let (short, long) = if a.1 - a.0 <= b.1 - b.0 {
        (a, b)
    } else {
        (b, a)
    };
    let length = short.1 - short.0;
    if length == 0 {
        return 0.0;
    }
    (0..=(long.1 - long.0 - length))
        .map(|offset| {
            (0..length)
                .map(|k| {
                    cosine(
                        &vectors[short.0 + k][..12],
                        &vectors[long.0 + offset + k][..12],
                    )
                })
                .sum::<f32>()
                / length as f32
        })
        .fold(f32::MIN, f32::max)
}

/// Neighbouring parts with the same letter are one part.
fn merge_neighbours(sections: Vec<Section>) -> Vec<Section> {
    let mut merged: Vec<Section> = Vec::new();
    for section in sections {
        match merged.last_mut() {
            Some(last) if last.label == section.label => {
                let (a, b) = (last.end_s - last.start_s, section.end_s - section.start_s);
                let power = |db: f32| 10f32.powf(db / 10.0);
                last.loudness_db = 10.0
                    * ((power(last.loudness_db) * a + power(section.loudness_db) * b) / (a + b))
                        .log10();
                last.brightness = (last.brightness * a + section.brightness * b) / (a + b);
                last.end_s = section.end_s;
            }
            _ => merged.push(section),
        }
    }
    merged
}

/// Split parts longer than `LONGEST` where they change most inside, as long
/// as that change is a real one and leaves both halves long enough.
fn split_long(starts: &mut Vec<usize>, n: usize, curve: &[f32]) {
    loop {
        let ends: Vec<usize> = starts.iter().skip(1).copied().chain([n]).collect();
        let split = starts.iter().zip(&ends).find_map(|(&from, &to)| {
            if to - from <= LONGEST {
                return None;
            }
            (from + SHORTEST..to.saturating_sub(SHORTEST))
                .max_by(|&a, &b| curve[a].total_cmp(&curve[b]))
                .filter(|&at| curve[at] >= LEAST_NOVELTY)
        });
        match split {
            Some(at) => {
                starts.push(at);
                starts.sort_unstable();
            }
            None => return,
        }
    }
}

/// Lines this far apart or more are the words coming back, not one line
/// sung twice in a row.
const WORDS_RETURN_S: f32 = 20.0;

/// A lyric line as compared: without spaces and punctuation, lowercased;
/// none when too short to tell apart.
fn words_key(text: &str) -> Option<String> {
    let key: String = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    (key.chars().count() >= 4).then_some(key)
}

/// With lyrics, the words say which parts are the same: parts that sing the
/// same lines are one kind of part, whatever the sound, and the part whose
/// lines come back most is the chorus. That is how listeners know a chorus
/// too. Without lines coming back, the sound's reading stands.
pub fn by_words(sections: Vec<Section>, lyrics: &[LyricLine]) -> Vec<Section> {
    if sections.len() < 2 {
        return sections;
    }
    let sections = align_to_words(sections, lyrics);
    let index_at = |at: f32| {
        sections
            .iter()
            .position(|s| s.start_s <= at && at < s.end_s)
            .unwrap_or(sections.len() - 1)
    };
    // Where each line is sung.
    let mut sung: HashMap<String, Vec<(f32, usize)>> = HashMap::new();
    for line in lyrics {
        if let Some(key) = words_key(&line.text) {
            sung.entry(key)
                .or_default()
                .push((line.at_s, index_at(line.at_s)));
        }
    }
    let returning: Vec<&Vec<(f32, usize)>> = sung
        .values()
        .filter(|times| {
            let first = times.iter().map(|t| t.0).fold(f32::MAX, f32::min);
            let last = times.iter().map(|t| t.0).fold(f32::MIN, f32::max);
            last - first >= WORDS_RETURN_S
        })
        .collect();
    if returning.is_empty() {
        return sections;
    }

    // Parts that share two returning lines are the same kind of part.
    let n = sections.len();
    let mut shared = vec![vec![0usize; n]; n];
    let mut weight = vec![0usize; n];
    for times in &returning {
        let mut parts: Vec<usize> = times.iter().map(|t| t.1).collect();
        parts.sort_unstable();
        parts.dedup();
        for &part in &parts {
            weight[part] += times.len() - 1;
        }
        for (a, &i) in parts.iter().enumerate() {
            for &j in &parts[a + 1..] {
                shared[i][j] += 1;
            }
        }
    }
    let mut root: Vec<usize> = (0..n).collect();
    fn find(root: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            if shared[i][j] >= 2 {
                let (a, b) = (find(&mut root, i), find(&mut root, j));
                root[a.max(b)] = a.min(b);
            }
        }
    }
    let mut sections = sections;
    for i in 0..n {
        let first = find(&mut root, i);
        if first != i {
            sections[i].label = sections[first].label;
        }
    }

    // The chorus: the kind of part whose lines come back most.
    let mut by_label: HashMap<char, (usize, usize)> = HashMap::new();
    for (i, section) in sections.iter().enumerate() {
        let entry = by_label.entry(section.label).or_default();
        entry.0 += weight[i];
        entry.1 += 1;
    }
    let chorus = by_label
        .iter()
        .filter(|(_, (lines, parts))| *parts > 1 && *lines >= 2)
        .max_by_key(|(label, (lines, _))| (*lines, std::cmp::Reverse(**label)))
        .map(|(label, _)| *label);
    if let Some(chorus) = chorus {
        for section in &mut sections {
            section.likely_chorus = section.label == chorus;
        }
    }
    merge_neighbours(sections)
}

/// A part heard to begin a few seconds off from where a run of returning
/// lines starts begins there: the sound's boundary is only as fine as a
/// second and blurs around the change, the first sung line does not.
fn align_to_words(mut sections: Vec<Section>, lyrics: &[LyricLine]) -> Vec<Section> {
    const NEAR_S: f32 = 6.0;
    const LEAD_S: f32 = 0.5;
    const LEAST_PART_S: f32 = 4.0;
    let keys: Vec<Option<String>> = lyrics.iter().map(|l| words_key(&l.text)).collect();
    let returns = |i: usize| {
        keys[i].as_ref().is_some_and(|key| {
            lyrics.iter().zip(&keys).any(|(other, other_key)| {
                other_key.as_ref() == Some(key)
                    && (other.at_s - lyrics[i].at_s).abs() >= WORDS_RETURN_S
            })
        })
    };
    // Where runs of returning lines start.
    let starts: Vec<f32> = (0..lyrics.len())
        .filter(|&i| returns(i) && (i == 0 || !returns(i - 1)))
        .map(|i| lyrics[i].at_s - LEAD_S)
        .collect();
    for i in 1..sections.len() {
        let boundary = sections[i].start_s;
        let near = starts
            .iter()
            .copied()
            .filter(|at| (at - boundary).abs() <= NEAR_S)
            .min_by(|a, b| (a - boundary).abs().total_cmp(&(b - boundary).abs()));
        if let Some(at) = near
            && at - sections[i - 1].start_s >= LEAST_PART_S
            && sections[i].end_s - at >= LEAST_PART_S
        {
            sections[i - 1].end_s = at;
            sections[i].start_s = at;
        }
    }
    sections
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
    fn a_long_part_is_split_where_it_changes_most() {
        let mut curve = vec![0.0f32; 100];
        curve[30] = 0.3;
        curve[70] = 0.05;
        let mut starts = vec![0];
        split_long(&mut starts, 100, &curve);
        // 0..100 splits at 30; 30..100 has nothing real left inside.
        assert_eq!(starts, vec![0, 30]);
        let mut short = vec![0];
        split_long(&mut short, 40, &curve);
        assert_eq!(short, vec![0]);
    }

    #[test]
    fn the_words_say_which_parts_are_the_same_and_which_is_the_chorus() {
        let section = |start_s, end_s, label, likely_chorus| Section {
            start_s,
            end_s,
            label,
            likely_chorus,
            loudness_db: 0.0,
            brightness: 1.0,
            change: 0.5,
        };
        // The sound heard four different parts and took the loud one for
        // the chorus; the words say B and D are the same, and the chorus.
        let heard = vec![
            section(0.0, 30.0, 'A', false),
            section(30.0, 60.0, 'B', false),
            section(60.0, 90.0, 'C', true),
            section(90.0, 120.0, 'D', false),
            section(120.0, 150.0, 'C', true),
        ];
        let line = |at_s: f32, text: &str| LyricLine {
            at_s,
            text: text.into(),
            section: None,
            in_chorus: false,
            lands_on: None,
        };
        let lyrics = vec![
            line(5.0, "第一段主歌的词"),
            line(35.0, "如果你看得见"),
            line(40.0, "就当我是灯塔"),
            line(65.0, "中间一段不一样"),
            line(95.0, "如果你看得见"),
            line(100.0, "就当我是灯塔"),
            line(125.0, "中间一段也不一样"),
        ];
        let parts = by_words(heard.clone(), &lyrics);
        // The chorus parts begin where their words do.
        assert_eq!(parts[1].start_s, 34.5);
        assert_eq!(parts[0].end_s, 34.5);
        assert_eq!(parts[3].start_s, 94.5);
        let labels: String = parts.iter().map(|s| s.label).collect();
        assert_eq!(labels, "ABCBC");
        let chorus: Vec<bool> = parts.iter().map(|s| s.likely_chorus).collect();
        assert_eq!(chorus, vec![false, true, false, true, false]);
        // No line comes back: the sound's reading stands.
        assert_eq!(by_words(heard.clone(), &lyrics[..1]).len(), 5);
        assert!(by_words(heard, &lyrics[..1])[2].likely_chorus);
    }

    #[test]
    fn a_song_that_never_changes_is_one_section() {
        let per_second = spectrum::per_second(&spectrum::analyze(&audio(chord(&VERSE, 30.0, 0.3))));
        let sections = sections(&per_second);
        assert_eq!(sections.len(), 1);
        assert_eq!(repetition(&sections), 0.0);
    }
}
