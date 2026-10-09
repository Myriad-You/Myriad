//! Serials she follows on her own: books she picked, each one part a day.
//!
//! The books are the public-domain books of Project Gutenberg (English and
//! Chinese) and Aozora Bunko (Japanese), fiction and not, tens of thousands
//! of them (see `library`), fetched from archives that allow it. Each day a
//! shelf is put out, a handful drawn at random in each language she reads.
//! She may follow a few books at once, as anyone reads more than one; while
//! she follows fewer, which one she takes up next is hers to choose, as this
//! personality, and she may let any of them go after any part, for good.
//!
//! A new part comes out each day from the day she started; she reads it when
//! she chooses to, in her own time (see `doing`). After each part she writes
//! what stayed with her and a guess at what happens next. Whether the guess
//! held is judged against the next part by the judgment model, not by her:
//! models grade themselves too kindly. Committing to a guess before the
//! answer is where surprise, and so learning, happens (Brod et al. 2018), and
//! what she guessed wrong is part of who she has been (see `self_story`).

use std::collections::HashSet;
use std::time::Duration;

use chrono::Utc;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use serde_json::{Value, json};

use super::sources::{Carry, Intake, Kept, Thing};
#[cfg(test)]
pub(crate) use myriad_merope::serial::Held;
use myriad_merope::serial::{
    CATALOG, HOW, JUDGE_SCHEMA, Judged, PART_CHARS, PART_CHARS_EN, Past, Source, asks, chapter,
    clean_aozora, clean_gutenberg, judge_schema, judge_system,
};
pub use myriad_merope::serial::{Ended, Following, Guessed, Work, parts, work};

pub const NAMESPACE: &str = "merope_serial";
/// What she follows, as it was kept when she could follow only one.
const FOLLOWING: &str = "following";
/// Everything she follows now.
const READING: &str = "reading";
/// Books she follows at once, at most, each a part a day.
const AT_ONCE: usize = 3;
const PAST: &str = "past";
const SHELF: &str = "shelf";
/// Each work she was shown or took up, so it reads the same whatever the
/// library holds later.
const WORK: &str = "work:";
const KEEP_DAYS: i64 = 400;
const FETCH_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_BOOK_BYTES: usize = 4 * 1024 * 1024;
const USER_AGENT: &str = "MyriadSerialReader/1.0";
/// Books put out a day in each language, drawn at random from the library.
const PER_LANGUAGE: usize = 5;
/// Books opened and not followed are dropped from disk after this long.
const KEEP_TEXT_DAYS: u64 = 30;
const JUDGE_TIMEOUT: Duration = Duration::from_secs(45);

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

async fn put<T: Serialize>(db: &DatabaseConnection, record: &str, value: &T) {
    let keep_until = (Utc::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    if let Err(error) =
        crate::services::runtime_registry::put(db, NAMESPACE, record, identity(), value, keep_until)
            .await
    {
        tracing::warn!(%error, "[Merope] could not keep where she is in her serial");
    }
}

/// The books she follows now.
pub async fn following(db: &DatabaseConnection) -> Vec<Following> {
    if let Ok(Some(all)) =
        crate::services::runtime_registry::get::<Vec<Following>>(db, NAMESPACE, READING).await
    {
        return all;
    }
    crate::services::runtime_registry::get::<Following>(db, NAMESPACE, FOLLOWING)
        .await
        .ok()
        .flatten()
        .into_iter()
        .collect()
}

/// What she is following, or None when it could not be read: before
/// writing it back, not read is not following nothing.
async fn following_to_change(db: &DatabaseConnection) -> Option<Vec<Following>> {
    match crate::services::runtime_registry::get::<Vec<Following>>(db, NAMESPACE, READING).await {
        Ok(Some(all)) => return Some(all),
        Ok(None) => {}
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not read what she is following");
            return None;
        }
    }
    match crate::services::runtime_registry::get::<Following>(db, NAMESPACE, FOLLOWING).await {
        Ok(one) => Some(one.into_iter().collect()),
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not read what she is following");
            None
        }
    }
}

async fn keep_following(db: &DatabaseConnection, all: &[Following]) {
    put(db, READING, &all).await;
    let _ = crate::services::runtime_registry::delete(db, NAMESPACE, FOLLOWING).await;
}

