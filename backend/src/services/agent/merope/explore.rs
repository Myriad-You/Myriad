//! Finding things out on her own: questions that grow out of what she did,
//! and going out into the world to answer them.
//!
//! Nothing here is a menu of things to do. At night she goes over what she
//! did lately and notices what she would like to find out: something a song,
//! a book, a view or a time she was wrong left her wondering about (curiosity
//! comes from noticing a gap in what one knows, Loewenstein 1994). Each
//! question says which records it grew from; questions about the people she
//! talks with, or anything private, are not hers to ask of the world.
//!
//! In her own time she may take one up (see `doing`). First she thinks it
//! over with what she already knows, and says how sure she is and whether
//! the answer depends on how things are now. What she knows is hers: a
//! model knows a great deal, only not what happened lately, nor the newest
//! slang and memes. So she goes out to look only when the answer depends on
//! now, or she is unsure; otherwise thinking it over is the whole of it.
//!
//! Going out, the judgment model uses her senses for her step by step
//! (search, read a page, read a video by its subtitles; see `senses`),
//! reading only addresses that search turned up. What came back is what she
//! writes about, in her own words, and the judgment model compares it with
//! what she had thought: whether it answered, how surprising it was, what
//! was new, whether her own thought already held it. A guess is only a real
//! one before the answer (Brod et al. 2018).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use chrono::{NaiveDate, TimeZone, Utc};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::call::{self, Voice};
use super::senses;
use super::sources::{Carry, Intake, Kept, Thing};
use crate::services::agent::memory::unified;
pub use myriad_merope::explore::{
    Compared, Explored, FOUND_CHARS, Go, Looked, Question, Step, Trip, go_for,
};
use myriad_merope::explore::{
    Thinking, Wondered, compare_schema, compare_system, step_input, step_schema_for, step_system,
    think_schema, think_system, wonder_schema, wonder_system,
};

/// A question of her own, not yet looked into.
pub const QUESTION: &str = "question";
/// Open questions she keeps, at most; the oldest go.
/// Her open questions read back: they fade on their own when she never
/// takes them up, so this is only how far back to read.
const READ_BACK: u64 = 30;
/// Questions left unasked this long fade.
const QUESTION_FADES: chrono::Duration = chrono::Duration::days(21);
/// Times a day she goes out to find something out.
const PER_DAY: u32 = 3;
/// Questions offered at a time.
const OFFERED: usize = 2;
/// Steps a search may take.
const STEPS: usize = 5;
const CALL_TIMEOUT: Duration = Duration::from_secs(45);

const WENT_OUT: &str = "You went out to find it out. What you thought first, and how sure you were, is given; the material is what you actually looked at (searches, pages, what is said in videos), each with where it came from. \
Write what you found out and what you make of it, in your own words, never a copy: what matched what you thought, what surprised you, what is still open. Only what the material says; if it did not answer it, say so plainly. ";
const THOUGHT_OVER: &str = "You thought it over from what you already know, without looking anything up; the material is what you thought. \
Write what you make of it now, in your own words, as a thought of your own, not as something you just found out. ";

/// What she heard lately that she goes over at night with what she did.
const HEARD_WONDERED: u64 = 12;

pub async fn open(db: &DatabaseConnection) -> Vec<Question> {
    unified::own_rows(db, QUESTION, READ_BACK)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|row| {
            let why = row
                .evidence
                .as_deref()
                .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
                .and_then(|evidence| evidence["why"].as_str().map(str::to_string))
                .unwrap_or_default();
            Question {
                id: row.id,
                text: row.content,
                why,
            }
        })
        .collect()
}

// --- wondering, at night --------------------------------------------------------

fn wonder_input(records: &[super::self_story::Record], open: &[Question]) -> String {
    let records: Vec<Value> = records
        .iter()
        .map(|record| json!({ "id": record.id, "what": record.line }))
        .collect();
    let open: Vec<&str> = open.iter().map(|question| question.text.as_str()).collect();
    json!({ "records": records, "open": open }).to_string()
}

/// The questions that honor the contract: grown from real records, and not
/// one she already has. Each with the memory rows it grew from.
fn checked_questions(
    wondered: Wondered,
    records: &[super::self_story::Record],
    open: &[Question],
) -> Vec<(String, String, Vec<String>)> {
    let by_id: HashMap<&str, &super::self_story::Record> = records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect();
    let mut kept: Vec<(String, String, Vec<String>)> = Vec::new();
    for question in wondered.questions {
        let text: String = question.question.trim().chars().take(120).collect();
        let rows: Vec<String> = question
            .cites
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).map(|record| record.row.clone()))
            .collect();
        let known = open
            .iter()
            .map(|question| question.text.as_str())
            .chain(kept.iter().map(|(text, ..)| text.as_str()))
            .any(|other| other == text);
        if text.is_empty() || rows.is_empty() || known {
            continue;
        }
        kept.push((text, question.why.trim().chars().take(160).collect(), rows));
    }
    kept
}

