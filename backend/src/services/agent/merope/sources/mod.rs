//! What she can spend her own time on, and how each kind of thing reaches
//! her.
//!
//! Her own time (see `doing`) is one loop whatever she does: she chooses
//! among things at hand, takes one in, writes what stayed with her and how
//! it landed, and keeps it. What differs is where things come from and how
//! they reach her, and that is all here, one kind to a place:
//!
//! - a song from the site's playlist, heard from its recording (`song`);
//! - a note published on the site, read (`note`);
//! - the next part of a book she follows (`serial`);
//! - a question of her own, thought over or looked into (`explore`).
//!
//! Each kind offers things (`options`), may start preparing one as she
//! takes it up (`begin`), hands her the material with how it came to her
//! and anything more it asks of her (`intake`), and, once she has written,
//! keeps what only it knows (`after`): what she heard in a song, whether her
//! guess held, how finding something out compared with what she thought.

pub mod note;
pub mod song;

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::{explore, serial};
use crate::models::entities::agent_memories as unified_row;

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

/// What she takes in, as its kind hands it to her.
pub struct Intake {
    pub material: Option<String>,
    /// How much of the material she reads.
    pub limit: usize,
    /// How the material came to her, for writing about it.
    pub how: String,
    /// What more this kind asks her to write: (field, schema).
    pub asks: Vec<(&'static str, Value)>,
    /// Shown with the material: (heading, text), untrusted.
    pub alongside: Vec<(String, String)>,
    /// Something reached her; if not, there is no taste to keep.
    pub reached: bool,
    /// What the kind needs again once she has written.
    pub carry: Carry,
}

impl Intake {
    pub fn plain(material: Option<String>, limit: usize, how: impl Into<String>) -> Self {
        Self {
            material,
            limit,
            how: how.into(),
            asks: Vec::new(),
            alongside: Vec::new(),
            reached: true,
            carry: Carry::Nothing,
        }
    }
}

pub enum Carry {
    Nothing,
    Heard(Arc<myriad_listening::ListeningSheet>),
    Chapter {
        part: String,
        guessed_before: Option<String>,
        knew_it: bool,
    },
    Trip(explore::Trip),
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

fn shuffle<T>(items: &mut [T]) {
    for index in (1..items.len()).rev() {
        items.swap(index, rand::random_range(0..=index));
    }
}

/// A few things at hand she has not just done: songs she has not heard in a
/// while, notes she has never read, the next part of her serial or a book
/// to start, questions of her own.
pub async fn options(db: &DatabaseConnection, lately: &[unified_row::Model]) -> Vec<Thing> {
    let now = Utc::now();
    let done: HashSet<String> = lately
        .iter()
        .filter_map(|row| {
            let (key, _) = super::doing::key_of(row)?;
            let recent = now.signed_duration_since(row.created_at) < song::AGAIN_AFTER;
            (key.starts_with("note:") || recent).then_some(key)
        })
        .collect();
    let fresh = |things: Vec<Thing>| -> Vec<Thing> {
        things
            .into_iter()
            .filter(|thing| !done.contains(&thing.key()))
            .collect()
    };
    let mut songs = fresh(song::options(db).await);
    let mut notes = fresh(note::options(db).await);
    shuffle(&mut songs);
    shuffle(&mut notes);
    songs.truncate(song::OFFERED);
    notes.truncate(note::OFFERED);
    let mut options = songs;
    options.extend(notes);
    options.extend(serial::options(db).await);
    options.extend(explore::options(db).await);
    shuffle(&mut options);
    options
}

/// An option as she sees it.
pub async fn view(db: &DatabaseConnection, index: usize, thing: &Thing) -> Value {
    let mut view = json!({
        "index": index,
        "kind": thing.kind(),
        "title": thing.title(),
        "minutes": thing.minutes(),
    });
    if let Some(by) = thing.by() {
        view["by"] = json!(by);
    }
    let more = match thing {
        Thing::Chapter {
            serial,
            index,
            total,
            ..
        } => serial::view(serial, *index, *total),
        Thing::Inquiry { question_id, .. } => explore::view(db, question_id).await,
        Thing::Song { .. } | Thing::Note { .. } => Map::new(),
    };
    if let Some(object) = view.as_object_mut() {
        object.extend(more);
    }
    view
}

/// How long it takes, once she has picked it.
pub async fn length(db: &DatabaseConnection, thing: &Thing) -> chrono::Duration {
    match thing {
        Thing::Song { duration_ms, .. } => {
            chrono::Duration::milliseconds((*duration_ms).clamp(30_000, 15 * 60_000))
        }
        Thing::Note { item_id, .. } => chrono::Duration::minutes(note::minutes(db, *item_id).await),
        Thing::Chapter { .. } | Thing::Inquiry { .. } => chrono::Duration::minutes(thing.minutes()),
    }
}

/// As she takes it up: hearing a song starts, finding out sets off.
pub fn begin(db: &DatabaseConnection, owner: i32, thing: &Thing) {
    match thing {
        Thing::Song { .. } => super::hearing::start(db.clone(), thing.key(), thing.clone()),
        Thing::Inquiry {
            question_id,
            question,
        } => explore::start(owner, question_id.clone(), question.clone()),
        Thing::Note { .. } | Thing::Chapter { .. } => {}
    }
}

/// What she takes in, once she is done with it; none if it would not come.
pub async fn intake(db: &DatabaseConnection, owner: i32, thing: &Thing) -> Option<Intake> {
    match thing {
        Thing::Song { .. } => Some(song::intake(db, thing).await),
        Thing::Note { item_id, .. } => Some(note::intake(db, *item_id).await),
        Thing::Chapter { serial, index, .. } => serial::intake(db, serial, *index).await,
        Thing::Inquiry {
            question_id,
            question,
        } => explore::intake(owner, question_id, question).await,
    }
}

/// Once she has written: what only its kind keeps. `wrote` holds the fields
/// the kind asked of her.
pub async fn after(
    db: &DatabaseConnection,
    owner: i32,
    thing: &Thing,
    wrote: &Map<String, Value>,
    carry: Carry,
) -> Kept {
    match (thing, carry) {
        (Thing::Song { .. }, Carry::Heard(sheet)) => Kept {
            heard: Some(sheet.gist()),
            ..Kept::default()
        },
        (
            Thing::Chapter {
                serial,
                index,
                total,
                ..
            },
            Carry::Chapter {
                part,
                guessed_before,
                knew_it,
            },
        ) => {
            serial::after(
                db,
                owner,
                serial,
                *index,
                *total,
                wrote,
                &part,
                guessed_before,
                knew_it,
            )
            .await
        }
        (Thing::Inquiry { question_id, .. }, Carry::Trip(trip)) => {
            explore::after(db, owner, question_id, &trip).await
        }
        _ => Kept::default(),
    }
}

/// Where she is in it right now, beyond how long she has been at it.
pub fn so_far(thing: &Thing, seconds_in: f32) -> String {
    match thing {
        Thing::Song { .. } => song::so_far(thing, seconds_in),
        Thing::Note { .. } | Thing::Chapter { .. } | Thing::Inquiry { .. } => String::new(),
    }
}

/// How the material reached her, as the semantic suite gives it: by what
/// she did (`what`) and whether there is material.
#[cfg(test)]
pub(crate) fn probe_intake(what: &str, material: Option<&str>) -> Intake {
    let mut intake = if what.starts_with("finding out ") {
        explore::probe_intake(material)
    } else if what.starts_with("reading part ") {
        serial::probe_intake(material)
    } else if what.starts_with("reading ") {
        note::probe_intake(material)
    } else {
        song::probe_intake(material)
    };
    intake.material = material.map(str::to_string);
    intake
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let inquiry = Thing::Inquiry {
            question_id: "q".into(),
            question: "为什么？".into(),
        };
        assert_eq!((inquiry.kind(), inquiry.done_verb()), ("find_out", "查完"));
        // What is kept serializes flat, and nothing when there is nothing.
        assert_eq!(serde_json::to_value(Kept::default()).unwrap(), json!({}));
    }
}
