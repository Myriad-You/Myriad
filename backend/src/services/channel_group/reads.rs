//! What her group mind reads and keeps, read and kept here: plain reads of
//! memories (which rows, newest first, how many), their cues and vectors;
//! what comes to mind of them is worked out in the core.

use myriad_merope::group::{Answer, May, MemoryRow, Newest, ToRead, ToWrite};

use super::*;
use crate::services::agent::memory::unified;

fn row(model: crate::models::entities::agent_memories::Model) -> MemoryRow {
    MemoryRow {
        concepts: serde_json::from_value(model.concepts.clone()).unwrap_or_default(),
        id: model.id,
        content: model.content,
        source: model.source,
        evidence: model.evidence,
        venue: Some(model.venue),
        created_at: model.created_at.timestamp_micros(),
        access_count: model.access_count,
        last_accessed_at: model.last_accessed_at.map(|at| at.timestamp_micros()),
    }
}

/// Whether the database is down for her own things (in a recording, as the
/// scene says).
fn down(venue: &str) -> bool {
    #[cfg(test)]
    if g0::fails("down", "database", venue) {
        return true;
    }
    let _ = venue;
    db().is_err()
}

/// A recording without a database of its own reads nothing kept and keeps
/// nothing (what it would have kept is written down instead).
pub(super) fn no_database() -> bool {
    #[cfg(test)]
    if g0::on() && g0::database().is_none() {
        return true;
    }
    false
}

