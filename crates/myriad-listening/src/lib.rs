//! Listening to a song from its sound.
//!
//! A song is heard in three layers, each with its grounds:
//!
//! 1. What happens in it, measured from the sound: loudness, brightness and
//!    onsets frame by frame from a short-time Fourier transform; tempo and
//!    how clear the pulse is; key and mode; the sections and which of them
//!    come back; the moments where the sound surges, drops, opens up or
//!    builds.
//! 2. The lyrics on the same timeline, so a line is known to land where the
//!    music lifts.
//! 3. Readings of those facts through what music psychology has found about
//!    how listeners are moved: chills, tension and release, groove, the cues
//!    listeners hear emotion in, the pull of repetition. Each reading says
//!    where it comes from.
//!
//! The result is a [`ListeningSheet`]: facts anyone could check against the
//! recording, and what they tend to do to a listener. Whoever listens (the
//! persona) feels from it; nothing in it is felt for her.
//!
//! Thresholds are first estimates, to be tuned against real songs.

mod decode;
mod describe;
mod lyrics;
mod moments;
mod reading;
mod rhythm;
mod spectrum;
mod structure;
mod tonal;

pub use decode::{Audio, DecodeError, decode_mono};
pub use lyrics::{LyricLine, parse_lrc};
pub use reading::Reading;

use serde::Serialize;

/// A song, heard: what happens in it and what that tends to do.
#[derive(Debug, Clone, Serialize)]
pub struct ListeningSheet {
    pub duration_s: f32,
    pub tempo_bpm: Option<f32>,
    /// How clearly a beat can be felt, 0 to 1.
    pub pulse_clarity: f32,
    /// Share of onset energy between the beats, 0 to 1.
    pub syncopation: f32,
    /// Such as "A minor"; none when the key is too unclear to name.
    pub key: Option<String>,
    /// How clearly the music sits in that key, 0 to 1.
    pub key_clarity: f32,
    /// Loudness spread between its quiet and loud parts, in dB.
    pub dynamic_range_db: f32,
    /// Mean spectral centroid: how bright it sounds, in Hz.
    pub brightness_hz: f32,
    /// Share of the song spent in sections that come back.
    pub repetition: f32,
    pub sections: Vec<Section>,
    pub moments: Vec<Moment>,
    pub lyrics: Vec<LyricLine>,
    pub readings: Vec<Reading>,
}

/// A stretch of the song, labelled so that sections that sound alike share a
/// letter.
#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub start_s: f32,
    pub end_s: f32,
    pub label: char,
    /// The loudest section that comes back: most likely the chorus.
    pub likely_chorus: bool,
    /// Loudness relative to the whole song, in dB.
    pub loudness_db: f32,
    /// Brightness relative to the whole song, as a ratio.
    pub brightness: f32,
    /// How different it sounds from what came before, 0 to 1 (the novelty
    /// at its start; 0 for the first).
    pub change: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MomentKind {
    /// It gets much louder within a couple of seconds.
    Surge,
    /// It falls away suddenly.
    Drop,
    /// The sound widens: much brighter all at once.
    OpensUp,
    /// It grows steadily louder for a while, ending here.
    Build,
    /// A new section begins.
    NewSection,
}

#[derive(Debug, Clone, Serialize)]
pub struct Moment {
    pub at_s: f32,
    pub kind: MomentKind,
    /// How much: dB for loudness moments, the ratio for brightness, the
    /// seconds a build lasted.
    pub amount: f32,
}

