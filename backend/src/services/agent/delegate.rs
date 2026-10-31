//! Handing work off from chat, over IM.
//!
//! In a private IM conversation she talks as herself (Chat). When they ask
//! her to get something done, she hands it off: she says so, and puts one
//! `[[work: …]]` line at the end with what to do. The channel then starts an
//! ordinary Work run for them with that instruction, as if they had asked for
//! it themselves: the same granted permissions, the same confirmations and
//! questions, delivered the same way. Her own ideas she asks about first.
//!
//! Each thing she hands off is a piece of its own, numbered in the chat
//! (`#1`, `#2`, …), and several can be under way at once. What they say
//! later for one of them she passes on with `[[work #N: …]]`: the answer to
//! its question if it is waiting for one, else a new instruction into it,
//! nothing started over (a piece that is done picks up again in its own
//! session). `[[stop #N]]` stops that one alone; a stopped one stays stopped.
//!
//! The next time they talk she sees every piece: what it is, how far it got,
//! whether it waits for them. Only a channel sets this up
//! (`RequestContext::channel_chat`, never from a web request); the web panel
//! keeps its own two modes.

use serde::{Deserialize, Serialize};

use crate::services::agent::types::ChannelChat;

const OPEN: &str = "[[";
const CLOSE: &str = "]]";
const MAX_INSTRUCTION_CHARS: usize = 500;

/// One line of hers for the channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HandOff {
    /// A new piece: what to do.
    New { text: String },
    /// For piece `work`: the answer to its question, or more to go on.
    To { work: u32, text: String },
    /// Stop piece `work`, and only it.
    Stop { work: u32 },
}

/// `work: …`, `work #2: …`, `stop #2`, as she writes them inside `[[ ]]`.
fn parse(inner: &str) -> Option<Option<HandOff>> {
    let inner = inner.trim();
    let number = |rest: &str| -> Option<u32> {
        rest.trim().trim_start_matches(['#', '＃']).trim().parse().ok()
    };
    if let Some(rest) = inner.strip_prefix("stop") {
        return Some(number(rest).map(|work| HandOff::Stop { work }));
    }
    let rest = inner.strip_prefix("work")?;
    let (head, text) = rest.split_once([':', '：'])?;
    let text: String = text.trim().chars().take(MAX_INSTRUCTION_CHARS).collect();
    if text.is_empty() {
        return Some(None);
    }
    if head.trim().is_empty() {
        return Some(Some(HandOff::New { text }));
    }
    Some(number(head).map(|work| HandOff::To { work, text }))
}

/// Take her hand-off lines out of a reply: the text without them, and what
/// they say, in the order she wrote them.
pub fn split(raw: &str) -> (String, Vec<HandOff>) {
    let mut text = raw.to_string();
    let mut handed = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(OPEN) {
        let start = from + found;
        let after = start + OPEN.len();
        let Some(close) = text[after..].find(CLOSE) else {
            break;
        };
        match parse(&text[after..after + close]) {
            // Someone else's line (a sticker, an outfit): left for them.
            None => from = after,
            Some(line) => {
                handed.extend(line);
                text.replace_range(start..after + close + CLOSE.len(), "");
                from = start;
            }
        }
    }
    (text.trim_end().to_string(), handed)
}

