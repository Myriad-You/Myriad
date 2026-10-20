//! The synthetic corpus (fixtures/host-filter, copied from the native
//! client's `Tests/MyriadCoreTests/Fixtures/HostFilter`): every item's host
//! annotation is what the v1 rules must find, and its expected disposition
//! is what the real rules plus the recorded on-device model answers must
//! reach.
//!
//! `cargo test -p myriad-perception-filter --test corpus -- --nocapture`
//! prints the report in the form the native design doc uses.

mod synthetic;

use std::collections::{BTreeMap, BTreeSet};

use myriad_perception_filter::{
    Classify, Decision, Disposition, Private, Relay, RelayCheck, SourceKind, Sources, check_relay,
    decide, identifiers, secrets, text, text_hits, window,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    lang: String,
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
    bundle: String,
    title: String,
    text: String,
    expect: String,
    url: Option<String>,
    host: Vec<String>,
    private: Option<bool>,
    disputed: Option<String>,
}

#[derive(Deserialize)]
struct Replay {
    rows: BTreeMap<String, Recorded>,
}

#[derive(Deserialize)]
struct Recorded {
    language: Option<String>,
    general: Option<String>,
    tagging: Option<String>,
    relay: Option<RecordedRelay>,
}

#[derive(Deserialize)]
struct RecordedRelay {
    error: bool,
    who: String,
    topic: String,
    category: String,
}

const LANGUAGES: [&str; 4] = ["zh-Hans", "ja", "en", "mixed"];

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/fixtures/host-filter/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{path}"))
}

fn corpus() -> Vec<(String, Item)> {
    LANGUAGES
        .iter()
        .flat_map(|lang| {
            let file: File =
                serde_json::from_str(&fixture(&format!("host-filter-{lang}"))).unwrap();
            assert_eq!(&file.lang, lang);
            file.items
                .into_iter()
                .map(move |item| (lang.to_string(), item))
        })
        .collect()
}

fn sources() -> Sources {
    serde_json::from_str(&fixture("host-filter-sources")).unwrap()
}

fn replay() -> Replay {
    serde_json::from_str(&fixture("model-replay")).unwrap()
}

fn url_host(url: &str) -> Option<&str> {
    let rest = &url[url.find("://")? + 3..];
    Some(rest.split(['/', '?', '#', ':']).next().unwrap_or(rest))
}

/// What the real rules find on one item, as the corpus names the rules.
struct Found {
    v1: BTreeSet<&'static str>,
    /// With the v2 text rules (holdings tables, pay and offers).
    v2: BTreeSet<&'static str>,
    decision: Decision,
    /// The text the model reads: after both scans.
    scrubbed: String,
    secret_scrubbed: String,
}

