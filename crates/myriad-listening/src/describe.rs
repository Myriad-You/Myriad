//! The sheet as text a listener reads: the song from start to end, with
//! its lyrics and moments in place, then the readings with their sources.

use std::fmt::Write;

use crate::reading::clock;
use crate::{ListeningSheet, Moment, MomentKind};

fn moment_text(moment: &Moment) -> Option<String> {
    Some(match moment.kind {
        MomentKind::Surge => format!("swells {:+.0} dB", moment.amount),
        MomentKind::Drop => format!("falls away {:.0} dB", -moment.amount),
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
        let mut out = String::new();
        let _ = write!(out, "Length {}.", clock(self.duration_s));
        match self.tempo_bpm {
            Some(tempo) => {
                let _ = write!(
                    out,
                    " About {tempo:.0} BPM, pulse clarity {:.2}, syncopation {:.2}.",
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
        let _ = writeln!(
            out,
            " Loudness range {:.0} dB; spectral centroid {:.0} Hz. {:.0}% of it is parts that come back.",
            self.dynamic_range_db,
            self.brightness_hz,
            self.repetition * 100.0
        );

        out.push_str("\nHow it goes:\n");
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
        out
    }
}

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
    }
}
