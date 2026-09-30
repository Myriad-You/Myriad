//! How someone types, as a profile, and how far a few lines are from it.
//!
//! Anyone who knows a person can tell, a few lines in, when it is not them
//! typing: the particles, the punctuation, how long the lines run, what
//! they laugh with. The profile is the character pairs and triples someone
//! uses and the marks of their lines (how they end, laughing, emoji, the
//! particles), from their own messages. How far a few new lines are from it
//! is weighed against how far the person's own lines usually stray: some
//! people type one way always, some are all over the place. Short lines say
//! little about who typed them, so too little is no answer, not a guess.

use std::collections::HashMap;

/// Lines of someone's own before their way of typing is known.
pub const KNOWN_AFTER: usize = 100;
/// Characters of new lines before they can be told apart.
pub const TELLS_AFTER: usize = 30;
/// How many draws from someone's own lines show how far they usually stray.
const DRAWS: usize = 40;
/// Grams kept in a profile.
const GRAMS_KEPT: usize = 400;

const PARTICLES: [char; 12] = [
    '吧', '呢', '啊', '呀', '嘛', '哦', '噢', '嗯', '啦', '哈', '捏', '惹',
];

/// The marks of a line, as rates over lines.
const MARKS: usize = 10;

fn marks(line: &str) -> [f64; MARKS] {
    let line = line.trim();
    let last = line.chars().last();
    let ends = |set: &[char]| last.is_some_and(|last| set.contains(&last));
    let chars = line.chars().count().max(1) as f64;
    let emoji = line.chars().filter(|ch| (*ch as u32) >= 0x1F000).count();
    let latin = line.chars().filter(char::is_ascii_alphabetic).count() as f64;
    let particles = line.chars().filter(|ch| PARTICLES.contains(ch)).count() as f64;
    let laughs = ["哈哈", "233", "草", "www", "lol", "xswl", "笑死"]
        .iter()
        .any(|laugh| line.to_lowercase().contains(laugh));
    [
        f64::from(u8::from(ends(&['。', '.']))),
        f64::from(u8::from(ends(&['！', '!']))),
        f64::from(u8::from(ends(&['？', '?']))),
        f64::from(u8::from(ends(&['～', '~']))),
        f64::from(u8::from(ends(&['…']) || line.ends_with("..."))),
        f64::from(u8::from(laughs)),
        f64::from(u8::from(emoji > 0)),
        particles / chars,
        latin / chars,
        (chars.ln() / 5.0).min(1.5),
    ]
}

/// Someone's way of typing, from their lines.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Profile {
    grams: HashMap<String, f64>,
    marks: [f64; MARKS],
    lines: usize,
    chars: usize,
}

impl Profile {
    pub fn lines(&self) -> usize {
        self.lines
    }

    pub fn chars(&self) -> usize {
        self.chars
    }
}

pub fn profile<S: AsRef<str>>(lines: &[S]) -> Profile {
    let mut counts: HashMap<String, f64> = HashMap::new();
    let mut marked = [0.0; MARKS];
    let mut chars = 0;
    let mut kept = 0;
    for line in lines {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        kept += 1;
        let text: Vec<char> = line
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .collect();
        chars += text.len();
        for width in [2, 3] {
            for gram in text.windows(width) {
                *counts.entry(gram.iter().collect()).or_default() += 1.0;
            }
        }
        for (sum, mark) in marked.iter_mut().zip(marks(line)) {
            *sum += mark;
        }
    }
    let mut grams: Vec<(String, f64)> = counts.into_iter().collect();
    grams.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));
    grams.truncate(GRAMS_KEPT);
    let norm = grams
        .iter()
        .map(|(_, count)| count * count)
        .sum::<f64>()
        .sqrt()
        .max(f64::EPSILON);
    let lines = kept.max(1) as f64;
    Profile {
        grams: grams
            .into_iter()
            .map(|(gram, count)| (gram, count / norm))
            .collect(),
        marks: marked.map(|sum| sum / lines),
        lines: kept,
        chars,
    }
}

/// How far two ways of typing are apart: the grams they do not share, and
/// how differently their lines are marked.
pub fn distance(left: &Profile, right: &Profile) -> f64 {
    let shared: f64 = left
        .grams
        .iter()
        .filter_map(|(gram, weight)| right.grams.get(gram).map(|other| weight * other))
        .sum();
    let marks: f64 = left
        .marks
        .iter()
        .zip(&right.marks)
        .map(|(a, b)| (a - b).abs())
        .sum::<f64>()
        / MARKS as f64;
    (1.0 - shared) + marks
}

