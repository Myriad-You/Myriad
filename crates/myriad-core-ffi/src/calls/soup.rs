//! Turtle soup: hosting a game (`soup`) and the puzzles she makes up
//! herself (`making`).

use myriad_merope::{making, soup};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Nothing, at, input, opt_at, text};
use crate::Failure;

pub(super) fn texts(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(json!({
        "startMarker": soup::START_MARKER,
        "maxAsked": soup::MAX_ASKED,
        "startSchemaName": soup::START_SCHEMA,
        "judgeSchemaName": soup::JUDGE_SCHEMA,
        "putAwayAfterSeconds": soup::PUT_AWAY_AFTER.num_seconds(),
        "fair": soup::FAIR,
        "startSchema": soup::start_schema(),
        "settings": soup::SETTINGS,
        "notThisTime": soup::NOT_THIS_TIME,
        "judgeSystem": soup::JUDGE_SYSTEM,
        "holdBack": soup::HOLD_BACK,
        "groupOffer": soup::GROUP_OFFER,
        "offer": soup::OFFER,
        "madeSource": making::SOURCE,
        "makeSchemaName": making::MAKE_SCHEMA,
        "makeSchema": making::make_schema(),
    }))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Asked {
    #[serde(default)]
    by: Option<String>,
    question: String,
    verdict: soup::Verdict,
}

/// A game on at a table, times in microseconds.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Game {
    surface: String,
    truth: String,
    keys: Vec<String>,
    asked: Vec<Asked>,
    found: Vec<usize>,
    #[serde(default)]
    ending: Option<soup::Ending>,
    #[serde(default)]
    solver: Option<String>,
    started: i64,
    #[serde(default)]
    last: Option<i64>,
    #[serde(default)]
    made: Option<String>,
}

impl Game {
    fn into_crate(self) -> Result<soup::Game, Failure> {
        Ok(soup::Game {
            surface: self.surface,
            truth: self.truth,
            keys: self.keys,
            asked: self
                .asked
                .into_iter()
                .map(|asked| soup::Asked {
                    by: asked.by,
                    question: asked.question,
                    verdict: asked.verdict,
                })
                .collect(),
            found: self.found,
            ending: self.ending,
            solver: self.solver,
            started: at(self.started)?,
            last: opt_at(self.last)?,
            made: self.made,
        })
    }

    fn from_crate(game: soup::Game) -> Self {
        Self {
            surface: game.surface,
            truth: game.truth,
            keys: game.keys,
            asked: game
                .asked
                .into_iter()
                .map(|asked| Asked {
                    by: asked.by,
                    question: asked.question,
                    verdict: asked.verdict,
                })
                .collect(),
            found: game.found,
            ending: game.ending,
            solver: game.solver,
            started: game.started.timestamp_micros(),
            last: game.last.map(|last| last.timestamp_micros()),
            made: game.made,
        }
    }
}

pub(super) fn split_start(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        raw: String,
    }
    let req: In = input(raw)?;
    let (rest, started) = soup::split_start(&req.raw);
    Ok(json!({ "text": rest, "started": started }))
}

pub(super) fn start_system(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        soul: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(soup::start_system(&req.soul))))
}

pub(super) fn judge_schema(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        keys: usize,
    }
    let req: In = input(raw)?;
    Ok(json!({ "schema": soup::judge_schema(req.keys) }))
}

pub(super) fn judge_input(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        game: Game,
        message: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(soup::judge_input(
        &req.game.into_crate()?,
        &req.message,
    ))))
}

pub(super) fn apply(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Judged {
        verdict: soup::Verdict,
        found: Vec<usize>,
        solved: bool,
        gave_up: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        game: Game,
        judged: Judged,
        #[serde(default)]
        asker: Option<String>,
        words: String,
    }
    let req: In = input(raw)?;
    let mut game = req.game.into_crate()?;
    let judged = soup::Judged {
        verdict: req.judged.verdict,
        found: req.judged.found,
        solved: req.judged.solved,
        gave_up: req.judged.gave_up,
    };
    soup::apply(&mut game, &judged, req.asker.as_deref(), &req.words);
    Ok(json!({ "game": Game::from_crate(game) }))
}

