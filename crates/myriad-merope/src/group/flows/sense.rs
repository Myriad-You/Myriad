//! Before she answers, she reads the talk: what is going on, what it means,
//! what people are telling her about herself (kept, once), with what the
//! group told her before (the latest few, and an older one the talk comes
//! back to by its words or its meaning). See `crate::making_sense`.

use std::collections::HashMap;

use myriad_agent_rules::memory::rank;
use serde::{Deserialize, Serialize};

use super::*;
use crate::making_sense as rules;

/// Where what people there told her about herself is kept.
pub(crate) const TOLD_SOURCE: &str = "about_me";
/// She thinks the reading through: misreading who a line is for and what
/// it means costs more than a few seconds more (at 8 seconds most readings
/// were cut off, and she answered unread).
const READ_WITHIN_MS: u64 = 25_000;
/// The same thing, still on the screen, is read again every turn: once in
/// this long is being told once.
const TOLD_ONCE_WITHIN_MINUTES: i64 = 30;
const TOLD_SHOWN: usize = 3;
/// Older tellings the talk can bring back besides those.
const TOLD_BROUGHT_BACK: usize = 2;
/// How many of one source in one venue the talk can bring back from.
const LOOKED_OVER: u64 = 500;
/// Memories still without a vector given one behind the reply, at most.
pub(crate) const FILL: usize = 63;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sensing {
    pub venue: String,
    pub conversation: Vec<String>,
    pub said: String,
    pub owner: Option<i32>,
    pub stage: SenseStage,
    #[serde(default)]
    pub sense: Option<crate::making_sense::Sense>,
    #[serde(default)]
    pub fresh: Vec<MemoryRow>,
    #[serde(default)]
    pub older: Vec<MemoryRow>,
    #[serde(default)]
    pub cues: HashMap<String, String>,
    #[serde(default)]
    pub vectors: Vec<(String, Vec<f32>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SenseStage {
    Soul,
    Asking,
    Lately,
    Told,
    Cues,
    Vectors,
    Asked,
}

/// What a step of the reading came to.
pub(crate) enum Sensed {
    Wait(String),
    Done(Option<String>),
}

/// The group's venue as memories keep it (`group:telegram:-100123`).
pub(crate) fn stored(venue: &str) -> String {
    format!("group:{venue}")
}

pub(crate) fn sense_start(
    cx: &mut Cx,
    venue: String,
    conversation: Vec<String>,
    said: String,
    owner: Option<i32>,
) -> (Sensing, Sensed) {
    let mut sensing = Sensing {
        venue,
        conversation,
        said,
        owner,
        stage: SenseStage::Soul,
        sense: None,
        fresh: Vec::new(),
        older: Vec::new(),
        cues: HashMap::new(),
        vectors: Vec::new(),
    };
    // Nobody hosts her there to bill the reading to: no reading.
    if owner.is_none() {
        return (sensing, Sensed::Done(None));
    }
    let id = cx.effect(|id| Effect::Read {
        id,
        of: Of::Hers,
        what: ToRead::Soul,
    });
    sensing.stage = SenseStage::Soul;
    (sensing, Sensed::Wait(id))
}

fn told_rows(cx: &mut Cx, s: &mut Sensing) -> Sensed {
    s.stage = SenseStage::Told;
    let venue = stored(&s.venue);
    Sensed::Wait(cx.effect(|id| Effect::Read {
        id,
        of: Of::Others,
        what: ToRead::VenueRows {
            venue,
            source: TOLD_SOURCE.into(),
            newest: Newest::Updated,
            limit: LOOKED_OVER,
        },
    }))
}

pub(crate) fn sense_step(cx: &mut Cx, s: &mut Sensing, answer: Option<Answer>) -> Sensed {
    let now = cx.now();
    match (s.stage, answer) {
        (SenseStage::Soul, Some(Answer::Soul { text })) => {
            s.stage = SenseStage::Asking;
            let owner = s.owner.unwrap_or_default();
            let input = rules::input(&s.conversation, cx.group.shown_as.as_deref());
            Sensed::Wait(cx.effect(|id| Effect::Ask {
                id,
                how: AskKind::Json,
                voice: "thorough".into(),
                owner,
                operation: "group_making_sense".into(),
                timeout_ms: READ_WITHIN_MS,
                system: rules::system(&text),
                input,
                schema_name: rules::SCHEMA_NAME.into(),
                schema: rules::schema(),
                turn: None,
            }))
        }
        (SenseStage::Asking, Some(Answer::Asked { raw, .. })) => {
            let Some(raw) = raw else {
                return told_rows(cx, s);
            };
            // Words or memes in the talk she is not sure of: hers to look up.
            let terms = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim())
                .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
                .map(|value| crate::memes::unsure_terms(&value))
                .unwrap_or_default();
            learn(
                cx,
                s.owner.unwrap_or_default(),
                s.venue.clone(),
                terms,
                &s.conversation,
            );
            s.sense = rules::parse(&raw);
            if s.sense
                .as_ref()
                .is_some_and(|sense| sense.told(&s.conversation).is_some())
            {
                s.stage = SenseStage::Lately;
                let venue = stored(&s.venue);
                return Sensed::Wait(cx.effect(|id| Effect::Read {
                    id,
                    of: Of::Others,
                    what: ToRead::VenueRows {
                        venue,
                        source: TOLD_SOURCE.into(),
                        newest: Newest::Created,
                        limit: 1,
                    },
                }));
            }
            told_rows(cx, s)
        }
        (SenseStage::Lately, Some(Answer::Rows { rows })) => {
            let lately = rows
                .iter()
                .any(|row| now - row.created_at < TOLD_ONCE_WITHIN_MINUTES * 60_000_000);
            if !lately
                && let Some(sense) = s.sense.as_ref()
                && let Some(told) = sense.told(&s.conversation)
            {
                cx.out.effects.push(Effect::Write {
                    of: Of::Others,
                    what: ToWrite::RememberInVenue {
                        venue: stored(&s.venue),
                        content: told.to_string(),
                        evidence:
                            serde_json::json!({ "line": s.said, "quote": sense.about_you_quote })
                                .to_string(),
                        source: TOLD_SOURCE.into(),
                    },
                });
            }
            told_rows(cx, s)
        }
        (SenseStage::Told, Some(Answer::Rows { mut rows })) => {
            let older = rows.split_off(TOLD_SHOWN.min(rows.len()));
            s.fresh = rows;
            s.older = older;
            if s.said.trim().is_empty() || s.older.is_empty() {
                return Sensed::Done(finish(s, &[], now));
            }
            s.stage = SenseStage::Cues;
            let ids = s.older.iter().map(|row| row.id.clone()).collect();
            Sensed::Wait(cx.effect(|id| Effect::Read {
                id,
                of: Of::Others,
                what: ToRead::Cues { ids },
            }))
        }
        (SenseStage::Cues, Some(Answer::Cues { cues })) => {
            s.cues = cues;
            s.stage = SenseStage::Vectors;
            let rows = with_cues(&s.older, &s.cues)
                .into_iter()
                .map(|row| (row.id, row.content))
                .collect();
            Sensed::Wait(cx.effect(|id| Effect::Read {
                id,
                of: Of::Others,
                what: ToRead::Vectors { rows, fill: FILL },
            }))
        }
        (SenseStage::Vectors, Some(Answer::Vectors { vectors, known })) => {
            s.vectors = vectors;
            if !known {
                let brought = named_among(
                    &s.older,
                    &s.cues,
                    &[],
                    None,
                    &s.said,
                    TOLD_BROUGHT_BACK,
                    now,
                );
                return Sensed::Done(finish(s, &brought, now));
            }
            s.stage = SenseStage::Asked;
            let text = s.said.trim().to_string();
            Sensed::Wait(cx.effect(|id| Effect::Embed { id, text }))
        }
        (SenseStage::Asked, Some(Answer::Embedded { vector })) => {
            let brought = named_among(
                &s.older,
                &s.cues,
                &s.vectors,
                vector.as_deref(),
                &s.said,
                TOLD_BROUGHT_BACK,
                now,
            );
            Sensed::Done(finish(s, &brought, now))
        }
        _ => Sensed::Done(None),
    }
}

/// Rows as recall reads them: each with its cues after what it says.
pub(crate) fn with_cues(rows: &[MemoryRow], cues: &HashMap<String, String>) -> Vec<MemoryRow> {
    rows.iter()
        .map(|row| {
            let mut row = row.clone();
            if let Some(cues) = cues.get(&row.id) {
                row.content = format!("{}\n{cues}", row.content);
            }
            row
        })
        .collect()
}

/// Of `rows`, at most `limit` that `query` names by their words or their
/// meaning, best first, as they were said; none when it names none.
pub(crate) fn named_among(
    rows: &[MemoryRow],
    cues: &HashMap<String, String>,
    vectors: &[(String, Vec<f32>)],
    asked: Option<&[f32]>,
    query: &str,
    limit: usize,
    now: i64,
) -> Vec<MemoryRow> {
    if query.trim().is_empty() || rows.is_empty() || limit == 0 {
        return Vec::new();
    }
    let read = with_cues(rows, cues);
    let pairs: Vec<(&str, &[f32])> = vectors
        .iter()
        .map(|(id, vector)| (id.as_str(), vector.as_slice()))
        .collect();
    let by_meaning = rank::standing_out(&rank::closeness(asked, &pairs));
    let view: Vec<rank::Row> = read.iter().map(MemoryRow::view).collect();
    let ranked = rank::rank_marked(
        &view,
        Some(query),
        limit,
        &HashMap::new(),
        0.0,
        &by_meaning,
        lines::time(now).fixed_offset(),
    );
    ranked
        .chosen
        .into_iter()
        .filter(|(index, brought)| !brought && !ranked.filler.contains(index))
        .map(|(index, _)| rows[index].clone())
        .collect()
}

fn finish(s: &Sensing, brought: &[MemoryRow], now: i64) -> Option<String> {
    let told: Vec<(String, String)> = s
        .fresh
        .iter()
        .chain(brought)
        .map(|row| {
            (
                row.content.clone(),
                crate::doing::ago_text(lines::age(now, row.created_at)),
            )
        })
        .collect();
    let sections: Vec<String> = [
        rules::told_section(&told),
        s.sense.as_ref().map(rules::section),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}