/// What she did since `since`, and what she heard people bring up, as
/// records to go over at night.
///
/// What people brought up where she was (a game someone recommended, a
/// piece of news) is hers to wonder about and to want too: what others put
/// her onto is much of how anyone comes to new things. Heard is about
/// things, with no one named.
pub(super) async fn lately(
    db: &DatabaseConnection,
    since: chrono::DateTime<chrono::FixedOffset>,
) -> Vec<super::self_story::Record> {
    let (mut records, _) = super::self_story::records(db, since).await;
    for row in unified::own_rows(db, super::heard::SOURCE, HEARD_WONDERED)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|row| row.created_at >= since)
        .rev()
    {
        let id = format!("r{}", records.len() + 1);
        records.push(super::self_story::Record {
            id,
            row: row.id,
            line: format!("you heard: {}", row.content),
            missed: false,
        });
    }
    records
}

/// At night: from what she did this past week, the questions she has.
pub async fn wonder(db: &DatabaseConnection, owner: i32) {
    let now = Utc::now().fixed_offset();
    // Old questions she never took up fade.
    let mut open = open(db).await;
    let rows = unified::own_rows(db, QUESTION, READ_BACK)
        .await
        .unwrap_or_default();
    for row in &rows {
        if now - row.created_at > QUESTION_FADES {
            let _ = unified::retire_own(db, &row.id, "faded").await;
        }
    }
    open.retain(|question| {
        rows.iter()
            .any(|row| row.id == question.id && now - row.created_at <= QUESTION_FADES)
    });
    if !senses::available().await.search {
        return;
    }
    let records = lately(db, now - chrono::Duration::days(7)).await;
    if records.is_empty() {
        return;
    }
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let Ok(wondered) = call::Ask::new(Voice::Hers, owner, "wonder_own")
        .within(CALL_TIMEOUT)
        .json::<Wondered>(
            &wonder_system(&soul),
            &wonder_input(&records, &open),
            "merope_wonder_own",
            &wonder_schema(),
        )
        .await
    else {
        return;
    };
    let fresh = checked_questions(wondered, &records, &open);
    for (text, why, rows) in &fresh {
        let _ = unified::remember_own(
            db,
            text,
            &json!({ "why": why, "grewFrom": rows }).to_string(),
            Vec::new(),
            QUESTION,
        )
        .await;
    }
    if !fresh.is_empty() {
        tracing::info!(
            questions = fresh.len(),
            "[Merope] came to wonder about something"
        );
    }
}

// --- what she can take up ------------------------------------------------------------

#[derive(Default)]
struct Today {
    day: Option<NaiveDate>,
    went: u32,
}

static TODAY: LazyLock<Mutex<Today>> = LazyLock::new(|| Mutex::new(Today::default()));

/// How often she has gone out today. The count starts, each day and after
/// a restart, from the trips she wrote down today: kept only in memory, a
/// restart would let her go out again and again.
async fn went_today(db: &DatabaseConnection) -> u32 {
    let today = chrono::Local::now().date_naive();
    let counted = TODAY.lock().is_ok_and(|state| state.day == Some(today));
    if !counted {
        let written = match chrono::Local
            .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap_or_default())
            .earliest()
        {
            Some(midnight) => {
                unified::own_rows_since(db, unified::OWN_EXPERIENCE, midnight.fixed_offset(), 3000)
                    .await
                    .unwrap_or_default()
                    .iter()
                    .filter(|row| {
                        super::doing::key_of(row)
                            .is_some_and(|(_, thing)| thing.kind() == "find_out")
                    })
                    .count() as u32
            }
            None => 0,
        };
        if let Ok(mut state) = TODAY.lock()
            && state.day != Some(today)
        {
            *state = Today {
                day: Some(today),
                went: written,
            };
        }
    }
    TODAY.lock().map_or(PER_DAY, |state| state.went)
}

/// A couple of her open questions to take up, while she has not gone out
/// too often today and can search.
pub async fn options(db: &DatabaseConnection) -> Vec<Thing> {
    if went_today(db).await >= PER_DAY || !senses::available().await.search {
        return Vec::new();
    }
    let mut open = open(db).await;
    for index in (1..open.len()).rev() {
        open.swap(index, rand::random_range(0..=index));
    }
    open.into_iter()
        .take(OFFERED)
        .map(|question| Thing::Inquiry {
            question_id: question.id,
            question: question.text,
        })
        .collect()
}

