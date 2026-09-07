//! Agora AccessToken2 (version `007`) for combined RTC/RTM conversation access.
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
pub const RTM_SERVICE_TYPE: u16 = 2;
pub const PRIVILEGE_JOIN_CHANNEL: u16 = 1;
pub const PRIVILEGE_PUBLISH_AUDIO: u16 = 2;
pub const PRIVILEGE_PUBLISH_VIDEO: u16 = 3;
pub const PRIVILEGE_PUBLISH_DATA: u16 = 4;
pub const PRIVILEGE_LOGIN: u16 = 1;

#[derive(Debug)]
pub enum AgoraTokenError {
    InvalidAppId,
    InvalidCertificate,
    InvalidUid,
    Crypto(String),
}

impl std::fmt::Display for AgoraTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAppId => write!(f, "Agora App ID is invalid"),
            Self::InvalidCertificate => write!(f, "Agora App Certificate is invalid"),
            Self::InvalidUid => write!(f, "Agora RTC/RTM user ID must be nonzero"),
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

/// Build one AccessToken2 containing both RTC and RTM services.
pub fn build_rtc_rtm_token(
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
    if app_id.len() != 32 || !app_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AgoraTokenError::InvalidAppId);
    }
    if app_certificate.len() != 32 || !app_certificate.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AgoraTokenError::InvalidCertificate);
    }
    // RTC can auto-assign UID 0, but RTM must authenticate the exact same ID.
    if uid == 0 {
        return Err(AgoraTokenError::InvalidUid);
    }

    // AccessToken2 privileges contain durations from issue_ts, not Unix timestamps.
    let privilege_expire = expire_seconds;
    let privileges = [
        (PRIVILEGE_JOIN_CHANNEL, privilege_expire),
        (PRIVILEGE_PUBLISH_AUDIO, privilege_expire),
        (PRIVILEGE_PUBLISH_VIDEO, privilege_expire),
        (PRIVILEGE_PUBLISH_DATA, privilege_expire),
    ];

    let uid_str = uid.to_string();

    let mut rtc_service = Vec::new();
    pack_u16(&mut rtc_service, RTC_SERVICE_TYPE);
    pack_map_u32(&mut rtc_service, &privileges);
    pack_string(&mut rtc_service, channel);
    pack_string(&mut rtc_service, &uid_str);

    // AccessToken2 RTM service layout is service type, login privilege map,
    // then the RTM user ID. The browser uses the same numeric UID for RTC/RTM.
    let mut rtm_service = Vec::new();
    pack_u16(&mut rtm_service, RTM_SERVICE_TYPE);
    pack_map_u32(&mut rtm_service, &[(PRIVILEGE_LOGIN, privilege_expire)]);
    pack_string(&mut rtm_service, &uid_str);

    let mut info = Vec::new();
    pack_string(&mut info, app_id);
    pack_u32(&mut info, issue_ts);
    pack_u32(&mut info, expire_seconds);
    pack_u32(&mut info, salt);
    pack_u16(&mut info, 2);
    info.extend_from_slice(&rtc_service);
    info.extend_from_slice(&rtm_service);

    let mut ts_bytes = Vec::new();
    pack_u32(&mut ts_bytes, issue_ts);
    let signing = hmac_sha256(&ts_bytes, app_certificate.as_bytes())?;
    let mut salt_bytes = Vec::new();
    pack_u32(&mut salt_bytes, salt);
    let signing = hmac_sha256(&salt_bytes, &signing)?;
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

