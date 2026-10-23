//! The host filter: through the C entry point and straight from the crate.

use myriad_perception_filter::{
    self as filter, Decision, RULES, SHAPE, Sources, decide, identifiers, secrets,
};
use serde_json::{Value, json};

use super::{STATUS_BAD_INPUT, ffi, ok};

pub(super) const TESTED: &[&str] = &[
    "filter.check_relay",
    "filter.decide",
    "filter.lists",
    "filter.page",
    "filter.scan_identifiers",
    "filter.scan_secrets",
    "filter.source",
    "filter.text_hits",
    "filter.window",
];

/// A key-shaped value, spliced so no literal key sits in the repository.
fn key() -> String {
    ["gh", "p_", "Zq8RkT3vNw7YbXc2Lm5Pd9Hs4Jf6Ga1Ke0Ut"].concat()
}

#[test]
fn scans_match_the_crate() {
    let text = format!("export GITHUB_TOKEN={} 卡号 4111 1111 1111 1111", key());
    let out = ok("filter.scan_secrets", json!({ "text": text }));
    let scan = secrets::scan(&text);
    assert_eq!(out["shape"], SHAPE);
    assert_eq!(out["text"], scan.text);
    // The prefix wipes the value first; the assignment then sees only the mark.
    assert_eq!(out["wiped"], json!([{ "kind": "vendor_prefix" }]));
    assert!(!out.to_string().contains(&key()));

    let after = out["text"].as_str().unwrap();
    let ids = ok(
        "filter.scan_identifiers",
        json!({ "text": after, "shape": 1 }),
    );
    assert_eq!(ids["text"], identifiers::scan(after).text);
    assert_eq!(ids["wiped"], json!([{ "kind": "card" }]));
    assert!(!ids.to_string().contains("4111"));
}

#[test]
fn window_source_text_and_relay_match_the_crate() {
    assert_eq!(
        ok(
            "filter.window",
            json!({ "bundleId": "com.apple.Passwords", "title": "All" })
        ),
        json!({ "withheld": "password_app", "shape": 1 })
    );
    assert_eq!(
        ok("filter.window", json!({ "title": "vim .env.local" }))["withheld"],
        "key_file"
    );
    let long = "长".repeat(200);
    let seen = ok("filter.window", json!({ "title": format!("a\n{long}") }));
    let filter::Window::Seen(expected) =
        filter::window(None, &format!("a\n{long}"), Sources::built_in())
    else {
        panic!("withheld");
    };
    assert_eq!(seen["seen"], expected);

    assert_eq!(
        ok("filter.source", json!({ "bundleId": "com.apple.mail" }))["kind"],
        "messaging"
    );
    assert_eq!(
        ok(
            "filter.source",
            json!({ "urlHost": "www.dbs.com.sg", "market": "sg" })
        )["kind"],
        "finance"
    );
    assert_eq!(
        ok(
            "filter.source",
            json!({ "urlHost": "www.dbs.com.sg", "market": "jp" })
        )["kind"],
        Value::Null
    );
    // No market: every market's sites count.
    assert_eq!(
        ok("filter.source", json!({ "urlHost": "smbc.co.jp" }))["kind"],
        "finance"
    );
    let table = json!({ "hosts": { "clinic.example": "health" } });
    assert_eq!(
        ok(
            "filter.source",
            json!({ "urlHost": "a.clinic.example", "table": table })
        )["kind"],
        "health"
    );

    let page = "本期账单 ¥3,280.00 最低还款 ¥328.00";
    assert_eq!(
        ok("filter.text_hits", json!({ "text": page }))["hits"],
        serde_json::to_value(filter::text_hits(page)).unwrap()
    );
    assert_eq!(
        ok("filter.text_hits", json!({ "text": page }))["hits"],
        json!([{ "kind": "finance", "rule": "finance.amount" }])
    );

    let message = "周五晚上 7 点在老地方吃火锅，订金 300 元我先付了";
    assert_eq!(
        ok(
            "filter.check_relay",
            json!({ "source": message, "who": "小周", "topic": "约饭" })
        ),
        json!({ "ok": true, "shape": 1 })
    );
    assert_eq!(
        ok(
            "filter.check_relay",
            json!({ "source": message, "who": "小周", "topic": "订金300" })
        ),
        json!({ "ok": false, "reason": "digits", "shape": 1 })
    );
}