pub(super) async fn read(venue: &str, what: ToRead) -> Answer {
    // What she has of her own cannot be read with the database down: what
    // needs it (whether she speaks up, her answer to someone from outside,
    // looking up words) is left, as before it was the core's.
    if matches!(what, ToRead::OwnRows { .. } | ToRead::Life { .. }) && down(venue) {
        return Answer::Unread;
    }
    // What a game was dealt lately is kept in the runtime registry (a
    // recording keeps its own).
    // What she is called where people see her.
    if let ToRead::HerName = what {
        let name = match db() {
            Ok(db) => crate::services::agent::merope::get_persona(&db)
                .await
                .ok()
                .flatten()
                .map(|persona| persona.name),
            Err(_) => None,
        };
        return Answer::Label {
            label: crate::services::agent::merope::public_persona_name(true, name.as_deref()),
        };
    }
    if let ToRead::RecentDealt { game } = &what {
        return Answer::Surfaces {
            surfaces: dealt_lately(game).await,
        };
    }
    if no_database() && !matches!(what, ToRead::Soul | ToRead::Now | ToRead::Titles { .. }) {
        return match what {
            ToRead::Cues { .. } => Answer::Cues {
                cues: Default::default(),
            },
            ToRead::Vectors { .. } => Answer::Vectors {
                vectors: Vec::new(),
                known: false,
            },
            ToRead::Life { .. } => Answer::Life {
                did: Vec::new(),
                would_tell: Vec::new(),
                now: None,
            },
            ToRead::Stickers { .. } => Answer::Stickers {
                stickers: Vec::new(),
                can_make: false,
                left: 0,
            },
            ToRead::Label { user_id } => Answer::Label {
                label: user_id.to_string(),
            },
            ToRead::Game { .. } => Answer::Game { game: None },
            ToRead::RecentSurfaces => Answer::Surfaces {
                surfaces: Vec::new(),
            },
            _ => Answer::Rows { rows: Vec::new() },
        };
    }
    match what {
        ToRead::Soul => Answer::Soul { text: soul().await },
        ToRead::VenueRows {
            venue,
            source,
            newest,
            limit,
        } => {
            let Ok(db) = db() else {
                return Answer::Rows { rows: Vec::new() };
            };
            let rows = match newest {
                Newest::Updated => {
                    unified::venue_source_rows(&db, None, &venue, &source, limit).await
                }
                Newest::Created => unified::latest_in_venue(&db, &venue, &source, limit).await,
            };
            Answer::Rows {
                rows: rows.unwrap_or_default().into_iter().map(row).collect(),
            }
        }
        ToRead::OwnRows { source, limit } => {
            let Ok(db) = db() else {
                return Answer::Unread;
            };
            let Some(source) = own_source(&source) else {
                return Answer::Rows { rows: Vec::new() };
            };
            Answer::Rows {
                rows: unified::own_rows(&db, source, limit)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(row)
                    .collect(),
            }
        }
        // What her life on the site says: the facts, as her life keeps
        // them.
        ToRead::Life { talk, related } => {
            use crate::services::agent::merope::group::doing;
            let Ok(db) = db() else {
                return Answer::Unread;
            };
            Answer::Life {
                did: doing::recalled(&db, Some(&talk), 0, related).await,
                would_tell: doing::would_tell(&db, None, Some(&talk)).await,
                now: doing::now_text(crate::services::agent::memory::utc_now()),
            }
        }
        ToRead::Unowned {
            venue,
            source,
            marker,
        } => {
            let (Ok(db), Some(source)) = (db(), unowned_source(&source)) else {
                return Answer::Rows { rows: Vec::new() };
            };
            Answer::Rows {
                rows: unified::unowned_with_evidence(&db, &venue, source, &marker)
                    .await
                    .ok()
                    .flatten()
                    .into_iter()
                    .map(row)
                    .collect(),
            }
        }
        ToRead::UnownedElsewhere {
            here,
            source,
            marker,
            limit,
        } => {
            let (Ok(db), Some(source)) = (db(), unowned_source(&source)) else {
                return Answer::Rows { rows: Vec::new() };
            };
            Answer::Rows {
                rows: unified::unowned_elsewhere(&db, &here, source, &marker, limit)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(row)
                    .collect(),
            }
        }
        ToRead::OtherGroups {
            here,
            sources,
            limit,
        } => {
            let sources: Vec<&'static str> = sources
                .iter()
                .filter_map(|source| group_source(source))
                .collect();
            let Ok(db) = db() else {
                return Answer::Rows { rows: Vec::new() };
            };
            if sources.is_empty() {
                return Answer::Rows { rows: Vec::new() };
            }
            Answer::Rows {
                rows: unified::in_other_groups(&db, None, &here, &sources, limit)
                    .await
                    .into_iter()
                    .map(row)
                    .collect(),
            }
        }
        ToRead::Titles { venues } => {
            let mut titles = Vec::with_capacity(venues.len());
            for venue in venues {
                let name = super::title(&venue).await;
                titles.push((venue, name));
            }
            Answer::Titles { titles }
        }
        ToRead::Label { user_id } => {
            let Ok(db) = db() else {
                return Answer::Label {
                    label: user_id.to_string(),
                };
            };
            Answer::Label {
                label: crate::services::agent::merope::group::speaking::resolve_addressee_label(
                    &db, user_id,
                )
                .await,
            }
        }
        ToRead::Game { table } => Answer::Game {
            game: crate::services::agent::merope::group::soup::game_at(&table).await,
        },
        ToRead::RecentSurfaces => Answer::Surfaces {
            surfaces: crate::services::agent::merope::group::soup::recent_surfaces(),
        },
        ToRead::RecentDealt { game } => Answer::Surfaces {
            surfaces: dealt_lately(&game).await,
        },
        ToRead::HerName => Answer::Label {
            label: String::new(),
        },
        // Her clock, and what her life on the site says she is doing.
        ToRead::Now => Answer::Now {
            local: crate::services::agent::merope::group::speaking::local_now()
                .fixed_offset()
                .to_rfc3339(),
            doing: crate::services::agent::merope::group::doing::now_text(
                crate::services::agent::memory::utc_now(),
            ),
        },
        ToRead::Stickers { venue } => {
            use crate::services::agent::merope::group::stickers;
            let Ok(db) = db() else {
                return Answer::Stickers {
                    stickers: Vec::new(),
                    can_make: false,
                    left: 0,
                };
            };
            let can_make = stickers::current_identity(&db).await.is_some();
            let rows = stickers::sendable(&db, Some(&venue)).await;
            let left = if can_make {
                stickers::left_this_month(&db).await
            } else {
                0
            };
            Answer::Stickers {
                stickers: rows
                    .into_iter()
                    .map(|sticker| myriad_merope::group::StickerRow {
                        id: sticker.id,
                        meaning: sticker.meaning,
                        venue: sticker.venue,
                        sent: sticker.sent,
                        last_sent_at: sticker.last_sent_at.map(|at| at.timestamp_micros()),
                        kept_from: sticker.kept_from,
                    })
                    .collect(),
                can_make,
                left,
            }
        }
        ToRead::Cues { ids } => {
            let cues = match db() {
                Ok(db) => unified::cues_of(&db, &ids).await,
                Err(_) => Default::default(),
            };
            Answer::Cues { cues }
        }
        ToRead::Vectors { rows, fill } => {
            let rows: Vec<crate::services::agent::memory::meaning::Memo> = rows
                .into_iter()
                .map(|(id, content)| crate::services::agent::memory::meaning::Memo { id, content })
                .collect();
            let vectors = match db() {
                Ok(db) => {
                    crate::services::agent::memory::meaning::vectors_of(&db, &rows, fill).await
                }
                Err(_) => None,
            };
            Answer::Vectors {
                known: vectors.is_some(),
                vectors: vectors
                    .map(|vectors| {
                        // In the rows' order, so the answer reads the same
                        // every time.
                        rows.iter()
                            .filter_map(|row| Some((row.id.clone(), vectors.get(&row.id)?.clone())))
                            .collect()
                    })
                    .unwrap_or_default(),
            }
        }
    }
}

/// Sources kept with no owner that her group mind reads: her notes on
/// people from outside, and her guesses at who they are.
fn unowned_source(source: &str) -> Option<&'static str> {
    match source {
        "stranger" => Some("stranger"),
        "maybe_is" => Some("maybe_is"),
        _ => None,
    }
}