/// What she knows about handing work off, for a private IM chat.
pub fn section(chat: &ChannelChat) -> String {
    let mut lines = vec![
        "## Getting things done".to_string(),
        "You cannot use tools yourself in this chat, but a helper can: looking things up on the web, working with their data on this site, making or changing things for them. \
When they ask you to get something like that done, say in a few words that you are on it, and put [[work: what exactly to do, in one line, keeping their own words]] on its own last line. \
The helper does it with their permissions; any question or confirmation it needs goes to them directly, and the result comes back to them. \
If it is your own idea, ask first and hand it off only after they say yes. Do not hand off plain conversation. Do not read these lines aloud."
            .to_string(),
        "Each thing you hand off is a piece of its own, numbered, and several can be under way at once. \
When what they say now is for one of them (more to go on, a change of mind, or the answer to its question), put [[work #N: their words, or the answer]] on its own last line instead of a new one: it goes into that piece, nothing is started over, and a piece that is done picks up again where it ended. \
Numbers are given by the channel: use #N only for a piece listed below, never for a new one. \
When a piece is waiting on a question with numbered choices, answer with the number of the choice they mean. \
When they want one of them stopped, put [[stop #N]]: only that one stops. \
If you cannot tell which one they mean, ask them in a few words, with no line."
            .to_string(),
    ];
    if chat.waiting > 1 {
        lines.push(format!(
            "{} pieces are waiting for their answer: if what they said answers one, say which with [[work #N: the answer]], giving the number of the choice when its question has numbered choices; if you cannot tell which, ask.",
            chat.waiting
        ));
    }
    if !chat.works.is_empty() {
        lines.push(myriad_agent_rules::untrusted_block(
            "handed_off",
            &chat.works.join("\n"),
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(text: &str) -> HandOff {
        HandOff::New { text: text.into() }
    }

    #[test]
    fn her_hand_off_line_is_taken_out_of_what_she_says() {
        assert_eq!(
            split("好，我让助手去查。\n[[work: 查一下明天东京的天气]]"),
            ("好，我让助手去查。".into(), vec![new("查一下明天东京的天气")])
        );
        assert_eq!(split("今天好累"), ("今天好累".into(), vec![]));
        assert_eq!(split("嗯[[work:   ]]"), ("嗯".into(), vec![]));
        // An unfinished line is left for the stream to hold back.
        assert_eq!(split("嗯[[work: 查"), ("嗯[[work: 查".into(), vec![]));
    }

    #[test]
    fn a_line_for_one_piece_goes_to_that_piece() {
        assert_eq!(
            split("好，改成周六。\n[[work #2: 改成周六，原话：还是周六吧]]\n[[stop #1]]"),
            (
                "好，改成周六。".into(),
                vec![
                    HandOff::To {
                        work: 2,
                        text: "改成周六，原话：还是周六吧".into()
                    },
                    HandOff::Stop { work: 1 },
                ]
            )
        );
        // Written loosely, it is still hers.
        assert_eq!(
            split("[[work 3：是]][[stop＃4]]").1,
            vec![
                HandOff::To {
                    work: 3,
                    text: "是".into()
                },
                HandOff::Stop { work: 4 }
            ]
        );
        // A piece without a number is not one; it says nothing.
        assert_eq!(split("嗯[[stop]][[work #x: 查]]"), ("嗯".into(), vec![]));
        // Another kind of line is left where it is.
        assert_eq!(
            split("好呀[[sticker: 开心]]\n[[work: 查]]"),
            ("好呀[[sticker: 开心]]".into(), vec![new("查")])
        );
    }

    #[test]
    fn she_hands_off_only_what_they_asked_for() {
        let section = section(&ChannelChat::default());
        assert!(section.contains("If it is your own idea, ask first"));
        assert!(section.contains("with their permissions"));
        assert!(section.contains("[[stop #N]]"));
        assert!(section.contains("never for a new one"));
        assert!(!section.contains("handed_off"));
        assert!(!section.contains("waiting for their answer"));
        let busy = super::section(&ChannelChat {
            works: vec![
                "#1「查天气」在办，已办 2 分钟（40%）".into(),
                "#2「订餐厅」在等对方回答：要几点？".into(),
                "#3「订票」在等对方回答：哪天？".into(),
            ],
            waiting: 2,
            ..Default::default()
        });
        assert!(busy.contains("handed_off"));
        assert!(busy.contains("#2「订餐厅」"));
        assert!(busy.contains("2 pieces are waiting for their answer"));
    }

    #[test]
    fn what_goes_to_the_channel_says_which_piece() {
        assert_eq!(
            serde_json::to_value(split("[[work #2: 是]][[stop #3]][[work: 查]]").1).unwrap(),
            serde_json::json!([
                {"kind": "to", "work": 2, "text": "是"},
                {"kind": "stop", "work": 3},
                {"kind": "new", "text": "查"},
            ])
        );
    }
}
