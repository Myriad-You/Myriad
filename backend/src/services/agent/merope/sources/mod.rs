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

use std::sync::Arc;

use sea_orm::DatabaseConnection;
use serde_json::{Map, Value, json};

use super::{explore, serial};
pub use myriad_merope::sources::{Kept, Thing};
use myriad_merope::taste::Taste;

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

fn shuffle<T>(items: &mut [T]) {
    for index in (1..items.len()).rev() {
        items.swap(index, rand::random_range(0..=index));
    }
}

/// A few things at hand: songs she would hear again by now (soon if they
/// moved her, not for a long while if they were not for her), likelier by
/// whoever keeps getting to her; notes she has never read; the next part of
/// her serial or a book to start; questions of her own.
pub async fn options(db: &DatabaseConnection, taste: &Taste) -> Vec<Thing> {
    let songs: Vec<Thing> = song::options(db)
        .await
        .into_iter()
        .filter(|thing| taste.would_again(thing))
        .collect();
    let pulls: Vec<f64> = songs.iter().map(|thing| taste.pull(thing)).collect();
    let rolls: Vec<f64> = songs.iter().map(|_| rand::random::<f64>()).collect();
    let songs = myriad_merope::taste::draw(songs, &pulls, &rolls, song::OFFERED);
    let mut notes: Vec<Thing> = note::options(db)
        .await
        .into_iter()
        .filter(|thing| taste.last_time(thing).is_none())
        .collect();
    shuffle(&mut notes);
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
        } => serial::view(db, serial, *index, *total).await,
        Thing::Inquiry { question_id, .. } => explore::view(db, question_id).await,
        Thing::Song { .. } | Thing::Note { .. } => Map::new(),
    };
    if let Some(object) = view.as_object_mut() {
        object.extend(more);
    }
    view
}

/// What she picked, ready to take up; none if it will not come.
pub async fn open(db: &DatabaseConnection, thing: Thing) -> Option<Thing> {
    match thing {
        Thing::Chapter { .. } => serial::open(db, thing).await,
        _ => Some(thing),
    }
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