/// Hear a decoded song, with its lyrics if there are any (LRC).
pub fn listen(audio: &Audio, lrc: Option<&str>) -> ListeningSheet {
    let frames = spectrum::analyze(audio);
    let per_second = spectrum::per_second(&frames);
    let rhythm = rhythm::analyze(&frames);
    let tonal = tonal::analyze(&frames);
    let sections = structure::sections(&per_second);
    let moments = moments::find(&per_second, &sections);
    let lyrics = lrc
        .map(parse_lrc)
        .unwrap_or_default()
        .into_iter()
        .filter(|line| line.at_s <= frames.duration_s + 1.0)
        .collect::<Vec<_>>();
    let mut sheet = ListeningSheet {
        duration_s: frames.duration_s,
        tempo_bpm: rhythm.tempo_bpm,
        pulse_clarity: rhythm.pulse_clarity,
        syncopation: rhythm.syncopation,
        key: tonal.key,
        key_clarity: tonal.clarity,
        dynamic_range_db: per_second.dynamic_range_db(),
        brightness_hz: per_second.mean_brightness(),
        repetition: structure::repetition(&sections),
        sections,
        moments,
        lyrics,
        readings: Vec::new(),
    };
    sheet.lyrics = lyrics::place(&sheet.lyrics, &sheet.sections, &sheet.moments);
    sheet.readings = reading::read(&sheet, tonal.minor);
    sheet
}

/// Decode and hear a song from its file bytes (`ext` such as "mp3" helps
/// the decoder guess).
pub fn listen_to_bytes(
    bytes: Vec<u8>,
    ext: Option<&str>,
    lrc: Option<&str>,
) -> Result<ListeningSheet, DecodeError> {
    let audio = decode_mono(bytes, ext)?;
    Ok(listen(&audio, lrc))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::Audio;

    pub const RATE: u32 = 22_050;

    pub fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * RATE as f32) as usize]
    }

    /// Tones at these frequencies, summed, at this amplitude.
    pub fn chord(freqs: &[f32], seconds: f32, amp: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                freqs
                    .iter()
                    .map(|f| (2.0 * std::f32::consts::PI * f * t).sin())
                    .sum::<f32>()
                    * amp
                    / freqs.len() as f32
            })
            .collect()
    }

    /// Short noise bursts at a steady tempo.
    pub fn clicks(bpm: f32, seconds: f32, amp: f32) -> Vec<f32> {
        let n = (seconds * RATE as f32) as usize;
        let period = (60.0 / bpm * RATE as f32) as usize;
        let mut state: u32 = 12345;
        (0..n)
            .map(|i| {
                let into = i % period;
                if into < 400 {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    let noise = ((state >> 16) as f32 / 32_768.0) - 1.0;
                    noise * amp * (1.0 - into as f32 / 400.0)
                } else {
                    0.0
                }
            })
            .collect()
    }

    pub fn audio(samples: Vec<f32>) -> Audio {
        Audio {
            samples,
            rate: RATE,
        }
    }
}

/// Hear every recording in `LISTEN_TUNE_DIR` (with `<name>.lrc` beside it)
/// and write each sheet to `LISTEN_TUNE_OUT/<name>.txt`: for tuning the
/// thresholds against real songs.
#[cfg(test)]
mod tune {
    #[test]
    #[ignore = "needs LISTEN_TUNE_DIR and LISTEN_TUNE_OUT"]
    fn hear_a_folder() {
        let (Ok(dir), Ok(out)) = (
            std::env::var("LISTEN_TUNE_DIR"),
            std::env::var("LISTEN_TUNE_OUT"),
        ) else {
            return;
        };
        let mut paths: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext != "lrc"))
            .collect();
        paths.sort();
        for path in paths {
            let name = path.file_stem().unwrap().to_string_lossy().to_string();
            let lrc = std::fs::read_to_string(path.with_extension("lrc")).ok();
            let ext = path
                .extension()
                .map(|ext| ext.to_string_lossy().to_string());
            let bytes = std::fs::read(&path).unwrap();
            let Ok(sheet) = super::listen_to_bytes(bytes, ext.as_deref(), lrc.as_deref()) else {
                println!("{name}: could not hear");
                continue;
            };
            std::fs::write(
                std::path::Path::new(&out).join(format!("{name}.txt")),
                sheet.describe(),
            )
            .unwrap();
            println!(
                "{name}: tempo {:?} clarity {:.2} syncopation {:.2} key {:?} sections {}",
                sheet.tempo_bpm.map(|t| t.round()),
                sheet.pulse_clarity,
                sheet.syncopation,
                sheet.key,
                sheet.sections.len()
            );
        }
    }
}
