//! Minimal MP3 / FLAC / Ogg tag readers for the local music library.
//!
//! Only title/artist (and duration when cheap) — no codec decode.

/// Parsed tags from an audio file's embedded metadata.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AudioTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    /// Embedded cover art (mime, bytes).
    pub cover: Option<(String, Vec<u8>)>,
    /// Embedded lyrics (USLT / Vorbis LYRICS).
    pub lyrics: Option<String>,
}

fn clean_text(raw: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(raw)
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .trim()
        .to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn decode_id3_text(enc: u8, data: &[u8]) -> Option<String> {
    let text = match enc {
        0 => String::from_utf8_lossy(data).into_owned(), // ISO-8859-1
        1 => {
            // UTF-16 with BOM
            if data.len() >= 2 {
                let (be, body) = if data[0] == 0xFF && data[1] == 0xFE {
                    (false, &data[2..])
                } else if data[0] == 0xFE && data[1] == 0xFF {
                    (true, &data[2..])
                } else {
                    (false, &data[2..])
                };
                let units: Vec<u16> = body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| {
                        if be {
                            u16::from_be_bytes(*c)
                        } else {
                            u16::from_le_bytes(*c)
                        }
                    })
                    .collect();
                String::from_utf16_lossy(&units)
            } else {
                String::from_utf8_lossy(data).into_owned()
            }
        }
        2 => {
            let units: Vec<u16> = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes(*c))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(data).into_owned(),
    };
    let text = text
        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn synchsafe(bytes: &[u8]) -> usize {
    if bytes.len() < 4 {
        return 0;
    }
    ((bytes[0] as usize & 0x7F) << 21)
        | ((bytes[1] as usize & 0x7F) << 14)
        | ((bytes[2] as usize & 0x7F) << 7)
        | (bytes[3] as usize & 0x7F)
}

fn parse_id3v2(bytes: &[u8]) -> AudioTags {
    let mut tags = AudioTags::default();
    if bytes.len() < 10 || &bytes[0..3] != b"ID3" {
        return tags;
    }
    let ver = bytes[3];
    let flags = bytes[5];
    let tag_size = synchsafe(&bytes[6..10]);
    let mut pos = 10;
    if flags & 0x40 != 0 && pos + 4 <= bytes.len() {
        // extended header
        let ext_size = if ver >= 4 {
            synchsafe(&bytes[pos..pos + 4])
        } else {
            u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                as usize
                + 4
        };
        pos += ext_size.max(4);
    }
    let end = (10 + tag_size).min(bytes.len());
    while pos + 10 <= end {
        let id = &bytes[pos..pos + 4];
        if id[0] == 0 {
            break;
        }
        let size = if ver >= 4 {
            synchsafe(&bytes[pos + 4..pos + 8])
        } else {
            u32::from_be_bytes([bytes[pos + 4], bytes[pos + 5], bytes[pos + 6], bytes[pos + 7]])
                as usize
        };
        let frame_start = pos + 10;
        let frame_end = frame_start.saturating_add(size).min(end);
        if frame_end <= frame_start {
            break;
        }
        let data = &bytes[frame_start..frame_end];
        match id {
            b"TIT2" | b"TT2" => {
                if let Some(enc) = data.first() {
                    tags.title = decode_id3_text(*enc, &data[1..]);
                }
            }
            b"TPE1" | b"TP1" => {
                if let Some(enc) = data.first() {
                    tags.artist = decode_id3_text(*enc, &data[1..]);
                }
            }
            b"TALB" | b"TAL" => {
                if let Some(enc) = data.first() {
                    tags.album = decode_id3_text(*enc, &data[1..]);
                }
            }
            b"TLEN" => {
                if let Some(enc) = data.first() {
                    if let Some(text) = decode_id3_text(*enc, &data[1..]) {
                        if let Ok(ms) = text.trim().parse::<i64>() {
                            if ms > 0 {
                                tags.duration_ms = Some(ms);
                            }
                        }
                    }
                }
            }
            b"APIC" | b"PIC" => {
                if let Some(picture) = parse_id3_apic(data) {
                    // Prefer front cover / large art over other pictures.
                    let better = match &tags.cover {
                        None => true,
                        Some((_, existing)) => picture.1.len() > existing.len(),
                    };
                    if better {
                        tags.cover = Some(picture);
                    }
                }
            }
            // encoding + lang(3) + desc\0 + text
            b"USLT" | b"ULT" if data.len() > 4 => {
                let enc = data[0];
                let body = &data[4..];
                let text = if let Some(pos) = find_nul_terminator(body, enc) {
                    decode_id3_text(enc, &body[pos + 1..])
                } else {
                    decode_id3_text(enc, body)
                };
                if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
                    tags.lyrics = Some(text);
                }
            }
            _ => {}
        }
        pos = frame_end;
    }
    tags
}

