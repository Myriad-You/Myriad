//! What the facts of a song tend to do to a listener, per music psychology.
//!
//! Each reading pairs something that happens in this song with a finding
//! about how listeners respond to that kind of thing, and names where the
//! finding comes from. The findings are tendencies across listeners, not
//! what any one listener must feel.

use serde::Serialize;

use crate::{ListeningSheet, MomentKind};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Reading {
    /// Where in the song, when it is about one place.
    pub at_s: Option<f32>,
    /// What happens in this song.
    pub heard: String,
    /// What that tends to do to listeners.
    pub tends_to: &'static str,
    pub source: &'static str,
}

const CHILLS: &str = "Moments like this (a sudden swell, the sound widening, a new section breaking in after a quieter one) are where listeners most often report chills, shivers or a lump in the throat.";
const CHILLS_SOURCE: &str = "Sloboda 1991, Psychology of Music; Grewe et al. 2007, Music Perception; Guhn, Hamm & Zentner 2007, Music Perception";

const ARRIVAL: &str = "A long build makes the arrival foreseeable; when it comes, the fulfilled expectation is felt as release and pleasure, and a longer wait tends to make the payoff stronger.";
const WITHHELD: &str = "When a build is cut off instead of arriving, the broken expectation lands as surprise, and the arrival that does come later tends to feel stronger by contrast.";
const EXPECTATION_SOURCE: &str = "Huron 2006, Sweet Anticipation (ITPRA theory of expectation)";

const GROOVY: &str = "A clear beat with a moderate amount happening between the beats is what listeners rate most groovy, with the strongest urge to move; too little syncopation feels square, too much loses the beat.";
const SQUARE: &str = "A clear beat with little happening between the beats is easy to follow and move to, though listeners rate it less groovy than moderately syncopated rhythm.";
const LOST: &str = "With this much falling between the beats, listeners tend to lose the pulse; groove ratings fall at high syncopation.";
const FAINT: &str = "The beat is faint: listeners hear music like this as flowing or floating more than as something to move to.";
const GROOVE_SOURCE: &str = "Witek et al. 2014, PLoS ONE; Janata, Tomic & Haberman 2012, J. Exp. Psychology: General; Lartillot et al. 2008 (pulse clarity)";

const PREFERRED_TEMPO: &str =
    "This is close to the tempo people spontaneously tap and walk to, around 120 BPM.";
const PREFERRED_TEMPO_SOURCE: &str = "Moelants 2002, ICMPC 7 (preferred tempo)";

const HAPPY_CUES: &str = "Fast tempo, bright sound and major mode are the cues listeners most consistently hear as happiness or excitement, the same cues a cheerful voice carries.";
const SAD_CUES: &str = "Slow tempo, soft dark sound and minor mode are the cues listeners most consistently hear as sadness or tenderness; music heard as sad is still often enjoyed, and felt as moving more than as sad.";
const MIXED_CUES: &str = "The cues pull different ways; listeners tend to hear mixed or bittersweet feeling in such music, with tempo and mode weighing most.";
const CUES_SOURCE: &str = "Juslin & Laukka 2003, Psychological Bulletin; Gabrielsson & Lindström 2010, Handbook of Music and Emotion; Vuoskoski et al. 2012, Music Perception";

const RETURNS: &str = "Repetition lets listeners hear ahead: by its return the chorus is anticipated and joined in, often inwardly, as if singing along; much of what makes music feel like music is that it comes back.";
const RETURNS_SOURCE: &str = "Margulis 2014, On Repeat: How Music Plays the Mind";

const WORDS: &str = "Lyrics tend to intensify what sad or angry music carries, and to slightly dampen happy or calm music.";
const WORDS_SOURCE: &str = "Ali & Peynircioğlu 2006, Psychology of Music";

