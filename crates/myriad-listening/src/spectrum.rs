//! The sound frame by frame: a short-time Fourier transform, and what each
//! frame sounds like.
//!
//! Frames of 2048 samples (about 93 ms at 22 kHz), one every 512 (about
//! 23 ms), under a Hann window. Per frame: loudness (RMS, dB), brightness
//! (spectral centroid), onset strength (spectral flux on log-compressed
//! magnitudes in bands spaced like pitch, the usual onset detector), and the energy of each of the
//! twelve pitch classes (chroma) between 65 Hz and 4.2 kHz.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::Audio;

const WINDOW: usize = 2048;
const HOP: usize = 512;
const CHROMA_LOW_HZ: f32 = 65.0;
/// Onsets are measured in bands spaced like pitch, so the many high bins do
/// not drown the few low ones (a kick drum, a bass note).
const ONSET_BANDS: usize = 36;
const ONSET_LOW_HZ: f32 = 40.0;
const ONSET_HIGH_HZ: f32 = 10_000.0;
const CHROMA_HIGH_HZ: f32 = 4_200.0;
/// Quieter than this counts as silence.
const FLOOR_DB: f32 = -80.0;

pub struct Frames {
    pub per_second: f32,
    pub duration_s: f32,
    pub loudness_db: Vec<f32>,
    pub brightness_hz: Vec<f32>,
    pub onset: Vec<f32>,
    pub chroma: Vec<[f32; 12]>,
}

pub fn analyze(audio: &Audio) -> Frames {
    let rate = audio.rate as f32;
    let samples = &audio.samples;
    let window: Vec<f32> = (0..WINDOW)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW - 1) as f32).cos())
        .collect();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(WINDOW);
    let bins = WINDOW / 2 + 1;
    let bin_hz = rate / WINDOW as f32;
    let pitch_class: Vec<Option<usize>> = (0..bins)
        .map(|bin| {
            let hz = bin as f32 * bin_hz;
            (CHROMA_LOW_HZ..=CHROMA_HIGH_HZ).contains(&hz).then(|| {
                let midi = 69.0 + 12.0 * (hz / 440.0).log2();
                (midi.round() as i64).rem_euclid(12) as usize
            })
        })
        .collect();

    let band_of: Vec<Option<usize>> = (0..bins)
        .map(|bin| {
            let hz = bin as f32 * bin_hz;
            (ONSET_LOW_HZ..ONSET_HIGH_HZ).contains(&hz).then(|| {
                let place = (hz / ONSET_LOW_HZ).ln() / (ONSET_HIGH_HZ / ONSET_LOW_HZ).ln();
                ((place * ONSET_BANDS as f32) as usize).min(ONSET_BANDS - 1)
            })
        })
        .collect();
    let mut band_bins = [0usize; ONSET_BANDS];
    for band in band_of.iter().flatten() {
        band_bins[*band] += 1;
    }

    let count = if samples.len() >= WINDOW {
        (samples.len() - WINDOW) / HOP + 1
    } else {
        1
    };
    let mut frames = Frames {
        per_second: rate / HOP as f32,
        duration_s: samples.len() as f32 / rate,
        loudness_db: Vec::with_capacity(count),
        brightness_hz: Vec::with_capacity(count),
        onset: Vec::with_capacity(count),
        chroma: Vec::with_capacity(count),
    };
    let mut buffer = vec![Complex::new(0.0f32, 0.0); WINDOW];
    let mut previous = [0.0f32; ONSET_BANDS];
    for index in 0..count {
        let start = index * HOP;
        let mut energy = 0.0f32;
        for (i, slot) in buffer.iter_mut().enumerate() {
            let sample = samples.get(start + i).copied().unwrap_or(0.0);
            energy += sample * sample;
            *slot = Complex::new(sample * window[i], 0.0);
        }
        fft.process(&mut buffer);
        let rms = (energy / WINDOW as f32).sqrt();
        frames
            .loudness_db
            .push((20.0 * rms.max(1e-9).log10()).max(FLOOR_DB));

        let mut weighted = 0.0f32;
        let mut total = 0.0f32;
        let mut bands = [0.0f32; ONSET_BANDS];
        let mut chroma = [0.0f32; 12];
        for bin in 0..bins {
            let magnitude = buffer[bin].norm();
            weighted += bin as f32 * bin_hz * magnitude;
            total += magnitude;
            if let Some(band) = band_of[bin] {
                bands[band] += magnitude;
            }
            if let Some(class) = pitch_class[bin] {
                chroma[class] += magnitude * magnitude;
            }
        }
        frames
            .brightness_hz
            .push(if total > 0.0 { weighted / total } else { 0.0 });
        let mut flux = 0.0f32;
        for (band, total) in bands.iter().enumerate() {
            if band_bins[band] == 0 {
                continue;
            }
            let compressed = (1.0 + 100.0 * total / band_bins[band] as f32).ln();
            flux += (compressed - previous[band]).max(0.0);
            previous[band] = compressed;
        }
        frames.onset.push(if index == 0 { 0.0 } else { flux });
        let sum: f32 = chroma.iter().sum();
        if sum > 0.0 {
            chroma.iter_mut().for_each(|value| *value /= sum);
        }
        frames.chroma.push(chroma);
    }
    frames
}

