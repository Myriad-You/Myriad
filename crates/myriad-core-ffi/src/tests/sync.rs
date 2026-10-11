//! Her data across sides: the golden files of `myriad-persona-sync` through
//! the C entry point, byte for byte, as another platform runs them.

use base64::Engine as _;
use myriad_persona_sync::{EntityVersion, Kind, Rev, canonicalize, compare, hlc};
use serde_json::{Value, json};

use super::{ffi, ok};
use crate::STATUS_BAD_INPUT;

pub(super) const TESTED: &[&str] = &[
    "sync.affect_fold",
    "sync.affect_replay",
    "sync.canonicalize",
    "sync.check_rev",
    "sync.compare",
    "sync.hlc_next",
    "sync.hlc_observe",
    "sync.is_empty",
    "sync.legacy_rev",
    "sync.persona_diff",
    "sync.rig_package_id",
];

macro_rules! fixture {
    ($path:literal) => {
        include_bytes!(concat!("../../../myriad-persona-sync/fixtures/", $path))
    };
}

const ENTITIES: &[(Kind, &[u8])] = &[
    (Kind::AffectAnchor, fixture!("entities/affect_anchor.json")),
    (Kind::AffectEvent, fixture!("entities/affect_event.json")),
    (Kind::Diary, fixture!("entities/diary.json")),
    (Kind::Memory, fixture!("entities/memory.json")),
    (Kind::Outfit, fixture!("entities/outfit.json")),
    (Kind::Persona, fixture!("entities/persona.json")),
    (
        Kind::SerialFollowing,
        fixture!("entities/serial_following.json"),
    ),
    (Kind::SerialPast, fixture!("entities/serial_past.json")),
    (Kind::SerialShelf, fixture!("entities/serial_shelf.json")),
    (Kind::SerialWork, fixture!("entities/serial_work.json")),
    (Kind::SoupGame, fixture!("entities/soup_game.json")),
    (Kind::Sticker, fixture!("entities/sticker.json")),
    (Kind::VitalsDay, fixture!("entities/vitals_day.json")),
];

fn golden(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes.strip_suffix(b"\n").unwrap_or(bytes)).unwrap()
}

#[test]
fn every_golden_entity_comes_back_in_its_own_bytes() {
    assert_eq!(ENTITIES.len(), Kind::ALL.len());
    let rough: Vec<Value> = ENTITIES
        .iter()
        .map(|(_, bytes)| {
            let mut entity: Value = serde_json::from_slice(bytes).unwrap();
            // A platform sends no digest; the core fills it in.
            entity["digest"] = json!("");
            entity
        })
        .collect();
    let out = ok("sync.canonicalize", json!({ "entities": rough }));
    let out = out["entities"].as_array().unwrap();
    for ((kind, bytes), got) in ENTITIES.iter().zip(out) {
        assert_eq!(got["canonical"].as_str().unwrap(), golden(bytes), "{kind}");
        let direct = canonicalize(serde_json::from_slice::<EntityVersion>(bytes).unwrap()).unwrap();
        assert_eq!(got["entity"], serde_json::to_value(&direct).unwrap());
    }
}

#[test]
fn an_entity_that_breaks_the_format_is_bad_input() {
    let mut memory: Value = serde_json::from_slice(ENTITIES[3].1).unwrap();
    memory["body"]["audience"]["members"] = json!(["7"]);
    let (status, out) = ffi("sync.canonicalize", &json!({ "entities": [memory] }));
    assert_eq!(status, STATUS_BAD_INPUT);
    assert_eq!(out["error"]["kind"], "bad_input");
    let (status, _) = ffi(
        "sync.hlc_next",
        &json!({ "last": "nope", "now": 0, "node": "site" }),
    );
    assert_eq!(status, STATUS_BAD_INPUT);
}