async fn past(db: &DatabaseConnection) -> Past {
    crate::services::runtime_registry::get(db, NAMESPACE, PAST)
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// The work under this id: as it was when she was shown it, or one of the
/// books picked by hand.
pub async fn known(db: &DatabaseConnection, id: &str) -> Option<Work> {
    let kept: Option<Work> =
        crate::services::runtime_registry::get(db, NAMESPACE, &format!("{WORK}{id}"))
            .await
            .ok()
            .flatten();
    kept.or_else(|| work(id).cloned())
}

async fn remember(db: &DatabaseConnection, work: &Work) {
    put(db, &format!("{WORK}{}", work.id), work).await;
}

/// What an option says about the work, for her choice.
pub async fn view(
    db: &DatabaseConnection,
    id: &str,
    index: usize,
    total: usize,
) -> serde_json::Map<String, Value> {
    myriad_merope::serial::view(known(db, id).await.as_ref(), index, total)
}

/// A new persona follows nothing and has read nothing.
pub async fn forget<C: sea_orm::ConnectionTrait>(db: &C) -> Result<u64, sea_orm::DbErr> {
    crate::services::runtime_registry::delete_matching(db, NAMESPACE, None, None, None, None).await
}

// --- the text ------------------------------------------------------------------

fn cache_path(work: &Work) -> std::path::PathBuf {
    crate::services::data_paths::paths()
        .agent
        .join("serials")
        .join(format!("{}.txt", work.id))
}

/// The whole book, fetched once and kept.
async fn text(work: &Work) -> Option<String> {
    let path = cache_path(work);
    if let Ok(text) = tokio::fs::read_to_string(&path).await {
        return Some(text);
    }
    let (url, aozora) = match &work.source {
        Source::Aozora { path } => {
            let file = path.rsplit('/').next().unwrap_or_default();
            (
                format!(
                    "https://raw.githubusercontent.com/aozorahack/aozorabunko_text/master/{path}/{file}.txt"
                ),
                true,
            )
        }
        Source::Gutenberg { path } => (format!("https://gutenberg.pglaf.org/{path}"), false),
    };
    let fetched = crate::services::outbound_security::get_public_following_redirects(
        &url,
        FETCH_TIMEOUT,
        Some(USER_AGENT),
    )
    .await
    .ok()?;
    if !fetched.response.status().is_success() {
        tracing::info!(work = %work.id, status = %fetched.response.status(), "[Merope] could not fetch a serial");
        return None;
    }
    let bytes =
        crate::services::outbound_security::read_limited_body(fetched.response, MAX_BOOK_BYTES)
            .await
            .ok()?;
    let text = if aozora {
        let (decoded, _, _) = encoding_rs::SHIFT_JIS.decode(&bytes);
        clean_aozora(&decoded)
    } else {
        clean_gutenberg(&String::from_utf8_lossy(&bytes))
    };
    if text.is_empty() {
        return None;
    }
    if let Some(dir) = path.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    let _ = tokio::fs::write(&path, &text).await;
    Some(text)
}

/// Part `index` of a work, and how many parts it has.
pub async fn part(db: &DatabaseConnection, id: &str, index: usize) -> Option<(String, usize)> {
    let work = known(db, id).await?;
    let parts = parts(&text(&work).await?, &work.lang);
    let total = parts.len();
    parts.into_iter().nth(index).map(|part| (part, total))
}

/// What she could take up: the next part of each book she follows once it
/// is out, and, while she follows fewer than she could, the books on
/// today's shelf.
pub async fn options(db: &DatabaseConnection) -> Vec<Thing> {
    let following = following(db).await;
    let now = Utc::now();
    let mut options = Vec::new();
    for following in &following {
        if following.next >= following.out(now, &super::clock::zone()) {
            continue;
        }
        if let Some(work) = known(db, &following.id).await {
            options.push(chapter(&work, following.next, following.total));
        }
    }
    if following.len() >= AT_ONCE {
        return options;
    }
    let past = past(db).await;
    options.extend(
        shelf(db)
            .await
            .into_iter()
            .filter(|work| {
                !past.finished.contains(&work.id)
                    && !past.dropped.contains(&work.id)
                    && !following.iter().any(|following| following.id == work.id)
            })
            // How long it is shows once she opens it (see `open`).
            .map(|work| chapter(&work, 0, 0)),
    );
    options
}

/// A book from the shelf she picked, opened: its first part with how many
/// there are. None if its text will not come; it leaves the shelf.
pub async fn open(db: &DatabaseConnection, thing: Thing) -> Option<Thing> {
    let Thing::Chapter {
        serial,
        index: 0,
        total: 0,
        ..
    } = &thing
    else {
        return Some(thing);
    };
    let work = known(db, serial).await?;
    let total = match text(&work).await {
        Some(text) => parts(&text, &work.lang).len(),
        None => 0,
    };
    if total == 0 {
        tracing::info!(serial = %work.id, "[Merope] a book she picked would not open");
        let mut shelf: Shelf = crate::services::runtime_registry::get(db, NAMESPACE, SHELF)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        shelf.books.retain(|book| book.id != work.id);
        put(db, SHELF, &shelf).await;
        return None;
    }
    Some(chapter(&work, 0, total))
}

/// The books put out for the day, drawn at random from the library.
#[derive(Default, Serialize, serde::Deserialize)]
struct Shelf {
    day: String,
    books: Vec<Work>,
    /// Every book put out before, so the next shelf has others.
    #[serde(default)]
    shown: Vec<String>,
}

async fn shelf(db: &DatabaseConnection) -> Vec<Work> {
    let today = chrono::Local::now().date_naive().to_string();
    let mut shelf: Shelf = crate::services::runtime_registry::get(db, NAMESPACE, SHELF)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if shelf.day == today {
        return shelf.books;
    }
    let past = past(db).await;
    let done: HashSet<String> = past.finished.iter().chain(&past.dropped).cloned().collect();
    let mut passed: HashSet<String> = done.iter().chain(&shelf.shown).cloned().collect();
    let mut library = super::library::works().await;
    if library.is_empty() {
        library = CATALOG.clone();
    }
    let mut roll = |below: usize| rand::random_range(0..below);
    let mut books = Vec::new();
    for lang in myriad_merope::library::LANGUAGES {
        for _ in 0..PER_LANGUAGE {
            let picked = myriad_merope::library::pick(&library, lang, &passed, &mut roll)
                // Every book in this language was put out once: again,
                // only not what she finished or let go.
                .or_else(|| {
                    let today: HashSet<String> =
                        books.iter().map(|work: &Work| work.id.clone()).collect();
                    let passed: HashSet<String> = done.union(&today).cloned().collect();
                    myriad_merope::library::pick(&library, lang, &passed, &mut roll)
                })
                .cloned();
            let Some(work) = picked else {
                break;
            };
            passed.insert(work.id.clone());
            books.push(work);
        }
    }
    drop(library);
    for work in &books {
        remember(db, work).await;
    }
    shelf.shown.extend(books.iter().map(|work| work.id.clone()));
    shelf.day = today;
    shelf.books = books;
    put(db, SHELF, &shelf).await;
    prune_texts(db).await;
    tracing::info!(
        books = shelf.books.len(),
        "[Merope] a shelf of books is out for the day"
    );
    shelf.books
}

/// Books opened long ago and not followed; not the ones she follows.
async fn prune_texts(db: &DatabaseConnection) {
    let following: Vec<String> = following(db)
        .await
        .into_iter()
        .map(|following| following.id)
        .collect();
    let Some(dir) = cache_path(&CATALOG[0])
        .parent()
        .map(std::path::Path::to_path_buf)
    else {
        return;
    };
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return;
    };
    let old = std::time::SystemTime::now() - Duration::from_secs(KEEP_TEXT_DAYS * 24 * 60 * 60);
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Some(id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".txt"))
        else {
            continue;
        };
        if following.iter().any(|following| following == id) {
            continue;
        }
        let modified = entry.metadata().await.and_then(|meta| meta.modified());
        if modified.is_ok_and(|modified| modified < old) {
            let _ = tokio::fs::remove_file(&path).await;
        }
    }
}