#[test]
fn decide_matches_the_crate_and_names_its_rules() {
    let input = json!({
        "capture": "seen", "window": "seen", "identifiers": [], "source": null,
        "private": "unknown", "textHits": [], "model": "available", "language": "supported",
        "relay": null, "classify": { "general": "not_sensitive", "tagging": "not_sensitive" },
    });
    let out = ok("filter.decide", input.clone());
    let decision: Decision = serde_json::from_value(input).unwrap();
    let mut expected = serde_json::to_value(decide(&decision)).unwrap();
    expected["rules"] = json!(RULES);
    expected["shape"] = json!(SHAPE);
    assert_eq!(out, expected);
    assert_eq!(out["disposition"], "carry");
    assert_eq!(out["leaves"], "scrubbed_text");

    // Nothing given: closed, not refused.
    let out = ok("filter.decide", json!({}));
    assert_eq!(out["disposition"], "blind");
    assert_eq!(out["missing"], "capture");

    let lists = ok("filter.lists", json!({}));
    assert_eq!(lists["classifyAsks"], json!(["general", "tagging"]));
    assert_eq!(lists["presentOnlyLeavesKind"], false);
    assert_eq!(lists["rules"], RULES);
    assert!(
        lists["captureExcluded"]
            .as_array()
            .unwrap()
            .contains(&json!("com.apple.keychainaccess"))
    );
}

/// Errors are payloads too: a refusal never quotes the text it was given.
#[test]
fn refusals_never_quote_the_input() {
    let secret = key();
    for input in [
        json!({ "text": 42, "note": secret }),
        json!({ "text": [secret] }),
        json!({ "title": { "x": secret } }),
        json!({ "capture": secret, "private": [secret] }),
        json!({ "shape": 2, "text": secret }),
    ] {
        for call in ["filter.scan_secrets", "filter.window", "filter.decide"] {
            let (status, out) = ffi(call, &input);
            assert_eq!(status, STATUS_BAD_INPUT, "{call} {out}");
            assert!(!out.to_string().contains(&secret), "{call}: {out}");
        }
    }
    let (status, out) = ffi("filter.scan_secrets", &json!({ "shape": 2, "text": "x" }));
    assert_eq!(status, STATUS_BAD_INPUT);
    assert_eq!(
        out["error"]["message"],
        "this core speaks filter shape 1 only"
    );
    let (_, out) = ffi("filter.scan_secrets", &json!({ "txet": "x" }));
    assert_eq!(out["error"]["message"], "unknown field `txet`");
}

fn vectors(name: &str) -> Value {
    let path = format!(
        "{}/../myriad-perception-filter/fixtures/host-filter/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// The consistency vectors, through the C entry point as a client runs them.
#[test]
fn page_and_decide_vectors_hold_through_the_core() {
    let doc = vectors("pages");
    for case in doc["cases"].as_array().unwrap() {
        let id = &case["id"];
        let mut input = json!({ "urlHost": case["urlHost"], "table": doc["table"] });
        for field in ["path", "title", "passwordField", "market"] {
            if let Some(value) = case.get(field) {
                input[field] = value.clone();
            }
        }
        let mut placed = ok("filter.page", input.clone());
        assert_eq!(placed["shape"], SHAPE);
        placed.as_object_mut().unwrap().remove("shape");
        assert_eq!(placed, case["expect"], "{id}");
        // Nothing of the host, path or title comes back.
        for field in ["urlHost", "path", "title"] {
            if let Some(text) = case[field].as_str() {
                assert!(!placed.to_string().contains(text), "{id} {field}");
            }
        }
        let messaging = placed["page"] == "messaging";
        let decided = ok(
            "filter.decide",
            json!({
                "capture": "seen", "window": "seen",
                "passwordField": case.get("passwordField").cloned().unwrap_or(json!(false)),
                "identifiers": [], "source": placed["kind"], "page": placed["page"],
                "private": false, "textHits": [], "model": "available", "language": "supported",
                "relay": if messaging { json!({ "result": "ok", "category": "plans" }) } else { Value::Null },
                "classify": if messaging { Value::Null } else { json!({ "general": "not_sensitive", "tagging": "not_sensitive" }) },
            }),
        );
        for field in ["disposition", "kind", "reason", "leaves"] {
            assert_eq!(
                decided.get(field).cloned().unwrap_or(Value::Null),
                case["decide"].get(field).cloned().unwrap_or(Value::Null),
                "{id} {field}"
            );
        }
    }
    // The built-in table, with no `table`.
    let built_in = ok(
        "filter.page",
        json!({ "urlHost": "en.wikipedia.org", "path": "/wiki/Rust" }),
    );
    assert_eq!(
        (built_in["read"].clone(), built_in["category"].clone()),
        (json!(true), json!("reference"))
    );
}

#[test]
fn terminal_vectors_hold_through_the_core() {
    let doc = vectors("terminal");
    for case in doc["cases"].as_array().unwrap() {
        let out = ok("filter.scan_secrets", json!({ "text": case["text"] }));
        let kinds: Vec<Value> = out["wiped"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["kind"].clone())
            .collect();
        assert_eq!(json!(kinds), case["wiped"], "{}", case["id"]);
        let text = out["text"].as_str().unwrap();
        for gone in case["gone"].as_array().unwrap() {
            assert!(!text.contains(gone.as_str().unwrap()), "{}", case["id"]);
        }
        for kept in case["kept"].as_array().unwrap() {
            assert!(text.contains(kept.as_str().unwrap()), "{}", case["id"]);
        }
    }
}
