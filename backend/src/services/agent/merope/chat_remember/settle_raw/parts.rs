//! A pass as the core is shown it: each of their lines and hers a line of
//! its own, a long one part by part ([`raw::line_parts`]), each part with
//! when its line was said and the message it is of, so a quote runs on from
//! one part into the next only within one message; and back from the parts
//! to the messages, which of them were let go whole.

use myriad_merope::settle_raw::{self as raw, Who};

use super::Spoken;

/// When a line was said, as she is shown it.
pub(super) fn said_at(at: chrono::DateTime<chrono::FixedOffset>) -> String {
    at.with_timezone(&crate::services::agent::merope::clock::zone())
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

pub(super) fn shown<'a>(lines: &'a [Spoken], at: &'a [Option<String>]) -> Vec<raw::Line<'a>> {
    lines
        .iter()
        .zip(at)
        .map(|(line, at)| as_shown(line, at.as_deref(), None, &line.text))
        .collect()
}

fn as_shown<'a>(
    line: &'a Spoken,
    at: Option<&'a str>,
    message: Option<&'a str>,
    text: &'a str,
) -> raw::Line<'a> {
    raw::Line {
        who: if line.theirs { Who::Them } else { Who::You },
        text,
        in_game: line.in_game,
        before: line.gone,
        at,
        message,
    }
}

/// Of each of `lines`, its message (`agent_messages.id`) as the core is
/// told it: a quote runs on from one part into the next only within one
/// message, never across two said in the same minute.
pub(super) fn messages_of(lines: &[Spoken]) -> Vec<String> {
    lines.iter().map(|line| line.id.to_string()).collect()
}

/// `lines` as a pass is shown them, a long one part by part
/// ([`raw::line_parts`]): the parts, and of each its line's index. Each part
/// is accounted for, and is of its line's message (`messages`,
/// [`messages_of`]).
pub(super) fn in_parts<'a>(
    lines: &'a [Spoken],
    at: &'a [Option<String>],
    messages: &'a [String],
) -> (Vec<raw::Line<'a>>, Vec<usize>) {
    let mut parts = Vec::new();
    let mut of = Vec::new();
    for (index, ((line, at), message)) in lines.iter().zip(at).zip(messages).enumerate() {
        for part in raw::line_parts(&line.text) {
            parts.push(as_shown(line, at.as_deref(), Some(message), part));
            of.push(index);
        }
    }
    (parts, of)
}

/// `lines`, each part of a long one a line of its own of the same message,
/// so a pass may end within it; of the one gone over in part, the parts that
/// were are gone.
pub(in crate::services::agent::merope) fn by_part(
    lines: Vec<Spoken>,
    partway: Option<(i32, usize)>,
) -> Vec<Spoken> {
    let mut parts = Vec::new();
    for line in lines {
        for (nth, text) in raw::line_parts(&line.text).into_iter().enumerate() {
            let gone = partway.is_some_and(|(id, done)| line.id == id && nth < done);
            parts.push(Spoken {
                text: text.to_string(),
                gone: line.gone || gone,
                ..line.clone()
            });
        }
    }
    parts
}

/// Of the messages `lines` are of, those said whole in `let_go` (indexes):
/// one only a part of which they took back is not let go.
pub(super) fn whole(
    lines: &[Spoken],
    parts: &[raw::Line],
    of: &[usize],
    let_go: &[usize],
) -> Vec<i32> {
    let mut ids: Vec<i32> = Vec::new();
    for index in let_go.iter().filter_map(|part| of.get(*part)) {
        let id = lines[*index].id;
        let all = (0..parts.len())
            .filter(|part| lines[of[*part]].id == id)
            .all(|part| let_go.contains(&part) || (!parts[part].before && !parts[part].to_place()));
        if all && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}
