//! The shared conformance files in `crates/conformance/v1/` (log and lease
//! cases) run here through the crate, and through the C entry point in
//! `myriad-core-ffi`; another platform runs the same files through its own
//! runtime. Below them, the properties the files only sample: forgetting in
//! any order, union in any order, and a takeover never leaving two minds.
//!
//! `MYRIAD_CONFORMANCE_BLESS=1 cargo test -p myriad-persona-log` rewrites
//! every case's `output` after a deliberate change of the rules.

use std::path::PathBuf;

use myriad_persona_sync::{Rev, canonical::canonical_bytes};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use super::*;
use crate::{
    lease::{HereInput, MoveInput, OwnerPlace},
    vector::{answer, holdings, missing},
};

/// The conformance files' directory.
pub(crate) fn conformance() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../conformance/v1")
}

fn blessing() -> bool {
    std::env::var_os("MYRIAD_CONFORMANCE_BLESS").is_some()
}

fn input<T: DeserializeOwned>(value: Value) -> Result<T, LogError> {
    serde_json::from_value(value).map_err(|error| LogError::new(error.to_string()))
}

fn json_of<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializes")
}

/// One call as the shared core answers it, without the C ABI.
fn run(call: &str, raw: Value) -> Result<Value, LogError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OneRow {
        row: Row,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Rows {
        rows: Vec<Row>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Vectors {
        mine: Vector,
        theirs: Vector,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Answer {
        rows: Vec<Row>,
        theirs: Vector,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Two {
        a: Grant,
        b: Grant,
    }
    Ok(match call {
        "log.canonical" => {
            let OneRow { row } = input(raw)?;
            row.check()?;
            json!({
                "row": row,
                "canonical": String::from_utf8(row.canonical_bytes()).expect("UTF-8"),
                "digest": row.digest(),
            })
        }
        "log.holdings" => {
            let Rows { rows } = input(raw)?;
            json_of(&holdings(&rows))
        }
        "log.missing" => {
            let Vectors { mine, theirs } = input(raw)?;
            json!({ "missing": missing(&mine, &theirs) })
        }
        "log.answer" => {
            let Answer { rows, theirs } = input(raw)?;
            json!({ "rows": answer(&rows, &theirs) })
        }
        "log.admit" => json_of(&admit(input(raw)?)),
        "log.merge" => json_of(&merge(input(raw)?)),
        "log.fold" => json_of(&fold(input(raw)?)?),
        "log.clock_offset" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct In {
                sent: i64,
                server_time: i64,
                received: i64,
            }
            let In {
                sent,
                server_time,
                received,
            } = input(raw)?;
            json_of(&clock::offset(sent, server_time, received)?)
        }
        "log.clock_next" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct In {
                #[serde(default)]
                last: Option<Rev>,
                now: i64,
                offset: i64,
                node: String,
            }
            let In {
                last,
                now,
                offset,
                node,
            } = input(raw)?;
            let rev = clock::next(last.as_ref(), now, offset, &node)?;
            json!({ "rev": rev, "poisoned": clock::poisoned(&rev, now, offset)? })
        }
        "log.clock_observe" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct In {
                #[serde(default)]
                last: Option<Rev>,
                seen: Rev,
                now: i64,
                offset: i64,
                node: String,
            }
            let In {
                last,
                seen,
                now,
                offset,
                node,
            } = input(raw)?;
            json!({ "last": clock::observe(last.as_ref(), &seen, now, offset, &node)? })
        }
        "log.clock_repair" => json_of(&clock::repair(input(raw)?)?),
        "lease.move" => json_of(&lease::decide(input(raw)?)?),
        "lease.here" => json_of(&lease::here(input(raw)?)?),
        "lease.newer" => {
            let Two { a, b } = input(raw)?;
            a.check()?;
            b.check()?;
            json!({ "newer": if lease::newer(&a, &b) == &a { "a" } else { "b" } })
        }
        other => return Err(LogError::new(format!("no call {other}"))),
    })
}

/// What a case expects when the call refuses its input.
pub(crate) fn refused() -> Value {
    json!({ "error": "bad_input" })
}