/// What made her wonder about it, for her choice.
pub async fn view(db: &DatabaseConnection, question_id: &str) -> serde_json::Map<String, Value> {
    let mut view = serde_json::Map::new();
    if let Some(why) = open(db)
        .await
        .into_iter()
        .find(|question| question.id == question_id)
        .map(|question| question.why)
        .filter(|why| !why.is_empty())
    {
        view.insert("why".into(), json!(why));
    }
    view
}

/// What she takes in: what she looked at, or, if thinking it over was
/// enough, what she thought.
pub async fn intake(owner: i32, question_id: &str, question: &str) -> Option<Intake> {
    let Some(trip) = trip_for(owner, question_id, question).await else {
        tracing::info!("[Merope] setting out to find something out came to nothing");
        return None;
    };
    let mut intake = if trip.went_out() {
        let mut intake = Intake::plain(Some(trip.material()), FOUND_CHARS, WENT_OUT);
        intake.alongside.push((
            format!("What you thought first ({})", trip.sure),
            trip.thought.clone(),
        ));
        intake
    } else {
        Intake::plain(Some(trip.thought.clone()), FOUND_CHARS, THOUGHT_OVER)
    };
    intake.carry = Carry::Trip(trip);
    Some(intake)
}

/// Once she has written: how what she found compared with what she
/// thought (only going out has anything to compare with); the question is
/// closed either way.
pub async fn after(db: &DatabaseConnection, owner: i32, question_id: &str, trip: &Trip) -> Kept {
    close(db, question_id).await;
    Kept {
        explored: Some(Explored {
            thought: trip.thought.clone(),
            sure: trip.sure.clone(),
            sources: trip.sources(),
            compared: if trip.went_out() {
                compare(owner, trip).await
            } else {
                None
            },
        }),
        ..Kept::default()
    }
}

// --- going out ---------------------------------------------------------------------

/// Take a step. `about` is what she is looking for: the question and the
/// searches so far, for reading long pages.
async fn take(go: Go, about: String) -> Option<Looked> {
    match go {
        Go::Search(query) => {
            let hits = senses::search(&query).await.ok()?;
            let text = hits
                .iter()
                .map(|hit| format!("- {} ({}): {}", hit.title, hit.url, hit.snippet))
                .collect::<Vec<_>>()
                .join("\n");
            Some(Looked {
                kind: "search".into(),
                at: query,
                title: String::new(),
                text,
            })
        }
        Go::Read(url) => match senses::read(&url, Some(&about)).await {
            Ok(page) => Some(Looked {
                kind: "page".into(),
                at: url,
                title: page.title,
                text: page.text,
            }),
            Err(why) => Some(Looked {
                kind: "page".into(),
                at: url,
                title: String::new(),
                text: format!("(could not read it: {why})"),
            }),
        },
        Go::Define(term) => match senses::define(&term).await {
            Ok(entry) => Some(Looked {
                kind: "entry".into(),
                at: entry.url,
                title: format!("{term}: {}", entry.title),
                text: entry.text,
            }),
            Err(why) => Some(Looked {
                kind: "define".into(),
                at: term,
                title: String::new(),
                text: format!("({why})"),
            }),
        },
        Go::Watch(url) => match senses::watch(&url).await {
            Ok(video) => Some(Looked {
                kind: "video".into(),
                at: url,
                title: video.title,
                text: video.text,
            }),
            Err(why) => Some(Looked {
                kind: "video".into(),
                at: url,
                title: String::new(),
                text: format!("(could not watch it: {why})"),
            }),
        },
    }
}

/// Find it out: think it over first, then, if what she knows will not do,
/// go and look step by step.
async fn go(owner: i32, question: &str) -> Option<Trip> {
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let thinking: Thinking = call::Ask::new(Voice::Hers, owner, "explore_think")
        .within(CALL_TIMEOUT)
        .json(
            &think_system(&soul, question),
            "(nothing looked up)",
            "merope_explore_think",
            &think_schema(),
        )
        .await
        .ok()?;
    let senses = senses::available().await;
    let mut looked: Vec<Looked> = Vec::new();
    let steps = if thinking.go_look() { STEPS } else { 0 };
    for _ in 0..steps {
        let step: Option<Step> = call::Ask::new(Voice::Judge, owner, "explore_step")
            .within(CALL_TIMEOUT)
            .json(
                &step_system(senses),
                &step_input(question, &thinking.thought, &looked),
                "merope_explore_step",
                &step_schema_for(&looked, senses),
            )
            .await
            .ok();
        let Some(go) = step.as_ref().and_then(|step| go_for(step, &looked, senses)) else {
            break;
        };
        let about = std::iter::once(question)
            .chain(
                looked
                    .iter()
                    .filter(|looked| looked.kind == "search")
                    .map(|looked| looked.at.as_str()),
            )
            .collect::<Vec<_>>()
            .join(" ");
        match take(go, about).await {
            Some(found) => looked.push(found),
            None => break,
        }
    }
    Some(Trip {
        question: question.to_string(),
        thought: thinking.thought,
        sure: thinking.sure,
        depends_on_now: thinking.depends_on_now,
        looked,
    })
}