fn parse_id3v1(bytes: &[u8]) -> AudioTags {
    let mut tags = AudioTags::default();
    if bytes.len() < 128 {
        return tags;
    }
    let tail = &bytes[bytes.len() - 128..];
    if &tail[0..3] != b"TAG" {
        return tags;
    }
    tags.title = clean_text(&tail[3..33]);
    tags.artist = clean_text(&tail[33..63]);
    tags.album = clean_text(&tail[63..93]);
    tags
}

fn parse_vorbis_comment_block(block: &[u8]) -> AudioTags {
    let mut tags = AudioTags::default();
    if block.len() < 8 {
        return tags;
    }
    let vendor_len = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
    let mut pos = 4 + vendor_len;
    if pos + 4 > block.len() {
        return tags;
    }
    let count = u32::from_le_bytes([block[pos], block[pos + 1], block[pos + 2], block[pos + 3]])
        as usize;
    pos += 4;
    for _ in 0..count {
        if pos + 4 > block.len() {
            break;
        }
        let len = u32::from_le_bytes([block[pos], block[pos + 1], block[pos + 2], block[pos + 3]])
            as usize;
        pos += 4;
        if pos + len > block.len() {
            break;
        }
        let entry = &block[pos..pos + len];
        pos += len;
        let text = String::from_utf8_lossy(entry);
        let (key, value) = match text.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_uppercase(), v.trim().to_string()),
            None => continue,
        };
        if value.is_empty() {
            continue;
        }
        match key.as_str() {
            "TITLE" => tags.title = Some(value),
            "ARTIST" | "AUTHOR" => tags.artist = Some(value),
            "ALBUM" => tags.album = Some(value),
            _ => {}
        }
    }
    tags
}

fn parse_flac(bytes: &[u8]) -> AudioTags {
    let mut tags = AudioTags::default();
    if bytes.len() < 8 || &bytes[0..4] != b"fLaC" {
        return tags;
    }
    let mut pos = 4;
    loop {
        if pos + 4 > bytes.len() {
            break;
        }
        let header = bytes[pos];
        let block_type = header & 0x7F;
        let last = header & 0x80 != 0;
        let len = ((bytes[pos + 1] as usize) << 16)
            | ((bytes[pos + 2] as usize) << 8)
            | (bytes[pos + 3] as usize);
        pos += 4;
        if pos + len > bytes.len() {
            break;
        }
        let body = &bytes[pos..pos + len];
        match block_type {
            0 => {
                // STREAMINFO: sample rate (20 bits) + total samples (36 bits)
                if len >= 18 {
                    let b = &body[10..18];
                    let packed = u64::from_be_bytes([
                        0,
                        0,
                        0,
                        0,
                        b[0],
                        b[1],
                        b[2],
                        b[3],
                    ]);
                    // bytes 10-12: sample rate high 20 bits of 64-bit window starting at 10
                    let sample_rate = (((body[10] as u32) << 12)
                        | ((body[11] as u32) << 4)
                        | ((body[12] as u32) >> 4))
                        as u64;
                    let total_samples = (((body[12] as u64) & 0x0F) << 32)
                        | ((body[13] as u64) << 24)
                        | ((body[14] as u64) << 16)
                        | ((body[15] as u64) << 8)
                        | (body[16] as u64);
                    if sample_rate > 0 && total_samples > 0 {
                        tags.duration_ms =
                            Some((total_samples * 1000 / sample_rate) as i64);
                    }
                    let _ = packed;
                }
            }
            4 => {
                apply_vorbis_extras(&mut tags, body);
            }
            6 => {
                if let Some(pic) = parse_flac_picture_block(body) {
                    tags.cover = Some(pic);
                }
            }
            _ => {}
        }
        pos += len;
        if last {
            break;
        }
    }
    tags
}