fn same(one: &Value, other: &Value) -> bool {
    canonical_bytes(one) == canonical_bytes(other)
}

/// Every case file under the conformance directory but `grants/`.
pub(crate) fn case_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for area in ["log", "lease"] {
        let dir = conformance().join(area);
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "json") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn every_conformance_case_answers_as_recorded() {
    let files = case_files();
    assert!(files.len() >= 10, "{files:?}");
    for path in files {
        let mut file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(file["version"], 1, "{}", path.display());
        let call = file["call"].as_str().unwrap().to_string();
        let mut changed = false;
        for case in file["cases"].as_array_mut().unwrap() {
            let name = case["name"].as_str().unwrap().to_string();
            let got = run(&call, case["input"].clone()).unwrap_or_else(|_| refused());
            if blessing() {
                changed |= case.get("output") != Some(&got);
                case["output"] = got;
                continue;
            }
            assert!(
                same(&case["output"], &got),
                "{} › {name}\nexpected {}\n     got {}",
                path.display(),
                case["output"],
                got
            );
        }
        if changed {
            let mut text = serde_json::to_string_pretty(&file).unwrap();
            text.push('\n');
            std::fs::write(&path, text).unwrap();
        }
    }
}

/// The files only say `bad_input`; each refused row is refused for the
/// reason its case is named after, not for another one.
#[test]
fn every_refused_row_is_refused_for_its_own_reason() {
    let reasons = [
        ("her row without an epoch", "carries no lease epoch"),
        ("the owner's row with an epoch", "carries no lease epoch"),
        ("a kind the log does not hold", "not one the log holds"),
        ("a node kind about a memory", "is about a node"),
        ("a shared row without dseq", "needs a dseq"),
        ("the clock is another side's", "is not the origin"),
        (
            "someone else in the owner's audience",
            "subject is not one of",
        ),
        ("a subject in her own audience", "subject is not one of"),
        ("a forget with a payload", "holds no payload"),
    ];
    let file: Value =
        serde_json::from_slice(&std::fs::read(conformance().join("log/canonical.json")).unwrap())
            .unwrap();
    let mut seen = 0;
    for case in file["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let Some(reason) = name.strip_prefix("refused: ") else {
            continue;
        };
        let (_, wanted) = reasons
            .iter()
            .find(|(named, _)| *named == reason)
            .unwrap_or_else(|| panic!("no reason listed for {name:?}"));
        let row: Row = serde_json::from_value(case["input"]["row"].clone()).unwrap();
        let error = row.check().unwrap_err().to_string();
        assert!(error.contains(wanted), "{name}: {error}");
        seen += 1;
    }
    assert_eq!(seen, reasons.len());
}

#[test]
fn the_grant_cases_are_a_stub_until_the_rules_are_in_core() {
    let file: Value =
        serde_json::from_slice(&std::fs::read(conformance().join("grants/check.json")).unwrap())
            .unwrap();
    assert_eq!(file["version"], 1);
    assert_eq!(file["status"], "pending");
    assert_eq!(file["cases"], json!([]));
}

// --- properties -----------------------------------------------------------

const OWNER: &str = "personal:o1";

fn rev(wall_ms: u64, node: &str) -> Rev {
    Rev::new(wall_ms, 0, node).unwrap()
}

fn row(id: &str, origin: &str, dseq: u64, at: u64, object: &str, kind: &str) -> Row {
    Row {
        version: LOG_VERSION,
        id: id.into(),
        origin: origin.into(),
        author: Author::Her,
        epoch: Some(1),
        dseq: Some(dseq),
        hlc: rev(at, origin),
        audience: OWNER.into(),
        subject: None,
        object: object.into(),
        kind: kind.into(),
        payload: (kind != "forget").then(|| Payload {
            reference: format!("blob:{id}"),
            key: None,
        }),
        replicate: Replicate::Shared,
        forgotten: false,
    }
}

fn owner(mut row: Row) -> Row {
    row.author = Author::Owner;
    row.epoch = None;
    row
}

