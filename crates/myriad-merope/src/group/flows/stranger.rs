//! Her answer to someone from outside the community, in a group: the same
//! voice, far less context (see `crate::strangers`). Who they are to her
//! (her note on them here, what she noted of them in her other groups, whom
//! she wonders they might be), the hour and what she is doing, the group's
//! own bits and days and those of her other groups, her views and what she
//! heard that their words touch, the turtle soup on offer, the stickers she
//! can send, the words and memes she knows and the groups' own, how the
//! group reads; put together here from what is read, asked of her once
//! (once more when the answer is cut), and taken apart again: the sticker
//! she chose, the puzzle she said she would think up.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

const NOTE_SOURCE: &str = "stranger";
const GUESS_SOURCE: &str = "maybe_is";
/// Her notes on the same account from other groups.
const NOTES_SHOWN: u64 = 4;
/// A group's bits and days: the freshest few, and older ones the talk
/// brings back, from this many.
const FRESH: usize = 3;
const LOOKED_OVER: u64 = 500;
const BROUGHT_BACK: usize = 2;
const BIT_SOURCE: &str = "bit";
const DAY_SOURCE: &str = "group_day";
/// Her other groups' bits and days the talk brings back, and from how many.
const THEIRS_LOOKED_OVER: u64 = 500;
const THEIRS_SHOWN: usize = 3;
const VIEWS_HELD: u64 = 300;
const VIEWS_SHOWN: usize = 2;
const HEARD_SOURCE: &str = "heard";
const HEARD_HELD: u64 = 300;
const HEARD_SHOWN: usize = 3;
const MEMES_HELD: u64 = 500;
const JARGON_SOURCE: &str = "jargon";
const JARGON_READ_BACK: u64 = 300;
/// Stickers offered to her in a turn, at most.
const OFFERED: usize = 8;
/// People from outside she keeps what she offered them for, per group.
const OFFERS_KEPT: usize = 64;
/// The lines of the talk the words and memes in it are looked for in.
const TALK_LINES: usize = 12;
const REPLY_WITHIN_MS: u64 = 60_000;
/// The whole answer, put together and asked, within this long.
const DEADLINE_MS: u64 = 90_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Replying {
    /// The group, as sessions keep it (`telegram:-100123`).
    pub venue: String,
    pub owner: i32,
    pub stranger: Stranger,
    pub transcript: Vec<Spoken>,
    pub words: String,
    #[serde(default)]
    pub why: Option<String>,
    /// How the group reads (how people type, how her lines differ, what she
    /// made of the talk).
    #[serde(default)]
    pub reading: Vec<String>,
    pub began: i64,
    pub stage: ReplyStage,
    #[serde(default)]
    pub soul: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub elsewhere: Vec<MemoryRow>,
    /// Whom she wonders they might be: (account, how sure, why).
    #[serde(default)]
    pub guess: Option<(i32, String, String)>,
    #[serde(default)]
    pub candidate: Option<String>,
    #[serde(default)]
    pub local: Option<String>,
    #[serde(default)]
    pub doing: Option<String>,
    #[serde(default)]
    pub bits: Vec<MemoryRow>,
    #[serde(default)]
    pub days: Vec<MemoryRow>,
    #[serde(default)]
    pub theirs: Vec<MemoryRow>,
    #[serde(default)]
    pub views: Vec<(String, String)>,
    #[serde(default)]
    pub heard: Vec<MemoryRow>,
    #[serde(default)]
    pub soup: String,
    /// The stickers offered, best first; none when she has no section.
    #[serde(default)]
    pub stickers: Option<Vec<StickerRow>>,
    #[serde(default)]
    pub left: usize,
    #[serde(default)]
    pub memes: Vec<(String, String)>,
    #[serde(default)]
    pub jargon: Vec<MemoryRow>,
    #[serde(default)]
    pub titles: BTreeMap<String, Option<String>>,
    /// Older rows the talk may bring back, while she looks them over.
    #[serde(default)]
    pub recall: Option<Recall>,
    /// The game at the table, while she sees to it.
    #[serde(default)]
    pub table: Option<AtTable>,
    /// What their words mean, once embedded: none when it gave none.
    #[serde(default)]
    pub asked: Option<Option<Vec<f32>>>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub asks: u8,
    /// Her words, while the puzzle she said she would think up comes.
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub sticker: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplyStage {
    Soul,
    Note,
    Elsewhere,
    Guess,
    Label,
    Now,
    Bits,
    Days,
    Theirs,
    Views,
    Heard,
    Soup,
    Stickers,
    Memes,
    Jargon,
    Titles,
    Asking,
    Opening,
    After,
}

/// Older rows the talk may bring back by their words or their meaning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recall {
    /// Those always shown (the freshest), before what is brought back.
    #[serde(default)]
    pub fresh: Vec<MemoryRow>,
    pub rows: Vec<MemoryRow>,
    pub limit: usize,
    pub of: Of,
    #[serde(default)]
    pub cues: HashMap<String, String>,
    #[serde(default)]
    pub vectors: Vec<(String, Vec<f32>)>,
    pub stage: RecallStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RecallStage {
    Cues,
    Vectors,
    Asked,
}

/// What a step of her answer came to: waiting, or her words and sticker
/// (none: nothing to say).
pub(crate) enum Replied {
    Wait(String),
    Done(Option<(String, Option<Value>)>),
}

fn read(cx: &mut Cx, of: Of, what: ToRead) -> String {
    cx.effect(|id| Effect::Read { id, of, what })
}

fn marker(r: &Replying) -> String {
    crate::strangers::evidence_marker(&r.stranger.who)
}

fn stored(r: &Replying) -> String {
    super::sense::stored(&r.venue)
}

/// Begin her answer to the stranger the answer `a` is to.
pub(crate) fn reply_start(cx: &mut Cx, a: &Answering) -> (Replying, Replied) {
    let replying = Replying {
        venue: a.message.venue(),
        owner: a.owner.unwrap_or_default(),
        stranger: a.stranger.clone().unwrap_or(Stranger {
            who: String::new(),
            name: String::new(),
        }),
        transcript: a.transcript.clone(),
        words: a.said.clone(),
        why: a.chime.clone(),
        reading: a.sections.clone(),
        began: cx.now(),
        stage: ReplyStage::Soul,
        soul: String::new(),
        note: None,
        elsewhere: Vec::new(),
        guess: None,
        candidate: None,
        local: None,
        doing: None,
        bits: Vec::new(),
        days: Vec::new(),
        theirs: Vec::new(),
        views: Vec::new(),
        heard: Vec::new(),
        soup: String::new(),
        stickers: None,
        left: 0,
        memes: Vec::new(),
        jargon: Vec::new(),
        titles: BTreeMap::new(),
        recall: None,
        table: None,
        asked: None,
        prompt: String::new(),
        asks: 0,
        text: String::new(),
        sticker: None,
    };
    let id = read(cx, Of::Hers, ToRead::Soul);
    (replying, Replied::Wait(id))
}

/// Begin looking over `older` for what their words bring back (at most
/// `limit`), after `fresh`; none to look over, and it is done at once.
fn recall_start(
    cx: &mut Cx,
    r: &mut Replying,
    of: Of,
    fresh: Vec<MemoryRow>,
    older: Vec<MemoryRow>,
    limit: usize,
) -> Result<String, Vec<MemoryRow>> {
    if r.words.trim().is_empty() || older.is_empty() || limit == 0 {
        return Err(fresh);
    }
    let ids = older.iter().map(|row| row.id.clone()).collect();
    r.recall = Some(Recall {
        fresh,
        rows: older,
        limit,
        of,
        cues: HashMap::new(),
        vectors: Vec::new(),
        stage: RecallStage::Cues,
    });
    Ok(read(cx, of, ToRead::Cues { ids }))
}

fn recalled(recall: &Recall, asked: Option<&[f32]>, words: &str, now: i64) -> Vec<MemoryRow> {
    let mut rows = recall.fresh.clone();
    rows.extend(named_among(
        &recall.rows,
        &recall.cues,
        if asked.is_some() {
            &recall.vectors
        } else {
            &[]
        },
        asked,
        words,
        recall.limit,
        now,
    ));
    rows
}

fn recall_step(
    cx: &mut Cx,
    r: &mut Replying,
    mut recall: Recall,
    answer: Option<Answer>,
) -> Result<String, Vec<MemoryRow>> {
    let now = cx.now();
    match (recall.stage, answer) {
        (RecallStage::Cues, Some(Answer::Cues { cues })) => {
            recall.cues = cues;
            recall.stage = RecallStage::Vectors;
            let rows = with_cues(&recall.rows, &recall.cues)
                .into_iter()
                .map(|row| (row.id, row.content))
                .collect();
            let of = recall.of;
            r.recall = Some(recall);
            Ok(read(cx, of, ToRead::Vectors { rows, fill: FILL }))
        }
        (RecallStage::Vectors, Some(Answer::Vectors { vectors, known })) => {
            recall.vectors = vectors;
            if !known {
                return Err(recalled(&recall, None, &r.words, now));
            }
            if let Some(asked) = &r.asked {
                return Err(recalled(&recall, asked.as_deref(), &r.words, now));
            }
            recall.stage = RecallStage::Asked;
            r.recall = Some(recall);
            let text = r.words.trim().to_string();
            Ok(cx.effect(|id| Effect::Embed { id, text }))
        }
        (RecallStage::Asked, Some(Answer::Embedded { vector })) => {
            r.asked = Some(vector);
            let asked = r.asked.clone().flatten();
            Err(recalled(&recall, asked.as_deref(), &r.words, now))
        }
        _ => Err(recall.fresh),
    }
}

/// A step of her answer.
pub(crate) fn reply_step(cx: &mut Cx, r: &mut Replying, answer: Option<Answer>) -> Replied {
    let now = cx.now();
    // Past the turn's deadline, she lets it go.
    if now - r.began >= DEADLINE_MS as i64 * 1_000 {
        return Replied::Done(None);
    }
    if let Some(recall) = r.recall.take() {
        return match recall_step(cx, r, recall, answer) {
            Ok(id) => Replied::Wait(id),
            Err(rows) => after_recall(cx, r, rows),
        };
    }
    if let Some(mut table) = r.table.take() {
        return match at_table_step(cx, &mut table, answer) {
            AtTableNext::Wait(id) => {
                r.table = Some(table);
                Replied::Wait(id)
            }
            AtTableNext::Done(text) => at_table_done(cx, r, table.what, text),
        };
    }
    match (r.stage, answer) {
        (ReplyStage::Soul, Some(Answer::Soul { text })) => {
            r.soul = text;
            r.stage = ReplyStage::Note;
            let what = ToRead::Unowned {
                venue: stored(r),
                source: NOTE_SOURCE.into(),
                marker: marker(r),
            };
            Replied::Wait(read(cx, Of::Others, what))
        }
        (ReplyStage::Note, Some(Answer::Rows { rows })) => {
            r.note = rows.into_iter().next().map(|row| row.content);
            r.stage = ReplyStage::Elsewhere;
            let what = ToRead::UnownedElsewhere {
                here: stored(r),
                source: NOTE_SOURCE.into(),
                marker: marker(r),
                limit: NOTES_SHOWN,
            };
            Replied::Wait(read(cx, Of::Others, what))
        }
        (ReplyStage::Elsewhere, Some(Answer::Rows { rows })) => {
            r.elsewhere = rows;
            r.stage = ReplyStage::Guess;
            let what = ToRead::Unowned {
                venue: stored(r),
                source: GUESS_SOURCE.into(),
                marker: marker(r),
            };
            Replied::Wait(read(cx, Of::Others, what))
        }
        (ReplyStage::Guess, Some(Answer::Rows { rows })) => {
            r.guess = rows.into_iter().next().and_then(|row| {
                let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
                Some((
                    i32::try_from(evidence.get("userId")?.as_i64()?).ok()?,
                    evidence.get("sure")?.as_str()?.to_string(),
                    row.content,
                ))
            });
            match r.guess {
                Some((user_id, _, _)) => {
                    r.stage = ReplyStage::Label;
                    Replied::Wait(read(cx, Of::Others, ToRead::Label { user_id }))
                }
                None => now_read(cx, r),
            }
        }
        (ReplyStage::Label, Some(Answer::Label { label })) => {
            r.candidate = Some(label);
            now_read(cx, r)
        }
        (ReplyStage::Now, Some(Answer::Now { local, doing })) => {
            r.local = Some(local);
            r.doing = doing;
            r.stage = ReplyStage::Bits;
            Replied::Wait(venue_rows(cx, r, BIT_SOURCE))
        }
        (ReplyStage::Bits | ReplyStage::Days, Some(Answer::Rows { mut rows })) => {
            let older = rows.split_off(FRESH.min(rows.len()));
            match recall_start(cx, r, Of::Others, rows, older, BROUGHT_BACK) {
                Ok(id) => Replied::Wait(id),
                Err(rows) => after_recall(cx, r, rows),
            }
        }
        (ReplyStage::Theirs | ReplyStage::Heard, Some(Answer::Rows { rows })) => {
            let (of, limit) = if r.stage == ReplyStage::Theirs {
                (Of::Others, THEIRS_SHOWN)
            } else {
                (Of::Hers, HEARD_SHOWN)
            };
            match recall_start(cx, r, of, Vec::new(), rows, limit) {
                Ok(id) => Replied::Wait(id),
                Err(rows) => after_recall(cx, r, rows),
            }
        }
        (ReplyStage::Views, Some(Answer::Rows { rows })) => {
            let held: Vec<crate::views::Held> = rows
                .iter()
                .map(|row| crate::views::Held {
                    content: &row.content,
                    evidence: row.evidence.as_deref(),
                    concepts: &row.concepts,
                })
                .collect();
            r.views = crate::views::touched(&held, &r.words, VIEWS_SHOWN);
            r.stage = ReplyStage::Heard;
            let what = ToRead::OwnRows {
                source: HEARD_SOURCE.into(),
                limit: HEARD_HELD,
            };
            Replied::Wait(read(cx, Of::Hers, what))
        }
        (
            ReplyStage::Stickers,
            Some(Answer::Stickers {
                stickers,
                can_make,
                left,
            }),
        ) => {
            let offered = offered(stickers, &r.words);
            if can_make || !offered.is_empty() {
                offer(cx.group, &r.stranger.who, &offered);
                r.stickers = Some(offered);
                r.left = if can_make { left } else { 0 };
            }
            r.stage = ReplyStage::Memes;
            let what = ToRead::OwnRows {
                source: crate::memes::SOURCE.into(),
                limit: MEMES_HELD,
            };
            Replied::Wait(read(cx, Of::Hers, what))
        }
        (ReplyStage::Memes, Some(Answer::Rows { rows })) => {
            let known: Vec<crate::memes::Known> = rows
                .iter()
                .filter_map(|row| crate::memes::read_kept(&row.content))
                .collect();
            let talk = talk(r);
            r.memes = crate::memes::known_in(&talk, &known)
                .into_iter()
                .map(|known| (known.term.clone(), known.meaning.clone()))
                .collect();
            r.stage = ReplyStage::Jargon;
            let what = ToRead::OtherGroups {
                here: String::new(),
                sources: vec![JARGON_SOURCE.into()],
                limit: JARGON_READ_BACK,
            };
            Replied::Wait(read(cx, Of::Others, what))
        }
        (ReplyStage::Jargon, Some(Answer::Rows { rows })) => {
            r.jargon = rows;
            let venues = venues_named(r);
            if venues.is_empty() {
                return ask(cx, r);
            }
            r.stage = ReplyStage::Titles;
            Replied::Wait(read(cx, Of::Others, ToRead::Titles { venues }))
        }
        (ReplyStage::Titles, Some(Answer::Titles { titles })) => {
            r.titles = titles.into_iter().collect();
            ask(cx, r)
        }
        (ReplyStage::Asking, Some(Answer::Asked { raw, failure })) => match raw {
            Some(raw) => answered(cx, r, &raw),
            // A reply the provider dropped halfway reached no one: asked
            // once more.
            None if failure.as_deref() == Some("cut") && r.asks < 2 => ask_again(cx, r),
            None => Replied::Done(None),
        },
        _ => Replied::Done(None),
    }
}

fn now_read(cx: &mut Cx, r: &mut Replying) -> Replied {
    r.stage = ReplyStage::Now;
    Replied::Wait(read(cx, Of::Hers, ToRead::Now))
}

fn venue_rows(cx: &mut Cx, r: &mut Replying, source: &str) -> String {
    let what = ToRead::VenueRows {
        venue: stored(r),
        source: source.into(),
        newest: Newest::Updated,
        limit: LOOKED_OVER,
    };
    read(cx, Of::Others, what)
}

/// What their words brought back of one set, and the next read.
fn after_recall(cx: &mut Cx, r: &mut Replying, rows: Vec<MemoryRow>) -> Replied {
    match r.stage {
        ReplyStage::Bits => {
            r.bits = rows;
            r.stage = ReplyStage::Days;
            Replied::Wait(venue_rows(cx, r, DAY_SOURCE))
        }
        ReplyStage::Days => {
            r.days = rows;
            r.stage = ReplyStage::Theirs;
            let what = ToRead::OtherGroups {
                here: stored(r),
                sources: vec![BIT_SOURCE.into(), DAY_SOURCE.into()],
                limit: THEIRS_LOOKED_OVER,
            };
            Replied::Wait(read(cx, Of::Others, what))
        }
        ReplyStage::Theirs => {
            r.theirs = rows;
            r.stage = ReplyStage::Views;
            let what = ToRead::OwnRows {
                source: "view".into(),
                limit: VIEWS_HELD,
            };
            Replied::Wait(read(cx, Of::Hers, what))
        }
        ReplyStage::Heard => {
            r.heard = rows;
            r.stage = ReplyStage::Soup;
            let (venue, asker, words, owner) = (
                r.venue.clone(),
                r.stranger.name.clone(),
                r.words.clone(),
                r.owner,
            );
            seat_at_table(cx, r, AtTableDo::Turn, &venue, &asker, &words, owner)
        }
        _ => Replied::Done(None),
    }
}

/// The talk the words and memes she knows are looked for in: the latest
/// lines, newest first, and what they said.
fn talk(r: &Replying) -> String {
    r.transcript
        .iter()
        .rev()
        .take(TALK_LINES)
        .map(|line| line.content.clone())
        .chain(std::iter::once(r.words.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The stickers she may send, those their words touch first, then a
/// group's own, then those she sends most (as read).
pub(crate) fn offered(stickers: Vec<StickerRow>, words: &str) -> Vec<StickerRow> {
    let documents: Vec<myriad_agent_rules::memory::lexical::Document> = stickers
        .iter()
        .map(|sticker| myriad_agent_rules::memory::lexical::Document {
            text: &sticker.meaning,
            concepts: &[],
        })
        .collect();
    let mut order: Vec<(usize, f64)> =
        myriad_agent_rules::memory::lexical::score_all(words, &documents)
            .into_iter()
            .enumerate()
            .map(|(index, score)| {
                let touched = if score.strong { score.value } else { 0.0 };
                // A group's own stickers come before those for anywhere:
                // there are few, and they are why she made them.
                let own = if stickers[index].venue.is_some() {
                    1.0
                } else {
                    0.0
                };
                (index, touched + own)
            })
            .collect();
    // Stable: ties keep the most-sent-first order.
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    order
        .into_iter()
        .take(OFFERED)
        .map(|(index, _)| stickers[index].clone())
        .collect()
}

/// Remember what was offered to `who`.
pub(crate) fn offer(group: &mut Group, who: &str, stickers: &[StickerRow]) {
    if group.offered.len() >= OFFERS_KEPT && !group.offered.contains_key(who) {
        group.offered.clear();
    }
    group.offered.insert(
        who.to_string(),
        stickers.iter().map(|sticker| sticker.id.clone()).collect(),
    );
}

/// `venue` (`group:<platform>:<chat>` or `<platform>:<chat>`) without its
/// prefix.
fn unprefixed(venue: &str) -> &str {
    venue.strip_prefix("group:").unwrap_or(venue)
}

/// The groups whose names what she brings in says it came from.
fn venues_named(r: &Replying) -> Vec<String> {
    let mut venues = BTreeSet::new();
    for row in r.elsewhere.iter().chain(&r.theirs) {
        if let Some(venue) = row.venue.as_deref().and_then(|v| v.strip_prefix("group:")) {
            venues.insert(venue.to_string());
        }
    }
    for row in &r.heard {
        if let Some(group) = heard_in(row)
            && group != r.venue
        {
            venues.insert(unprefixed(&group).to_string());
        }
    }
    for sticker in r.stickers.iter().flatten() {
        if let Some(from) = &sticker.kept_from {
            venues.insert(unprefixed(from).to_string());
        }
    }
    let here = stored(r);
    let talk = talk(r);
    for row in &r.jargon {
        let Some(word) = crate::memes::read_kept(&row.content) else {
            continue;
        };
        if !talk.contains(&word.term) {
            continue;
        }
        if let Some(venue) = row.venue.as_deref().filter(|venue| *venue != here) {
            venues.insert(unprefixed(venue).to_string());
        }
    }
    venues.into_iter().collect()
}

fn title(r: &Replying, venue: &str) -> Option<String> {
    r.titles.get(unprefixed(venue)).cloned().flatten()
}

/// The group a thing she heard was heard in, as its evidence says.
fn heard_in(row: &MemoryRow) -> Option<String> {
    row.evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
        .and_then(|evidence| evidence.get("group")?.as_str().map(str::to_string))
}

fn evidence_of(row: &MemoryRow) -> Value {
    row.evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str(evidence).ok())
        .unwrap_or_default()
}

fn day_of(row: &MemoryRow) -> String {
    evidence_of(row)
        .get("day")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| lines::time(row.created_at).format("%Y-%m-%d").to_string())
}

/// Her whole prompt, put together.
fn prompt(cx: &Cx, r: &Replying) -> String {
    let now = cx.now();
    let name = &r.stranger.name;
    let elsewhere: Vec<crate::other_groups::FromGroup> = r
        .elsewhere
        .iter()
        .map(|row| crate::other_groups::FromGroup {
            group: row
                .venue
                .as_deref()
                .filter(|venue| venue.starts_with("group:"))
                .and_then(|venue| title(r, venue)),
            text: row.content.clone(),
        })
        .collect();
    let local = r
        .local
        .as_deref()
        .and_then(|local| chrono::DateTime::parse_from_rfc3339(local).ok())
        .unwrap_or_else(|| lines::time(now).fixed_offset());
    let mut sections = vec![
        crate::speaking::group_speaking_section(name),
        crate::speaking::chat_app_section().to_string(),
        crate::strangers::section(name, r.note.as_deref(), &elsewhere),
        crate::speaking::format_now_section(local),
    ];
    if let Some(shown_as) = &cx.group.shown_as {
        sections.insert(1, crate::speaking::shown_as_section(shown_as));
    }
    // Whom she takes them for, if anyone: a guess, nothing more.
    if let (Some((_, sure, why)), Some(candidate)) = (&r.guess, &r.candidate) {
        sections.push(crate::recognizing::section(name, candidate, sure, why));
    }
    sections.extend(crate::speaking::format_doing_section(
        r.doing.as_deref(),
        &[],
    ));
    let bits: Vec<(String, String)> = r
        .bits
        .iter()
        .filter_map(|row| {
            let handle = evidence_of(row).get("handle")?.as_str()?.trim().to_string();
            (!handle.is_empty()).then(|| (handle, row.content.clone()))
        })
        .collect();
    sections.extend(crate::speaking::format_bits_section(&bits, true));
    let mut days: Vec<(String, String)> = r
        .days
        .iter()
        .map(|row| (day_of(row), row.content.clone()))
        .collect();
    // Freshest first, then what was brought back: by date, oldest first.
    days.sort_by(|left, right| left.0.cmp(&right.0));
    sections.extend(crate::speaking::format_group_days_section(&days));
    let mut theirs = Vec::new();
    for row in &r.theirs {
        let evidence = evidence_of(row);
        let text = |key: &str| {
            evidence
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let brought = if row.source == BIT_SOURCE {
            let Some(handle) = text("handle") else {
                continue;
            };
            crate::other_groups::Theirs::Bit {
                handle,
                how: row.content.clone(),
            }
        } else {
            crate::other_groups::Theirs::Day {
                date: text("day")
                    .unwrap_or_else(|| lines::time(row.created_at).format("%Y-%m-%d").to_string()),
                what: row.content.clone(),
            }
        };
        let group = row
            .venue
            .as_deref()
            .filter(|venue| venue.starts_with("group:"))
            .and_then(|venue| title(r, venue));
        theirs.push((group, brought));
    }
    sections.extend(crate::other_groups::theirs_section(&theirs));
    sections.extend(crate::speaking::format_views_section(&r.views));
    // What she heard in groups about things, as with anyone in the group.
    let heard: Vec<crate::heard::Recalled> = r
        .heard
        .iter()
        .map(|row| {
            use crate::heard::Where;
            let heard = match heard_in(row) {
                Some(group) if group == r.venue => Where::Here,
                Some(group) => match title(r, &group) {
                    Some(name) => Where::In(name),
                    None => Where::SomeGroup,
                },
                None => Where::SomeGroup,
            };
            crate::heard::Recalled {
                thing: row.content.clone(),
                heard,
                ago: crate::doing::ago_text(lines::age(now, row.created_at)),
            }
        })
        .collect();
    sections.extend(crate::heard::section(&heard));
    // The group's turtle soup: anyone may ask, and so may they.
    sections.push(r.soup.clone());
    // Stickers of her: hers, and this group's own.
    if let Some(stickers) = &r.stickers {
        let meanings: Vec<String> = stickers
            .iter()
            .map(|sticker| {
                let line = crate::stickers::offered_line(
                    &sticker.meaning,
                    sticker.sent,
                    sticker.last_sent_at.map(|at| lines::age(now, at)),
                );
                match &sticker.kept_from {
                    Some(from) => crate::stickers::kept_line(&line, title(r, from).as_deref()),
                    None => line,
                }
            })
            .collect();
        sections.extend(crate::stickers::format_sticker_section(&meanings, r.left));
    }
    sections.extend(r.reading.iter().cloned());
    // Words and memes in the talk that she looked up before.
    let memes: Vec<crate::memes::Known> = r
        .memes
        .iter()
        .map(|(term, meaning)| crate::memes::Known {
            term: term.clone(),
            meaning: meaning.clone(),
        })
        .collect();
    sections.extend(crate::memes::section(&memes.iter().collect::<Vec<_>>()));
    // And the groups' own words in it.
    let here = stored(r);
    let talk = talk(r);
    let jargon: Vec<crate::jargon::Known> = r
        .jargon
        .iter()
        .filter_map(|row| {
            let word = crate::memes::read_kept(&row.content)?;
            if !talk.contains(&word.term) {
                return None;
            }
            let venue = row.venue.as_deref().unwrap_or_default();
            let (group, elsewhere) = if venue == here {
                (None, false)
            } else {
                (title(r, venue), true)
            };
            Some(crate::jargon::Known {
                term: word.term,
                meaning: word.meaning,
                group,
                elsewhere,
            })
        })
        .collect();
    sections.extend(crate::jargon::section(&jargon));
    // They did not call her: she speaks for a reason of her own.
    if let Some(why) = &r.why {
        sections.push(crate::joining::speaking_up_section(why));
    }
    let history: Vec<crate::chat_history::HistoryMessage> = r
        .transcript
        .iter()
        .map(|line| crate::chat_history::HistoryMessage {
            role: &line.role,
            content: &line.content,
            sent_at: None,
        })
        .collect();
    crate::chat_history::group_chat_prompt(&r.soul, &sections.join("\n\n"), &history, &r.words)
}

fn ask(cx: &mut Cx, r: &mut Replying) -> Replied {
    r.prompt = prompt(cx, r);
    ask_again(cx, r)
}

/// Ask her, within what is left of the turn.
fn ask_again(cx: &mut Cx, r: &mut Replying) -> Replied {
    let spent = u64::try_from((cx.now() - r.began) / 1_000).unwrap_or_default();
    let left = DEADLINE_MS.saturating_sub(spent);
    if left == 0 {
        return Replied::Done(None);
    }
    r.stage = ReplyStage::Asking;
    r.asks += 1;
    let (owner, input) = (r.owner, r.prompt.clone());
    Replied::Wait(cx.effect(|id| Effect::Ask {
        id,
        how: AskKind::Say,
        voice: "hers".into(),
        owner,
        operation: "group_stranger".into(),
        timeout_ms: REPLY_WITHIN_MS.min(left),
        system: String::new(),
        input,
        schema_name: String::new(),
        schema: Value::Null,
        turn: None,
    }))
}

/// What she said, taken apart: the sticker she chose, the puzzle she said
/// she would think up.
fn answered(cx: &mut Cx, r: &mut Replying, raw: &str) -> Replied {
    let (said, started) = crate::soup::split_start(raw);
    // A game with seats she said she would host: sign-up opens, and she
    // says how to join.
    let (mut said, seated) = crate::hosting::split_seat_game(&said);
    if let Some(kind) = seated {
        let to = Target {
            platform: r.venue.split(':').next().unwrap_or_default().to_string(),
            chat: r
                .venue
                .split_once(':')
                .map(|(_, chat)| chat.to_string())
                .unwrap_or_default(),
            thread: None,
            message_id: String::new(),
        };
        if let Some(line) = open_sign_up(cx, kind, to, r.owner) {
            said = format!("{said}\n\n{line}");
        }
    }
    let (said, choice) = crate::stickers::split_sticker_directive(&said);
    r.sticker = match choice {
        Some(crate::stickers::Choice::Send(number)) => cx
            .group
            .offered
            .get(&r.stranger.who)
            .and_then(|ids| ids.get(usize::from(number).checked_sub(1)?))
            .map(|id| serde_json::json!({ "send": id })),
        Some(crate::stickers::Choice::Make { picture, meaning }) => Some(serde_json::json!({
            "make": { "picture": picture, "meaning": meaning }
        })),
        None => None,
    };
    r.text = crate::strangers::without_directives(&said);
    // She said she would think one up: the puzzle follows her words.
    if started {
        r.stage = ReplyStage::Opening;
        let (venue, asker, words, owner) = (
            r.venue.clone(),
            r.stranger.name.clone(),
            r.words.clone(),
            r.owner,
        );
        return seat_at_table(cx, r, AtTableDo::Start, &venue, &asker, &words, owner);
    }
    finish(cx, r)
}

/// See to the game at the table.
fn seat_at_table(
    cx: &mut Cx,
    r: &mut Replying,
    what: AtTableDo,
    venue: &str,
    asker: &str,
    words: &str,
    owner: i32,
) -> Replied {
    let (table, next) = at_table(cx, what, venue, asker, words, owner, None);
    match next {
        AtTableNext::Wait(id) => {
            r.table = Some(table);
            Replied::Wait(id)
        }
        AtTableNext::Done(text) => at_table_done(cx, r, what, text),
    }
}

/// What came of the game at the table, and her answer going on.
fn at_table_done(cx: &mut Cx, r: &mut Replying, what: AtTableDo, text: Option<String>) -> Replied {
    match what {
        AtTableDo::Turn => {
            // The group's turtle soup: anyone may ask, and so may they.
            r.soup = text.unwrap_or_else(|| {
                let on = cx.group.seating.as_ref().map(|s| s.name.clone());
                crate::hosting::group_offer(on.as_deref())
            });
            r.stage = ReplyStage::Stickers;
            let venue = r.venue.clone();
            Replied::Wait(read(cx, Of::Hers, ToRead::Stickers { venue }))
        }
        AtTableDo::Start => {
            if let Some(opening) = text {
                r.text = format!("{}\n\n{opening}", r.text).trim().to_string();
            }
            finish(cx, r)
        }
        AtTableDo::After => {
            let text = std::mem::take(&mut r.text);
            let sticker = r.sticker.take();
            Replied::Done((!text.is_empty() || sticker.is_some()).then_some((text, sticker)))
        }
    }
}

fn finish(cx: &mut Cx, r: &mut Replying) -> Replied {
    // A game this line ended is over, and the group remembers it.
    let (venue, asker, words, owner) = (
        r.venue.clone(),
        r.stranger.name.clone(),
        r.words.clone(),
        r.owner,
    );
    r.stage = ReplyStage::After;
    seat_at_table(cx, r, AtTableDo::After, &venue, &asker, &words, owner)
}