/// A small, fixed stream of draws, so the same lines weigh the same.
struct Draws(u64);

impl Draws {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % below.max(1) as u64) as usize
    }
}

/// How unlike someone's own lines (`history`) the new ones (`recent`) are,
/// in their own deviations: how far `recent` is from their profile, against
/// how far as many of their own lines drawn at random usually are. None
/// when their way of typing is not known yet, or the new lines are too
/// short to tell.
pub fn unlike<S: AsRef<str>>(history: &[S], recent: &[S]) -> Option<f64> {
    let history: Vec<&str> = history
        .iter()
        .map(AsRef::as_ref)
        .filter(|line| !line.trim().is_empty())
        .collect();
    let recent: Vec<&str> = recent
        .iter()
        .map(AsRef::as_ref)
        .filter(|line| !line.trim().is_empty())
        .collect();
    let chars: usize = recent.iter().map(|line| line.chars().count()).sum();
    if history.len() < KNOWN_AFTER || chars < TELLS_AFTER || recent.is_empty() {
        return None;
    }
    // Every fifth line of theirs is kept out of the profile, to draw from:
    // new lines are not in the profile either, so the usual straying is
    // measured the same way, on lines it has not seen.
    let aside: Vec<&str> = history.iter().copied().step_by(5).collect();
    let kept: Vec<&str> = history
        .iter()
        .copied()
        .enumerate()
        .filter(|(index, _)| index % 5 != 0)
        .map(|(_, line)| line)
        .collect();
    let theirs = profile(&kept);
    let mut draws = Draws(history.len() as u64 ^ (chars as u64) << 20);
    let usual: Vec<f64> = (0..DRAWS)
        .map(|_| {
            let sample: Vec<&str> = (0..recent.len())
                .map(|_| aside[draws.next(aside.len())])
                .collect();
            distance(&profile(&sample), &theirs)
        })
        .collect();
    let mean = usual.iter().sum::<f64>() / DRAWS as f64;
    let deviation = (usual.iter().map(|d| (d - mean).powi(2)).sum::<f64>() / DRAWS as f64).sqrt();
    if deviation <= f64::EPSILON {
        return None;
    }
    Some((distance(&profile(&recent), &theirs) - mean) / deviation)
}

/// How far past someone's usual straying lines must be before she would
/// notice it is not like them: well past, since a bad day changes how
/// anyone types, and taking a friend for someone else hurts more than
/// missing it.
pub const NOTICED_AT: f64 = 3.0;

/// What she reads when someone's last lines do not read like them: facts,
/// and that what she knows of them in private is theirs until she knows it
/// is them.
pub const NOTICED: &str = "## Not quite like them\nTheir last few lines do not read like the way they usually type: the words, the marks, how the lines run. Someone else may be at their phone, or they are not themselves today. What you make of it, and whether to say anything, is yours; until you know it is them, what you know of them in private is theirs.";

#[cfg(test)]
mod tests {
    use super::*;

    fn aming(index: usize) -> String {
        let lines = [
            "哈哈哈哈好家伙",
            "我今天又加班了啊",
            "年糕又把纸巾拽了一地哈哈",
            "晚上吃啥呢",
            "啊这",
            "明天再说吧",
            "笑死我了哈哈哈",
            "你们在玩啥呀",
        ];
        format!("{}{}", lines[index % lines.len()], "啊".repeat(index % 3))
    }

    #[test]
    fn a_few_lines_unlike_theirs_stand_out_and_their_own_do_not() {
        let history: Vec<String> = (0..200).map(aming).collect();
        let theirs: Vec<String> = (200..206).map(aming).collect();
        let someone_else = vec![
            "Could you please send me the report by Friday.".to_string(),
            "Thanks. I will review it tomorrow morning.".to_string(),
            "Regards, and see you at the meeting.".to_string(),
        ];
        let own = unlike(&history, &theirs).unwrap();
        let other = unlike(&history, &someone_else).unwrap();
        assert!(own < NOTICED_AT, "their own lines: {own}");
        assert!(other > NOTICED_AT, "someone else's: {other}");
        // Too little to go on is no answer.
        assert_eq!(unlike(&history, &["嗯".to_string()]), None);
        assert_eq!(unlike(&history[..20], &someone_else), None);
        // A profile is closest to itself.
        let profile = profile(&history);
        assert!(distance(&profile, &profile) < 1e-9);
    }
}
