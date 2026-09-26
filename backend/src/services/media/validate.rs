//! Content sniffing for persistent media. Multipart MIME and filenames are not trusted.
//!
//! Image decode uses the same pixel budget as the existing editor. GIF and video
//! are accepted by container magic only — this is not a full codec parser.

use sha2::{Digest, Sha256};

use super::error::MediaError;

pub const ALLOWED_IMAGE_MIMES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];
pub const ALLOWED_VIDEO_MIMES: [&str; 3] = ["video/mp4", "video/webm", "video/quicktime"];
/// Local music library uploads (Myriad local source).
pub const ALLOWED_AUDIO_MIMES: [&str; 6] = [
    "audio/mpeg",
    "audio/mp4",
    "audio/flac",
    "audio/wav",
    "audio/ogg",
    "audio/aac",
];

const MAX_IMAGE_EDGE: u32 = 8192;
const MAX_IMAGE_PIXELS: u64 = 16_777_216;
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaClass {
    Image,
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedPayload {
    pub mime: String,
    pub ext: &'static str,
    pub checksum_sha256: String,
    pub size: i64,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub class: MediaClass,
}

pub fn allowed_media_mimes() -> impl Iterator<Item = &'static str> {
    ALLOWED_IMAGE_MIMES
        .into_iter()
        .chain(ALLOWED_VIDEO_MIMES)
        .chain(ALLOWED_AUDIO_MIMES)
}

/// Map browser/vendor MIME aliases onto the canonical allowlist values.
pub fn canonical_mime_alias(claimed: &str) -> String {
    let mime = claimed
        .split(';')
        .next()
        .unwrap_or(claimed)
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        // Browsers often send audio/mp3 for .mp3
        "audio/mp3" | "audio/mpeg3" | "audio/x-mpeg-3" | "audio/x-mp3" | "audio/mpg" => {
            "audio/mpeg".into()
        }
        "audio/m4a" | "audio/x-m4a" | "audio/mp4a-latm" => "audio/mp4".into(),
        "audio/x-flac" | "audio/flac" => "audio/flac".into(),
        "audio/x-wav" | "audio/wave" | "audio/vnd.wave" | "audio/x-pn-wav" => "audio/wav".into(),
        "audio/x-ogg" | "application/ogg" | "audio/vorbis" | "audio/opus" => "audio/ogg".into(),
        "audio/aacp" | "audio/x-aac" | "audio/x-hx-aac-adts" => "audio/aac".into(),
        _ => mime,
    }
}

/// Infer audio MIME from filename when the browser sends octet-stream/empty.
/// Local music library only accepts tagged audio: mp3 / flac / ogg.
pub fn audio_mime_from_filename(filename: &str) -> Option<&'static str> {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp3" | "mpga" | "mpeg" => Some("audio/mpeg"),
        "flac" => Some("audio/flac"),
        "ogg" | "oga" => Some("audio/ogg"),
        _ => None,
    }
}

pub fn normalize_mime(claimed: &str) -> Result<String, MediaError> {
    let mime = canonical_mime_alias(claimed);
    if allowed_media_mimes().any(|allowed| allowed == mime) {
        Ok(mime)
    } else {
        Err(MediaError::invalid("Unsupported media type"))
    }
}

pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    match mime {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "video/mp4" => Some("mp4"),
        "video/webm" => Some("webm"),
        "video/quicktime" => Some("mov"),
        "audio/mpeg" => Some("mp3"),
        "audio/mp4" => Some("m4a"),
        "audio/flac" => Some("flac"),
        "audio/wav" => Some("wav"),
        "audio/ogg" => Some("ogg"),
        "audio/aac" => Some("aac"),
        _ => None,
    }
}

