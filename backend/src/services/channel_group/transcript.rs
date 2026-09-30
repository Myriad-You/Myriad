//! A group's recent lines as the conversation she answers in.

use super::*;

/// What this line says as she has it in mind, pictures as she saw them.
pub(super) fn said_now(message: &GroupLine) -> String {
    with_group(&message.venue(), |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| !line.hers && line.message_id.as_deref() == Some(&message.message_id))
            .map(Line::said)
    })
    .flatten()
    .unwrap_or_else(|| message.said())
}

/// How long ago `message` was said, when that is long enough ago that she
/// knows she is only now seeing it.
pub(super) fn seen_late(message: &GroupLine) -> Option<String> {
    said_ago(message)
        .filter(|age| *age >= LATE)
        .map(myriad_merope::doing::ago_text)
}

/// How long ago `message` was said in its group, while its line is kept.
pub(super) fn said_ago(message: &GroupLine) -> Option<chrono::Duration> {
    with_group(&message.venue(), |group| {
        group
            .lines
            .iter()
            .rev()
            .find(|line| !line.hers && line.message_id.as_deref() == Some(&message.message_id))
            .map(|line| chrono::Utc::now() - line.at)
    })
    .flatten()
}

/// `@123` (someone @-ed by their platform id, as QQ gives it without a
/// name) as `@name`, for whoever spoke in the group lately.
pub(super) fn by_name(text: &str, people: &[(String, String)]) -> String {
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
/// name they showed last.
pub(super) fn people(venue: &str) -> Vec<(String, String)> {
    with_group(venue, |group| {
        let mut people: Vec<(String, String)> = Vec::new();
        for line in group
            .lines
            .iter()
            .rev()
            .filter(|line| within(line, TRANSCRIPT_FOR))
        {
            let Some(from) = line.from.as_ref().filter(|_| !line.hers) else {
                continue;
            };
            if !people.iter().any(|(_, id)| id == from) {
                people.push((line.name.clone(), from.clone()));
            }
        }
        people
    })
    .unwrap_or_default()
}

/// How the group's people type there lately (their lines, not hers), once
/// there is enough of it.
pub(super) fn room(venue: &str) -> Option<myriad_merope::talk_shape::Shape> {
    with_group(venue, |group| {
        let lines: Vec<(&str, i64, &str)> = group
            .lines
            .iter()
            .filter(|line| !line.hers && within(line, TRANSCRIPT_FOR))
            .map(|line| {
                (
                    line.from.as_deref().unwrap_or(line.name.as_str()),
                    line.at.timestamp(),
                    line.text.as_str(),
                )
            })
            .collect();
        myriad_merope::talk_shape::room_of(&lines)
    })
    .flatten()
}

/// The group's recent lines before `message_id` (all of them without one),
/// oldest first. Others' lines carry their name; hers are her own turns.
pub(super) fn transcript(venue: &str, message_id: Option<&str>) -> Vec<ConversationMessage> {
    with_group(venue, |group| {
        group
            .lines
            .iter()
            .filter(|line| within(line, TRANSCRIPT_FOR))
            .take_while(|line| message_id.is_none() || line.message_id.as_deref() != message_id)
            .map(|line| ConversationMessage {
                role: if line.hers { "assistant" } else { "user" }.into(),
                content: if line.hers {
                    line.text.clone()
                } else {
                    format!("{}：{}", line.name, line.said())
                },
                created_at: None,
            })
            .collect()
    })
    .unwrap_or_default()
}