fn permutations(rows: &[Row]) -> Vec<Vec<Row>> {
    if rows.len() <= 1 {
        return vec![rows.to_vec()];
    }
    let mut all = Vec::new();
    for at in 0..rows.len() {
        let mut rest = rows.to_vec();
        let first = rest.remove(at);
        for mut tail in permutations(&rest) {
            tail.insert(0, first.clone());
            all.push(tail);
        }
    }
    all
}

fn folded(rows: Vec<Row>) -> Value {
    json_of(
        &fold(FoldInput {
            rows,
            leases: Vec::new(),
        })
        .unwrap(),
    )
}

/// A node with a view, a memory about the owner, a forget of the node from
/// another side (clocked before the node's last rows), and a forget of
/// everything about the owner.
fn forgetting_log() -> Vec<Row> {
    let mut about = row("m1", "dev_mac", 3, 130, "memory:m1", "memory.write");
    about.subject = Some("o1".into());
    vec![
        row("n1", "dev_mac", 1, 100, "node:lamp", "node.created"),
        row("n2", "dev_mac", 2, 120, "node:lamp", "node.view_changed"),
        about,
        owner(row("f1", "dev_win", 1, 110, "node:lamp", "forget")),
        row("n3", "dev_mac", 4, 140, "node:lamp", "node.talked"),
        owner(row("f2", "site", 1, 90, "subject:o1", "forget")),
    ]
}

#[test]
fn a_forgotten_object_never_reads_back_in_any_order() {
    let log = forgetting_log();
    let expected = folded(log.clone());
    let objects = expected["objects"].as_array().unwrap();
    assert!(
        objects.iter().all(|object| object["forgotten"] == true),
        "{expected}"
    );
    for order in permutations(&log) {
        assert_eq!(folded(order.clone()), expected);
        // Arriving one at a time, as separate merges, ends the same way:
        // every row of the forgotten object a stub, nothing to read.
        let mut held: Vec<Row> = Vec::new();
        for one in order {
            held = merge(MergeInput {
                log: held,
                incoming: vec![one],
            })
            .log;
        }
        assert!(
            held.iter()
                .filter(|row| row.kind != "forget")
                .all(|row| row.forgotten && row.payload.is_none()),
            "{held:?}"
        );
        assert_eq!(folded(held), expected);
    }
}

#[test]
fn merging_is_a_union_in_any_order() {
    let a = vec![
        row("a1", "dev_mac", 1, 100, "node:n", "node.created"),
        row("a2", "dev_mac", 2, 200, "node:n", "node.view_changed"),
    ];
    let b = vec![
        row("b1", "site", 1, 150, "node:n", "node.talked"),
        row("a2", "dev_mac", 2, 200, "node:n", "node.view_changed"),
        owner(row("b2", "site", 2, 250, "memory:x", "memory.write")),
    ];
    let ab = merge(MergeInput {
        log: a.clone(),
        incoming: b.clone(),
    });
    let ba = merge(MergeInput {
        log: b.clone(),
        incoming: a.clone(),
    });
    assert_eq!(ab.log, ba.log);
    assert_eq!(ab.duplicates, 1);
    assert!(ab.conflicts.is_empty());
    let again = merge(MergeInput {
        log: ab.log.clone(),
        incoming: b,
    });
    assert_eq!(again.log, ab.log);
    assert!(again.added.is_empty());
    assert_eq!(
        holdings(&ab.log).vector,
        Vector::from([("dev_mac".into(), 2), ("site".into(), 2)])
    );
}

#[test]
fn what_one_side_answers_fills_the_other() {
    let mac: Vec<Row> = (1..=4)
        .map(|n| {
            row(
                &format!("m{n}"),
                "dev_mac",
                n,
                100 + n,
                "node:n",
                "node.noted",
            )
        })
        .collect();
    let mut site = vec![mac[0].clone()];
    let theirs = holdings(&site).vector;
    let lacking = missing(&theirs, &holdings(&mac).vector);
    assert_eq!(
        lacking,
        vec![Span {
            origin: "dev_mac".into(),
            from: 2,
            through: 4
        }]
    );
    let sent = answer(&mac, &theirs);
    let admitted = admit(AdmitInput {
        rows: sent,
        now: 1_000_000,
        known: Default::default(),
        members: Default::default(),
        ledger: Vec::new(),
        epochs: Default::default(),
    });
    assert!(admitted.refused.is_empty());
    site = merge(MergeInput {
        log: site,
        incoming: admitted.accepted,
    })
    .log;
    assert_eq!(holdings(&site), holdings(&mac));
}

