//! A row to write, checked by the core's rules before anything is kept:
//! its body read by `Body::read` (the one byte form), its node made from
//! the audience and key, where it was said, the other node it names.
//! Pure: no clock, no database.

use myriad_nodes::{Body, SaidBy, Sync, key::Key, node_id, tags::audience_fits};
use myriad_persona_log::row::Replicate;
use serde_json::Value;

use super::Error;
use crate::services::her_log::NODE;

/// Who writes a row: she, or the person whose audience it is (what they
/// said, set, renamed, had her forget).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Author {
    Her,
    Owner,
}

impl Author {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Her => "her",
            Self::Owner => "owner",
        }
    }

    pub(crate) fn core(self) -> myriad_persona_log::row::Author {
        match self {
            Self::Her => myriad_persona_log::row::Author::Her,
            Self::Owner => myriad_persona_log::row::Author::Owner,
        }
    }
}

/// One row as the caller gives it: its id (made by the core, as
/// `myriad_nodes::entries::row_id_from` makes it), the node it is about,
/// its kind and body, and who writes it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowIn {
    pub id: String,
    pub node: String,
    pub kind: String,
    pub body: Value,
    pub author: Author,
}

/// A row checked: its body in its one byte form, and what its envelope
/// takes from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub id: String,
    pub node: String,
    pub kind: String,
    /// The canonical bytes `Body::read` gives back, as text.
    pub body: String,
    pub said_by: Vec<SaidBy>,
    pub author: Author,
    pub replicate: Replicate,
    /// The other node a merge or a split names (`node:<id>`).
    pub to: Option<String>,
    /// The spans a split moves.
    pub takes: Vec<String>,
    /// Whether it is the row that makes its node (`node.created`).
    pub created: bool,
}

impl Prepared {
    /// The same row as `other`: same node, kind, body and writer.
    pub(crate) fn same(&self, node: &str, kind: &str, body: &str, author: &str) -> bool {
        self.node == node
            && self.kind == kind
            && self.body == body
            && self.author.as_str() == author
    }
}