/// A group's own sources her group mind reads from other groups.
fn group_source(source: &str) -> Option<&'static str> {
    match source {
        "bit" => Some("bit"),
        "group_day" => Some("group_day"),
        "jargon" => Some("jargon"),
        _ => None,
    }
}

/// Her own sources her group mind reads.
fn own_source(source: &str) -> Option<&'static str> {
    match source {
        "view" => Some(unified::OWN_VIEW),
        "heard" => Some(crate::services::agent::merope::group::heard::SOURCE),
        "meme" => Some(myriad_merope::memes::SOURCE),
        "made_soup" => Some(myriad_merope::making::SOURCE),
        _ => None,
    }
}

/// Sources kept in a venue that her group mind may keep.
fn source(source: &str) -> Option<&'static str> {
    match source {
        "about_me" => Some("about_me"),
        "game" => Some("game"),
        _ => None,
    }
}

/// Whether she may look a word up now: without a database to keep the
/// day's allowance in, she leaves it.
pub(super) async fn may_look_up(term: &str) -> May {
    if no_database() {
        return May::Tried;
    }
    match db() {
        Ok(db) => crate::services::agent::merope::group::memes::may_look_up(&db, term).await,
        Err(_) => May::Tried,
    }
}

/// Where what a kind of game was dealt lately is kept: hers, a month.
const DEALT_NAMESPACE: &str = "merope_game_dealt";
const DEALT_DAYS: i64 = 30;

async fn dealt_lately(game: &str) -> Vec<String> {
    let Ok(db) = db() else {
        return Vec::new();
    };
    kept::<Vec<String>>(&db, DEALT_NAMESPACE, game)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

async fn keep_dealt(game: &str, dealt: String) {
    let Ok(db) = db() else {
        return;
    };
    let mut lately = dealt_lately(game).await;
    lately.retain(|kept| *kept != dealt);
    lately.push(dealt);
    let excess = lately
        .len()
        .saturating_sub(myriad_merope::undercover::RECENT_KEPT);
    lately.drain(..excess);
    if let Err(error) = keep(
        &db,
        DEALT_NAMESPACE,
        game,
        &lately,
        kept_for(DEALT_DAYS * 86_400),
    )
    .await
    {
        warn!(%error, game, "[Group] could not keep what a game was dealt");
    }
}

pub(super) async fn write(what: ToWrite) {
    if let ToWrite::Dealt { game, dealt } = what {
        return keep_dealt(&game, dealt).await;
    }
    #[cfg(test)]
    if no_database() {
        let ToWrite::RememberInVenue {
            venue,
            content,
            evidence,
            ..
        } = what
        else {
            return;
        };
        let line = serde_json::from_str::<serde_json::Value>(&evidence)
            .ok()
            .and_then(|evidence| evidence["line"].as_str().map(str::to_string))
            .unwrap_or_default();
        g0::note(
            "told",
            serde_json::json!({ "venue": venue, "told": content, "line": line }),
        );
        return;
    }
    match what {
        ToWrite::RememberInVenue {
            venue,
            content,
            evidence,
            source: kept_as,
        } => {
            let (Ok(db), Some(kept_as)) = (db(), source(&kept_as)) else {
                return;
            };
            match unified::remember_in_venue(&db, &venue, &content, &evidence, kept_as).await {
                Ok(Some(_)) => {
                    info!(%venue, "[Merope] kept what the group told her about herself")
                }
                Ok(None) => {}
                Err(error) => warn!(%error, "[Merope] could not keep what she was told"),
            }
        }
        ToWrite::RememberOwn {
            content,
            evidence,
            concepts,
            source: kept_as,
        } => {
            let (Ok(db), Some(kept_as)) = (db(), own_source(&kept_as)) else {
                return;
            };
            match unified::remember_own(&db, &content, &evidence, concepts, kept_as).await {
                Ok(Some(_)) => info!("[Merope] learned a word or meme"),
                Ok(None) => {}
                Err(error) => warn!(%error, "[Merope] could not keep a word she learned"),
            }
        }
        ToWrite::Game { table, game } => {
            crate::services::agent::merope::group::soup::keep_at(&table, game).await;
        }
        ToWrite::TakeGame { table } => {
            crate::services::agent::merope::group::soup::take_at(&table).await;
        }
        ToWrite::UsedSurface { surface } => {
            crate::services::agent::merope::group::soup::used_surface(surface);
        }
        ToWrite::Dealt { .. } => {}
        ToWrite::Tried { id, tried } => {
            let Ok(db) = db() else {
                return;
            };
            crate::services::agent::merope::group::soup::kept_tried(&db, &id, tried).await;
        }
        ToWrite::Watch { venue, term, used } => {
            let Ok(db) = db() else {
                return;
            };
            crate::services::agent::merope::group::jargon::nominate(&db, &venue, &term, &used)
                .await;
        }
    }
}
