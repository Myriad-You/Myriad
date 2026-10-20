//! A group's recent lines: as she keeps them, and as the conversation she
//! reads and answers in.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::*;

/// A line as she keeps it in mind (and as the end keeps it across a
/// restart: this shape is the stored one, field names and all).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub at: DateTime<Utc>,
    pub message_id: Option<String>,
    pub name: String,
    /// Who said it, as the platform knows them: how she can mention them.
    #[serde(default)]
    pub from: Option<String>,
    pub text: String,
    pub hers: bool,
    /// Whether it called her.
    #[serde(default)]
    pub addressed: bool,
    /// Pictures in it, and what she saw of each once she looked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<GroupImage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen: Vec<Option<Seen>>,
}

/// How a line that @-ed her reads: the platforms take her @ out of the text,
/// and without it a 「我喜欢你」 @-ed at her read as said to no one.
pub const AT_YOU: &str = "@你";

impl Line {
    /// What it says, with its pictures as she saw them, and [`AT_YOU`] in
    /// front when it @-ed her.
    pub fn said(&self) -> String {
        let pictures: Vec<String> = self
            .images
            .iter()
            .enumerate()
            .map(|(index, image)| {
                crate::seeing::as_said(
                    self.seen.get(index).and_then(Option::as_ref),
                    image.hint.as_deref(),
                    image.sticker,
                )
            })
            .collect();
        let at = if self.addressed && !self.hers {
            AT_YOU
        } else {
            ""
        };
        [at.to_string(), self.text.clone(), pictures.join("")]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub(super) fn at_micros(&self) -> i64 {
        self.at.timestamp_micros()
    }
}

pub(super) fn time(micros: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_micros(micros).unwrap_or_default()
}

pub(super) fn age(now: i64, at: i64) -> chrono::Duration {
    chrono::Duration::microseconds(now - at)
}

pub(super) fn seconds(seconds: f64) -> i64 {
    (seconds * 1_000_000.0) as i64
}

/// Whether a line is still one she keeps in mind.
pub(super) fn within(line: &Line, now: i64) -> bool {
    let age = now - line.at_micros();
    age < 0 || age < TRANSCRIPT_FOR_SECONDS * 1_000_000
}

pub(super) fn push_line(group: &mut Group, line: Line, now: i64) {
    group.lines.retain(|line| within(line, now));
    group.lines.push_back(line);
    while group.lines.len() > TRANSCRIPT_LINES {
        group.lines.pop_front();
    }
}

/// Lines kept from before a restart, merged under the ones heard since.
pub(super) fn merge_restored(group: &mut Group, stored: Vec<Line>, now: i64) {
    let mut lines: Vec<Line> = stored;
    for line in group.lines.drain(..) {
        let known = line.message_id.is_some()
            && lines.iter().any(|kept| kept.message_id == line.message_id);
        if !known {
            lines.push(line);
        }
    }
    lines.sort_by_key(|line| line.at);
    for line in lines {
        push_line(group, line, now);
    }
}

/// Keep a line in mind; whether it was new (a line handed in again is
/// already there). A new one is kept for a restart too.
pub(super) fn remember_line(group: &mut Group, line: Line, now: i64, out: &mut Outcome) -> bool {
    let again = line.message_id.as_ref().is_some_and(|id| {
        group
            .lines
            .iter()
            .any(|kept| kept.message_id.as_ref() == Some(id))
    });
    if again {
        return false;
    }
    push_line(group, line, now);
    out.effects.push(Effect::KeepLines {
        lines: group.lines.iter().cloned().collect(),
    });
    true
}

pub(super) fn bounded(text: &str) -> String {
    text.chars().take(MAX_LINE_CHARS).collect()
}

/// A line of hers in the group.
pub(super) fn record_hers(group: &mut Group, text: &str, now: i64, out: &mut Outcome) {
    let line = Line {
        at: time(now),
        message_id: None,
        name: String::new(),
        from: None,
        text: bounded(text),
        hers: true,
        addressed: false,
        images: Vec::new(),
        seen: Vec::new(),
    };
    remember_line(group, line, now, out);
}

/// `@123` (someone @-ed by their platform id, as QQ gives it without a
/// name) as `@name`, for whoever spoke in the group lately.
pub fn by_name(text: &str, people: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at + 1]);
        rest = &rest[at + 1..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            continue;
        }
        let id = &rest[..digits];
        match people.iter().find(|(_, from)| from == id) {
            Some((name, _)) if !name.is_empty() => out.push_str(name),
            _ => out.push_str(id),
        }
        rest = &rest[digits..];
    }
    out.push_str(rest);
    out
}

/// Who she can mention in the group: whoever spoke there lately, by the
/// name they showed last (name, platform id).
pub fn people(group: &Group, now: i64) -> Vec<(String, String)> {
    let mut people: Vec<(String, String)> = Vec::new();
    for line in group.lines.iter().rev().filter(|line| within(line, now)) {
        let Some(from) = line.from.as_ref().filter(|_| !line.hers) else {
            continue;
        };
        if !people.iter().any(|(_, id)| id == from) {
            people.push((line.name.clone(), from.clone()));
        }
    }
    people
}

/// How the group's people type there lately (their lines, not hers).
pub fn room(group: &Group, now: i64) -> Option<crate::talk_shape::Shape> {
    let lines: Vec<(&str, i64, &str)> = group
        .lines
        .iter()
        .filter(|line| !line.hers && within(line, now))
        .map(|line| {
            (
                line.from.as_deref().unwrap_or(line.name.as_str()),
                line.at.timestamp(),
                line.text.as_str(),
            )
        })
        .collect();
    crate::talk_shape::room_of(&lines)
}