/// "1:05"
pub fn clock(seconds: f32) -> String {
    let whole = seconds.max(0.0).round() as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// The line sung right there, if any.
fn line_at(sheet: &ListeningSheet, at_s: f32) -> Option<&str> {
    sheet
        .lyrics
        .iter()
        .find(|line| (line.at_s - at_s).abs() <= 1.5)
        .map(|line| line.text.as_str())
}

fn chorus_starts_at(sheet: &ListeningSheet, at_s: f32) -> bool {
    sheet
        .sections
        .iter()
        .any(|section| section.likely_chorus && (section.start_s - at_s).abs() <= 2.0)
}

fn chills(sheet: &ListeningSheet) -> Vec<Reading> {
    let mut found: Vec<(f32, f32, String)> = sheet
        .moments
        .iter()
        .filter_map(|moment| {
            let (score, what) = match moment.kind {
                MomentKind::Surge => (
                    moment.amount,
                    format!("it swells {:.0} dB within two seconds", moment.amount),
                ),
                MomentKind::OpensUp => (
                    (moment.amount - 1.0) * 10.0,
                    format!("the sound opens up, {:.1}x brighter at once", moment.amount),
                ),
                MomentKind::NewSection if moment.amount >= 6.0 => (
                    moment.amount,
                    format!(
                        "a section breaks in {:.0} dB louder than the one before",
                        moment.amount
                    ),
                ),
                _ => return None,
            };
            let chorus = chorus_starts_at(sheet, moment.at_s);
            let mut heard = format!("At {} {what}", clock(moment.at_s));
            if chorus {
                heard.push_str(", into the chorus");
            }
            if let Some(line) = line_at(sheet, moment.at_s) {
                heard.push_str(&format!("; the line 「{line}」 is sung right there"));
            }
            let score = score + if chorus { 5.0 } else { 0.0 };
            Some((moment.at_s, score, heard))
        })
        .collect();
    // The same place heard twice (a surge that is also a new section) is
    // one moment.
    found.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut kept: Vec<(f32, f32, String)> = Vec::new();
    for candidate in found {
        if kept.len() < 3 && kept.iter().all(|other| (other.0 - candidate.0).abs() > 3.0) {
            kept.push(candidate);
        }
    }
    kept.sort_by(|a, b| a.0.total_cmp(&b.0));
    kept.into_iter()
        .map(|(at_s, _, heard)| Reading {
            at_s: Some(at_s),
            heard,
            tends_to: CHILLS,
            source: CHILLS_SOURCE,
        })
        .collect()
}

fn expectation(sheet: &ListeningSheet) -> Vec<Reading> {
    sheet
        .moments
        .iter()
        .filter(|moment| moment.kind == MomentKind::Build)
        .filter_map(|build| {
            let after = sheet.moments.iter().find(|next| {
                next.at_s > build.at_s - 1.0
                    && next.at_s <= build.at_s + 3.0
                    && matches!(next.kind, MomentKind::Surge | MomentKind::Drop)
            });
            let start = clock(build.at_s - build.amount);
            let (then, tends_to) = match after.map(|next| next.kind) {
                Some(MomentKind::Drop) => (
                    format!("then falls away at {}", clock(after?.at_s)),
                    WITHHELD,
                ),
                Some(_) => (format!("then arrives at {}", clock(after?.at_s)), ARRIVAL),
                None if chorus_starts_at(sheet, build.at_s) => {
                    (format!("into the chorus at {}", clock(build.at_s)), ARRIVAL)
                }
                None => return None,
            };
            Some(Reading {
                at_s: Some(build.at_s),
                heard: format!("From {start} it builds for {:.0} s, {then}", build.amount),
                tends_to,
                source: EXPECTATION_SOURCE,
            })
        })
        .take(2)
        .collect()
}

fn groove(sheet: &ListeningSheet) -> Vec<Reading> {
    let mut readings = Vec::new();
    let Some(tempo) = sheet.tempo_bpm else {
        readings.push(Reading {
            at_s: None,
            heard: format!(
                "No steady beat can be made out (pulse clarity {:.2})",
                sheet.pulse_clarity
            ),
            tends_to: FAINT,
            source: GROOVE_SOURCE,
        });
        return readings;
    };
    let heard = format!(
        "About {tempo:.0} BPM; pulse clarity {:.2}, syncopation {:.2}",
        sheet.pulse_clarity, sheet.syncopation
    );
    let tends_to = if sheet.pulse_clarity < 0.3 {
        FAINT
    } else if sheet.syncopation > 0.5 {
        LOST
    } else if sheet.syncopation >= 0.15 {
        GROOVY
    } else {
        SQUARE
    };
    readings.push(Reading {
        at_s: None,
        heard,
        tends_to,
        source: GROOVE_SOURCE,
    });
    if (110.0..=135.0).contains(&tempo) && sheet.pulse_clarity >= 0.3 {
        readings.push(Reading {
            at_s: None,
            heard: format!("The tempo is about {tempo:.0} BPM"),
            tends_to: PREFERRED_TEMPO,
            source: PREFERRED_TEMPO_SOURCE,
        });
    }
    readings
}

fn cues(sheet: &ListeningSheet, minor: bool) -> Option<Reading> {
    let tempo = sheet.tempo_bpm?;
    let fast = tempo >= 120.0;
    let slow = tempo < 90.0;
    let bright = sheet.brightness_hz >= 2_500.0;
    let dark = sheet.brightness_hz < 1_500.0;
    let mode = sheet.key.as_ref().map(|_| minor);
    let tends_to = match mode {
        Some(false) if fast && !dark => HAPPY_CUES,
        Some(true) if slow && !bright => SAD_CUES,
        None if fast && bright => HAPPY_CUES,
        None if slow && dark => SAD_CUES,
        _ => MIXED_CUES,
    };
    let pace = if fast {
        "fast"
    } else if slow {
        "slow"
    } else {
        "moderate"
    };
    let colour = if bright {
        "bright"
    } else if dark {
        "dark"
    } else {
        "neither bright nor dark"
    };
    let key = match &sheet.key {
        Some(key) => format!("in {key}"),
        None => "with no clear key".to_string(),
    };
    let contrast = if sheet.dynamic_range_db >= 15.0 {
        format!(
            ", with wide contrast between quiet and loud ({:.0} dB)",
            sheet.dynamic_range_db
        )
    } else if sheet.dynamic_range_db < 6.0 {
        format!(
            ", at nearly the same loudness throughout ({:.0} dB range)",
            sheet.dynamic_range_db
        )
    } else {
        String::new()
    };
    Some(Reading {
        at_s: None,
        heard: format!(
            "A {pace} tempo ({tempo:.0} BPM), a {colour} sound ({:.0} Hz spectral centroid), {key}{contrast}",
            sheet.brightness_hz
        ),
        tends_to,
        source: CUES_SOURCE,
    })
}

fn repetition(sheet: &ListeningSheet) -> Option<Reading> {
    let chorus = sheet.sections.iter().find(|s| s.likely_chorus)?;
    let returns = sheet
        .sections
        .iter()
        .filter(|s| s.label == chorus.label)
        .count();
    (sheet.repetition >= 0.4).then(|| Reading {
        at_s: None,
        heard: format!(
            "About {:.0}% of the song is parts that come back; the chorus comes {returns} times",
            sheet.repetition * 100.0
        ),
        tends_to: RETURNS,
        source: RETURNS_SOURCE,
    })
}

fn words(sheet: &ListeningSheet) -> Option<Reading> {
    if sheet.lyrics.is_empty() {
        return None;
    }
    let mut distinct: Vec<&str> = sheet.lyrics.iter().map(|line| line.text.as_str()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    Some(Reading {
        at_s: None,
        heard: format!(
            "It is sung: {} lines, {} of them different",
            sheet.lyrics.len(),
            distinct.len()
        ),
        tends_to: WORDS,
        source: WORDS_SOURCE,
    })
}

pub fn read(sheet: &ListeningSheet, minor: bool) -> Vec<Reading> {
    let mut readings = chills(sheet);
    readings.extend(expectation(sheet));
    readings.extend(groove(sheet));
    readings.extend(cues(sheet, minor));
    readings.extend(repetition(sheet));
    readings.extend(words(sheet));
    readings
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{LyricLine, Moment, Section};

    pub(crate) fn sheet() -> ListeningSheet {
        let section = |start_s, end_s, label, likely_chorus, loudness_db| Section {
            start_s,
            end_s,
            label,
            likely_chorus,
            loudness_db,
            brightness: 1.0,
        };
        let moment = |at_s, kind, amount| Moment { at_s, kind, amount };
        ListeningSheet {
            duration_s: 120.0,
            tempo_bpm: Some(124.0),
            pulse_clarity: 0.7,
            syncopation: 0.3,
            key: Some("E major".into()),
            key_clarity: 0.8,
            dynamic_range_db: 18.0,
            brightness_hz: 2_800.0,
            repetition: 0.7,
            sections: vec![
                section(0.0, 30.0, 'A', false, -6.0),
                section(30.0, 60.0, 'B', true, 4.0),
                section(60.0, 90.0, 'A', false, -6.0),
                section(90.0, 120.0, 'B', true, 4.0),
            ],
            moments: vec![
                moment(30.0, MomentKind::Build, 12.0),
                moment(30.0, MomentKind::NewSection, 10.0),
                moment(30.5, MomentKind::Surge, 9.0),
                moment(90.0, MomentKind::NewSection, 10.0),
            ],
            lyrics: vec![LyricLine {
                at_s: 30.2,
                text: "就是现在".into(),
                section: Some('B'),
                in_chorus: true,
                lands_on: Some(MomentKind::Surge),
            }],
            readings: Vec::new(),
        }
    }

    #[test]
    fn each_fact_is_read_with_its_source() {
        let readings = read(&sheet(), false);
        let chills: Vec<&Reading> = readings.iter().filter(|r| r.tends_to == CHILLS).collect();
        // The surge and the new section at 0:30 are one moment; 1:30 is another.
        assert_eq!(chills.len(), 2, "{readings:#?}");
        assert!(chills[0].heard.contains("0:30"));
        assert!(chills[0].heard.contains("into the chorus"));
        assert!(chills[0].heard.contains("「就是现在」"));
        let arrival = readings.iter().find(|r| r.tends_to == ARRIVAL).unwrap();
        assert!(arrival.heard.contains("From 0:18 it builds for 12 s"));
        assert!(readings.iter().any(|r| r.tends_to == GROOVY));
        assert!(readings.iter().any(|r| r.tends_to == PREFERRED_TEMPO));
        assert!(readings.iter().any(|r| r.tends_to == HAPPY_CUES));
        assert!(
            readings
                .iter()
                .any(|r| r.tends_to == RETURNS && r.heard.contains("2 times"))
        );
        assert!(readings.iter().any(|r| r.tends_to == WORDS));
        assert!(readings.iter().all(|r| !r.source.is_empty()));
    }

    #[test]
    fn a_slow_dark_minor_song_is_read_by_its_cues() {
        let mut slow = sheet();
        slow.tempo_bpm = Some(72.0);
        slow.brightness_hz = 1_200.0;
        slow.key = Some("D minor".into());
        let readings = read(&slow, true);
        assert!(readings.iter().any(|r| r.tends_to == SAD_CUES));
        assert!(!readings.iter().any(|r| r.tends_to == PREFERRED_TEMPO));
        slow.tempo_bpm = None;
        assert!(read(&slow, true).iter().any(|r| r.tends_to == FAINT));
    }

    #[test]
    fn times_read_as_a_clock() {
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(0.0), "0:00");
    }
}
