//! Her link's shared frame cases (`core/crates/conformance/v1/link/check.json`)
//! against the host's own reading of a frame, as every end runs them: a frame
//! the core takes is one whose envelope the host reads alike, and what the
//! host asks the core (`core_check`, no roles) is what the cases say.
//! Where the host keeps its own answer is `where_the_core_and_the_host_differ`.

use super::*;

fn cases() -> Vec<Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../core/crates/conformance/v1/link/check.json");
    let file: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(file["call"], "link.check");
    file["cases"].as_array().unwrap().clone()
}

#[test]
fn every_frame_the_core_takes_the_host_reads_alike() {
    let mut read = 0;
    for case in cases() {
        let name = case["name"].as_str().unwrap();
        let Some(accepted) = case["output"].get("accepted") else {
            continue;
        };
        let frame = &case["input"]["frame"];
        let inbound: Inbound = serde_json::from_str(&frame.to_string())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(inbound.t, accepted["t"].as_str().unwrap(), "{name}");
        assert_eq!(Some(inbound.seq), frame["seq"].as_u64(), "{name}");
        assert_eq!(Some(inbound.ack), frame["ack"].as_u64(), "{name}");
        assert_eq!(inbound.id.as_deref(), frame["id"].as_str(), "{name}");
        read += 1;
    }
    assert!(read >= 30, "{read}");
}

#[test]
fn what_the_host_asks_the_core_is_what_the_cases_say() {
    let mut asked = 0;
    for case in cases() {
        let name = case["name"].as_str().unwrap();
        // The host gives no roles: a case that names some is another end's.
        if case["input"].get("roles").is_some() {
            continue;
        }
        let refused = core_check(&case["input"]["frame"])
            .err()
            .map(|refusal| refusal.code);
        let said = case["output"]["refused"]["code"].as_str();
        assert_eq!(refused.as_deref(), said, "{name}");
        asked += 1;
    }
    assert!(asked >= 15, "{asked}");
}
