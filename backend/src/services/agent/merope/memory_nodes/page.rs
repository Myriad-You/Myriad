//! The site owner's page of her nodes (decision N9): what she keeps, one
//! audience at a time, and one node as it stands. Read only; what the
//! owner may change is [`super::page_edit`].
//!
//! - **Whose**: the site owner's own. Their private talk
//!   (`personal:local-owner`), her own (`own:`), and each group she is in
//!   (`group:<id>`, every group's nodes are kept with the site owner,
//!   [`super::audience`]). Another account's private talk is never on the
//!   page, nor named: it is kept under that account, and an audience not
//!   of these three is refused before anything is read ([`on_the_page`]).
//! - **What is on a node**: its head (summary, views, what is not wrapped
//!   up, status), its entries, its spans, her intents with the
//!   instruction each came from, and what was heard about the owner and
//!   never confirmed (「听说」). An entry whose memory was replaced, faded or
//!   let go is not shown, as nowhere else ([`super::known::held`]); one
//!   only seen (a member's words about the owner, on 「关于你」) is shown,
//!   marked so, since it is kept all the same.
//! - **What went**: forgotten rows are not in the fold. Where a span's
//!   words went, or the message an instruction was given in, the page says
//!   so instead of showing them.

use std::collections::{BTreeSet, HashSet};

use myriad_nodes::trust::Tier;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Serialize;
use serde_json::Value;

use super::page_intents::{IntentLine, intents_of};
use super::{Error, audience, known, shown};

/// Whether `audience` is one of the site owner's page.
pub(crate) fn on_the_page(audience: &str) -> bool {
    audience == audience::owners_talk()
        || audience == audience::OWN
        || audience
            .strip_prefix("group:")
            .is_some_and(|id| !id.trim().is_empty())
}

/// Which of the three an audience is.
fn place_of(audience: &str) -> &'static str {
    if audience == audience::OWN {
        "own"
    } else if audience.starts_with("group:") {
        "group"
    } else {
        "private"
    }
}

/// The node her page opens on in `audience`: 「关于你」 in the owner's talk,
/// 「她自己」 in her own; none in a group.
fn person_node(audience: &str, owner: i32) -> Option<String> {
    let key = match place_of(audience) {
        "private" => audience::person_key(audience, owner),
        "own" => "her".to_string(),
        _ => return None,
    };
    myriad_nodes::node_id(audience, &key).ok()
}

/// One node in a list.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodeLine {
    pub id: String,
    pub name: Option<String>,
    /// `topic`, `task` or `person`.
    pub kind: Option<String>,
    pub status: String,
    /// 「关于你」 or 「她自己」.
    pub own: bool,
    pub entries: usize,
    pub spans: usize,
    pub waiting: usize,
    /// Its newest entry or span, Unix microseconds.
    pub latest: i64,
}

/// One audience of the page.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudienceLine {
    pub audience: String,
    /// `private`, `own` or `group`.
    pub place: &'static str,
    /// A group's id (`qq:5`).
    pub group: Option<String>,
    pub nodes: Vec<NodeLine>,
}

/// A head as kept, with its columns.
struct Kept {
    id: String,
    kind: Option<String>,
    name: Option<String>,
    status: String,
    node: Value,
}

async fn kept(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: Option<&str>,
) -> Result<Vec<Kept>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, kind, name, status, node FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 AND ($3::text IS NULL OR node_id = $3) \
             ORDER BY node_id",
            [
                owner.into(),
                audience.into(),
                node.map(str::to_string).into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(Kept {
                id: row.try_get("", "node_id")?,
                kind: row.try_get("", "kind")?,
                name: row.try_get("", "name")?,
                status: row.try_get("", "status")?,
                node: row.try_get("", "node")?,
            })
        })
        .collect()
}

fn items<'a>(node: &'a Value, field: &str) -> impl Iterator<Item = &'a Value> {
    node[field].as_array().into_iter().flatten()
}

/// The memory rows the entries of `nodes` name.
fn named<'a>(nodes: impl Iterator<Item = &'a Value>) -> Vec<String> {
    nodes
        .flat_map(|node| items(node, "entries"))
        .flat_map(|entry| shown::cited(&entry["refs"]))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Whether an entry is still there: it names no memory, or one still held.
fn standing(entry: &Value, held: &HashSet<String>) -> bool {
    let memories = shown::cited(&entry["refs"]);
    memories.is_empty() || memories.iter().any(|memory| held.contains(memory))
}

fn latest_of(node: &Value) -> i64 {
    items(node, "entries")
        .chain(items(node, "spans"))
        .filter_map(|item| item["at"].as_i64())
        .chain(node["createdAt"].as_i64())
        .max()
        .unwrap_or(0)
}

fn waiting_of(node: &Value) -> usize {
    node["intents"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, item)| item["value"]["status"] == "waiting")
        .count()
}