#[test]
fn the_golden_comparison_comes_back_the_same() {
    let local: Value = serde_json::from_slice(fixture!("compare/local.json")).unwrap();
    let remote: Value = serde_json::from_slice(fixture!("compare/remote.json")).unwrap();
    let out = ok(
        "sync.compare",
        json!({ "local": local.clone(), "remote": remote.clone() }),
    );
    let want: Value = serde_json::from_slice(fixture!("compare/comparison.json")).unwrap();
    assert_eq!(out, want);
    let direct = compare(
        serde_json::from_value(local).unwrap(),
        serde_json::from_value(remote).unwrap(),
    )
    .unwrap();
    assert_eq!(out, serde_json::to_value(direct).unwrap());
    assert_eq!(out["needsSubject"], true);
}

#[test]
fn empty_is_no_persona_and_nothing_held() {
    let empty = |input: Value| ok("sync.is_empty", input)["empty"].as_bool().unwrap();
    assert!(empty(
        json!({ "persona": false, "memories": 0, "diary": 0, "affectEvents": 0 })
    ));
    assert!(!empty(
        json!({ "persona": true, "memories": 0, "diary": 0, "affectEvents": 0 })
    ));
    assert!(!empty(
        json!({ "persona": false, "memories": 0, "diary": 1, "affectEvents": 0 })
    ));
    let (status, _) = ffi("sync.is_empty", &json!({ "persona": false }));
    assert_eq!(status, STATUS_BAD_INPUT);
}

#[test]
fn the_clock_matches_the_crate() {
    let node = "dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60";
    let mut last: Option<String> = None;
    for now in [
        1_760_000_000_123_456_i64,
        1_760_000_000_123_999,
        1_700_000_000_000_000,
    ] {
        let got = ok(
            "sync.hlc_next",
            json!({ "last": last, "now": now, "node": node }),
        )["rev"]
            .as_str()
            .unwrap()
            .to_string();
        let parsed = last.as_deref().map(|raw| raw.parse::<Rev>().unwrap());
        assert_eq!(
            got,
            hlc::next(parsed.as_ref(), now, node).unwrap().to_string()
        );
        last = Some(got);
    }
    assert_eq!(
        last.as_deref(),
        Some("001760000000123-000002-dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60")
    );
    let seen = "001760000000500-000007-site";
    let observed = ok(
        "sync.hlc_observe",
        json!({ "last": last, "seen": seen, "now": 1_760_000_000_200_000_i64, "node": node }),
    );
    assert_eq!(
        observed["last"],
        "001760000000500-000008-dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60"
    );
    assert_eq!(
        ok(
            "sync.legacy_rev",
            json!({ "updatedAt": 1_760_000_000_123_456_i64, "origin": null })
        )["rev"],
        "001760000000123-000000-site"
    );
}

#[test]
fn a_revision_too_far_ahead_of_the_site_is_not_accepted() {
    let now = 1_760_000_000_000_000_i64;
    let check = |rev: &str| ok("sync.check_rev", json!({ "rev": rev, "now": now }));
    let fine = check("001760000300000-000009-dev_a");
    assert_eq!(
        fine,
        json!({ "accepted": true, "aheadMs": 300_000, "maxAheadMs": 300_000 })
    );
    let ahead = check("001760000300001-000000-dev_a");
    assert_eq!(
        ahead,
        json!({ "accepted": false, "aheadMs": 300_001, "maxAheadMs": 300_000 })
    );
    let rev: Rev = "001760000300001-000000-dev_a".parse().unwrap();
    assert_eq!(
        ahead["accepted"],
        json!(hlc::check_ahead(&rev, now).is_ok())
    );
    assert_eq!(check("001759999000000-000000-site")["aheadMs"], 0);
    let (status, _) = ffi("sync.check_rev", &json!({ "rev": "nope", "now": now }));
    assert_eq!(status, STATUS_BAD_INPUT);
}

