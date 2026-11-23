//! The site's own `log.vector`s waiting for a link (its rows, or another
//! end's, told as they were kept): one that tells the end nothing it was not
//! told on this link is not sent. While a long frame is worked through each
//! row leaves one, as the log was then, and the end, sent those rows in its
//! answer, would send them back for every vector that lacks them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use myriad_persona_log::{Vector, vector::missing};
use serde_json::Value;

use super::connection::Outgoing;

/// What a `log.vector` says the site holds: as many as each side's count, and
/// those past a gap.
struct Held {
    vector: Vector,
    beyond: BTreeMap<String, BTreeSet<u64>>,
}

impl Held {
    fn of(body: &Value) -> Option<Self> {
        let read = |field: &str| {
            body.get(field)
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()))
        };
        Some(Self {
            vector: serde_json::from_value(read("vector")).ok()?,
            beyond: serde_json::from_value(read("beyond")).ok()?,
        })
    }

    /// Everything `other` says is said here: past a gap too, where the count
    /// stays as it is however much more is held.
    fn covers(&self, other: &Held) -> bool {
        missing(&self.vector, &other.vector).is_empty()
            && other.beyond.iter().all(|(origin, seqs)| {
                let count = self.vector.get(origin).copied().unwrap_or(0);
                let past = self.beyond.get(origin);
                seqs.iter()
                    .all(|seq| *seq <= count || past.is_some_and(|past| past.contains(seq)))
            })
    }
}

/// What the last `log.vector` of each audience sent on this link said.
#[derive(Default)]
pub(super) struct Said(HashMap<String, Held>);

impl Said {
    pub(super) fn sent(&mut self, t: &str, body: &Value) {
        if let Some((audience, held)) = said(t, body) {
            self.0.insert(audience.to_string(), held);
        }
    }

    /// Those of `waiting` worth sending, in their order: all but the
    /// `log.vector`s the end was told as much as, or is by another one
    /// waiting (the later of two that say the same). One that asks for
    /// bodies or names a page always goes.
    pub(super) fn worth_sending(&self, waiting: Vec<Outgoing>) -> Vec<Outgoing> {
        let says: Vec<Option<(&str, Held)>> =
            waiting.iter().map(|one| said(one.t, &one.body)).collect();
        let goes: Vec<bool> = waiting
            .iter()
            .enumerate()
            .map(|(at, one)| {
                let Some((audience, held)) = &says[at] else {
                    return true;
                };
                if one.body.get("ask").is_some() || one.body.get("after").is_some() {
                    return true;
                }
                let told = self.0.get(*audience).is_some_and(|said| said.covers(held));
                !told
                    && !says.iter().enumerate().any(|(other, says)| {
                        says.as_ref().is_some_and(|(of, more)| {
                            other != at
                                && of == audience
                                && more.covers(held)
                                && (other > at || !held.covers(more))
                        })
                    })
            })
            .collect();
        drop(says);
        waiting
            .into_iter()
            .zip(goes)
            .filter_map(|(one, goes)| goes.then_some(one))
            .collect()
    }
}

fn said<'a>(t: &str, body: &'a Value) -> Option<(&'a str, Held)> {
    (t == "log.vector")
        .then(|| Some((body["audience"].as_str()?, Held::of(body)?)))
        .flatten()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn vector(audience: &str, site: u64) -> Outgoing {
        told(json!({ "audience": audience, "vector": { "site": site } }))
    }

    fn told(body: Value) -> Outgoing {
        super::super::connection::frame("log.vector", body)
    }

    fn bodies(sent: &[Outgoing]) -> Vec<Value> {
        sent.iter().map(|one| one.body.clone()).collect()
    }

    fn said_so(body: Value) -> Said {
        let mut said = Said::default();
        said.sent("log.vector", &body);
        said
    }

    #[test]
    fn of_those_waiting_only_what_says_most_goes_whichever_came_first() {
        let said = Said::default();
        let goes = said.worth_sending(vec![
            vector("own", 3),
            vector("personal:p", 9),
            vector("own", 5),
            vector("own", 4),
            vector("personal:p", 9),
        ]);
        assert_eq!(
            bodies(&goes),
            bodies(&[vector("own", 5), vector("personal:p", 9)])
        );
        // Neither says all of the other.
        let apart = || {
            vec![
                told(json!({ "audience": "own", "vector": { "site": 5, "dev_mac": 1 } })),
                told(json!({ "audience": "own", "vector": { "site": 4, "dev_mac": 2 } })),
            ]
        };
        assert_eq!(bodies(&said.worth_sending(apart())), bodies(&apart()));
    }

    #[test]
    fn what_was_told_on_the_link_is_not_told_again() {
        let mut said = said_so(json!({ "audience": "own", "vector": { "site": 5 } }));
        assert!(
            said.worth_sending(vec![vector("own", 4), vector("own", 5)])
                .is_empty()
        );
        let more = vec![vector("own", 6), vector("personal:p", 1)];
        assert_eq!(bodies(&said.worth_sending(more.clone())), bodies(&more));
        // Another frame of that body says nothing of it.
        said.sent(
            "log.full",
            &json!({ "audience": "own", "vector": { "site": 9 } }),
        );
        assert_eq!(said.worth_sending(vec![vector("own", 6)]).len(), 1);
    }

    #[test]
    fn rows_held_past_a_gap_are_told_though_the_count_stays() {
        let held = |beyond: &[u64]| json!({ "audience": "own", "vector": { "dev_air": 2 }, "beyond": { "dev_air": beyond } });
        let said = said_so(held(&[4]));
        assert_eq!(
            bodies(&said.worth_sending(vec![told(held(&[4])), told(held(&[4, 5]))])),
            vec![held(&[4, 5])]
        );
        // Counted once the gap filled.
        let said = said_so(json!({ "audience": "own", "vector": { "dev_air": 5 } }));
        assert!(said.worth_sending(vec![told(held(&[4, 5]))]).is_empty());
    }

    #[test]
    fn what_asks_or_names_a_page_goes_and_the_rest_keep_their_order() {
        let said = said_so(json!({ "audience": "own", "vector": { "site": 9 } }));
        let waiting = vec![
            super::super::connection::frame("log.rows", json!({ "audience": "own", "rows": [] })),
            vector("own", 8),
            told(json!({ "audience": "own", "vector": { "site": 8 }, "ask": ["r1"] })),
            super::super::connection::frame("lease.epoch", json!({ "epoch": 1 })),
            told(json!({ "audience": "own", "vector": {}, "after": "page_1" })),
            vector("own", 10),
        ];
        let mut left = bodies(&waiting);
        left.remove(1);
        assert_eq!(bodies(&said.worth_sending(waiting)), left);
    }
}
