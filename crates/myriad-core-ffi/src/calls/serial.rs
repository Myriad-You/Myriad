//! Following a book one part a day (`serial`), and the library she picks
//! one from (`library`).

use myriad_merope::{library, serial};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Nothing, Zone, at, input, on_zone, text};
use crate::Failure;

pub(super) fn texts(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    let asks: Vec<Value> = serial::asks()
        .into_iter()
        .map(|(name, schema)| json!([name, schema]))
        .collect();
    Ok(json!({
        "partCharsJa": serial::PART_CHARS_JA,
        "partCharsEn": serial::PART_CHARS_EN,
        "partChars": serial::PART_CHARS,
        "minutes": serial::MINUTES,
        "how": serial::HOW,
        "judgeSchemaName": serial::JUDGE_SCHEMA,
        "judgeSystem": serial::judge_system(),
        "judgeSchema": serial::judge_schema(),
        "asks": asks,
        "gutenbergCatalog": library::GUTENBERG_CATALOG,
        "aozoraCatalog": library::AOZORA_CATALOG,
        "languages": library::LANGUAGES,
        "catalog": *serial::CATALOG,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    raw: String,
}

pub(super) fn clean_aozora(raw: &[u8]) -> Result<Value, Failure> {
    let req: Raw = input(raw)?;
    Ok(text(Some(serial::clean_aozora(&req.raw))))
}

pub(super) fn clean_gutenberg(raw: &[u8]) -> Result<Value, Failure> {
    let req: Raw = input(raw)?;
    Ok(text(Some(serial::clean_gutenberg(&req.raw))))
}

pub(super) fn parts(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        text: String,
        lang: String,
    }
    let req: In = input(raw)?;
    Ok(json!({ "parts": serial::parts(&req.text, &req.lang) }))
}

/// A book she follows, `started` in microseconds.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Following {
    id: String,
    next: usize,
    total: usize,
    started: i64,
    #[serde(default)]
    guess: Option<String>,
    #[serde(default)]
    knew_it: bool,
}

impl Following {
    fn into_crate(self) -> Result<serial::Following, Failure> {
        Ok(serial::Following {
            id: self.id,
            next: self.next,
            total: self.total,
            started: at(self.started)?,
            guess: self.guess,
            knew_it: self.knew_it,
        })
    }

    fn from_crate(following: serial::Following) -> Self {
        Self {
            id: following.id,
            next: following.next,
            total: following.total,
            started: following.started.timestamp_micros(),
            guess: following.guess,
            knew_it: following.knew_it,
        }
    }
}

pub(super) fn out(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        following: Following,
        now: i64,
        zone: String,
    }
    let req: In = input(raw)?;
    let following = req.following.into_crate()?;
    let now = at(req.now)?;
    let zone = Zone::parse(&req.zone)?;
    Ok(json!({ "out": on_zone!(zone, |zone| following.out(now, &zone)) }))
}

pub(super) fn advance(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Read {
        id: String,
        index: usize,
        total: usize,
        #[serde(default)]
        guess: Option<String>,
        go_on: bool,
        knew_it: bool,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        all: Vec<Following>,
        read: Read,
        now: i64,
    }
    let req: In = input(raw)?;
    let mut all = req
        .all
        .into_iter()
        .map(Following::into_crate)
        .collect::<Result<Vec<_>, _>>()?;
    let read = serial::Read {
        id: &req.read.id,
        index: req.read.index,
        total: req.read.total,
        guess: req.read.guess,
        go_on: req.read.go_on,
        knew_it: req.read.knew_it,
    };
    let ended = serial::advance(&mut all, read, at(req.now)?);
    let all: Vec<Following> = all.into_iter().map(Following::from_crate).collect();
    Ok(json!({ "all": all, "ended": ended }))
}

pub(super) fn view(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        work: Option<serial::Work>,
        index: usize,
        total: usize,
    }
    let req: In = input(raw)?;
    Ok(json!({ "view": serial::view(req.work.as_ref(), req.index, req.total) }))
}

pub(super) fn looking_back(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        guessed: Option<serial::Guessed>,
        #[serde(default)]
        ended: Option<serial::Ended>,
    }
    let req: In = input(raw)?;
    let (line, wrong) = serial::looking_back(req.guessed.as_ref(), req.ended);
    Ok(json!({ "line": line, "wrong": wrong }))
}

pub(super) fn csv_records(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        text: String,
    }
    let req: In = input(raw)?;
    Ok(json!({ "records": library::csv_records(&req.text) }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Csv {
    csv: String,
}

pub(super) fn from_gutenberg(raw: &[u8]) -> Result<Value, Failure> {
    let req: Csv = input(raw)?;
    Ok(json!({ "works": library::from_gutenberg(&req.csv) }))
}

pub(super) fn from_aozora(raw: &[u8]) -> Result<Value, Failure> {
    let req: Csv = input(raw)?;
    Ok(json!({ "works": library::from_aozora(&req.csv) }))
}