#[test]
fn a_row_reads_the_same_whatever_order_its_fields_came_in() {
    let one = row("r1", "dev_mac", 1, 100, "node:n", "node.created");
    let mut shuffled: serde_json::Map<String, Value> = Default::default();
    let fields = one.to_value();
    for (key, value) in fields.as_object().unwrap().iter().rev() {
        shuffled.insert(key.clone(), value.clone());
    }
    let back: Row = serde_json::from_value(Value::Object(shuffled)).unwrap();
    assert_eq!(back.canonical_bytes(), one.canonical_bytes());
    assert_eq!(one.digest().len(), 32);
}

/// Whatever the times, the server takes over only once the holder has
/// stopped by itself: never two minds.
#[test]
fn a_takeover_only_happens_once_the_holder_has_stopped() {
    let minute = 60_000_000_i64;
    let lease = Grant {
        epoch: 4,
        holder: "dev_mac".into(),
        issued: rev(1, "site"),
        how: How::Handover,
        through: Some(3),
    };
    let mut granted = 0;
    for last_seen in (0..=8).map(|m| m * minute) {
        for since in (0..=8).map(|m| m * minute / 2) {
            for now in (0..=12).map(|m| m * minute) {
                // The owner was last at the Mac at most when they arrived
                // elsewhere; any time before that is as good for the proof.
                for owner_left in [since, since - minute, 0]
                    .into_iter()
                    .filter(|t| *t <= since)
                {
                    let moved = lease::decide(MoveInput {
                        lease: lease.clone(),
                        request: Request::Takeover {
                            server: "site".into(),
                        },
                        now,
                        issued: rev(2, "site"),
                        holder_last_seen: Some(last_seen),
                        holder_place: Some("home".into()),
                        owner_place: Some(OwnerPlace {
                            place: "office".into(),
                            since,
                            seen_at: now.max(since),
                        }),
                    })
                    .unwrap();
                    if let Moved::Granted(_) = moved {
                        granted += 1;
                        let at_holder = lease::here(HereInput {
                            me: "dev_mac".into(),
                            lease: Some(lease.clone()),
                            now,
                            disconnected_since: Some(last_seen),
                            owner_here_at: Some(owner_left),
                        })
                        .unwrap();
                        assert!(
                            matches!(at_holder, Here::Stopped { .. }),
                            "last seen {last_seen}, since {since}, now {now}: {at_holder:?}"
                        );
                    }
                }
            }
        }
    }
    assert!(granted > 0);
}

#[test]
fn only_a_forced_move_makes_rows_concurrent() {
    let grants = |how| {
        vec![
            Grant {
                epoch: 1,
                holder: "dev_mac".into(),
                issued: rev(10, "site"),
                how: How::First,
                through: None,
            },
            Grant {
                epoch: 2,
                holder: "dev_win".into(),
                issued: rev(200, "site"),
                how,
                through: None,
            },
        ]
    };
    let late = row("late", "dev_mac", 5, 300, "node:n", "node.view_changed");
    let early = row("early", "dev_mac", 4, 150, "node:n", "node.view_changed");
    assert_eq!(
        lease::standing(&late, &grants(How::Force)),
        lease::Stand::Concurrent
    );
    assert_eq!(
        lease::standing(&early, &grants(How::Force)),
        lease::Stand::Normal
    );
    assert_eq!(
        lease::standing(&late, &grants(How::Takeover)),
        lease::Stand::Normal
    );
    assert_eq!(
        lease::standing(&owner(late.clone()), &grants(How::Force)),
        lease::Stand::Normal
    );
    assert_eq!(
        lease::standing(&late, &grants(How::Handover)),
        lease::Stand::AfterHandover
    );
}
