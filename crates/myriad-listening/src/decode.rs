//! From file bytes to one channel of samples, around 22 kHz.
//!
//! Features are measured below 11 kHz, so the sound is mixed down to mono
//! and thinned to about 22 kHz (averaging neighbours as it goes, which keeps
//! higher frequencies from folding back in).

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Longest song heard: past this the rest is not decoded.
const MAX_SECONDS: f32 = 15.0 * 60.0;
const TARGET_RATE: u32 = 22_050;

#[derive(Debug, Clone)]
pub struct Audio {
    pub samples: Vec<f32>,
    pub rate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Not a format it can read.
    Unsupported,
    /// The file breaks off or is damaged.
    Damaged,
    /// Nothing to hear in it.
    Empty,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unsupported => "audio format not supported",
            Self::Damaged => "audio file is damaged",
            Self::Empty => "audio file has no sound",
        })
    }
}

impl std::error::Error for DecodeError {}

/// Decode a whole file to mono near 22 kHz.
pub fn decode_mono(bytes: Vec<u8>, ext: Option<&str>) -> Result<Audio, DecodeError> {
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = ext {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| DecodeError::Unsupported)?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(DecodeError::Unsupported)?;
    let track_id = track.id;
    let rate = track
        .codec_params
        .sample_rate
        .ok_or(DecodeError::Unsupported)?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| DecodeError::Unsupported)?;

    let step = (rate as f32 / TARGET_RATE as f32).round().max(1.0) as usize;
    let limit = (MAX_SECONDS * rate as f32) as usize;
    let mut mono: Vec<f32> = Vec::new();
    let mut read = 0usize;
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(_) => return Err(DecodeError::Damaged),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A bad frame here and there is skipped, as a player would.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(_) => return Err(DecodeError::Damaged),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let buffer =
            buffer.get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        if buffer.capacity() < decoded.capacity() * channels {
            *buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks(channels) {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
        }
        read += buffer.samples().len() / channels;
        if read >= limit {
            break;
        }
    }
    if mono.is_empty() {
        return Err(DecodeError::Empty);
    }
    let samples = if step > 1 {
        mono.chunks(step)
            .map(|group| group.iter().sum::<f32>() / group.len() as f32)
            .collect()
    } else {
        mono
    };
    Ok(Audio {
        samples,
        rate: rate / step as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit stereo WAV file of these samples (same on both channels).
    fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
        let data_len = (samples.len() * 4) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
            out.extend_from_slice(&value.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    #[test]
    fn a_file_becomes_one_channel_near_22_khz() {
        let samples: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let audio = decode_mono(wav(&samples, 44_100), Some("wav")).unwrap();
        assert_eq!(audio.rate, 22_050);
        assert!((audio.samples.len() as i64 - 22_050).abs() < 10);
        assert!(audio.samples.iter().any(|s| s.abs() > 0.3));
        assert_eq!(
            decode_mono(b"not audio at all".to_vec(), None).unwrap_err(),
            DecodeError::Unsupported
        );
    }
}
