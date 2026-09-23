//! HMAC-SHA256 for outbound signed credentials and inbound `/tapi` verify.

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, KeyInit, Mac};
use myriad_tapp_contract::manifest::TappRouteVerifyEncoding;
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub fn hmac_sha256(secret: &[u8], material: &[u8]) -> Vec<u8> {
    hmac_sha256_segments(secret, &[material])
}

/// HMAC over the concatenation of `segments`, fed incrementally so callers never
/// build the joined signing material.
pub fn hmac_sha256_segments(secret: &[u8], segments: &[&[u8]]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts any key length");
    for segment in segments {
        mac.update(segment);
    }
    mac.finalize().into_bytes().to_vec()
}

pub fn encode_hmac(mac: &[u8], encoding: TappRouteVerifyEncoding) -> String {
    match encoding {
        TappRouteVerifyEncoding::Hex => hex::encode(mac),
        TappRouteVerifyEncoding::Base64 => BASE64.encode(mac),
    }
}

pub fn hmac_matches(
    secret: &[u8],
    material: &[u8],
    presented: &str,
    encoding: TappRouteVerifyEncoding,
) -> bool {
    hmac_segments_match(secret, &[material], presented, encoding)
}

/// Constant-time check of `presented` against the HMAC of the concatenated
/// `segments` (see [`hmac_sha256_segments`]).
pub fn hmac_segments_match(
    secret: &[u8],
    segments: &[&[u8]],
    presented: &str,
    encoding: TappRouteVerifyEncoding,
) -> bool {
    let expected = hmac_sha256_segments(secret, segments);
    let actual = match encoding {
        TappRouteVerifyEncoding::Hex => hex::decode(presented).ok(),
        TappRouteVerifyEncoding::Base64 => BASE64.decode(presented.trim()).ok(),
    };
    match actual {
        Some(actual) if actual.len() == expected.len() => {
            bool::from(actual.as_slice().ct_eq(expected.as_slice()))
        }
        _ => {
            let _ = bool::from(expected.as_slice().ct_eq(expected.as_slice()));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_hmac, hmac_matches, hmac_sha256, hmac_sha256_segments};
    use myriad_tapp_contract::manifest::TappRouteVerifyEncoding;

    #[test]
    fn segments_match_concatenated_material() {
        let secret = b"secret";
        let joined = hmac_sha256(secret, b"POST\n/tapi/a\nbody");
        let segmented = hmac_sha256_segments(secret, &[b"POST", b"\n", b"/tapi/a\n", b"body"]);
        assert_eq!(joined, segmented);
    }

    #[test]
    fn rfc4231_case_1() {
        let key = [0x0b; 20];
        let mac = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            hex::encode(mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn rejects_wrong_hex_without_leaking_length() {
        let secret = b"secret";
        let material = b"GET\n/tapi/com.example.app/sponsors\n1\nnonce\n";
        let good = encode_hmac(&hmac_sha256(secret, material), TappRouteVerifyEncoding::Hex);
        assert!(hmac_matches(
            secret,
            material,
            &good,
            TappRouteVerifyEncoding::Hex
        ));
        assert!(!hmac_matches(
            secret,
            material,
            "00",
            TappRouteVerifyEncoding::Hex
        ));
        assert!(!hmac_matches(
            secret,
            material,
            "zzzz",
            TappRouteVerifyEncoding::Hex
        ));
    }
}
