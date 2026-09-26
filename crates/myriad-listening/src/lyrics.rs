//! Lyrics on the song's timeline.
//!
//! Timed lyrics (LRC) put each line at a moment of the recording, so a line
//! is known to fall in the chorus, or to land right where the music swells.

use serde::Serialize;

use crate::{Moment, MomentKind, Section};

/// A line lands on a moment this close to it.
const LANDS_WITHIN_S: f32 = 1.5;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LyricLine {
    pub at_s: f32,
    pub text: String,
    /// The section it is sung in.
    pub section: Option<char>,
    pub in_chorus: bool,
    /// Something that happens in the music right as it is sung.
    pub lands_on: Option<MomentKind>,
}

fn timestamp(tag: &str) -> Option<f32> {
    let (minutes, rest) = tag.split_once(':')?;
    let minutes: f32 = minutes.trim().parse().ok()?;
    // Fractions come as "12.34" or "12:34".
    let seconds: f32 = match rest.split_once(':') {
        Some((whole, fraction)) => format!("{whole}.{fraction}").parse().ok()?,
        None => rest.trim().parse().ok()?,
    };
    Some(minutes * 60.0 + seconds)
}

/// Credits ("作词 : …") and markers that are not sung.
fn is_sung(text: &str) -> bool {
    let credit = [" : ", "："].iter().any(|separator| {
        text.split_once(separator)
            .is_some_and(|(role, _)| role.trim().chars().count() <= 6)
    });
    !text.is_empty() && !credit && !text.contains("纯音乐")
}

/// Parse LRC: each line may carry several timestamps; metadata tags and
/// credits are left out.
pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(inner) = rest.strip_prefix('[') {
            let Some(end) = inner.find(']') else { break };
            if let Some(at) = timestamp(&inner[..end]) {
                times.push(at);
            }
            rest = inner[end + 1..].trim_start();
        }
        let text = rest.trim();
        if !is_sung(text) {
            continue;
        }
        lines.extend(times.into_iter().map(|at_s| LyricLine {
            at_s,
            text: text.to_string(),
            section: None,
            in_chorus: false,
            lands_on: None,
        }));
    }
    lines.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
    lines
}

/// Place each line in its section and on any moment it lands on.
pub fn place(lines: &[LyricLine], sections: &[Section], moments: &[Moment]) -> Vec<LyricLine> {
    let weight = |kind: MomentKind| match kind {
        MomentKind::Surge | MomentKind::OpensUp => 3,
        MomentKind::Drop | MomentKind::Build => 2,
        MomentKind::NewSection => 1,
    };
    lines
        .iter()
        .map(|line| {
            let section = sections
                .iter()
                .find(|s| s.start_s <= line.at_s && line.at_s < s.end_s)
                .or_else(|| sections.last().filter(|s| line.at_s >= s.end_s));
            let lands_on = moments
                .iter()
                .filter(|moment| (moment.at_s - line.at_s).abs() <= LANDS_WITHIN_S)
                .max_by_key(|moment| weight(moment.kind))
                .map(|moment| moment.kind);
            LyricLine {
                section: section.map(|s| s.label),
                in_chorus: section.is_some_and(|s| s.likely_chorus),
                lands_on,
                ..line.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_lines_are_read_and_credits_left_out() {
        let lrc = "[ti:某首歌]\n[00:00.00] 作词 : 某人\n[00:01.00] 作曲 : 某人\n[00:12.50]第一句\n[00:30.10][01:10.20]副歌那句\n[00:40:50]冒号分秒\n[00:50.00]\n{\"t\":0,\"c\":[]}";
        let lines = parse_lrc(lrc);
        let texts: Vec<(&str, f32)> = lines.iter().map(|l| (l.text.as_str(), l.at_s)).collect();
        assert_eq!(
            texts,
            vec![
                ("第一句", 12.5),
                ("副歌那句", 30.1),
                ("冒号分秒", 40.5),
                ("副歌那句", 70.2)
            ]
        );
        assert!(parse_lrc("[00:00.00]纯音乐，请欣赏").is_empty());
    }

    #[test]
    fn a_line_knows_its_section_and_what_it_lands_on() {
        let section = |start_s, end_s, label, likely_chorus| Section {
            start_s,
            end_s,
            label,
            likely_chorus,
            loudness_db: 0.0,
            brightness: 1.0,
            change: 0.5,
        };
        let sections = [
            section(0.0, 30.0, 'A', false),
            section(30.0, 60.0, 'B', true),
        ];
        let moments = [
            Moment {
                at_s: 30.0,
                kind: MomentKind::NewSection,
                amount: 8.0,
            },
            Moment {
                at_s: 30.5,
                kind: MomentKind::Surge,
                amount: 8.0,
            },
        ];
        let lines = place(
            &parse_lrc("[00:10.00]主歌\n[00:30.20]副歌"),
            &sections,
            &moments,
        );
        assert_eq!(lines[0].section, Some('A'));
        assert!(!lines[0].in_chorus && lines[0].lands_on.is_none());
        assert_eq!(lines[1].section, Some('B'));
        assert!(lines[1].in_chorus);
        assert_eq!(lines[1].lands_on, Some(MomentKind::Surge));
    }
}