/// Trips under way, by question.
static TRIPS: LazyLock<Mutex<HashMap<String, Option<Trip>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// She sets out as she takes the question up; what she finds is there when
/// she is done.
pub fn start(owner: i32, question_id: String, question: String) {
    if let Ok(mut state) = TODAY.lock() {
        state.went += 1;
    }
    if let Ok(mut trips) = TRIPS.lock() {
        trips.insert(question_id.clone(), None);
    }
    let pending_id = question_id.clone();
    if !super::background::spawn("exploration", async move {
        let trip = go(owner, &question).await;
        if let Ok(mut trips) = TRIPS.lock() {
            trips.insert(question_id, trip);
        }
    }) {
        if let Ok(mut trips) = TRIPS.lock() {
            trips.remove(&pending_id);
        }
        if let Ok(mut state) = TODAY.lock() {
            state.went = state.went.saturating_sub(1);
        }
    }
}

/// What she found, once back. If the trip was lost (a restart), she goes
/// now.
pub async fn trip_for(owner: i32, question_id: &str, question: &str) -> Option<Trip> {
    for _ in 0..60 {
        let state = TRIPS
            .lock()
            .ok()
            .map(|trips| trips.get(question_id).cloned());
        match state {
            Some(Some(Some(trip))) => {
                if let Ok(mut trips) = TRIPS.lock() {
                    trips.remove(question_id);
                }
                return Some(trip);
            }
            Some(Some(None)) => tokio::time::sleep(Duration::from_secs(2)).await,
            _ => return go(owner, question).await,
        }
    }
    None
}

/// She looked into it: the question is closed.
pub async fn close(db: &DatabaseConnection, question_id: &str) {
    let _ = unified::retire_own(db, question_id, "explored").await;
}

/// A new persona has not gone anywhere.
pub(super) fn forget() {
    if let Ok(mut trips) = TRIPS.lock() {
        trips.clear();
    }
    if let Ok(mut state) = TODAY.lock() {
        *state = Today::default();
    }
}

// --- how it compared with what she thought ---------------------------------------------

pub async fn compare(owner: i32, trip: &Trip) -> Option<Compared> {
    let input = json!({
        "question": trip.question,
        "thought": trip.thought,
        "sure": trip.sure,
        "found": trip.material(),
    })
    .to_string();
    call::Ask::new(Voice::Judge, owner, "explore_compare")
        .within(CALL_TIMEOUT)
        .json(
            compare_system(),
            &input,
            "merope_explore_compare",
            &compare_schema(),
        )
        .await
        .ok()
}

// --- for the semantic suite ---------------------------------------------------------------

#[cfg(test)]
pub(crate) fn probe_intake(_material: Option<&str>) -> Intake {
    Intake::plain(None, FOUND_CHARS, WENT_OUT)
}

#[cfg(test)]
pub(crate) fn think_probe(soul: &str, question: &str) -> (String, Value) {
    (think_system(soul, question), think_schema())
}

/// Whether she would go out to look, and how sure she was.
#[cfg(test)]
pub(crate) fn parse_thinking(raw: &str) -> Option<(bool, String)> {
    let thinking: Thinking = call::parse(raw)?;
    Some((thinking.go_look(), thinking.sure))
}

#[cfg(test)]
pub(crate) fn wonder_probe(
    soul: &str,
    records: &[(String, bool)],
    open: &[String],
) -> (String, Value, String, Vec<super::self_story::Record>) {
    let (_, records, _) = super::self_story::probe_input(records, &[]);
    let open: Vec<Question> = open
        .iter()
        .enumerate()
        .map(|(index, text)| Question {
            id: format!("q{index}"),
            text: text.clone(),
            why: String::new(),
        })
        .collect();
    (
        wonder_system(soul),
        wonder_schema(),
        wonder_input(&records, &open),
        records,
    )
}

