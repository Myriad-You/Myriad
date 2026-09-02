//! Shared HMAC-SHA256 helper for outbound signed credentials and inbound `/tapi` verify.

pub use myriad_tapp_rules::{encode_hmac, hmac_matches, hmac_sha256};

#[cfg(test)]
mod tests {
    use super::{encode_hmac, hmac_matches, hmac_sha256};
    use myriad_tapp_contract::manifest::TappRouteVerifyEncoding;

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