fn as_spoken(line: &Line) -> Spoken {
    Spoken {
        role: if line.hers { "assistant" } else { "user" }.into(),
        content: if line.hers {
            line.text.clone()
        } else {
            format!("{}：{}", line.name, line.said())
        },
    }
}

/// The group's recent lines before `message_id` (all of them without one),
/// oldest first.
pub fn transcript(group: &Group, message_id: Option<&str>, now: i64) -> Vec<Spoken> {
    group
        .lines
        .iter()
        .filter(|line| within(line, now))
        .take_while(|line| message_id.is_none() || line.message_id.as_deref() != message_id)
        .map(as_spoken)
        .collect()
}

/// What this line says as she has it in mind, pictures as she saw them.
pub fn said_now(group: &Group, message: &Message) -> String {
    group
        .lines
        .iter()
        .rev()
        .find(|line| !line.hers && line.message_id.as_deref() == Some(&message.message_id))
        .map(Line::said)
        .unwrap_or_else(|| message.said())
}

/// How long ago `message` was said in its group, while its line is kept.
pub fn said_ago(group: &Group, message: &Message, now: i64) -> Option<chrono::Duration> {
    group
        .lines
        .iter()
        .rev()
        .find(|line| !line.hers && line.message_id.as_deref() == Some(&message.message_id))
        .map(|line| age(now, line.at_micros()))
}

/// How long ago `message` was said, when that is long enough ago that she
/// knows she is only now seeing it.
pub fn seen_late(group: &Group, message: &Message, now: i64) -> Option<String> {
    said_ago(group, message, now)
        .filter(|age| *age >= chrono::Duration::minutes(LATE_MINUTES))
        .map(crate::doing::ago_text)
}

/// What she reads when she gets to a line that calls her.
pub struct Reading {
    pub transcript: Vec<Spoken>,
    pub said: String,
}

/// Whether `message` was already answered with a line before it.
pub fn read_already(group: &Group, message: &Message) -> bool {
    group.read_with.contains(&message.message_id)
}

/// Read the group as it is when she gets to `message`: what came after it
/// is in the talk, and what the same person added is answered with it.
pub fn reading(group: &mut Group, message: &Message, now: i64) -> Reading {
    let lines: Vec<&Line> = group
        .lines
        .iter()
        .filter(|line| within(line, now))
        .collect();
    let Some(at) = lines.iter().position(|line| {
        !line.hers && line.message_id.as_deref() == Some(message.message_id.as_str())
    }) else {
        return Reading {
            transcript: transcript(group, Some(&message.message_id), now),
            said: said_now(group, message),
        };
    };
    let added: Vec<(String, String)> = lines[at + 1..]
        .iter()
        .take_while(|line| !line.hers)
        .filter(|line| line.from.as_deref() == Some(message.from.as_str()))
        .filter_map(|line| Some((line.message_id.clone()?, line.said())))
        .collect();
    let said = std::iter::once(lines[at].said())
        .chain(added.iter().map(|(_, said)| said.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    let transcript = lines.iter().map(|line| as_spoken(line)).collect();
    let added: Vec<String> = added.into_iter().map(|(id, _)| id).collect();
    group
        .waiting
        .retain(|line| !(line.from == message.from && added.contains(&line.message_id)));
    group.read_with.extend(added.iter().cloned());
    while group.read_with.len() > WAITING_LINES {
        group.read_with.pop_front();
    }
    if group
        .pending
        .as_ref()
        .is_some_and(|line| added.contains(&line.message_id))
    {
        group.pending = None;
        group.unjudged_since = None;
    }
    Reading { transcript, said }
}

/// The newest line someone else said in the group, as a line to answer.
pub fn newest_theirs(group: &Group, like: &Message) -> Option<Message> {
    let line = group.lines.iter().rev().find(|line| !line.hers).cloned()?;
    let message_id = line.message_id?;
    (message_id != like.message_id).then(|| Message {
        platform: like.platform.clone(),
        chat: like.chat.clone(),
        message_id,
        thread: like.thread,
        from: line.from.unwrap_or_default(),
        display_name: line.name,
        text: line.text,
        addressed: line.addressed,
        reply_to: None,
        images: line.images,
        shown_as: None,
    })
}

/// What the group said after `since`, not hers, oldest first: who, what,
/// and whether it was the one she answers or called her.
pub fn came_since(group: &Group, since: i64, from: &str) -> Vec<(String, String, bool)> {
    group
        .lines
        .iter()
        .filter(|said| !said.hers && said.at_micros() > since)
        .map(|said| {
            let theirs = !from.is_empty() && said.from.as_deref() == Some(from);
            (said.name.clone(), said.said(), theirs || said.addressed)
        })
        .collect()
}

/// The conversation as a judgment reads it: the latest `at_most` lines,
/// hers as `you：…`.
pub(super) fn conversation(lines: &[Spoken], at_most: usize) -> Vec<String> {
    lines
        .iter()
        .skip(lines.len().saturating_sub(at_most))
        .map(|line| {
            if line.role == "assistant" {
                format!("you：{}", line.content)
            } else {
                line.content.clone()
            }
        })
        .collect()
}

/// Lines kept of a group, as its talk read back (oldest first, hers
/// marked).
pub fn kept_as_heard(
    platform: &str,
    chat: &str,
    thread: Option<i64>,
    lines: Vec<Line>,
) -> Vec<Past> {
    lines
        .into_iter()
        .map(|line| Past {
            at: line.at_micros(),
            hers: line.hers,
            message: Message {
                platform: platform.to_string(),
                chat: chat.to_string(),
                message_id: line.message_id.unwrap_or_default(),
                thread,
                from: line.from.unwrap_or_default(),
                display_name: line.name,
                text: line.text,
                addressed: line.addressed,
                reply_to: None,
                images: line.images,
                shown_as: None,
            },
        })
        .collect()
}