/// The song second by second: what changes over a song is heard at this
/// scale.
pub struct Seconds {
    /// Length of one step, close to a second.
    pub step_s: f32,
    pub loudness_db: Vec<f32>,
    pub brightness_hz: Vec<f32>,
    pub chroma: Vec<[f32; 12]>,
}

pub fn per_second(frames: &Frames) -> Seconds {
    let step = frames.per_second.round().max(1.0) as usize;
    let mut seconds = Seconds {
        step_s: step as f32 / frames.per_second,
        loudness_db: Vec::new(),
        brightness_hz: Vec::new(),
        chroma: Vec::new(),
    };
    for start in (0..frames.loudness_db.len()).step_by(step) {
        let end = (start + step).min(frames.loudness_db.len());
        let span = start..end;
        // Loudness is averaged as power, then back to dB.
        let power: f32 = frames.loudness_db[span.clone()]
            .iter()
            .map(|db| 10f32.powf(db / 10.0))
            .sum::<f32>()
            / (end - start) as f32;
        seconds
            .loudness_db
            .push((10.0 * power.max(1e-12).log10()).max(FLOOR_DB));
        let loud: Vec<usize> = span
            .clone()
            .filter(|&i| frames.loudness_db[i] > FLOOR_DB + 20.0)
            .collect();
        let brightness = if loud.is_empty() {
            0.0
        } else {
            loud.iter().map(|&i| frames.brightness_hz[i]).sum::<f32>() / loud.len() as f32
        };
        seconds.brightness_hz.push(brightness);
        let mut chroma = [0.0f32; 12];
        for i in span {
            for (class, value) in frames.chroma[i].iter().enumerate() {
                chroma[class] += value;
            }
        }
        let sum: f32 = chroma.iter().sum();
        if sum > 0.0 {
            chroma.iter_mut().for_each(|value| *value /= sum);
        }
        seconds.chroma.push(chroma);
    }
    seconds
}

impl Seconds {
    /// Seconds that have sound in them.
    pub fn audible(&self) -> Vec<usize> {
        let loudest = self.loudness_db.iter().copied().fold(FLOOR_DB, f32::max);
        (0..self.loudness_db.len())
            .filter(|&i| self.loudness_db[i] > loudest - 40.0)
            .collect()
    }

    /// Between its quiet parts (5th percentile) and loud parts (95th).
    pub fn dynamic_range_db(&self) -> f32 {
        let mut levels: Vec<f32> = self
            .audible()
            .into_iter()
            .map(|i| self.loudness_db[i])
            .collect();
        if levels.len() < 2 {
            return 0.0;
        }
        levels.sort_by(f32::total_cmp);
        let at = |q: f32| levels[((levels.len() - 1) as f32 * q).round() as usize];
        at(0.95) - at(0.05)
    }

    pub fn mean_loudness_db(&self) -> f32 {
        let audible = self.audible();
        if audible.is_empty() {
            return FLOOR_DB;
        }
        audible.iter().map(|&i| self.loudness_db[i]).sum::<f32>() / audible.len() as f32
    }

    pub fn mean_brightness(&self) -> f32 {
        let audible = self.audible();
        if audible.is_empty() {
            return 0.0;
        }
        audible.iter().map(|&i| self.brightness_hz[i]).sum::<f32>() / audible.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{audio, chord, silence};

    #[test]
    fn a_frame_knows_its_loudness_brightness_and_pitch() {
        // A quiet low A for 2 s, then a loud high A for 2 s.
        let mut samples = chord(&[220.0], 2.0, 0.05);
        samples.extend(chord(&[1760.0], 2.0, 0.5));
        let frames = analyze(&audio(samples));
        let seconds = per_second(&frames);
        assert_eq!(seconds.loudness_db.len(), 4);
        assert!(seconds.loudness_db[3] - seconds.loudness_db[0] > 15.0);
        assert!(seconds.brightness_hz[3] > seconds.brightness_hz[0] * 4.0);
        // Pitch class A (index 9) carries the chroma.
        let a = frames.chroma[10][9];
        assert!(a > 0.8, "{a}");
        assert!(seconds.dynamic_range_db() > 15.0);
        // Silence has no brightness and is not audible.
        let quiet = per_second(&analyze(&audio(silence(2.0))));
        assert!(quiet.audible().is_empty() || quiet.mean_brightness() == 0.0);
    }
}
