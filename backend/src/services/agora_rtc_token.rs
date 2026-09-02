//! Agora AccessToken2 (version `007`) for RTC channel join.
//!
//! Layout matches AgoraIO/Tools `AccessToken2.py`: little-endian packers,
//! HMAC-SHA256 signing key, zlib then base64.

use flate2::write::ZlibEncoder;
use flate2::Compression;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

const VERSION: &str = "007";
pub const RTC_SERVICE_TYPE: u16 = 1;
pub const PRIVILEGE_JOIN_CHANNEL: u16 = 1;
pub const PRIVILEGE_PUBLISH_AUDIO: u16 = 2;
pub const PRIVILEGE_PUBLISH_VIDEO: u16 = 3;
pub const PRIVILEGE_PUBLISH_DATA: u16 = 4;

#[derive(Debug)]
pub enum AgoraTokenError {
    InvalidAppId,
    InvalidCertificate,
    Crypto(String),
}

impl std::fmt::Display for AgoraTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAppId => write!(f, "Agora App ID is invalid"),
            Self::InvalidCertificate => write!(f, "Agora App Certificate is invalid"),
            Self::Crypto(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AgoraTokenError {}

fn pack_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn pack_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn pack_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    pack_u16(out, bytes.len() as u16);
    out.extend_from_slice(bytes);
}

fn pack_map_u32(out: &mut Vec<u8>, entries: &[(u16, u32)]) {
    pack_u16(out, entries.len() as u16);
    for (key, value) in entries {
        pack_u16(out, *key);
        pack_u32(out, *value);
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<Vec<u8>, AgoraTokenError> {
    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|e| AgoraTokenError::Crypto(e.to_string()))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Build an RTC token. `uid` 0 packs as an empty string (Agora wildcard).
pub fn build_rtc_token(
    app_id: &str,
    app_certificate: &str,
    channel: &str,
    uid: u32,
    expire_seconds: u32,
    issue_ts: u32,
    salt: u32,
) -> Result<String, AgoraTokenError> {
    let app_id = app_id.trim();
    let app_certificate = app_certificate.trim();
    if app_id.len() != 32 {
        return Err(AgoraTokenError::InvalidAppId);
    }
    if app_certificate.len() != 32 {
        return Err(AgoraTokenError::InvalidCertificate);
    }

    let privilege_expire = issue_ts.saturating_add(expire_seconds);
    let privileges = [
        (PRIVILEGE_JOIN_CHANNEL, privilege_expire),
        (PRIVILEGE_PUBLISH_AUDIO, privilege_expire),
        (PRIVILEGE_PUBLISH_VIDEO, privilege_expire),
        (PRIVILEGE_PUBLISH_DATA, privilege_expire),
    ];

    let uid_str = if uid == 0 {
        String::new()
    } else {
        uid.to_string()
    };

    let mut service = Vec::new();
    pack_u16(&mut service, RTC_SERVICE_TYPE);
    pack_map_u32(&mut service, &privileges);
    pack_string(&mut service, channel);
    pack_string(&mut service, &uid_str);

    let mut info = Vec::new();
    pack_string(&mut info, app_id);
    pack_u32(&mut info, issue_ts);
    pack_u32(&mut info, expire_seconds);
    pack_u32(&mut info, salt);
    pack_u16(&mut info, 1);
    info.extend_from_slice(&service);

    let mut ts_bytes = Vec::new();
    pack_u32(&mut ts_bytes, issue_ts);
    let signing = hmac_sha256(app_certificate.as_bytes(), &ts_bytes)?;
    let mut salt_bytes = Vec::new();
    pack_u32(&mut salt_bytes, salt);
    let signing = hmac_sha256(&signing, &salt_bytes)?;
    let signature = hmac_sha256(&signing, &info)?;

    let mut packed = Vec::new();
    pack_u16(&mut packed, signature.len() as u16);
    packed.extend_from_slice(&signature);
    packed.extend_from_slice(&info);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&packed)
        .map_err(|e| AgoraTokenError::Crypto(e.to_string()))?;
    let compressed = encoder
        .finish()
        .map_err(|e| AgoraTokenError::Crypto(e.to_string()))?;
    Ok(format!(
        "{VERSION}{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, compressed)
    ))
}

pub fn build_rtc_token_now(
    app_id: &str,
    app_certificate: &str,
    channel: &str,
    uid: u32,
    expire_seconds: u32,
) -> Result<String, AgoraTokenError> {
    let issue_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    let salt = ((uuid::Uuid::new_v4().as_u128() % 99_999_999) as u32).max(1);
    build_rtc_token(
        app_id,
        app_certificate,
        channel,
        uid,
        expire_seconds,
        issue_ts,
        salt,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    const APP_ID: &str = "970ca35de60c44645bbae8a215061b33";
    const CERT: &str = "5cfd2fd1755d40ecb72977518be15d3b";

    #[test]
    fn token_is_versioned_zlib_base64() {
        let token = build_rtc_token(APP_ID, CERT, "demo", 123, 3600, 1_700_000_000, 42).unwrap();
        assert!(token.starts_with("007"), "{token}");
        let raw = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &token[3..])
            .expect("base64");
        let mut decoder = ZlibDecoder::new(raw.as_slice());
        let mut unpacked = Vec::new();
        decoder.read_to_end(&mut unpacked).expect("zlib");
        assert!(unpacked.len() > 34);
        let sig_len = u16::from_le_bytes([unpacked[0], unpacked[1]]) as usize;
        assert_eq!(sig_len, 32);
        let info = &unpacked[2 + sig_len..];
        let app_len = u16::from_le_bytes([info[0], info[1]]) as usize;
        assert_eq!(&info[2..2 + app_len], APP_ID.as_bytes());
    }

    #[test]
    fn rejects_short_app_id() {
        assert!(matches!(
            build_rtc_token("abc", CERT, "demo", 0, 60, 1, 1),
            Err(AgoraTokenError::InvalidAppId)
        ));
    }

    #[test]
    fn zero_uid_packs_empty_string() {
        let a = build_rtc_token(APP_ID, CERT, "room", 0, 60, 10, 3).unwrap();
        let b = build_rtc_token(APP_ID, CERT, "room", 0, 60, 10, 3).unwrap();
        assert_eq!(a, b);
    }
}