pub fn build_rtc_rtm_token_now(
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
    build_rtc_rtm_token(
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

    fn unpack_info(token: &str) -> Vec<u8> {
        let raw = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &token[3..])
            .expect("base64");
        let mut decoder = ZlibDecoder::new(raw.as_slice());
        let mut unpacked = Vec::new();
        decoder.read_to_end(&mut unpacked).expect("zlib");
        let sig_len = u16::from_le_bytes([unpacked[0], unpacked[1]]) as usize;
        unpacked[2 + sig_len..].to_vec()
    }

    fn take_u16(input: &[u8], cursor: &mut usize) -> u16 {
        let value = u16::from_le_bytes([input[*cursor], input[*cursor + 1]]);
        *cursor += 2;
        value
    }

    fn take_u32(input: &[u8], cursor: &mut usize) -> u32 {
        let value = u32::from_le_bytes([
            input[*cursor],
            input[*cursor + 1],
            input[*cursor + 2],
            input[*cursor + 3],
        ]);
        *cursor += 4;
        value
    }

    fn take_string(input: &[u8], cursor: &mut usize) -> String {
        let length = take_u16(input, cursor) as usize;
        let value = String::from_utf8(input[*cursor..*cursor + length].to_vec()).unwrap();
        *cursor += length;
        value
    }

    fn skip_privileges(input: &[u8], cursor: &mut usize) -> Vec<(u16, u32)> {
        let count = take_u16(input, cursor);
        (0..count)
            .map(|_| (take_u16(input, cursor), take_u32(input, cursor)))
            .collect()
    }

    #[test]
    fn token_is_versioned_zlib_base64() {
        let token =
            build_rtc_rtm_token(APP_ID, CERT, "demo", 123, 3600, 1_700_000_000, 42).unwrap();
        assert!(token.starts_with("007"), "{token}");
        let info = unpack_info(&token);
        let app_len = u16::from_le_bytes([info[0], info[1]]) as usize;
        assert_eq!(&info[2..2 + app_len], APP_ID.as_bytes());
    }

    #[test]
    fn combined_token_packs_rtc_and_rtm_services() {
        let token =
            build_rtc_rtm_token(APP_ID, CERT, "room", 123, 3600, 1_700_000_000, 42).unwrap();
        let info = unpack_info(&token);
        let mut cursor = 0;
        assert_eq!(take_string(&info, &mut cursor), APP_ID);
        assert_eq!(take_u32(&info, &mut cursor), 1_700_000_000);
        assert_eq!(take_u32(&info, &mut cursor), 3600);
        assert_eq!(take_u32(&info, &mut cursor), 42);
        assert_eq!(take_u16(&info, &mut cursor), 2);

        assert_eq!(take_u16(&info, &mut cursor), RTC_SERVICE_TYPE);
        assert_eq!(skip_privileges(&info, &mut cursor).len(), 4);
        assert_eq!(take_string(&info, &mut cursor), "room");
        assert_eq!(take_string(&info, &mut cursor), "123");

        assert_eq!(take_u16(&info, &mut cursor), RTM_SERVICE_TYPE);
        assert_eq!(
            skip_privileges(&info, &mut cursor),
            vec![(PRIVILEGE_LOGIN, 3600)]
        );
        assert_eq!(take_string(&info, &mut cursor), "123");
        assert_eq!(cursor, info.len());
    }

    #[test]
    fn rejects_short_app_id() {
        assert!(matches!(
            build_rtc_rtm_token("abc", CERT, "demo", 0, 60, 1, 1),
            Err(AgoraTokenError::InvalidAppId)
        ));
    }

    #[test]
    fn combined_transport_rejects_auto_assigned_uid() {
        assert!(matches!(
            build_rtc_rtm_token(APP_ID, CERT, "room", 0, 60, 10, 3),
            Err(AgoraTokenError::InvalidUid)
        ));
    }

    #[test]
    fn signature_matches_agora_official_node_reference() {
        // Generated with AgoraIO/Tools DynamicKey/AgoraDynamicKey/nodejs/src/AccessToken2.js:
        // RTC publisher privileges 1..4 + RTM login, all TTLs 3600; issue/salt fixed below.
        // Compare decompressed bytes: zlib encoders need not emit identical streams.
        let reference = "007eJxTYChZ6W58t+78nqCwzsulT6ys5kqyzuiV0JI8JJIVrbb4kpUCg6W5QXKisWlKqplBsomJmYlpUlJiqkWikaGpgZlhkrExw8fgVAE+BgYtBgYGJgZGBhYgBvGZwCQzmGSBkkX5+bnMDIZGxiCFjFAFQC4AQqkcSA==";
        let actual =
            build_rtc_rtm_token(APP_ID, CERT, "room", 123, 3600, 1_700_000_000, 42).unwrap();
        fn unpack(token: &str) -> Vec<u8> {
            let raw =
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &token[3..])
                    .unwrap();
            let mut bytes = Vec::new();
            ZlibDecoder::new(raw.as_slice())
                .read_to_end(&mut bytes)
                .unwrap();
            bytes
        }
        assert_eq!(unpack(&actual), unpack(reference));
    }
}