fn is_node_id(id: &str) -> bool {
    id.len() == 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn refused(what: &str) -> Error {
    Error::Refused(what.to_string())
}

/// `row` in `audience`, checked as every side reads it.
pub(crate) fn prepare(audience: &str, row: &RowIn) -> Result<Prepared, Error> {
    if !is_node_id(&row.node) {
        return Err(refused("a row's node is not a node id"));
    }
    let body = Body::read(&row.kind, &row.body)?;
    let (sync, said_by) = body.tags();
    let said_by = said_by.to_vec();
    let replicate = match sync {
        Sync::Shared => Replicate::Shared,
        Sync::Local(device) if device == NODE => Replicate::Local,
        Sync::Local(_) => return Err(refused("a local row here is the site's own")),
    };
    let mut to = None;
    let mut takes = Vec::new();
    let mut created = false;
    match &body {
        Body::Created(created_body) => {
            if node_id(audience, &created_body.key)? != row.node {
                return Err(refused(
                    "a created row's audience and key make another node",
                ));
            }
            created = true;
        }
        Body::Span(span) => {
            if !audience_fits(&span.audience, audience, &said_by) {
                return Err(refused(
                    "a span said elsewhere is not one this audience keeps",
                ));
            }
        }
        Body::Entry(entry) => {
            if !audience_fits(&entry.audience, audience, &said_by) {
                return Err(refused(
                    "an entry said elsewhere is not one this audience keeps",
                ));
            }
        }
        Body::Related(related) => {
            if !Key::parse(&related.to)?.fits(audience) {
                return Err(refused("a relation to a node that is not in this audience"));
            }
        }
        Body::Absorbed(absorbed) => {
            to = Some(format!("node:{}", node_id(audience, &absorbed.into)?));
        }
        Body::SplitOff(split) => {
            to = Some(format!("node:{}", node_id(audience, &split.to)?));
            takes = split.spans.clone();
        }
        _ => {}
    }
    let bytes = String::from_utf8(body.canonical_bytes())
        .map_err(|_| refused("a body's canonical bytes are not text"))?;
    Ok(Prepared {
        id: row.id.clone(),
        node: row.node.clone(),
        kind: row.kind.clone(),
        body: bytes,
        said_by,
        author: row.author,
        replicate,
        to,
        takes,
        created,
    })
}

/// A batch checked, each id once: the same row given twice is one, two
/// rows under one id are refused.
pub(crate) fn prepare_all(audience: &str, rows: &[RowIn]) -> Result<Vec<Prepared>, Error> {
    let mut out: Vec<Prepared> = Vec::with_capacity(rows.len());
    for row in rows {
        let prepared = prepare(audience, row)?;
        match out.iter().find(|seen| seen.id == prepared.id) {
            Some(seen) if *seen == prepared => {}
            Some(_) => return Err(refused("two rows of the batch share an id")),
            None => out.push(prepared),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn created(audience: &str, key: &str, name: &str) -> RowIn {
        RowIn {
            id: "u1".into(),
            node: node_id(audience, key).unwrap(),
            kind: "node.created".into(),
            body: json!({"version": 1, "sync": "shared", "saidBy": ["her"], "kind": "topic", "key": key, "name": name}),
            author: Author::Her,
        }
    }

    #[test]
    fn a_body_is_kept_in_its_one_byte_form() {
        let mut row = created("personal:7", "minted:plants", "plants");
        // Keys in another order, as a caller may build them.
        row.body = json!({"name": "plants", "key": "minted:plants", "kind": "topic", "saidBy": ["her"], "sync": "shared", "version": 1});
        let prepared = prepare("personal:7", &row).unwrap();
        assert_eq!(
            prepared.body,
            r#"{"key":"minted:plants","kind":"topic","name":"plants","saidBy":["her"],"sync":"shared","version":1}"#
        );
        assert!(prepared.created);
        assert_eq!(prepared.replicate, Replicate::Shared);
    }

    #[test]
    fn what_the_core_refuses_is_refused() {
        // A kind the core does not read.
        let mut row = created("personal:7", "minted:plants", "plants");
        row.kind = "node.noted".into();
        assert!(prepare("personal:7", &row).is_err());
        // A key the kind does not fit (a person is never a topic).
        let row = created("personal:7", "person:7", "seven");
        assert!(prepare("personal:7", &row).is_err());
        // A person in her own audience.
        let mut row = created("personal:7", "person:7", "seven");
        row.node = "0".repeat(32);
        assert!(prepare("own:", &row).is_err());
        // Made in another audience.
        let row = created("personal:8", "minted:plants", "plants");
        assert!(prepare("personal:7", &row).is_err());
        // An entry said in another account's talk.
        let entry = RowIn {
            id: "u2".into(),
            node: node_id("personal:7", "minted:plants").unwrap(),
            kind: "node.entry".into(),
            body: json!({"version": 1, "sync": "shared", "saidBy": ["partner"], "audience": "personal:8", "text": "has a fern", "refs": ["memory:m1"], "quoteOnly": true}),
            author: Author::Her,
        };
        assert!(prepare("personal:7", &entry).is_err());
        // Another device's local row.
        let mut row = created("personal:7", "minted:plants", "plants");
        row.body["sync"] = json!({"local": "dev_abc"});
        assert!(prepare("personal:7", &row).is_err());
    }

    #[test]
    fn one_id_is_one_row() {
        let row = created("personal:7", "minted:plants", "plants");
        assert_eq!(
            prepare_all("personal:7", &[row.clone(), row.clone()])
                .unwrap()
                .len(),
            1
        );
        let mut other = row.clone();
        other.body["name"] = json!("houseplants");
        assert!(prepare_all("personal:7", &[row, other]).is_err());
    }
}