#[test]
fn opaque_persona_keys_and_unreadable_pictures_in_the_golden_comparison() {
    let want: Value = serde_json::from_slice(fixture!("compare/comparison.json")).unwrap();
    let fields: Vec<&str> = want["persona"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field["field"].as_str().unwrap())
        .collect();
    for opaque in ["agentNote", "displayName", "draftSource"] {
        assert!(!fields.contains(&opaque), "{opaque}");
    }
    assert_eq!(
        want["unreadablePictures"],
        json!({
            "local": [],
            "remote": [{ "field": "image", "id": "stk_gone", "kind": "sticker" }],
        })
    );
}

#[test]
fn the_golden_rig_has_its_golden_id() {
    let manifest: Value = serde_json::from_slice(fixture!("rig/manifest.json")).unwrap();
    let atlas = base64::engine::general_purpose::STANDARD.encode(b"atlas-pixels");
    let out = ok(
        "sync.rig_package_id",
        json!({ "atlas": atlas, "manifest": manifest }),
    );
    assert_eq!(out["id"], golden(fixture!("rig/package-id.txt")));
    let (status, _) = ffi(
        "sync.rig_package_id",
        &json!({ "atlas": "%%%", "manifest": manifest }),
    );
    assert_eq!(status, STATUS_BAD_INPUT);
}

/// The golden affect event, replayed from the golden anchor through the C
/// entry, is what the crate gives; folding it in and replaying the rest is
/// the same.
#[test]
fn her_affect_is_replayed_and_folded_as_the_crate_does() {
    let anchor: EntityVersion =
        serde_json::from_slice(fixture!("entities/affect_anchor.json")).unwrap();
    let event: EntityVersion =
        serde_json::from_slice(fixture!("entities/affect_event.json")).unwrap();
    let mut change = event.body.clone();
    change["id"] = json!(event.id);
    let now = 1_760_003_600_400_000_i64;
    let baseline = json!({ "mood": 70.0, "arousal": 48.0 });
    let replayed = ok(
        "sync.affect_replay",
        json!({ "anchor": anchor.body, "events": [change], "now": now, "baselineNow": baseline }),
    );
    let expected = myriad_persona_sync::affect::replay(
        &serde_json::from_value(anchor.body.clone()).unwrap(),
        &[serde_json::from_value(change.clone()).unwrap()],
        now,
        serde_json::from_value(baseline.clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(replayed["affect"], serde_json::to_value(expected).unwrap());
    let folded = ok(
        "sync.affect_fold",
        json!({ "anchor": anchor.body, "events": [change], "upto": now }),
    );
    assert_eq!(folded["anchor"]["throughEvent"], json!(event.id));
    let again = ok(
        "sync.affect_replay",
        json!({ "anchor": folded["anchor"], "events": [change], "now": now, "baselineNow": baseline }),
    );
    assert_eq!(again, replayed);
    // Unknown fields of the call itself are refused.
    let (status, _) = ffi(
        "sync.affect_replay",
        &json!({ "anchor": anchor.body, "now": now, "baselineNow": baseline, "extra": 1 }),
    );
    assert_eq!(status, STATUS_BAD_INPUT);
}

#[test]
fn a_persona_write_is_diffed_field_by_field() {
    let diff = ok(
        "sync.persona_diff",
        json!({
            "before": { "name": "Merope", "summary": "住在站点上的她。" },
            "after": { "name": "Merope", "summary": "搬到了海边。", "zone": "Asia/Tokyo" },
            "revs": {},
            "legacyRev": "001760000000000-000000-site",
            "rev": "001760000005000-000001-dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60",
            "origin": "dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60",
        }),
    );
    assert_eq!(diff["changed"], json!(["summary", "zone"]));
    assert_eq!(diff["history"][0]["value"], "住在站点上的她。");
    assert_eq!(diff["history"][0]["rev"], "001760000000000-000000-site");
    assert_eq!(diff["history"][0]["reason"], "edit");
    assert_eq!(
        diff["revs"]["zone"]["origin"],
        "dev_8c1e0d5a6f2b4c3d9e7a1b2c3d4e5f60"
    );
}