/// The part she reads, with what she guessed after the one before.
pub async fn intake(db: &DatabaseConnection, id: &str, index: usize) -> Option<Intake> {
    let Some((part, _)) = part(db, id, index).await else {
        tracing::info!(serial = %id, "[Merope] the part she meant to read would not load");
        return None;
    };
    // What she guessed after the last part, and whether she knew the book
    // (so it was memory, not a guess).
    let (guessed_before, knew_it) = match following(db)
        .await
        .into_iter()
        .find(|following| following.id == id)
    {
        Some(following) if index > 0 => (following.guess, following.knew_it),
        _ => (None, false),
    };
    let mut intake = Intake::plain(Some(part.clone()), PART_CHARS, HOW);
    intake.asks = asks();
    if let Some(guess) = &guessed_before {
        intake
            .alongside
            .push(("What you guessed after the last part".into(), guess.clone()));
    }
    intake.carry = Carry::Chapter {
        part,
        guessed_before,
        knew_it,
    };
    Some(intake)
}

/// Once she has written: whether her last guess held, judged against this
/// part, and where she is in the book now.
#[allow(clippy::too_many_arguments)]
pub async fn after(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    index: usize,
    total: usize,
    wrote: &serde_json::Map<String, Value>,
    part: &str,
    guessed_before: Option<String>,
    knew_it: bool,
) -> Kept {
    let guessed = match &guessed_before {
        Some(guess) => judge_guess(owner, guess, knew_it, part).await,
        None => None,
    };
    let guess = wrote
        .get("guess")
        .and_then(Value::as_str)
        .map(str::to_string);
    let go_on = wrote.get("go_on").and_then(Value::as_bool).unwrap_or(true);
    let knew = wrote
        .get("knew_it")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ended = read(db, id, index, total, guess, go_on, knew).await;
    Kept {
        guessed,
        ended,
        ..Kept::default()
    }
}

