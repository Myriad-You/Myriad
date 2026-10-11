//! Canonical JSON: the one byte form a value has on every side.
//!
//! Object keys in byte order of their UTF-8, no whitespace, strings escaped
//! as `serde_json` escapes them, numbers in their shortest round-trip form.
//! The rig package id was the first thing hashed this way
//! ([`crate::rig_package_id`]); entity digests use the same writer.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Appends the canonical bytes of `value` to `output`.
pub fn write_canonical_json(value: &Value, output: &mut Vec<u8>) -> serde_json::Result<()> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(value) => output.extend_from_slice(if *value { b"true" } else { b"false" }),
        Value::Number(value) => output.extend_from_slice(value.to_string().as_bytes()),
        Value::String(_) => serde_json::to_writer(output, value)?,
        Value::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical_json(value, output)?;
            }
            output.push(b']');
        }
        Value::Object(values) => {
            output.push(b'{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key)?;
                output.push(b':');
                write_canonical_json(&values[key], output)?;
            }
            output.push(b'}');
        }
    }
    Ok(())
}

/// The canonical bytes of `value`.
pub fn canonical_bytes(value: &Value) -> Vec<u8> {
    let mut output = Vec::new();
    // Writing a `Value` into a `Vec` only fails on I/O, which a `Vec` never does.
    write_canonical_json(value, &mut output).expect("writing JSON into memory");
    output
}

/// The canonical bytes of anything that serializes to JSON.
pub fn canonical_of<T: Serialize>(value: &T) -> serde_json::Result<Vec<u8>> {
    Ok(canonical_bytes(&serde_json::to_value(value)?))
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// sha256 of `bytes`, in hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// The digest an entity carries: the first 16 bytes of sha256 of its
/// canonical body, in hex (32 characters). Used to compare and to make
/// writes idempotent, never as a secret.
pub fn digest(body: &Value) -> String {
    hex(&Sha256::digest(canonical_bytes(body))[..16])
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn keys_sort_by_bytes_and_nothing_else_moves() {
        let value = json!({
            "b": [3, {"z": null, "a": true}],
            "a": "é\"\n",
            "B": 1.5,
            "aa": -0.0,
        });
        assert_eq!(
            String::from_utf8(canonical_bytes(&value)).unwrap(),
            r#"{"B":1.5,"a":"é\"\n","aa":-0.0,"b":[3,{"a":true,"z":null}]}"#
        );
    }

    #[test]
    fn numbers_take_their_shortest_round_trip_form() {
        let value: Value = serde_json::from_str("[0.1, 1e3, 1.0, 41.300000000000004, 7]").unwrap();
        assert_eq!(
            String::from_utf8(canonical_bytes(&value)).unwrap(),
            "[0.1,1000.0,1.0,41.300000000000004,7]"
        );
    }

    #[test]
    fn digest_is_sixteen_bytes_of_the_canonical_body() {
        let one = json!({"a": 1, "b": 2});
        let other: Value = serde_json::from_str(r#"{ "b": 2, "a": 1 }"#).unwrap();
        assert_eq!(digest(&one), digest(&other));
        assert_eq!(digest(&one).len(), 32);
        assert_eq!(
            digest(&one),
            sha256_hex(br#"{"a":1,"b":2}"#)[..32].to_string()
        );
    }
}
