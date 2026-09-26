//! The sheet as text a listener reads: the song from start to end, with
//! its lyrics and moments in place, then the readings with their sources.

use std::fmt::Write;

use crate::reading::clock;
use crate::{ListeningSheet, Moment, MomentKind};

fn moment_text(moment: &Moment) -> Option<String> {
    Some(match moment.kind {
        MomentKind::Surge => format!("swells {:+.0} dB", moment.amount),
        MomentKind::Drop => format!("falls away {:.0} dB", moment.amount),
        MomentKind::OpensUp => format!("the sound opens up ({:.1}x brighter)", moment.amount),
        MomentKind::Build => format!(
            "a build that began at {} peaks here ({:.0} s)",
            clock(moment.at_s - moment.amount),
            moment.amount
        ),
        MomentKind::NewSection => return None,
    })
}

impl ListeningSheet {
    /// The whole sheet as text.
    pub fn describe(&self) -> String {
        let mut out = self.overall();
        out.push_str("\n\n");
        self.timeline(&mut out);
        out
    }

    /// In brief, to keep: the song overall and the moments that stand out.
    pub fn gist(&self) -> String {
        let mut out = self.overall();
        for reading in self.readings.iter().filter(|r| r.at_s.is_some()) {
            let _ = write!(out, " {}.", reading.heard);
        }
        out
    }

    fn overall(&self) -> String {
        let mut out = String::new();
        let _ = write!(out, "Length {}.", clock(self.duration_s));
        match self.tempo_bpm {
            Some(tempo) => {
                let about = if self.pulse_clarity >= crate::reading::STEADY {
                    "About"
                } else {
                    "A faint pulse, perhaps about"
                };
                let _ = write!(
                    out,
                    " {about} {tempo:.0} BPM, pulse clarity {:.2}, syncopation {:.2}.",
                    self.pulse_clarity, self.syncopation
                );
            }
            None => {
                let _ = write!(
                    out,
                    " No steady beat (pulse clarity {:.2}).",
                    self.pulse_clarity
                );
            }
        }
        match &self.key {
            Some(key) => {
                let _ = write!(out, " Key {key} (clarity {:.2}).", self.key_clarity);
            }
            None => out.push_str(" No clear key."),
        }
        let _ = write!(
            out,
            " Loudness range {:.0} dB; spectral centroid {:.0} Hz. {:.0}% of it is parts that come back.",
            self.dynamic_range_db,
            self.brightness_hz,
            self.repetition * 100.0
        );
        out
    }

    fn timeline(&self, out: &mut String) {
        out.push_str("How it goes:\n");
        for section in &self.sections {
            let chorus = if section.likely_chorus {
                ", likely the chorus"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "{}–{} part {}{chorus} ({:+.0} dB against the song, brightness {:.2}x)",
                clock(section.start_s),
                clock(section.end_s),
                section.label,
                section.loudness_db,
                section.brightness
            );
            let inside = |at: f32| {
                at >= section.start_s && at < section.end_s
                    || (at >= section.end_s && section.end_s >= self.duration_s - 0.5)
            };
            let mut events: Vec<(f32, String)> = self
                .moments
                .iter()
                .filter(|moment| inside(moment.at_s))
                .filter_map(|moment| moment_text(moment).map(|text| (moment.at_s, text)))
                .collect();
            events.extend(
                self.lyrics
                    .iter()
                    .filter(|line| inside(line.at_s))
                    .map(|line| (line.at_s, format!("「{}」", line.text))),
            );
            events.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (at, text) in events {
                let _ = writeln!(out, "  {} {text}", clock(at));
            }
        }

        if !self.readings.is_empty() {
            out.push_str("\nWhat listening research says about things like these:\n");
            for reading in &self.readings {
                let _ = writeln!(
                    out,
                    "- {}. {} ({})",
                    reading.heard, reading.tends_to, reading.source
                );
            }
        }
    }
}

impl ListeningSheet {
    /// Where a listener is at this point of the song: the part it is in,
    /// and the last few things just heard. Nothing after this point.
    pub fn so_far(&self, at_s: f32) -> String {
        if at_s >= self.duration_s {
            return format!("It has ended ({}).", clock(self.duration_s));
        }
        let mut out = format!("At {}", clock(at_s));
        if let Some(section) = self
            .sections
            .iter()
            .find(|s| s.start_s <= at_s && at_s < s.end_s)
        {
            // Whether a part is the chorus is only known once it came back,
            // so say so only then.
            let heard_before = self
                .sections
                .iter()
                .any(|s| s.label == section.label && s.end_s <= at_s);
            let chorus = if section.likely_chorus && heard_before {
                ", the chorus again"
            } else {
                ""
            };
            let _ = write!(
                out,
                ", in part {}{chorus}, which began at {}",
                section.label,
                clock(section.start_s)
            );
        }
        let mut events: Vec<(f32, String)> = self
            .moments
            .iter()
            .filter(|m| m.at_s <= at_s && at_s - m.at_s <= JUST_HEARD_S)
            .filter_map(|m| moment_text(m).map(|text| (m.at_s, text)))
            .collect();
        events.extend(
            self.lyrics
                .iter()
                .filter(|l| l.at_s <= at_s && at_s - l.at_s <= JUST_HEARD_S)
                .map(|l| (l.at_s, format!("「{}」", l.text))),
        );
        events.sort_by(|a, b| a.0.total_cmp(&b.0));
        let recent: Vec<String> = events
            .iter()
            .rev()
            .take(4)
            .rev()
            .map(|(at, text)| format!("{} {text}", clock(*at)))
            .collect();
        out.push('.');
        if !recent.is_empty() {
            let _ = write!(out, " Just heard: {}.", recent.join("; "));
        }
        out
    }
}

/// How far back "just heard" reaches.
const JUST_HEARD_S: f32 = 40.0;

#[cfg(test)]
mod tests {
    use crate::reading;

    #[test]
    fn the_song_reads_from_start_to_end() {
        let mut sheet = reading::tests::sheet();
        sheet.readings = reading::read(&sheet, false);
        let text = sheet.describe();
        assert!(text.starts_with("Length 2:00. About 124 BPM"));
        assert!(text.contains("Key E major"));
        assert!(text.contains("0:30–1:00 part B, likely the chorus (+4 dB"));
        let build = text.find("a build that began at 0:18").unwrap();
        let surge = text.find("0:31 swells +9 dB").unwrap();
        let line = text.find("0:30 「就是现在」").unwrap();
        assert!(build < line && line < surge, "{text}");
        assert!(text.contains("(Huron 2006"));
        assert!(text.contains("come back.\n\nHow it goes:\n"), "{text}");
        let gist = sheet.gist();
        assert!(
            gist.starts_with("Length 2:00.") && gist.contains("At 0:30 a section breaks in"),
            "{gist}"
        );
        assert!(!gist.contains("How it goes") && !gist.contains("Huron"));
    }

    #[test]
    fn so_far_holds_nothing_after_that_point() {
        let sheet = reading::tests::sheet();
        let first = sheet.so_far(31.0);
        assert!(
            first.starts_with("At 0:31, in part B, which began at 0:30."),
            "{first}"
        );
        assert!(
            first.contains("0:30 「就是现在」; 0:31 swells +9 dB"),
            "{first}"
        );
        let before = sheet.so_far(29.0);
        assert!(
            !before.contains("就是现在") && !before.contains("swells"),
            "{before}"
        );
        assert!(sheet.so_far(95.0).contains("the chorus again"));
        assert_eq!(sheet.so_far(200.0), "It has ended (2:00).");
    }
}
