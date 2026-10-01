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
            .map(as_message)
            .collect()
    })
    .unwrap_or_default()
}

fn as_message(line: &Line) -> ConversationMessage {
    ConversationMessage {
        role: if line.hers { "assistant" } else { "user" }.into(),
        content: if line.hers {
            line.text.clone()
        } else {
            format!("{}：{}", line.name, line.said())
        },
        created_at: None,
    }
}

/// What she reads when she gets to a line that calls her.
pub(super) struct Reading {
    /// The group's lines as of now, oldest first: the line in its place, and
    /// whatever came after it while she was getting to it.
    pub(super) transcript: Vec<ConversationMessage>,
    /// What they said to her: the line, with whatever more the same person
    /// added after it before she answered, as one turn.
    pub(super) said: String,
}

/// Whether `message` was already answered with a line before it (see
/// [`reading`]).
pub(super) fn read_already(message: &GroupLine) -> bool {
    with_group(&message.venue(), |group| {
        group.read_with.contains(&message.message_id)
    })
    .unwrap_or(false)
}

/// Read the group as it is when she gets to `message`: a person reading a
/// line that calls them sees what came after it too, so "你听过这首吗" the
/// same person added a moment later is part of what she answers, not a
/// line for another turn. Their added lines that were waiting for her are
/// answered with it; anyone else's still are, after.
pub(super) fn reading(message: &GroupLine) -> Reading {
    let venue = message.venue();
    let fallback = || Reading {
        transcript: transcript(&venue, Some(&message.message_id)),
        said: said_now(message),
    };
    with_group(&venue, |group| {
        let lines: Vec<&Line> = group
            .lines
            .iter()
            .filter(|line| within(line, TRANSCRIPT_FOR))
            .collect();
        let at = lines.iter().position(|line| {
            !line.hers && line.message_id.as_deref() == Some(message.message_id.as_str())
        })?;
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
        let transcript = lines.iter().map(|line| as_message(line)).collect();
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
        Some(Reading { transcript, said })
    })
    .flatten()
    .unwrap_or_else(fallback)
}

/// The group's people lately who are paired site users, each by the name
/// the group knows them by: what she calls them there, never their name on
/// the site, which the group may not know.
pub async fn names_here(db: &DatabaseConnection, venue: &str) -> HashMap<i32, String> {
    let Some(platform) = venue.split_once(':').and_then(|(slug, _)| {
        ChannelPlatform::ALL
            .into_iter()
            .find(|platform| platform.slug() == slug)
    }) else {
        return HashMap::new();
    };
    let mut names = HashMap::new();
    for (name, from) in people(venue) {
        if let Ok(PairingLookup::Paired { user_id }) =
            crate::services::channel_pairing::lookup_openid(db, PairingChannel::of(platform), &from)
                .await
        {
            names.entry(user_id).or_insert(name);
        }
    }
    names
}
