//! Which kinds of row the log holds, what each is about and how it folds.
//!
//! An allow-list: a kind missing here is refused, so a new kind of row
//! never leaves a device until it is added here on purpose. The node kinds
//! are the native app's node log changes; the case kinds its judged cases.

use serde::Serialize;

/// How rows of one kind fold into their object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// Something that happened: every row stays, in log order.
    Event,
    /// A value that changes: the last row in log order is the current one,
    /// the earlier ones are its history.
    Latest,
    /// Retires the object, one way, unless a later [`Rule::Restore`].
    Retire,
    /// Undoes an earlier [`Rule::Retire`].
    Restore,
    /// Forgets the object it names (or everything about a subject), on
    /// every side and in any order.
    Forget,
}

/// The object a row of `kind` must be about, as the part before `:`;
/// `None` takes any object.
type Object = Option<&'static str>;

/// Sorted by kind.
const KINDS: &[(&str, Object, Rule)] = &[
    ("affect.change", Some("affect"), Rule::Event),
    ("case.disagreed", Some("case"), Rule::Event),
    ("case.insisted", Some("case"), Rule::Event),
    ("case.outcome", Some("case"), Rule::Event),
    ("case.owned", Some("case"), Rule::Event),
    ("case.persuaded", Some("case"), Rule::Event),
    ("case.reconsidered", Some("case"), Rule::Event),
    ("case.tested", Some("case"), Rule::Event),
    ("case.verdict", Some("case"), Rule::Event),
    ("case.work", Some("case"), Rule::Event),
    ("forget", None, Rule::Forget),
    // What a thing (a talk, a loose end) hangs on: the row is about the
    // thing, its payload names the node. Re-hanging shows in its history.
    ("link.attach", None, Rule::Latest),
    ("memory.restore", Some("memory"), Rule::Restore),
    ("memory.retire", Some("memory"), Rule::Retire),
    ("memory.write", Some("memory"), Rule::Event),
    ("node.acted", Some("node"), Rule::Event),
    ("node.consolidated", Some("node"), Rule::Event),
    ("node.created", Some("node"), Rule::Event),
    ("node.decided", Some("node"), Rule::Event),
    ("node.drafted", Some("node"), Rule::Event),
    ("node.faded", Some("node"), Rule::Event),
    ("node.filed", Some("node"), Rule::Event),
    ("node.grown", Some("node"), Rule::Event),
    ("node.loosened", Some("node"), Rule::Event),
    ("node.merged", Some("node"), Rule::Event),
    ("node.noted", Some("node"), Rule::Event),
    ("node.notified", Some("node"), Rule::Event),
    ("node.observed", Some("node"), Rule::Event),
    ("node.opened", Some("node"), Rule::Event),
    ("node.raised", Some("node"), Rule::Event),
    ("node.rested", Some("node"), Rule::Event),
    ("node.retagged", Some("node"), Rule::Event),
    ("node.revised", Some("node"), Rule::Event),
    ("node.revived", Some("node"), Rule::Event),
    ("node.shown", Some("node"), Rule::Event),
    ("node.split", Some("node"), Rule::Event),
    ("node.talked", Some("node"), Rule::Event),
    ("node.view_changed", Some("node"), Rule::Latest),
    ("outfit.write", Some("outfit"), Rule::Latest),
    // One field of her persona per object: `persona:<field>`.
    ("persona.field", Some("persona"), Rule::Latest),
    ("segment.close", Some("segment"), Rule::Event),
    ("segment.line", Some("segment"), Rule::Event),
    ("segment.open", Some("segment"), Rule::Event),
];

/// The object type a forget names to forget everything about a subject in
/// its audience: `subject:<id>`. No other row is about it.
pub const SUBJECT: &str = "subject";

fn entry(kind: &str) -> Option<&'static (&'static str, Object, Rule)> {
    KINDS
        .binary_search_by(|(known, _, _)| (*known).cmp(kind))
        .ok()
        .map(|at| &KINDS[at])
}

/// How rows of `kind` fold, or `None` for a kind the log does not hold.
pub fn rule(kind: &str) -> Option<Rule> {
    entry(kind).map(|(_, _, rule)| *rule)
}

/// The object type rows of `kind` must be about; `None` for any (or for a
/// kind the log does not hold).
pub fn object_type(kind: &str) -> Option<&'static str> {
    entry(kind).and_then(|(_, object, _)| *object)
}

/// Every kind, sorted.
pub fn all() -> impl Iterator<Item = &'static str> {
    KINDS.iter().map(|(kind, _, _)| *kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_sorted_and_unique() {
        assert!(KINDS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(rule("node.view_changed"), Some(Rule::Latest));
        assert_eq!(rule("node.unknown"), None);
        assert_eq!(object_type("persona.field"), Some("persona"));
        assert_eq!(object_type("forget"), None);
    }
}