fn find(item: &Item, sources: &Sources) -> Found {
    let expanded = synthetic::expand(&item.text);
    assert!(
        !expanded.text.contains("{{"),
        "{} 有不认识的占位符",
        item.id
    );
    let title = synthetic::expand(&item.title).text;
    let mut found = BTreeSet::new();
    let captured = !sources.capture_excluded(&item.bundle);
    if !captured {
        found.insert("capture_exclusion");
    }
    let key_file = window::KEY_FILES.is_match(&title);
    if key_file {
        found.insert("secret_file_title");
    }
    let shown = window(Some(&item.bundle), &title, sources);
    assert_eq!(
        matches!(shown, window::Window::Withheld(_)),
        !captured || key_file,
        "{}",
        item.id
    );

    let secret = secrets::scan(&expanded.text);
    for key in &expanded.keys {
        assert!(
            !secret.text.contains(key.as_str()),
            "{} 的合成密钥没有被抹掉",
            item.id
        );
    }
    for id in &expanded.ids {
        assert!(
            secret.text.contains(id.as_str()),
            "{} 的号码被密钥扫描改动了",
            item.id
        );
    }
    let ids = identifiers::scan(&secret.text);
    for id in &expanded.ids {
        assert!(
            !ids.text.contains(id.as_str()),
            "{} 的号码没有被抹掉",
            item.id
        );
    }
    let source = sources.kind(
        Some(&item.bundle),
        item.url.as_deref().and_then(url_host),
        None,
    );
    let hits = text_hits(&ids.text);
    let withheld = !captured || key_file;
    // A window not read has no text to scan: its text is not part of the vector.
    if !withheld {
        if !secret.wiped.is_empty() {
            found.insert("secret_scan");
        }
        if !ids.wiped.is_empty() {
            found.insert("identifier_scan");
        }
        match source {
            Some(SourceKind::Messaging) => {
                found.insert("messaging_source");
            }
            Some(_) => {
                found.insert("sensitive_source");
            }
            None => {}
        }
        if item.private == Some(true) {
            found.insert("private_window");
        }
    }
    let mut v2 = found.clone();
    if !withheld {
        for hit in &hits {
            let rule = match hit.kind {
                text::TextKind::Finance => "finance_text",
                text::TextKind::Health => "health_text",
            };
            v2.insert(rule);
            if [
                text::FINANCE_AMOUNT,
                text::HEALTH_LAB,
                text::HEALTH_PRESCRIPTION,
                text::HEALTH_FORM,
            ]
            .contains(&hit.rule)
            {
                found.insert(rule);
            }
        }
    }
    let decision = Decision {
        capture: Some(if captured { "seen" } else { "excluded" }.into()),
        window: Some(if key_file { "withheld" } else { "seen" }.into()),
        identifiers: Some(ids.wiped.iter().map(|kind| format!("{kind:?}")).collect()),
        source: Some(source.map(|kind| {
            serde_json::to_value(kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })),
        private: Some(Private::Known(item.private == Some(true))),
        text_hits: Some(
            hits.iter()
                .map(|hit| {
                    serde_json::to_value(hit.kind)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string()
                })
                .collect(),
        ),
        ..Decision::default()
    };
    Found {
        v1: found,
        v2,
        decision,
        scrubbed: ids.text,
        secret_scrubbed: secret.text,
    }
}

/// The recorded model half for one item, as `decide` reads it.
fn with_model(mut decision: Decision, recorded: Option<&Recorded>, message: &str) -> Decision {
    decision.model = Some("available".into());
    let Some(recorded) = recorded else {
        return decision;
    };
    decision.language = recorded.language.clone();
    decision.relay = Some(recorded.relay.as_ref().map(|relay| Relay {
        result: if relay.error {
            "error".into()
        } else {
            match check_relay(message, &relay.who, &relay.topic) {
                RelayCheck::Ok => "ok".into(),
                _ => "rejected".into(),
            }
        },
        category: Some(relay.category.clone()),
    }));
    decision.classify = Some(recorded.general.as_ref().map(|general| Classify {
        general: Some(general.clone()),
        tagging: recorded.tagging.clone(),
        kind: None,
    }));
    decision
}

fn disposition(name: &str) -> Disposition {
    match name {
        "exclude" => Disposition::Exclude,
        "blind" => Disposition::Blind,
        "present_only" => Disposition::PresentOnly,
        "who_what" => Disposition::WhoWhat,
        "carry" => Disposition::Carry,
        _ => panic!("{name}"),
    }
}

/// How much leaves the device: nothing, who-and-what, the text.
fn carried(disposition: Disposition) -> u8 {
    match disposition {
        Disposition::WhoWhat => 1,
        Disposition::Carry => 2,
        _ => 0,
    }
}

fn class(id: &str) -> &str {
    id.split('-').nth(1).unwrap_or("")
}

#[test]
fn v1_rules_find_exactly_what_each_item_is_annotated_with() {
    let sources = sources();
    let mut wrong = Vec::new();
    for (_, item) in corpus() {
        let found = find(&item, &sources);
        let annotated: BTreeSet<&str> = item.host.iter().map(String::as_str).collect();
        if found.v1 != annotated {
            wrong.push(format!(
                "{}：规则 {:?}，标注 {:?}",
                item.id, found.v1, annotated
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "宿主规则与标注不一致：\n{}",
        wrong.join("\n")
    );
}

#[test]
fn every_synthetic_key_and_number_is_wiped_before_any_model() {
    let sources = sources();
    for (_, item) in corpus() {
        let found = find(&item, &sources);
        let expanded = synthetic::expand(&item.text);
        for value in expanded.keys.iter().chain(&expanded.ids) {
            assert!(!found.scrubbed.contains(value.as_str()), "{}", item.id);
        }
    }
}

/// The v2 text rules change these items and no others: each is a hit the
/// v1 annotation does not have.
#[test]
fn v2_text_rules_add_only_the_listed_hits() {
    let sources = sources();
    let mut added = BTreeMap::new();
    for (_, item) in corpus() {
        let found = find(&item, &sources);
        assert!(found.v1.is_subset(&found.v2), "{}", item.id);
        let extra: Vec<&str> = found.v2.difference(&found.v1).copied().collect();
        if !extra.is_empty() {
            added.insert(item.id.clone(), (item.expect.clone(), extra));
        }
    }
    eprintln!("v2 文字规则新增命中：{added:?}");
    let expected: BTreeSet<&str> = V2_ADDED.iter().copied().collect();
    assert_eq!(
        added.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        expected
    );
}

/// Items the v2 rules newly hit: the two v1 leaks (`zh-f-05` a fund table,
/// `en-o-07` an offer), holdings pages and job pages that the source table
/// already held. All are `present_only` already; no item expected to be
/// carried is newly held. `ja-p-02` is a recruiting ad (`年収` with `現職`):
/// on a site the table does not list, an ad like it is held too.
const V2_ADDED: &[&str] = &[
    "en-f-02", "en-o-07", "ja-f-02", "ja-p-02", "mx-f-02", "mx-p-02", "zh-f-02", "zh-f-05",
];

#[test]
fn real_rules_with_the_recorded_model_report() {
    let sources = sources();
    let replay = replay();
    let mut leaks = Vec::new();
    let mut tight = Vec::new();
    let mut by_class: BTreeMap<String, BTreeMap<(String, String), usize>> = BTreeMap::new();
    let mut by_lang: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut annotated_leaks = Vec::new();
    let mut annotated_tight = 0;
    for (lang, item) in corpus() {
        let found = find(&item, &sources);
        let recorded = replay.rows.get(&item.id);
        let message = &found.secret_scrubbed;
        let got = decide(&with_model(found.decision.clone(), recorded, message)).disposition;
        let expect = disposition(&item.expect);

        // The annotations in place of the rules: the native probe's column.
        let mut annotated = found.decision.clone();
        let host: BTreeSet<&str> = item.host.iter().map(String::as_str).collect();
        annotated.capture = Some(
            if host.contains("capture_exclusion") {
                "excluded"
            } else {
                "seen"
            }
            .into(),
        );
        annotated.window = Some(
            if host.contains("secret_file_title") {
                "withheld"
            } else {
                "seen"
            }
            .into(),
        );
        annotated.identifiers = Some(if host.contains("identifier_scan") {
            vec!["card".into()]
        } else {
            vec![]
        });
        annotated.source = Some(if host.contains("sensitive_source") {
            Some("finance".into())
        } else if host.contains("messaging_source") {
            Some("messaging".into())
        } else {
            None
        });
        annotated.text_hits = Some(
            ["finance_text", "health_text"]
                .iter()
                .filter(|rule| host.contains(*rule))
                .map(|rule| rule.to_string())
                .collect(),
        );
        let annotated = decide(&with_model(annotated, recorded, message)).disposition;

        if item.disputed.is_some() {
            continue;
        }
        let entry = by_class.entry(class(&item.id).to_string()).or_default();
        *entry
            .entry((item.expect.clone(), format!("{got:?}")))
            .or_default() += 1;
        let lang_entry = by_lang.entry(lang).or_default();
        if carried(got) > carried(expect) {
            leaks.push(item.id.clone());
            lang_entry.0 += 1;
        } else if carried(got) < carried(expect) {
            tight.push(item.id.clone());
            lang_entry.1 += 1;
        }
        if carried(annotated) > carried(expect) {
            annotated_leaks.push(item.id.clone());
        } else if carried(annotated) < carried(expect) {
            annotated_tight += 1;
        }
    }
    eprintln!(
        "【标注代替规则＋录下的模型】放过 {} 条：{}；收得过紧 {annotated_tight} 条",
        annotated_leaks.len(),
        annotated_leaks.join(" ")
    );
    eprintln!(
        "【真规则＋录下的模型】放过 {} 条：{}",
        leaks.len(),
        leaks.join(" ")
    );
    eprintln!("    收得过紧 {} 条：{}", tight.len(), tight.join(" "));
    for (lang, (leak, over)) in &by_lang {
        eprintln!("    {lang}：放过 {leak}，过紧 {over}");
    }
    for (class, cells) in &by_class {
        eprintln!("    {class}：{cells:?}");
    }
    // The native probe's own column, rebuilt from its annotations: proves the
    // replay and `decide` combine the way the probe did.
    assert_eq!(annotated_leaks, ["zh-f-05", "en-o-07"]);
    assert_eq!(annotated_tight, 56);
    assert_eq!(leaks, EXPECTED_LEAKS, "放过的样本变了");
}

/// What the real rules plus the recorded model still let through.
const EXPECTED_LEAKS: &[&str] = &[];
