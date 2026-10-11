//! The conversation as she has it in front of her in a chat
//! (`myriad_merope::chat_history`).

use myriad_merope::chat_history::{self, HistoryMessage};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{input, opt_at, text};
use crate::Failure;

/// A stored message, oldest first in a history.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Message {
    role: String,
    content: String,
    #[serde(default)]
    sent_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    history: Vec<Message>,
}

/// When each message was sent, if known.
type Sent = Vec<Option<chrono::DateTime<chrono::Utc>>>;

impl History {
    fn read(raw: &[u8]) -> Result<(Vec<Message>, Sent), Failure> {
        let req: Self = input(raw)?;
        let sent = req
            .history
            .iter()
            .map(|message| opt_at(message.sent_at))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((req.history, sent))
    }
}

fn messages<'a>(
    history: &'a [Message],
    sent: &[Option<chrono::DateTime<chrono::Utc>>],
) -> Vec<HistoryMessage<'a>> {
    history
        .iter()
        .zip(sent)
        .map(|(message, sent_at)| HistoryMessage {
            role: &message.role,
            content: &message.content,
            sent_at: *sent_at,
        })
        .collect()
}

pub(super) fn history_text(raw: &[u8]) -> Result<Value, Failure> {
    let (history, sent) = History::read(raw)?;
    Ok(text(Some(chat_history::chat_history_text(&messages(
        &history, &sent,
    )))))
}

pub(super) fn history_lines(raw: &[u8]) -> Result<Value, Failure> {
    let (history, sent) = History::read(raw)?;
    let lines: Vec<Value> = chat_history::chat_history_lines(&messages(&history, &sent))
        .into_iter()
        .map(|line| json!({ "index": line.index, "said": line.said, "pause": line.pause }))
        .collect();
    Ok(json!({ "lines": lines }))
}

pub(super) fn safe_content(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        content: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(chat_history::chat_safe_content(&req.content))))
}

pub(super) fn pause_text(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        seconds: i64,
    }
    let req: In = input(raw)?;
    Ok(text(Some(chat_history::pause_text(req.seconds))))
}
