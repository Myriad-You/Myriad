//! What she can spend her own time on, and what she keeps of it: a song, a
//! note, the next part of a book, or a question of her own, and what each
//! leaves behind besides her note (what she heard, her guess and whether it
//! held, what she found out). Offering, taking in and writing it down are the
//! backend's.

use crate::{explore, serial};
use serde::{Deserialize, Serialize};

/// Something she can spend her time on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Thing {
    #[serde(rename_all = "camelCase")]
    Song {
        id: String,
        source: String,
        name: String,
        artist: String,
        album: String,
        cover: String,
        duration_ms: i64,
    },
    #[serde(rename_all = "camelCase")]
    Note { item_id: i32, title: String },
    /// A part of the serial she follows (see `serial`).
    #[serde(rename_all = "camelCase")]
    Chapter {
        serial: String,
        title: String,
        author: String,
        /// Which part (0-based), of how many.
        index: usize,
        total: usize,
    },
    /// A question of her own to find out (see `explore`).
    #[serde(rename_all = "camelCase")]
    Inquiry {
        question_id: String,
        question: String,
    },
}

impl Thing {
    pub fn key(&self) -> String {
        match self {
            Self::Song { id, source, .. } => format!("song:{source}:{id}"),
            Self::Note { item_id, .. } => format!("note:{item_id}"),
            Self::Chapter { serial, index, .. } => format!("serial:{serial}:{index}"),
            Self::Inquiry { question_id, .. } => format!("inquiry:{question_id}"),
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Song { name, .. } => name,
            Self::Note { title, .. } | Self::Chapter { title, .. } => title,
            Self::Inquiry { question, .. } => question,
        }
    }

    pub fn by(&self) -> Option<&str> {
        match self {
            Self::Song { artist, .. } => Some(artist).filter(|artist| !artist.is_empty()),
            Self::Chapter { author, .. } => Some(author),
            Self::Note { .. } | Self::Inquiry { .. } => None,
        }
        .map(String::as_str)
    }

    /// "the song 「晴天」 by 周杰伦" / "「…」, a note on this site".
    pub fn describe(&self) -> String {
        match (self, self.by()) {
            (Self::Song { name, .. }, Some(by)) => format!("the song 「{name}」 by {by}"),
            (Self::Song { name, .. }, None) => format!("the song 「{name}」"),
            (Self::Note { title, .. }, _) => format!("「{title}」, a note on this site"),
            (
                Self::Chapter {
                    title,
                    author,
                    total: 0,
                    ..
                },
                _,
            ) => format!("「{title}」 by {author}, a book not yet opened"),
            (
                Self::Chapter {
                    title,
                    author,
                    index,
                    total,
                    ..
                },
                _,
            ) => format!("part {} of {total} of 「{title}」 by {author}", index + 1),
            (Self::Inquiry { question, .. }, _) => format!("「{question}」"),
        }
    }

    /// "listening to", "reading", "finding out".
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Song { .. } => "listening to",
            Self::Note { .. } | Self::Chapter { .. } => "reading",
            Self::Inquiry { .. } => "finding out",
        }
    }

    /// Having done it, as she would say it: 听完 / 读完 / 查完.
    pub fn done_verb(&self) -> &'static str {
        match self {
            Self::Song { .. } => "听完",
            Self::Note { .. } | Self::Chapter { .. } => "读完",
            Self::Inquiry { .. } => "查完",
        }
    }

    /// The kind as she sees it among her options.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Song { .. } => "song",
            Self::Note { .. } => "note",
            Self::Chapter { index: 0, .. } => "start_serial",
            Self::Chapter { .. } => "serial_next_part",
            Self::Inquiry { .. } => "find_out",
        }
    }

    /// Roughly how long it takes, as she weighs her options.
    pub fn minutes(&self) -> i64 {
        match self {
            Self::Song { duration_ms, .. } => (duration_ms / 60_000).max(1),
            Self::Note { .. } => 5,
            Self::Chapter { .. } => serial::MINUTES,
            Self::Inquiry { .. } => explore::MINUTES,
        }
    }
}

/// What only its kind knows about a thing she did, kept with it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// A song: what she heard in it, in brief.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heard: Option<String>,
    /// A serial: what she had guessed after the part before, and how it went.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guessed: Option<serial::Guessed>,
    /// A serial: it ended for her with this part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<serial::Ended>,
    /// Finding out: what she thought, where she looked, how it compared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explored: Option<explore::Explored>,
}

impl Kept {
    /// What it adds to the line she looks back on, and whether it did not go
    /// well (a guess that did not hold, a question left unanswered).
    pub fn looking_back(&self) -> (String, bool) {
        let (guessed, guessed_wrong) = serial::looking_back(self.guessed.as_ref(), self.ended);
        let (explored, found_nothing) = explore::looking_back(self.explored.as_ref());
        (
            format!("{guessed}{explored}"),
            guessed_wrong || found_nothing,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_thing_says_what_it_is() {
        let chapter = Thing::Chapter {
            serial: "pg-2852".into(),
            title: "The Hound of the Baskervilles".into(),
            author: "Arthur Conan Doyle".into(),
            index: 0,
            total: 33,
        };
        assert_eq!(chapter.kind(), "start_serial");
        assert_eq!(chapter.verb(), "reading");
        assert_eq!(
            chapter.describe(),
            "part 1 of 33 of 「The Hound of the Baskervilles」 by Arthur Conan Doyle"
        );
        let unopened = Thing::Chapter {
            serial: "pg-2852".into(),
            title: "The Hound of the Baskervilles".into(),
            author: "Arthur Conan Doyle".into(),
            index: 0,
            total: 0,
        };
        assert_eq!(unopened.kind(), "start_serial");
        assert_eq!(
            unopened.describe(),
            "「The Hound of the Baskervilles」 by Arthur Conan Doyle, a book not yet opened"
        );
        assert!(!crate::serial::view(None, 0, 0).contains_key("part"));
        assert!(crate::serial::view(None, 0, 33).contains_key("part"));
        let inquiry = Thing::Inquiry {
            question_id: "q".into(),
            question: "为什么？".into(),
        };
        assert_eq!((inquiry.kind(), inquiry.done_verb()), ("find_out", "查完"));
        // What is kept serializes flat, and nothing when there is nothing.
        assert_eq!(serde_json::to_value(Kept::default()).unwrap(), json!({}));
    }
}