#[cfg(test)]
pub(crate) fn probe_intake(_material: Option<&str>) -> Intake {
    let mut intake = Intake::plain(None, PART_CHARS, HOW);
    intake.asks = asks();
    intake
}

/// She read part `index`: go on to the next, or let it go, or it has
/// ended. Starting a work is reading its first part.
pub async fn read(
    db: &DatabaseConnection,
    id: &str,
    index: usize,
    total: usize,
    guess: Option<String>,
    go_on: bool,
    knew_it: bool,
) -> Option<Ended> {
    let mut all = following_to_change(db).await?;
    let ended = myriad_merope::serial::advance(
        &mut all,
        myriad_merope::serial::Read {
            id,
            index,
            total,
            guess,
            go_on,
            knew_it,
        },
        Utc::now(),
    );
    if let Some(ended) = ended {
        let mut past = match crate::services::runtime_registry::get::<Past>(db, NAMESPACE, PAST)
            .await
        {
            Ok(past) => past.unwrap_or_default(),
            Err(error) => {
                tracing::warn!(%error, "[Merope] could not read the books she finished or let go");
                return None;
            }
        };
        match ended {
            Ended::Finished => past.finished.push(id.to_string()),
            Ended::LetGo => past.dropped.push(id.to_string()),
        }
        put(db, PAST, &past).await;
    }
    keep_following(db, &all).await;
    ended
}

// --- was the guess right -----------------------------------------------------------

/// Whether her guess held, judged against the part that came next.
pub async fn judge_guess(
    owner: i32,
    guess: &str,
    remembered: bool,
    next_part: &str,
) -> Option<Guessed> {
    let input = json!({
        "guess": guess,
        "nextPart": next_part.chars().take(PART_CHARS_EN + 2_000).collect::<String>(),
    })
    .to_string();
    let judged: Judged = super::call::Ask::new(super::call::Voice::Judge, owner, "serial_guess")
        .within(JUDGE_TIMEOUT)
        .json(judge_system(), &input, JUDGE_SCHEMA, &judge_schema())
        .await
        .ok()?;
    Some(Guessed {
        said: guess.to_string(),
        held: judged.held,
        happened: judged.happened.chars().take(160).collect(),
        remembered,
    })
}

