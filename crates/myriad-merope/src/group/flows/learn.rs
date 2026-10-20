//! Words or memes in the talk she is not sure of, as a judgment of it named
//! them (see `crate::memes`): each she does not know yet she may look up,
//! once a day and within the day's allowance; the dictionaries people keep
//! for slang and memes first, else a search. What was found is made into
//! what it means, held to how it was used there, and kept as hers; one she
//! could not look up, or whose meaning did not fit, she watches as maybe
//! the group's own. Looking up goes on beside whatever else she does.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::*;

/// What she knows of words and memes, read back: the latest this many.
const KNOWN_READ: u64 = 500;
const DIGEST_WITHIN_MS: u64 = 45_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Learning {
    /// The group, as sessions keep it.
    pub venue: String,
    /// Who making out what it means is billed to.
    pub owner: i32,
    /// The words still to go, with the lines of the talk that used each.
    pub terms: VecDeque<(String, Vec<String>)>,
    /// The words she knew when she began.
    #[serde(default)]
    pub known: Vec<String>,
    /// The word she is on, and where what was found came from.
    #[serde(default)]
    pub term: Option<(String, Vec<String>)>,
    #[serde(default)]
    pub url: String,
    pub stage: LearnStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LearnStage {
    Known,
    May,
    Define,
    Search,
    Digest,
}

/// Begin looking up `terms` (the talk is `conversation`, in the group
/// `venue`), billed to `owner`.
pub(crate) fn learn(
    cx: &mut Cx,
    owner: i32,
    venue: String,
    terms: Vec<String>,
    conversation: &[String],
) {
    // What people go by there is not a word to look up: a group card read
    // as a meme came back as some made-up meaning of it.
    let now = cx.now();
    let names: Vec<String> = people(cx.group, now)
        .into_iter()
        .map(|(name, _)| name)
        .chain(cx.group.shown_as.clone())
        .filter(|name| !name.trim().is_empty())
        .collect();
    let terms: VecDeque<(String, Vec<String>)> = terms
        .into_iter()
        .filter(|term| {
            !names
                .iter()
                .any(|name| myriad_agent_rules::memory::lexical::uses_word(name, term))
        })
        .map(|term| {
            let used = crate::memes::used_in(&term, conversation);
            (term, used)
        })
        // Only a word the talk really uses, as a word, is held to its use.
        .filter(|(_, used)| !used.is_empty())
        .collect();
    if terms.is_empty() {
        return;
    }
    let id = cx.id();
    let awaiting = cx.effect(|id| Effect::Read {
        id,
        of: Of::Hers,
        what: ToRead::OwnRows {
            source: crate::memes::SOURCE.into(),
            limit: KNOWN_READ,
        },
    });
    cx.group.errands.push(Errand {
        id,
        awaiting,
        kind: Kind::Learn,
        doing: Doing::Learning(Box::new(Learning {
            venue,
            owner,
            terms,
            known: Vec::new(),
            term: None,
            url: String::new(),
            stage: LearnStage::Known,
        })),
    });
}

/// The next word she does not know yet, asked whether she may look it up;
/// none left, and she is done.
fn next_term(cx: &mut Cx, l: &mut Learning) -> Option<String> {
    while let Some((term, used)) = l.terms.pop_front() {
        if l.known
            .iter()
            .any(|known| known.to_lowercase() == term.to_lowercase())
        {
            continue;
        }
        l.stage = LearnStage::May;
        let asked = term.clone();
        l.term = Some((term, used));
        return Some(cx.effect(|id| Effect::MayLookUp { id, term: asked }));
    }
    None
}

/// Watch the word she is on as maybe the group's own.
fn watch(cx: &mut Cx, l: &mut Learning) {
    if let Some((term, used)) = l.term.take() {
        cx.out.effects.push(Effect::Write {
            of: Of::Others,
            what: ToWrite::Watch {
                venue: l.venue.clone(),
                term,
                used,
            },
        });
    }
}

/// Make what was found into what the word means.
fn digest(cx: &mut Cx, l: &mut Learning, found: String, url: String) -> String {
    l.stage = LearnStage::Digest;
    l.url = url;
    let (term, used) = l.term.clone().unwrap_or_default();
    let owner = l.owner;
    cx.effect(|id| Effect::Ask {
        id,
        how: AskKind::Json,
        voice: "thorough".into(),
        owner,
        operation: "meme".into(),
        timeout_ms: DIGEST_WITHIN_MS,
        system: crate::memes::digest_system(),
        input: crate::memes::digest_input(&term, &used, &found),
        schema_name: crate::memes::DIGEST_SCHEMA.into(),
        schema: crate::memes::digest_schema(),
        turn: None,
    })
}

/// A step of looking up: what it waits on next, or none when done.
pub(crate) fn learn_step(cx: &mut Cx, l: &mut Learning, answer: Option<Answer>) -> Option<String> {
    match (l.stage, answer) {
        (LearnStage::Known, Some(Answer::Rows { rows })) => {
            l.known = rows
                .iter()
                .filter_map(|row| crate::memes::read_kept(&row.content))
                .map(|known| known.term)
                .collect();
            next_term(cx, l)
        }
        (LearnStage::May, Some(Answer::MayLookUp { may })) => {
            match may {
                May::Yes => {
                    l.stage = LearnStage::Define;
                    let term = l.term.as_ref().map(|(term, _)| term.clone());
                    return Some(cx.effect(|id| Effect::Define {
                        id,
                        term: term.unwrap_or_default(),
                    }));
                }
                May::Tried => l.term = None,
                May::NoneLeft => watch(cx, l),
            }
            next_term(cx, l)
        }
        (LearnStage::Define, Some(Answer::Defined { entry })) => match entry {
            Some(entry) => Some(digest(
                cx,
                l,
                format!("{}\n{}", entry.title, entry.text),
                entry.url,
            )),
            None => {
                l.stage = LearnStage::Search;
                let query = l
                    .term
                    .as_ref()
                    .map(|(term, _)| crate::memes::search_query(term))
                    .unwrap_or_default();
                Some(cx.effect(|id| Effect::Search { id, query }))
            }
        },
        (LearnStage::Search, Some(Answer::Found { found })) => match found {
            Some((found, url)) => Some(digest(cx, l, found, url)),
            None => {
                watch(cx, l);
                next_term(cx, l)
            }
        },
        (LearnStage::Digest, Some(Answer::Asked { raw, .. })) => {
            match raw
                .as_deref()
                .and_then(crate::memes::parse_digest)
                .flatten()
            {
                Some(meaning) => {
                    if let Some((term, _)) = l.term.take() {
                        let evidence =
                            serde_json::json!({ "term": term, "url": l.url }).to_string();
                        cx.out.effects.push(Effect::Write {
                            of: Of::Hers,
                            what: ToWrite::RememberOwn {
                                content: crate::memes::kept_line(&term, &meaning),
                                evidence,
                                concepts: vec![myriad_agent_rules::Concept {
                                    name: term,
                                    aliases: Vec::new(),
                                }],
                                source: crate::memes::SOURCE.into(),
                            },
                        });
                    }
                }
                None => watch(cx, l),
            }
            next_term(cx, l)
        }
        _ => None,
    }
}
