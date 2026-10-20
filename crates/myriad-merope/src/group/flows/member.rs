//! Her answer to a member of the community, in a group (D2c-8 A, D2c-9 A):
//! the group's own part of what she answers from is put together here (the
//! game on at the table, her stickers, the words and memes she knows and the
//! groups' own, how the group reads, the talk itself); what she knows of
//! this person and how she is come whole from the end that hosts her, which
//! opens the turn as one of theirs and closes it again, keeping what she
//! said. Asked once (once more when nothing came, or it was cut), taken
//! apart: the puzzle she said she would think up, the sticker she chose.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

const MEMES_HELD: u64 = 500;
const JARGON_SOURCE: &str = "jargon";
const JARGON_READ_BACK: u64 = 300;
/// The lines of the talk the words and memes in it are looked for in.
const TALK_LINES: usize = 12;
/// Her turn, from opening to closing, within this long.
const DEADLINE_MS: u64 = 90_000;
/// She shows she is typing this often while her turn runs.
const TYPING_EVERY_SECONDS: f64 = 4.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberReply {
    pub user_id: i32,
    /// The site's owner, who hosts her there: a game she opens is theirs
    /// to pay for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_owner: Option<i32>,
    pub venue: String,
    pub to: Target,
    pub said: String,
    pub transcript: Vec<Spoken>,
    pub speaker: String,
    #[serde(default)]
    pub chime: Option<String>,
    #[serde(default)]
    pub first: bool,
    #[serde(default)]
    pub room: Option<crate::talk_shape::Shape>,
    #[serde(default)]
    pub differs: Option<String>,
    #[serde(default)]
    pub making_sense: Option<String>,
    #[serde(default)]
    pub late: Option<String>,
    pub began: i64,
    pub stage: MemberStage,
    /// The turn the end opened for it.
    #[serde(default)]
    pub turn: Option<String>,
    #[serde(default)]
    pub sections: Vec<String>,
    #[serde(default)]
    pub doing: Option<String>,
    #[serde(default)]
    pub soul: String,
    #[serde(default)]
    pub memes: Vec<(String, String)>,
    #[serde(default)]
    pub jargon: Vec<MemoryRow>,
    #[serde(default)]
    pub game: Option<String>,
    #[serde(default)]
    pub stickers: Option<Vec<StickerRow>>,
    #[serde(default)]
    pub left: usize,
    #[serde(default)]
    pub titles: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub asks: u8,
    #[serde(default)]
    pub text: String,
    /// The sticker she chose, as the chat app sends it.
    #[serde(default)]
    pub sticker: Option<Value>,
    /// The game at the table, while she sees to it.
    #[serde(default)]
    pub table: Option<AtTable>,
    /// Whether a game her turn may have ended is to be put away after it.
    #[serde(default)]
    pub after_game: bool,
    /// What the closed turn gave back.
    #[serde(default)]
    pub reply: Option<String>,
    #[serde(default)]
    pub bound: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemberStage {
    Begin,
    Soul,
    Memes,
    Jargon,
    Game,
    Stickers,
    Titles,
    Asking,
    Opening,
    Ending,
}

/// What a step of her answer came to: waiting, or her words and sticker
/// (none: nothing to say), and whether whom she answered is still hers.
pub(crate) enum MemberNext {
    Wait(String),
    Done(Option<(String, Option<Value>)>, bool),
}

fn read(cx: &mut Cx, of: Of, what: ToRead) -> String {
    cx.effect(|id| Effect::Read { id, of, what })
}

/// Who her stickers are offered to, in the group's state.
fn offered_to(user_id: i32) -> String {
    format!("member:{user_id}")
}

/// Begin her answer to a member: the turn opened as theirs, and her typing
/// shown while it runs.
pub(crate) fn member_start(
    cx: &mut Cx,
    a: &Answering,
    differs: Option<String>,
    late: Option<String>,
) -> (MemberReply, MemberNext) {
    let reply = MemberReply {
        user_id: a.owner.unwrap_or_default(),
        site_owner: a.site_owner,
        venue: a.message.venue(),
        to: a.message.target(),
        said: a.said.clone(),
        transcript: a.transcript.clone(),
        speaker: a.message.display_name.clone(),
        chime: a.chime.clone(),
        first: a.first,
        room: a.room.clone(),
        differs,
        making_sense: a.making_sense.clone(),
        late,
        began: cx.now(),
        stage: MemberStage::Begin,
        turn: None,
        sections: Vec::new(),
        doing: None,
        soul: String::new(),
        memes: Vec::new(),
        jargon: Vec::new(),
        game: None,
        stickers: None,
        left: 0,
        titles: BTreeMap::new(),
        prompt: String::new(),
        asks: 0,
        text: String::new(),
        sticker: None,
        table: None,
        after_game: false,
        reply: None,
        bound: false,
    };
    let (user_id, input, transcript, speaker, chime, first) = (
        reply.user_id,
        reply.said.clone(),
        reply.transcript.clone(),
        reply.speaker.clone(),
        reply.chime.clone(),
        reply.first,
    );
    let id = cx.effect(|id| Effect::TurnBegin {
        id,
        user_id,
        input,
        transcript,
        speaker,
        chime,
        first,
    });
    start_typing(cx, &reply.to, &id);
    (reply, MemberNext::Wait(id))
}

/// She shows she is typing now and every few seconds while the turn under
/// `token` runs.
fn start_typing(cx: &mut Cx, to: &Target, token: &str) {
    cx.group.typing_for = Some(token.to_string());
    let target = to.clone();
    cx.effect(|id| Effect::Typing { id, to: target });
    let errand = cx.id();
    let key = cx.wake(TYPING_EVERY_SECONDS);
    cx.group.errands.push(Errand {
        id: errand,
        awaiting: key,
        kind: Kind::Typer {
            token: token.to_string(),
            to: to.clone(),
        },
        doing: Doing::Timing,
    });
}

/// A tick of her typing: shown again while her turn still runs.
pub(crate) fn typing_tick(cx: &mut Cx, mut errand: Errand) {
    let Kind::Typer { token, to } = &errand.kind else {
        return;
    };
    if cx.group.typing_for.as_deref() != Some(token.as_str()) {
        return;
    }
    let target = to.clone();
    cx.effect(|id| Effect::Typing { id, to: target });
    let key = cx.wake(TYPING_EVERY_SECONDS);
    errand.awaiting = key;
    cx.group.errands.push(errand);
}

fn stop_typing(cx: &mut Cx) {
    cx.group.typing_for = None;
}

/// The talk the words and memes she knows are looked for in: the latest
/// lines, newest first, and what they said.
fn talk(r: &MemberReply) -> String {
    r.transcript
        .iter()
        .rev()
        .take(TALK_LINES)
        .map(|line| line.content.as_str())
        .chain(std::iter::once(r.said.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Close the turn: with what she said, it is kept; without, it is left.
fn end(
    cx: &mut Cx,
    r: &mut MemberReply,
    said: Option<String>,
    sticker: Option<Value>,
) -> MemberNext {
    let Some(turn) = r.turn.clone() else {
        stop_typing(cx);
        return MemberNext::Done(None, true);
    };
    // A game this line ended is over, and the group remembers it: after
    // her turn is kept, as a Chat turn at the table does.
    r.after_game = said.is_some() && r.chime.is_none();
    r.stage = MemberStage::Ending;
    r.text = String::new();
    let (user_id, first) = (r.user_id, r.first);
    MemberNext::Wait(cx.effect(|id| Effect::TurnEnd {
        id,
        turn,
        user_id,
        said,
        sticker,
        first,
    }))
}

/// A step of her answer.
pub(crate) fn member_step(cx: &mut Cx, r: &mut MemberReply, answer: Option<Answer>) -> MemberNext {
    let now = cx.now();
    if let Some(mut table) = r.table.take() {
        return match at_table_step(cx, &mut table, answer) {
            AtTableNext::Wait(id) => {
                r.table = Some(table);
                MemberNext::Wait(id)
            }
            AtTableNext::Done(text) => at_table_done(cx, r, table.what, text),
        };
    }
    // Past the turn's deadline, she lets it go.
    if r.stage != MemberStage::Ending && now - r.began >= DEADLINE_MS as i64 * 1_000 {
        return end(cx, r, None, None);
    }
    match (r.stage, answer) {
        (
            MemberStage::Begin,
            Some(Answer::TurnBegun {
                turn,
                sections,
                doing,
            }),
        ) => {
            let Some(turn) = turn else {
                stop_typing(cx);
                return MemberNext::Done(None, true);
            };
            r.turn = Some(turn);
            r.sections = sections;
            r.doing = doing;
            r.stage = MemberStage::Soul;
            MemberNext::Wait(read(cx, Of::Hers, ToRead::Soul))
        }
        (MemberStage::Soul, Some(Answer::Soul { text })) => {
            r.soul = text;
            r.stage = MemberStage::Memes;
            let what = ToRead::OwnRows {
                source: crate::memes::SOURCE.into(),
                limit: MEMES_HELD,
            };
            MemberNext::Wait(read(cx, Of::Hers, what))
        }
        (MemberStage::Memes, Some(Answer::Rows { rows })) => {
            let known: Vec<crate::memes::Known> = rows
                .iter()
                .filter_map(|row| crate::memes::read_kept(&row.content))
                .collect();
            r.memes = crate::memes::known_in(&talk(r), &known)
                .into_iter()
                .map(|known| (known.term.clone(), known.meaning.clone()))
                .collect();
            r.stage = MemberStage::Jargon;
            let what = ToRead::OtherGroups {
                here: String::new(),
                sources: vec![JARGON_SOURCE.into()],
                limit: JARGON_READ_BACK,
            };
            MemberNext::Wait(read(cx, Of::Others, what))
        }
        (MemberStage::Jargon, Some(Answer::Rows { rows })) => {
            r.jargon = rows;
            // Her own line in, a game is on at the group's table: this one
            // judged, or how she would start one. Not when she speaks up
            // unasked: then it is not a turn at the table.
            if r.chime.is_some() {
                return stickers(cx, r);
            }
            r.stage = MemberStage::Game;
            let turn = r.turn.clone();
            seat_at_table(cx, r, AtTableDo::Turn, turn)
        }
        (
            MemberStage::Stickers,
            Some(Answer::Stickers {
                stickers,
                can_make,
                left,
            }),
        ) => {
            let offered = super::stranger::offered(stickers, &r.said);
            if can_make || !offered.is_empty() {
                super::stranger::offer(cx.group, &offered_to(r.user_id), &offered);
                r.stickers = Some(offered);
                r.left = if can_make { left } else { 0 };
            }
            let venues = venues_named(r);
            if venues.is_empty() {
                return ask(cx, r);
            }
            r.stage = MemberStage::Titles;
            MemberNext::Wait(read(cx, Of::Others, ToRead::Titles { venues }))
        }
        (MemberStage::Titles, Some(Answer::Titles { titles })) => {
            r.titles = titles.into_iter().collect();
            ask(cx, r)
        }
        (MemberStage::Asking, Some(Answer::Asked { raw, failure })) => match raw {
            Some(raw) if !raw.trim().is_empty() => answered(cx, r, &raw),
            // Nothing came, or the provider dropped it halfway: nothing
            // reached anyone, so she is asked once more.
            _ if r.asks < 2 && (failure.is_none() || failure.as_deref() == Some("cut")) => {
                ask_again(cx, r)
            }
            _ => end(cx, r, None, None),
        },
        (MemberStage::Ending, Some(Answer::TurnEnded { reply, bound })) => {
            stop_typing(cx);
            r.reply = reply;
            r.bound = bound;
            if r.after_game {
                return seat_at_table(cx, r, AtTableDo::After, None);
            }
            ended(r)
        }
        _ => end(cx, r, None, None),
    }
}

fn stickers(cx: &mut Cx, r: &mut MemberReply) -> MemberNext {
    r.stage = MemberStage::Stickers;
    let venue = r.venue.clone();
    MemberNext::Wait(read(cx, Of::Hers, ToRead::Stickers { venue }))
}

/// The groups whose names what she brings in says it came from.
fn venues_named(r: &MemberReply) -> Vec<String> {
    let mut venues = BTreeSet::new();
    for sticker in r.stickers.iter().flatten() {
        if let Some(from) = &sticker.kept_from {
            venues.insert(unprefixed(from).to_string());
        }
    }
    let here = super::sense::stored(&r.venue);
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

fn unprefixed(venue: &str) -> &str {
    venue.strip_prefix("group:").unwrap_or(venue)
}

fn title(r: &MemberReply, venue: &str) -> Option<String> {
    r.titles.get(unprefixed(venue)).cloned().flatten()
}

/// `block`, with `section` after it.
fn then(block: String, section: String) -> String {
    if block.is_empty() {
        section
    } else {
        format!("{block}\n\n{section}")
    }
}

/// Her whole prompt, put together.
fn prompt(cx: &Cx, r: &MemberReply) -> String {
    let now = cx.now();
    // How she is this moment goes last, nearest their words: what she
    // answers from, not one more thing among the rest.
    let (moment, sections): (Vec<&String>, Vec<&String>) = r
        .sections
        .iter()
        .partition(|section| crate::speaking::is_her_moment(section));
    let moment = moment
        .iter()
        .map(|section| section.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let with_moment = |block: String| {
        if moment.is_empty() {
            block
        } else if block.is_empty() {
            moment.clone()
        } else {
            format!("{block}\n\n{moment}")
        }
    };
    let mut block = sections
        .iter()
        .map(|section| section.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let hers: Vec<&str> = r
        .transcript
        .iter()
        .filter(|line| line.role == "assistant")
        .map(|line| line.content.as_str())
        .collect();
    // What they call her by there.
    if let Some(name) = &cx.group.shown_as {
        block = then(block, crate::speaking::shown_as_section(name));
    }
    // What of her own time she has already told them in this talk.
    if let Some(about) = &r.doing {
        let lines: Vec<&str> = hers.iter().flat_map(|reply| reply.lines()).collect();
        let told = crate::speaking::already_told(about, &lines);
        if let Some(section) = crate::speaking::format_already_told_section(&told) {
            block = then(block, section);
        }
    }
    // A habit of hers in this talk, when one shows: known, not ordered.
    if let Some(section) = crate::speaking::format_habits_section(&hers) {
        block = then(block, section);
    }
    // Words and memes in the talk that she looked up before.
    let memes: Vec<crate::memes::Known> = r
        .memes
        .iter()
        .map(|(term, meaning)| crate::memes::Known {
            term: term.clone(),
            meaning: meaning.clone(),
        })
        .collect();
    if let Some(section) = crate::memes::section(&memes.iter().collect::<Vec<_>>()) {
        block = then(block, section);
    }
    // In a group, the groups' own words in the talk.
    let here = super::sense::stored(&r.venue);
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
    if let Some(section) = crate::jargon::section(&jargon) {
        block = then(block, section);
    }
    // A turtle soup on in this group, with this line judged; or how she
    // would start one for the group.
    if let Some(game) = &r.game {
        block = format!("{block}\n\n{game}");
    }
    block = format!("{block}\n\n{}", crate::speaking::chat_app_section());
    if let Some(room) = &r.room {
        block = format!(
            "{block}\n\n{}",
            crate::talk_shape::describe(room, "How people type here")
        );
    }
    // What she made of the talk before answering: she answers from it.
    if let Some(sense) = &r.making_sense {
        block = format!("{block}\n\n{sense}");
    }
    // She only now sees the line she answers.
    if let Some(ago) = &r.late {
        block = format!("{block}\n\n{}", crate::joining::seeing_it_late(ago));
    }
    if let Some(differs) = &r.differs {
        block = format!("{block}\n\n{differs}");
    }
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
        if let Some(section) = crate::stickers::format_sticker_section(&meanings, r.left) {
            block = format!("{block}\n\n{section}");
        }
    }
    // Nobody called her by name: she chose to say something.
    if let Some(why) = &r.chime {
        block = format!(
            "{block}\n\n{}",
            crate::joining::speaking_up_section(why.trim())
        );
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
    crate::chat_history::group_chat_prompt(&r.soul, &with_moment(block), &history, &r.said)
}

fn ask(cx: &mut Cx, r: &mut MemberReply) -> MemberNext {
    r.prompt = prompt(cx, r);
    ask_again(cx, r)
}

/// Ask her, within what is left of the turn.
fn ask_again(cx: &mut Cx, r: &mut MemberReply) -> MemberNext {
    let spent = u64::try_from((cx.now() - r.began) / 1_000).unwrap_or_default();
    let left = DEADLINE_MS.saturating_sub(spent);
    if left == 0 {
        return end(cx, r, None, None);
    }
    r.stage = MemberStage::Asking;
    r.asks += 1;
    let (owner, input, turn) = (r.user_id, r.prompt.clone(), r.turn.clone());
    MemberNext::Wait(cx.effect(|id| Effect::Ask {
        id,
        how: AskKind::Say,
        voice: "hers".into(),
        owner,
        operation: "group_member".into(),
        timeout_ms: left,
        system: String::new(),
        input,
        schema_name: String::new(),
        schema: Value::Null,
        turn,
    }))
}

/// What she said, taken apart: the puzzle she said she would think up.
fn answered(cx: &mut Cx, r: &mut MemberReply, raw: &str) -> MemberNext {
    let (text, started) = crate::soup::split_start(raw);
    // A game with seats she said she would host: sign-up opens, and she
    // says how to join (not when she spoke up unasked).
    let (mut text, seated) = crate::hosting::split_seat_game(&text);
    if let Some(kind) = seated.filter(|_| r.chime.is_none())
        && let Some(line) = open_sign_up(cx, kind, r.to.clone(), r.site_owner.unwrap_or(r.user_id))
    {
        text = format!("{text}\n\n{line}");
    }
    // She said she would think one up: the puzzle follows her words (at the
    // group's table only, not when she spoke up unasked).
    if started && r.chime.is_none() {
        r.stage = MemberStage::Opening;
        r.text = text;
        let turn = r.turn.clone();
        return seat_at_table(cx, r, AtTableDo::Start, turn);
    }
    taken_apart(cx, r, text.trim())
}

/// See to the game at the table, within the turn when it runs in one.
fn seat_at_table(
    cx: &mut Cx,
    r: &mut MemberReply,
    what: AtTableDo,
    turn: Option<String>,
) -> MemberNext {
    let (venue, asker, words, owner) = (
        r.venue.clone(),
        r.speaker.clone(),
        r.said.clone(),
        r.user_id,
    );
    let (table, next) = at_table(cx, what, &venue, &asker, &words, owner, turn);
    match next {
        AtTableNext::Wait(id) => {
            r.table = Some(table);
            MemberNext::Wait(id)
        }
        AtTableNext::Done(text) => at_table_done(cx, r, what, text),
    }
}

/// What came of the game at the table, and her answer going on.
fn at_table_done(
    cx: &mut Cx,
    r: &mut MemberReply,
    what: AtTableDo,
    text: Option<String>,
) -> MemberNext {
    match what {
        // A turtle soup on in this group, with this line judged; or how she
        // would start one for the group.
        AtTableDo::Turn => {
            r.game = Some(text.unwrap_or_else(|| {
                let on = cx.group.seating.as_ref().map(|s| s.name.clone());
                crate::hosting::group_offer(on.as_deref())
            }));
            stickers(cx, r)
        }
        AtTableDo::Start => {
            let text = match text {
                Some(opening) => format!("{}\n\n{opening}", r.text),
                None => r.text.clone(),
            };
            taken_apart(cx, r, text.trim())
        }
        AtTableDo::After => ended(r),
    }
}

fn ended(r: &mut MemberReply) -> MemberNext {
    let sticker = r.sticker.take();
    MemberNext::Done(r.reply.take().map(|reply| (reply, sticker)), r.bound)
}

/// Her words, the sticker she chose taken out, and the turn closed with them.
fn taken_apart(cx: &mut Cx, r: &mut MemberReply, text: &str) -> MemberNext {
    let (text, choice) = crate::stickers::split_sticker_directive(text);
    let sticker = match choice {
        Some(crate::stickers::Choice::Send(number)) => cx
            .group
            .offered
            .get(&offered_to(r.user_id))
            .and_then(|ids| ids.get(usize::from(number).checked_sub(1)?))
            .map(|id| serde_json::json!({ "send": id })),
        Some(crate::stickers::Choice::Make { picture, meaning }) => Some(serde_json::json!({
            "make": { "picture": picture, "meaning": meaning }
        })),
        None => None,
    };
    r.sticker = sticker.clone();
    end(cx, r, Some(text), sticker)
}