fn parse_ogg(bytes: &[u8]) -> AudioTags {
    let mut tags = AudioTags::default();
    // Find the Vorbis comment packet: "\x03vorbis" …
    if let Some(idx) = find_subslice(bytes, b"\x03vorbis") {
        let body = &bytes[idx + 7..];
        tags = parse_vorbis_comment_block(body);
        apply_vorbis_extras(&mut tags, body);
    }
    tags
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_nul_terminator(data: &[u8], encoding: u8) -> Option<usize> {
    if encoding == 1 || encoding == 2 {
        // UTF-16: 0x00 0x00 at even offset
        let mut i = 0;
        while i + 1 < data.len() {
            if data[i] == 0 && data[i + 1] == 0 {
                return Some(i);
            }
            i += 2;
        }
        None
    } else {
        data.iter().position(|b| *b == 0)
    }
}

fn parse_id3_apic(data: &[u8]) -> Option<(String, Vec<u8>)> {
    if data.is_empty() {
        return None;
    }
    let enc = data[0];
    let mut pos = 1;
    // mime (ISO-8859-1, NUL-terminated)
    let mime_end = data[pos..].iter().position(|b| *b == 0)? + pos;
    let mime = String::from_utf8_lossy(&data[pos..mime_end]).to_ascii_lowercase();
    pos = mime_end + 1;
    if pos >= data.len() {
        return None;
    }
    let _picture_type = data[pos];
    pos += 1;
    // description in `enc`, NUL-terminated
    let desc_nul = find_nul_terminator(&data[pos..], enc)?;
    pos += desc_nul + if enc == 1 || enc == 2 { 2 } else { 1 };
    if pos >= data.len() {
        return None;
    }
    let image = data[pos..].to_vec();
    if image.len() < 16 {
        return None;
    }
    let mime = if mime.contains("png") {
        "image/png".to_string()
    } else if mime.contains("webp") {
        "image/webp".to_string()
    } else if mime.contains("gif") {
        "image/gif".to_string()
    } else {
        "image/jpeg".to_string()
    };
    Some((mime, image))
}

fn parse_flac_picture_block(block: &[u8]) -> Option<(String, Vec<u8>)> {
    if block.len() < 32 {
        return None;
    }
    let be_u32 = |i: usize| {
        u32::from_be_bytes([block[i], block[i + 1], block[i + 2], block[i + 3]]) as usize
    };
    let mut pos = 4; // skip picture type
    let mime_len = be_u32(pos);
    pos += 4;
    if pos + mime_len > block.len() {
        return None;
    }
    let mime = String::from_utf8_lossy(&block[pos..pos + mime_len]).to_ascii_lowercase();
    pos += mime_len;
    let desc_len = be_u32(pos);
    pos += 4 + desc_len + 16; // desc + w/h/depth/colors
    if pos + 4 > block.len() {
        return None;
    }
    let data_len = be_u32(pos);
    pos += 4;
    if pos + data_len > block.len() {
        return None;
    }
    let image = block[pos..pos + data_len].to_vec();
    if image.len() < 16 {
        return None;
    }
    let mime = if mime.contains("png") {
        "image/png".to_string()
    } else if mime.contains("webp") {
        "image/webp".to_string()
    } else if mime.contains("gif") {
        "image/gif".to_string()
    } else {
        "image/jpeg".to_string()
    };
    Some((mime, image))
}

fn apply_vorbis_extras(tags: &mut AudioTags, block: &[u8]) {
    let mut extra = parse_vorbis_comment_block(block);
    if extra.title.is_some() {
        tags.title = extra.title.take();
    }
    if extra.artist.is_some() {
        tags.artist = extra.artist.take();
    }
    if extra.album.is_some() {
        tags.album = extra.album.take();
    }
    // Re-parse keys we dropped in parse_vorbis_comment_block
    if block.len() >= 8 {
        let vendor_len = u32::from_le_bytes([block[0], block[1], block[2], block[3]]) as usize;
        let mut pos = 4 + vendor_len;
        if pos + 4 <= block.len() {
            let count =
                u32::from_le_bytes([block[pos], block[pos + 1], block[pos + 2], block[pos + 3]])
                    as usize;
            pos += 4;
            for _ in 0..count {
                if pos + 4 > block.len() {
                    break;
                }
                let len = u32::from_le_bytes([
                    block[pos],
                    block[pos + 1],
                    block[pos + 2],
                    block[pos + 3],
                ]) as usize;
                pos += 4;
                if pos + len > block.len() {
                    break;
                }
                let entry = &block[pos..pos + len];
                pos += len;
                let text = String::from_utf8_lossy(entry);
                let (key, value) = match text.split_once('=') {
                    Some((k, v)) => (k.trim().to_ascii_uppercase(), v.trim().to_string()),
                    None => continue,
                };
                if value.is_empty() {
                    continue;
                }
                match key.as_str() {
                    "LYRICS" | "UNSYNCEDLYRICS" | "SYNCEDLYRICS" | "UNSYNCED LYRICS" => {
                        tags.lyrics = Some(value)
                    }
                    "METADATA_BLOCK_PICTURE" => {
                        if let Ok(raw) = decode_base64(value.as_bytes()) {
                            if let Some(pic) = parse_flac_picture_block(&raw) {
                                tags.cover = Some(pic);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn decode_base64(input: &[u8]) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 256] = &{
        let mut t = [0xFFu8; 256];
        let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut i = 0;
        while i < 64 {
            t[alphabet[i] as usize] = i as u8;
            i += 1;
        }
        t
    };
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in input {
        if c == b'=' || c == b'\n' || c == b'\r' || c == b' ' {
            continue;
        }
        let v = TABLE[c as usize];
        if v == 0xFF {
            return Err(());
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

/// Best-effort tags for mp3 / flac / ogg. Unknown layout returns empty tags.
pub fn parse_audio_tags(bytes: &[u8], ext: &str) -> AudioTags {
    match ext.to_ascii_lowercase().as_str() {
        "mp3" | "mpeg" => {
            let mut tags = parse_id3v2(bytes);
            if tags.title.is_none() && tags.artist.is_none() {
                let v1 = parse_id3v1(bytes);
                if tags.title.is_none() {
                    tags.title = v1.title;
                }
                if tags.artist.is_none() {
                    tags.artist = v1.artist;
                }
                if tags.album.is_none() {
                    tags.album = v1.album;
                }
            }
            tags
        }
        "flac" => parse_flac(bytes),
        "ogg" | "oga" => parse_ogg(bytes),
        _ => AudioTags::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_id3v2_title_artist() {
        // Build a tiny ID3v2.3 tag with TIT2/TPE1 (ISO-8859-1)
        let mut tag = Vec::new();
        tag.extend_from_slice(b"ID3");
        tag.extend_from_slice(&[3, 0, 0]); // ver 2.3, flags
        let mut body = Vec::new();
        fn frame(id: &[u8; 4], text: &str) -> Vec<u8> {
            let payload = {
                let mut p = vec![0u8]; // encoding ISO-8859-1
                p.extend_from_slice(text.as_bytes());
                p
            };
            let mut f = Vec::new();
            f.extend_from_slice(id);
            f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            f.extend_from_slice(&[0, 0]);
            f.extend_from_slice(&payload);
            f
        }
        body.extend_from_slice(&frame(b"TIT2", "Song"));
        body.extend_from_slice(&frame(b"TPE1", "Artist"));
        let size = body.len();
        tag.push(((size >> 21) & 0x7F) as u8);
        tag.push(((size >> 14) & 0x7F) as u8);
        tag.push(((size >> 7) & 0x7F) as u8);
        tag.push((size & 0x7F) as u8);
        tag.extend_from_slice(&body);
        tag.extend_from_slice(&[0u8; 16]);
        let tags = parse_audio_tags(&tag, "mp3");
        assert_eq!(tags.title.as_deref(), Some("Song"));
        assert_eq!(tags.artist.as_deref(), Some("Artist"));
    }

    #[test]
    fn parses_flac_vorbis_comments() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"fLaC");
        // STREAMINFO last=false type=0 len=34 (minimal padding ok for parser)
        let mut streaminfo = vec![0u8; 34];
        // sample rate 44100 at bytes 10-12, total samples 4410000 → ~100s
        streaminfo[10] = 0x0A; // 44100 >> 12 partial — approximate
        streaminfo[11] = 0xC4;
        streaminfo[12] = 0x00;
        bytes.push(0x00); // not last, type 0
        let len = streaminfo.len();
        bytes.push(((len >> 16) & 0xFF) as u8);
        bytes.push(((len >> 8) & 0xFF) as u8);
        bytes.push((len & 0xFF) as u8);
        bytes.extend_from_slice(&streaminfo);
        // VORBIS_COMMENT last=true type=4
        let mut comment = Vec::new();
        comment.extend_from_slice(&0u32.to_le_bytes()); // empty vendor
        comment.extend_from_slice(&2u32.to_le_bytes());
        for s in ["TITLE=Flac Song", "ARTIST=Flac Artist"] {
            comment.extend_from_slice(&(s.len() as u32).to_le_bytes());
            comment.extend_from_slice(s.as_bytes());
        }
        bytes.push(0x84); // last, type 4
        let clen = comment.len();
        bytes.push(((clen >> 16) & 0xFF) as u8);
        bytes.push(((clen >> 8) & 0xFF) as u8);
        bytes.push((clen & 0xFF) as u8);
        bytes.extend_from_slice(&comment);
        let tags = parse_audio_tags(&bytes, "flac");
        assert_eq!(tags.title.as_deref(), Some("Flac Song"));
        assert_eq!(tags.artist.as_deref(), Some("Flac Artist"));
    }
}