/// Where a status sorts: what is going on first, what rests, then what ended.
fn rank_of(status: &str) -> u8 {
    match status {
        "active" | "ongoing" => 0,
        "resting" => 1,
        _ => 2,
    }
}

/// The site owner's audiences that hold nodes, theirs first, then hers,
/// then each group; each with its live nodes, its own node first, then by
/// status and the newest first.
pub(crate) async fn audiences(
    db: &DatabaseConnection,
    owner: i32,
) -> Result<Vec<AudienceLine>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT audience FROM agent_node_heads WHERE user_id = $1 \
             AND (audience = $2 OR audience = $3 OR audience LIKE 'group:%') ORDER BY audience",
            [
                owner.into(),
                audience::owners_talk().into(),
                audience::OWN.into(),
            ],
        ))
        .await?;
    let mut names: Vec<String> = Vec::new();
    for row in &rows {
        let audience: String = row.try_get("", "audience")?;
        if on_the_page(&audience) {
            names.push(audience);
        }
    }
    names.sort_by_key(|audience| match place_of(audience) {
        "private" => 0,
        "own" => 1,
        _ => 2,
    });
    let mut out = Vec::new();
    for audience in names {
        let heads = kept(db, owner, &audience, None).await?;
        let held = known::held(db, named(heads.iter().map(|head| &head.node))).await?;
        let own = person_node(&audience, owner);
        let mut nodes: Vec<NodeLine> = heads
            .into_iter()
            .filter(|head| known::is_live(&head.status))
            .map(|head| NodeLine {
                own: own.as_deref() == Some(head.id.as_str()),
                entries: items(&head.node, "entries")
                    .filter(|entry| standing(entry, &held))
                    .count(),
                spans: items(&head.node, "spans").count(),
                waiting: waiting_of(&head.node),
                latest: latest_of(&head.node),
                id: head.id,
                name: head.name,
                kind: head.kind,
                status: head.status,
            })
            .collect();
        nodes.sort_by(|a, b| {
            b.own
                .cmp(&a.own)
                .then_with(|| rank_of(&a.status).cmp(&rank_of(&b.status)))
                .then_with(|| b.latest.cmp(&a.latest))
                .then_with(|| a.id.cmp(&b.id))
        });
        if nodes.is_empty() {
            continue;
        }
        out.push(AudienceLine {
            place: place_of(&audience),
            group: audience.strip_prefix("group:").map(str::to_string),
            audience,
            nodes,
        });
    }
    Ok(out)
}

/// One item of a head (a view, something not wrapped up).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeadItem {
    pub id: String,
    pub text: String,
    pub at: i64,
}

/// One entry, as the page shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryLine {
    pub row: String,
    pub text: String,
    pub at: i64,
    pub said_by: Vec<String>,
    /// Only seen: never a fact, kept on the experience.
    pub seen: bool,
    pub quote_only: bool,
}

/// One span of experience.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpanLine {
    pub row: String,
    pub text: String,
    pub at: i64,
    pub said_by: Vec<String>,
    /// The words it was cut from were all deleted: only her line is kept.
    pub words_gone: bool,
}

/// One node as the page shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NodePage {
    pub id: String,
    pub audience: String,
    /// Forgotten or taken back: nothing else of it is shown.
    pub gone: bool,
    /// Gone into another node: the one it is part of now.
    pub moved_to: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub status: String,
    pub own: bool,
    /// Its current name is one the owner set.
    pub named_by_owner: bool,
    pub aliases: Vec<String>,
    pub summary: Vec<String>,
    pub views: Vec<HeadItem>,
    pub open: Vec<HeadItem>,
    pub entries: Vec<EntryLine>,
    pub heard: Vec<EntryLine>,
    pub spans: Vec<SpanLine>,
    pub intents: Vec<IntentLine>,
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}

