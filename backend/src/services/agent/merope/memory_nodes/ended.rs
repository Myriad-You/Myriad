//! What stood on a node and has ended (a `node.standing` item whose latest
//! version is `ended`), as she reads it beside what still stands there.
//!
//! An ended item keeps the fact that ended it as its words (the core's
//! `stands::decide`), and since when. Left unsaid, the node's older entries
//! (`[08-21] 每周三晚上夜跑`) read as how things are; said first, with its
//! day, they read as what was before it. Only what she concluded is said: a
//! rule's item is what a device showed for a while, and its ending is
//! nothing about their life. A memory they had her forget takes the rows
//! resting on it with it ([`super::forget`]); one they took back or let go
//! keeps its rows, so the caller's `since` says nothing for an item none of
//! whose memories she still holds, and it is left out.

use myriad_nodes::stands;

use super::shown::Node;

/// How many ended things of one node are said: the latest.
pub(super) const MOST: usize = 2;

/// The ended things on `node`, newest first, at most `most`: each as a line
/// with since when (`since`, by item; one with none is left out).
pub(super) fn lines(
    node: &Node,
    since: impl Fn(&str) -> Option<String>,
    most: usize,
) -> Vec<String> {
    let Some(standing) = node.head.get("standing").and_then(|all| all.as_object()) else {
        return Vec::new();
    };
    let mut ended: Vec<(i64, &str, &str)> = standing
        .iter()
        .filter(|(item, held)| {
            stands::whose_of(item).is_some()
                && held.get("author").and_then(|author| author.as_str()) != Some("rule")
                && held["value"]["ended"].as_bool() == Some(true)
        })
        .filter_map(|(item, held)| {
            let text = held["value"]["text"].as_str()?.trim();
            (!text.is_empty()).then_some((held.get("at")?.as_i64()?, item.as_str(), text))
        })
        .collect();
    ended.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    ended
        .into_iter()
        .filter_map(|(_, item, text)| Some(line(text, &since(item)?)))
        .take(most)
        .collect()
}

/// One ended thing, as a line: what ended it and since when.
fn line(text: &str, since: &str) -> String {
    format!("ended: {text} (since {since})")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn node(standing: serde_json::Value) -> Node {
        Node {
            id: "n".into(),
            kind: Some("topic".into()),
            name: Some("夜跑".into()),
            status: None,
            gone: false,
            head: json!({ "standing": standing }),
            facts: Vec::new(),
            heard: Vec::new(),
            latest: 0,
        }
    }

    /// What ended on a node is said, newest first, with since when; what
    /// still stands, a rule's item and one with no words are not, and no
    /// more than asked.
    #[test]
    fn what_ended_is_said_with_since_when() {
        let item = |n: usize| stands::item_id("them", "n", "m", n);
        let node = node(json!({
            (item(0)): {"at": 30, "author": "her",
                "value": {"text": "膝盖伤了，不夜跑了", "because": [], "ended": true}},
            (item(1)): {"at": 20, "author": "her",
                "value": {"text": "每周三晚上夜跑", "because": []}},
            (item(2)): {"at": 10, "author": "her",
                "value": {"text": "健身卡到期，没续", "because": [], "ended": true}},
            (item(3)): {"at": 40, "author": "her",
                "value": {"text": " ", "because": [], "ended": true}},
            "now": {"at": 50, "author": "rule",
                "value": {"text": "在写报告", "because": [], "ended": true}},
        }));
        let since = |item: &str| Some(format!("2026-10-{}", &item[item.len() - 1..]));
        let all = lines(&node, since, MOST);
        assert_eq!(all.len(), 2, "{all:?}");
        assert!(all[0].starts_with("ended: 膝盖伤了，不夜跑了 (since 2026-10-"));
        assert!(all[1].starts_with("ended: 健身卡到期，没续 (since "));
        assert_eq!(lines(&node, since, 1).len(), 1);
        // No day to say: not said.
        assert!(lines(&node, |_| None, MOST).is_empty());
    }
}
