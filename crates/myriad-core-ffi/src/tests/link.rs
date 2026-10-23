//! Her link's frames through the C entry point: the shared conformance
//! cases (`crates/conformance/v1/link`) and every protocol-2 example in
//! `shared/her_link_fixtures.json`.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{ffi, ok};
use crate::STATUS_BAD_INPUT;

pub(super) const TESTED: &[&str] = &["link.check", "link.encode"];

#[test]
fn every_link_case_answers_through_the_c_entry_point() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../conformance/v1/link/check.json");
    let file: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for case in file["cases"].as_array().unwrap() {
        let mut got = ok("link.check", case["input"].clone());
        // The files keep the code; the detail is for people.
        if let Some(refused) = got.get_mut("refused").and_then(Value::as_object_mut) {
            refused.remove("detail");
        }
        assert_eq!(got, case["output"], "{}", case["name"]);
    }
}

#[test]
fn every_protocol_two_example_encodes_to_its_canonical_text() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../../shared/her_link_fixtures.json")).unwrap();
    for frame in fixtures["next"]["frames"].as_array().unwrap() {
        let text = ok("link.encode", json!({ "frame": frame }))["text"]
            .as_str()
            .unwrap()
            .to_string();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, frame);
    }
    let mut bad = fixtures["next"]["frames"][0].clone();
    bad["v"] = json!(2);
    let (status, out) = ffi("link.encode", &json!({ "frame": bad }));
    assert_eq!(status, STATUS_BAD_INPUT, "{out}");
}
