//! The consistency vectors every client runs (fixtures/host-filter
//! `pages.json`, `terminal.json`): here straight from the crate, in
//! `myriad-core-ffi` through the C entry point.

use myriad_perception_filter::{
    Classify, Decision, PageKind, Private, Relay, Sources, decide, page, secrets,
};
use serde::Deserialize;
use serde_json::{Value, json};

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/fixtures/host-filter/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageCase {
    id: String,
    url_host: String,
    path: Option<String>,
    title: Option<String>,
    #[serde(default)]
    password_field: bool,
    market: Option<String>,
    expect: Value,
    decide: Value,
}

fn name<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn pages_place_and_decide_as_the_vectors_say() {
    let doc = fixture("pages");
    let table: Sources = serde_json::from_value(doc["table"].clone()).unwrap();
    let cases: Vec<PageCase> = serde_json::from_value(doc["cases"].clone()).unwrap();
    assert!(cases.len() >= 20);
    for case in cases {
        let placed = page(
            &case.url_host,
            case.path.as_deref(),
            case.title.as_deref(),
            case.password_field,
            case.market.as_deref(),
            &table,
        );
        let got = json!({
            "read": placed.read(),
            "page": name(placed.kind),
            "basis": name(placed.basis),
            "kind": name(placed.source),
            "category": placed.category,
            "othersWords": placed.others_words(),
        });
        assert_eq!(got, case.expect, "{}", case.id);

        let messaging = placed.kind == PageKind::Messaging;
        let decision = Decision {
            capture: Some("seen".into()),
            window: Some("seen".into()),
            password_field: Some(case.password_field),
            identifiers: Some(vec![]),
            source: Some(name(placed.source).as_str().map(str::to_string)),
            page: name(placed.kind).as_str().map(str::to_string),
            private: Some(Private::Known(false)),
            text_hits: Some(vec![]),
            model: Some("available".into()),
            language: Some("supported".into()),
            relay: Some(messaging.then(|| Relay {
                result: "ok".into(),
                category: Some("plans".into()),
            })),
            classify: Some((!messaging).then(|| Classify {
                general: Some("not_sensitive".into()),
                tagging: Some("not_sensitive".into()),
                kind: None,
            })),
        };
        let decided = name(decide(&decision));
        for field in ["disposition", "kind", "reason", "leaves"] {
            assert_eq!(
                decided[field],
                case.decide.get(field).cloned().unwrap_or(Value::Null),
                "{} {field}",
                case.id
            );
        }
        // A page not read leaves nothing, not even the marker.
        if !placed.read() {
            assert_eq!(decided["leaves"], "nothing", "{}", case.id);
        }
    }
}

#[test]
fn terminal_credentials_are_wiped_as_the_vectors_say() {
    let doc = fixture("terminal");
    for case in doc["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let text = case["text"].as_str().unwrap();
        let scan = secrets::scan(text);
        assert_eq!(name(&scan.wiped), case["wiped"], "{id}: {}", scan.text);
        for gone in case["gone"].as_array().unwrap() {
            assert!(!scan.text.contains(gone.as_str().unwrap()), "{id}");
        }
        for kept in case["kept"].as_array().unwrap() {
            assert!(
                scan.text.contains(kept.as_str().unwrap()),
                "{id}: {}",
                scan.text
            );
        }
        if case["wiped"].as_array().unwrap().is_empty() {
            assert_eq!(scan.text, text, "{id}");
        }
    }
}