pub fn checksum_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn validate_bytes(
    bytes: &[u8],
    claimed_mime: &str,
    max_bytes: usize,
) -> Result<ValidatedPayload, MediaError> {
    if max_bytes == 0 {
        return Err(MediaError::invalid("Media size limit is missing"));
    }
    if bytes.is_empty() {
        return Err(MediaError::invalid("Empty file"));
    }
    if bytes.len() > max_bytes {
        return Err(MediaError::TooLarge { max_bytes });
    }
    let mime = normalize_mime(claimed_mime)?;
    let ext =
        extension_for_mime(&mime).ok_or_else(|| MediaError::invalid("Unsupported media type"))?;
    sniff_magic(bytes, &mime)?;
    let (width, height, class) = match mime.as_str() {
        "image/gif" => (None, None, MediaClass::Image),
        "image/jpeg" | "image/png" | "image/webp" => {
            let (width, height) = decode_image_dimensions(bytes)?;
            (Some(width), Some(height), MediaClass::Image)
        }
        "audio/mpeg" | "audio/mp4" | "audio/flac" | "audio/wav" | "audio/ogg" | "audio/aac" => {
            (None, None, MediaClass::Audio)
        }
        _ => (None, None, MediaClass::Video),
    };
    Ok(ValidatedPayload {
        mime,
        ext,
        checksum_sha256: checksum_sha256(bytes),
        size: bytes.len() as i64,
        width,
        height,
        class,
    })
}

fn sniff_magic(bytes: &[u8], mime: &str) -> Result<(), MediaError> {
    let ok = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "video/webm" => bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]),
        "video/mp4" | "video/quicktime" => bytes.len() >= 8 && &bytes[4..8] == b"ftyp",
        "audio/flac" => bytes.starts_with(b"fLaC"),
        "audio/wav" => {
            bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE"
        }
        "audio/ogg" => bytes.starts_with(b"OggS"),
        "audio/mpeg" => {
            bytes.starts_with(b"ID3")
                || (bytes.len() >= 2 && bytes[0] == 0xff && (bytes[1] & 0xe0) == 0xe0)
        }
        "audio/mp4" | "audio/aac" => {
            // m4a/mp4 family: ftyp box; ADTS AAC starts with 0xFFFx
            (bytes.len() >= 8 && &bytes[4..8] == b"ftyp")
                || (bytes.len() >= 2 && bytes[0] == 0xff && (bytes[1] & 0xf0) == 0xf0)
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(MediaError::invalid(
            "File content does not match its media type",
        ))
    }
}

fn decode_image_dimensions(bytes: &[u8]) -> Result<(i32, i32), MediaError> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| MediaError::invalid("Invalid image"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| MediaError::invalid("Invalid image"))?;
    if width == 0
        || height == 0
        || width > MAX_IMAGE_EDGE
        || height > MAX_IMAGE_EDGE
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(MediaError::invalid(
            "Image dimensions exceed the supported limit",
        ));
    }
    i32::try_from(width)
        .ok()
        .zip(i32::try_from(height).ok())
        .ok_or_else(|| MediaError::invalid("Image dimensions exceed the supported limit"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII=";

    fn png_bytes() -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(PNG)
            .unwrap()
    }

    #[test]
    fn rejects_empty_oversize_and_spoofed_mime() {
        let png = png_bytes();
        assert!(matches!(
            validate_bytes(&[], "image/png", 1024),
            Err(MediaError::Invalid { .. })
        ));
        assert!(matches!(
            validate_bytes(&png, "image/png", 1),
            Err(MediaError::TooLarge { max_bytes: 1 })
        ));
        assert!(validate_bytes(&png, "image/jpeg", 1024 * 1024).is_err());
        assert!(validate_bytes(&png, "application/octet-stream", 1024 * 1024).is_err());
        assert!(validate_bytes(&png, "image/png", 1024 * 1024).is_ok());
    }

    #[test]
    fn gif_and_video_are_magic_only() {
        let gif = b"GIF89a........";
        let parsed = validate_bytes(gif, "image/gif", 1024).unwrap();
        assert_eq!(parsed.ext, "gif");
        assert!(parsed.width.is_none());

        let mut mp4 = vec![0, 0, 0, 0x18];
        mp4.extend_from_slice(b"ftypisom");
        mp4.extend_from_slice(&[0; 16]);
        let video = validate_bytes(&mp4, "video/mp4", 1024).unwrap();
        assert_eq!(video.class, MediaClass::Video);
        assert!(validate_bytes(&[0; 32], "video/mp4", 1024).is_err());
    }

    #[test]
    fn too_large_error_has_no_path() {
        let err = MediaError::TooLarge { max_bytes: 12 };
        let app = err.into_app_error();
        let json = app.to_json().to_string();
        assert!(json.contains("MEDIA_TOO_LARGE") || app.code() == Some("MEDIA_TOO_LARGE"));
        assert!(!json.contains("/data"));
        assert!(!json.contains("media/"));
    }
}