pub(super) fn section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        game: Game,
        /// How the latest message was judged; null when it could not be.
        #[serde(default)]
        verdict: Option<soup::Verdict>,
        group: bool,
        #[serde(default)]
        asker: Option<String>,
    }
    let req: In = input(raw)?;
    Ok(text(Some(soup::section(
        &req.game.into_crate()?,
        req.verdict,
        req.group,
        req.asker.as_deref(),
    ))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tried {
    table: String,
    at: i64,
    /// `solved`, `gave_up`, or `left`.
    ending: String,
    asked: usize,
    #[serde(default)]
    solver: Option<String>,
}

/// A puzzle she made, times in microseconds.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Made {
    surface: String,
    truth: String,
    keys: Vec<String>,
    presentation: String,
    from: String,
    #[serde(default)]
    tried: Vec<Tried>,
}

impl Made {
    fn into_crate(self) -> Result<making::Made, Failure> {
        Ok(making::Made {
            id: String::new(),
            surface: self.surface,
            truth: self.truth,
            keys: self.keys,
            presentation: self.presentation,
            from: self.from,
            tried: self
                .tried
                .into_iter()
                .map(|tried| {
                    Ok(making::Tried {
                        table: tried.table,
                        at: at(tried.at)?,
                        ending: tried.ending,
                        asked: tried.asked,
                        solver: tried.solver,
                    })
                })
                .collect::<Result<_, Failure>>()?,
        })
    }
}

fn all_made(made: Vec<Made>) -> Result<Vec<making::Made>, Failure> {
    made.into_iter().map(Made::into_crate).collect()
}

/// One of her puzzles, and the line asked about it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct One {
    made: Made,
}

pub(super) fn how_it_went(raw: &[u8]) -> Result<Value, Failure> {
    let req: One = input(raw)?;
    Ok(text(Some(req.made.into_crate()?.how_it_went())))
}

pub(super) fn kept_line(raw: &[u8]) -> Result<Value, Failure> {
    let req: One = input(raw)?;
    Ok(text(Some(making::kept_line(&req.made.into_crate()?))))
}

pub(super) fn record_line(raw: &[u8]) -> Result<Value, Failure> {
    let req: One = input(raw)?;
    Ok(text(Some(making::record_line(&req.made.into_crate()?))))
}

pub(super) fn offer_own(raw: &[u8]) -> Result<Value, Failure> {
    let req: One = input(raw)?;
    Ok(text(Some(making::offer_own(&req.made.into_crate()?))))
}

pub(super) fn writing_about_own(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        surface: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(making::writing_about_own(&req.surface))))
}

/// Where in `all` the one found is.
fn position(all: &[making::Made], found: Option<&making::Made>) -> Value {
    json!({ "index": found.and_then(|found| all.iter().position(|made| std::ptr::eq(made, found))) })
}

pub(super) fn named_in(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        made: Vec<Made>,
        words: String,
    }
    let req: In = input(raw)?;
    let all = all_made(req.made)?;
    Ok(position(&all, making::named_in(&all, &req.words)))
}

pub(super) fn untried_at(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        made: Vec<Made>,
        table: String,
    }
    let req: In = input(raw)?;
    let all = all_made(req.made)?;
    Ok(position(&all, making::untried_at(&all, &req.table)))
}

pub(super) fn make_system(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        soul: String,
        what: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(making::make_system(&req.soul, &req.what))))
}

pub(super) fn make_input(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        took_in: String,
        material: String,
        before: Vec<Made>,
        made_today: usize,
        unplayed: usize,
    }
    let req: In = input(raw)?;
    Ok(text(Some(making::make_input(
        &req.took_in,
        &req.material,
        &all_made(req.before)?,
        req.made_today,
        req.unplayed,
    ))))
}

pub(super) fn parse_idea(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        raw: String,
    }
    let req: In = input(raw)?;
    Ok(match making::parse_idea(&req.raw) {
        None => json!({ "readable": false, "idea": null }),
        Some(None) => json!({ "readable": true, "idea": null }),
        Some(Some(made)) => json!({
            "readable": true,
            "idea": {
                "surface": made.surface,
                "truth": made.truth,
                "keys": made.keys,
                "presentation": made.presentation,
            },
        }),
    })
}