#[cfg(test)]
pub(crate) fn judge_probe_contract() -> (String, Value) {
    (judge_system().to_string(), judge_schema())
}

#[cfg(test)]
pub(crate) fn parse_judged(raw: &str) -> Option<Held> {
    let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
    serde_json::from_str::<Judged>(json.as_deref().unwrap_or(raw.trim()))
        .ok()
        .map(|judged| judged.held)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    #[test]
    fn an_aozora_text_reads_as_prose() {
        let raw = "こころ\n夏目漱石\n\n-------------------------------------------------------\n【テキスト中に現れる記号について】\n《》：ルビ\n-------------------------------------------------------\n\n［＃５字下げ］上　先生と私［＃「上　先生と私」は大見出し］\n　私《わたくし》はその人を常に先生と呼んでいた。｜麦藁帽《むぎわらぼう》を被った。※［＃「てへん＋劣」、第3水準1-84-77］\n\n底本：「こころ」新潮文庫\n入力：青空文庫\n";
        let text = clean_aozora(raw);
        assert_eq!(
            text,
            "上　先生と私\n　私はその人を常に先生と呼んでいた。麦藁帽を被った。"
        );
    }

    #[test]
    fn a_book_is_cut_between_paragraphs_into_days() {
        let paragraph = "あ".repeat(1_200);
        let text = [paragraph.as_str(); 9].join("\n");
        let parts = parts(&text, "ja");
        // 1,200 a paragraph, 5,000 a day: four paragraphs a part, and the
        // one left over is too short to stand alone.
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|part| part.chars().count() >= 4_800));
        let english = "word ".repeat(400);
        let text = [english.trim(); 12].join("\n\n");
        let parts = super::parts(&text, "en");
        assert_eq!(parts.len(), 3);
        assert!(!parts[0].contains("  "));
    }

    #[test]
    fn one_part_comes_out_a_day() {
        let started: DateTime<Utc> = "2026-09-20T02:00:00Z".parse().unwrap();
        let following = Following {
            id: "pg-2852".into(),
            next: 0,
            total: 5,
            started,
            guess: None,
            knew_it: false,
        };
        let zone = super::super::clock::zone();
        assert_eq!(following.out(started, &zone), 1);
        assert_eq!(following.out(started + chrono::Duration::days(2), &zone), 3);
        assert_eq!(
            following.out(started + chrono::Duration::days(30), &zone),
            5
        );
    }

    #[test]
    fn a_guess_is_judged_in_a_fixed_shape() {
        let (system, schema) = judge_probe_contract();
        assert!(system.contains("not_yet"));
        assert_eq!(schema["required"], json!(["held", "happened"]));
        assert_eq!(
            parse_judged(r#"{"held":"partly","happened":"他确实回来了，但不是为了那封信"}"#),
            Some(Held::Partly)
        );
        assert_eq!(parse_judged(r#"{"held":"maybe","happened":""}"#), None);
    }
}

/// Fetch real books end to end and show how they read and split.
/// `cargo test … read_real_books -- --ignored --nocapture`
#[cfg(test)]
mod live {
    #[tokio::test]
    #[ignore = "fetches real books"]
    async fn read_real_books() {
        let chinese = super::Work {
            id: "pg-27166".into(),
            title: "吶喊".into(),
            author: "Lu Xun".into(),
            lang: "zh".into(),
            about: String::new(),
            source: super::Source::Gutenberg {
                path: "cache/epub/27166/pg27166.txt".into(),
            },
        };
        for work in [
            super::work("aozora-773").unwrap(),
            super::work("pg-2852").unwrap(),
            &chinese,
        ] {
            let id = &work.id;
            let text = super::text(work).await.expect("text");
            let parts = super::parts(&text, &work.lang);
            println!(
                "{id}: {} chars, {} parts, first part {} chars\n--- start ---\n{}\n--- end of first part ---\n{}\n",
                text.chars().count(),
                parts.len(),
                parts[0].chars().count(),
                parts[0].chars().take(300).collect::<String>(),
                parts[0]
                    .chars()
                    .rev()
                    .take(120)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
            println!(
                "--- end of book ---\n{}\n",
                text.chars()
                    .rev()
                    .take(200)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
        }
    }
}