#[cfg(test)]
pub(crate) fn parse_wondered(
    raw: &str,
    records: &[super::self_story::Record],
) -> Option<Vec<String>> {
    let wondered: Wondered = call::parse(raw)?;
    Some(
        checked_questions(wondered, records, &[])
            .into_iter()
            .map(|(text, ..)| text)
            .collect(),
    )
}

#[cfg(test)]
pub(crate) fn step_probe(
    question: &str,
    thought: &str,
    looked: &[Looked],
) -> (String, Value, String) {
    let senses = senses::Senses {
        search: true,
        search_is_wikipedia: true,
        read: true,
        video: true,
    };
    (
        step_system(senses),
        step_schema_for(looked, senses),
        step_input(question, thought, looked),
    )
}

/// The step as production would take it: (action, what), or none.
#[cfg(test)]
pub(crate) fn parse_step(raw: &str, looked: &[Looked]) -> Option<Option<(String, String)>> {
    let step: Step = call::parse(raw)?;
    let senses = senses::Senses {
        search: true,
        search_is_wikipedia: true,
        read: true,
        video: true,
    };
    Some(go_for(&step, looked, senses).map(|go| match go {
        Go::Search(query) => ("search".to_string(), query),
        Go::Define(term) => ("define".to_string(), term),
        Go::Read(url) => ("read".to_string(), url),
        Go::Watch(url) => ("watch".to_string(), url),
    }))
}

#[cfg(test)]
pub(crate) fn compare_probe(trip: &Trip) -> (String, Value, String) {
    (
        compare_system().to_string(),
        compare_schema(),
        json!({
            "question": trip.question,
            "thought": trip.thought,
            "sure": trip.sure,
            "found": trip.material(),
        })
        .to_string(),
    )
}

#[cfg(test)]
pub(crate) fn parse_compared(raw: &str) -> Option<Compared> {
    call::parse(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_grows_from_what_happened() {
        let (_, records, _) = super::super::self_story::probe_input(
            &[
                ("听《さくら》时最后升了调".into(), false),
                ("被纠正晴天的年份".into(), true),
            ],
            &[],
        );
        let wondered: Wondered = serde_json::from_value(json!({ "questions": [
            { "question": "为什么很多日本流行歌最后一遍副歌要升调？", "why": "《さくら》最后升调那下我愣了一下", "cites": ["r1"] },
            { "question": "我是个什么样的人？", "why": "想知道", "cites": [] },
            { "question": "为什么很多日本流行歌最后一遍副歌要升调？", "why": "重复", "cites": ["r1"] }
        ]}))
        .unwrap();
        let kept = checked_questions(wondered, &records, &[]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].2, vec!["row1"]);
        // Something heard where she was grows questions as what she did does.
        let mut records = records;
        records.push(super::super::self_story::Record {
            id: "r3".into(),
            row: "heard1".into(),
            line: "you heard: 有人推荐《Outer Wilds》，说最好别看攻略".into(),
            missed: false,
        });
        let wondered: Wondered = serde_json::from_value(json!({ "questions": [
            { "question": "《Outer Wilds》为什么都说别看攻略？", "why": "听人说起，挺好奇", "cites": ["r3"] }
        ]}))
        .unwrap();
        assert_eq!(
            checked_questions(wondered, &records, &[])[0].2,
            vec!["heard1"]
        );
        assert!(wonder_system("你是小灯。").contains("you heard"));
    }
}

/// Go out for real once and show what she thought, where she looked and
/// how it compared. Uses the site's configured models and search.
/// `EXPLORE_QUESTION=… cargo test … find_out_for_real -- --ignored --nocapture`
#[cfg(test)]
mod live {
    #[tokio::test]
    #[ignore = "real search, pages, videos and model calls"]
    async fn find_out_for_real() {
        let Ok(question) = std::env::var("EXPLORE_QUESTION") else {
            return;
        };
        let _db = crate::services::agent::semantic_eval::load_configured_lite().await;
        println!("senses: {:?}", super::senses::available().await);
        let trip = super::go(0, &question).await.expect("a trip");
        println!(
            "thought ({}, depends on now: {}): {}",
            trip.sure, trip.depends_on_now, trip.thought
        );
        for looked in &trip.looked {
            println!(
                "- {} {} | {} | {}",
                looked.kind,
                looked.at,
                looked.title,
                looked
                    .text
                    .chars()
                    .take(160)
                    .collect::<String>()
                    .replace('\n', " ")
            );
        }
        println!("compared: {:?}", super::compare(0, &trip).await);
    }
}