fn head_items(node: &Value, field: &str, skip_closed: bool) -> Vec<HeadItem> {
    let mut out: Vec<HeadItem> = node[field]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, item)| !(skip_closed && item["value"]["closed"].as_bool() == Some(true)))
        .filter_map(|(id, item)| {
            Some(HeadItem {
                id: id.clone(),
                text: item["value"]["text"].as_str()?.to_string(),
                at: item["at"].as_i64().unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));
    out
}

/// The lines among `refs` (`message:<id>`) still there.
async fn lines_left(db: &DatabaseConnection, ids: Vec<i32>) -> Result<HashSet<i32>, Error> {
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_messages WHERE id = ANY($1)",
            [ids.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<i32>("", "id")?))
        .collect()
}

fn lines_of(refs: &Value) -> Vec<i32> {
    strings(refs)
        .iter()
        .filter_map(|reference| reference.strip_prefix("message:")?.parse().ok())
        .collect()
}

/// The entries of `node`, sorted for the page: (entries, heard).
fn entries_of(
    node: &Value,
    audience: &str,
    own: bool,
    held: &HashSet<String>,
) -> (Vec<EntryLine>, Vec<EntryLine>) {
    let (mut entries, mut heard) = (Vec::new(), Vec::new());
    for entry in items(node, "entries").filter(|entry| standing(entry, held)) {
        let said_by: Vec<myriad_nodes::SaidBy> =
            serde_json::from_value(entry["saidBy"].clone()).unwrap_or_default();
        let said_in = entry["audience"].as_str().unwrap_or(audience);
        let tier = shown::tier_of(&said_by, said_in, audience, own, None);
        let line = EntryLine {
            row: entry["row"].as_str().unwrap_or_default().to_string(),
            text: entry["text"].as_str().unwrap_or_default().to_string(),
            at: entry["at"].as_i64().unwrap_or_default(),
            said_by: strings(&entry["saidBy"]),
            seen: !matches!(tier, Some(Tier::Counts) | Some(Tier::Heard)),
            quote_only: entry["quoteOnly"].as_bool().unwrap_or(false),
        };
        if tier == Some(Tier::Heard) {
            heard.push(line);
        } else {
            entries.push(line);
        }
    }
    entries.reverse();
    heard.reverse();
    (entries, heard)
}

/// `node` of the site owner's `audience`, as the page shows it; none when
/// it is not one of theirs or not there.
pub(crate) async fn node(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
) -> Result<Option<NodePage>, Error> {
    if !on_the_page(audience) {
        return Ok(None);
    }
    let Some(head) = kept(db, owner, audience, Some(node))
        .await?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    let own = person_node(audience, owner).as_deref() == Some(head.id.as_str());
    let mut page = NodePage {
        id: head.id.clone(),
        audience: audience.to_string(),
        gone: !known::is_live(&head.status),
        moved_to: None,
        name: None,
        kind: head.kind.clone(),
        status: head.status.clone(),
        own,
        named_by_owner: false,
        aliases: Vec::new(),
        summary: Vec::new(),
        views: Vec::new(),
        open: Vec::new(),
        entries: Vec::new(),
        heard: Vec::new(),
        spans: Vec::new(),
        intents: Vec::new(),
    };
    if page.gone {
        if head.status == "absorbed" {
            page.moved_to = known::landing(db, owner, audience, node).await?;
        }
        return Ok(Some(page));
    }
    let value = &head.node;
    let held = known::held(db, named(std::iter::once(value))).await?;
    let (entries, heard) = entries_of(value, audience, own, &held);
    let gone_lines: Vec<i32> = items(value, "spans")
        .flat_map(|span| lines_of(&span["refs"]))
        .collect();
    let left = lines_left(db, gone_lines).await?;
    let mut spans: Vec<SpanLine> = items(value, "spans")
        .map(|span| {
            let lines = lines_of(&span["refs"]);
            SpanLine {
                row: span["row"].as_str().unwrap_or_default().to_string(),
                text: span["text"].as_str().unwrap_or_default().to_string(),
                at: span["at"].as_i64().unwrap_or_default(),
                said_by: strings(&span["saidBy"]),
                words_gone: !lines.is_empty() && !lines.iter().any(|line| left.contains(line)),
            }
        })
        .collect();
    spans.reverse();
    page.name = head.name;
    page.named_by_owner = value["name"]["author"] == "owner";
    page.aliases = strings(&value["aliases"]["value"]);
    page.summary = items(&value["summary"], "value")
        .filter_map(|sentence| sentence["text"].as_str().map(str::to_string))
        .collect();
    page.views = head_items(value, "views", false);
    page.open = head_items(value, "open", true);
    page.entries = entries;
    page.heard = heard;
    page.spans = spans;
    page.intents = intents_of(db, owner, audience, node).await?;
    Ok(Some(page))
}
