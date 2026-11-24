//! Unit tests for `together.rs`.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use super::super::{
    ApiError, JOB, claim_job, drawing::turn_places, fit_turns::Turns, served::missing_job,
};
use super::*;
use crate::services::{image_generation, see_through};

fn failed() -> ApiError {
    missing_job()
}

/// A failure that tells which step it is of.
fn failed_at(step: &str) -> ApiError {
    (failed().0, axum::Json(serde_json::json!({ "at": step })))
}

#[tokio::test(start_paused = true)]
async fn values_come_back_in_the_order_asked_for() {
    // The last asked for is the first done.
    let made = all_or_first_error((0..4u64).map(|index| async move {
        tokio::time::sleep(Duration::from_millis(40 - 10 * index)).await;
        Ok(index)
    }))
    .await
    .unwrap();
    assert_eq!(made, [0, 1, 2, 3]);
}

#[tokio::test(start_paused = true)]
async fn every_one_is_finished_before_the_first_failure_is_returned() {
    let finished = Arc::new(Mutex::new(Vec::new()));
    let work = (0..4u64).map(|index| {
        let finished = finished.clone();
        async move {
            // The second fails at once, the third fails last; the others take their time.
            let wait = [30, 0, 50, 20][index as usize];
            tokio::time::sleep(Duration::from_millis(wait)).await;
            finished.lock().unwrap().push(index);
            match index {
                1 | 2 => Err((failed().0, axum::Json(serde_json::json!({ "at": index })))),
                _ => Ok(index),
            }
        }
    });
    let (_, axum::Json(error)) = all_or_first_error(work).await.unwrap_err();
    // All four ran to their end, as they came; the failure given is the first in order.
    assert_eq!(*finished.lock().unwrap(), [1, 3, 0, 2]);
    assert_eq!(error["at"], 1);
}

#[tokio::test]
async fn what_was_made_is_kept_when_another_fails_and_not_made_again() {
    let cache = TurnCache::scratch();
    let name = |index: usize| format!("turned-{index}.png");
    let first = all_or_first_error((0..4usize).map(|index| {
        let cache = &cache;
        async move {
            kept_or_made(cache, &name(index), async move {
                if index == 1 {
                    return Err(failed());
                }
                // Done after the failure.
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(vec![index as u8 + 1])
            })
            .await
        }
    }))
    .await;
    assert!(first.is_err());
    for index in [0, 2, 3] {
        assert_eq!(
            cache.read(&name(index)).await,
            Some(vec![index as u8 + 1]),
            "the drawing made beside the failed one is kept"
        );
    }
    assert!(cache.read(&name(1)).await.is_none());
    // The next job makes only the missing one.
    let again = all_or_first_error((0..4usize).map(|index| {
        let cache = &cache;
        async move {
            kept_or_made(cache, &name(index), async move {
                assert_eq!(index, 1, "a kept drawing is not made again");
                Ok(vec![9])
            })
            .await
        }
    }))
    .await
    .unwrap();
    assert_eq!(again, [vec![1], vec![9], vec![3], vec![4]]);
    cache.remove();
}

/// How long the steps of a job take here, milliseconds for a bust's seconds,
/// set so that no two of them end at the same moment. Each drawing is one
/// byte, the time it comes in at; its decomposition is that byte plus 100.
const DRAWN: [u64; 4] = [30, 10, 20, 40];
const FRONT: u64 = 200;
const DECOMPOSE: u64 = 210;
const CHECK: u64 = 25;
/// A turn drawn again is 90 plus its place.
const AGAIN: u8 = 90;

/// The steps a job asks of the outside, as staged for a test.
struct Staged {
    started: tokio::time::Instant,
    /// The turns the check sends back.
    sent_back: Vec<usize>,
    /// How long a turn takes to draw again, and whether that fails.
    redraw: u64,
    redraw_fails: bool,
    /// The drawing that cannot be decomposed.
    cannot_split: Option<u8>,
    /// What was begun and what ended, and when.
    did: Mutex<Vec<(u64, String)>>,
}

impl Staged {
    fn sending_back(turns: &[usize], redraw: u64) -> Self {
        Self {
            started: tokio::time::Instant::now(),
            sent_back: turns.to_vec(),
            redraw,
            redraw_fails: false,
            cannot_split: None,
            did: Mutex::default(),
        }
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn note(&self, what: String) {
        let at = self.now();
        self.did.lock().unwrap().push((at, what));
    }

    fn did(&self) -> Vec<(u64, String)> {
        self.did.lock().unwrap().clone()
    }
}

impl TurnedSteps for Staged {
    async fn decompose(&self, png: &[u8], kept: Option<Vec<u8>>) -> ApiResult<(Vec<u8>, bool)> {
        let drawing = png[0];
        if let Some(kept) = kept {
            self.note(format!("checked the kept one of {drawing}"));
            return Ok((kept, true));
        }
        self.note(format!("splitting {drawing}"));
        tokio::time::sleep(Duration::from_millis(DECOMPOSE)).await;
        self.note(format!("split {drawing}"));
        if self.cannot_split == Some(drawing) {
            return Err(failed_at("split"));
        }
        Ok((vec![drawing + 100], true))
    }

    async fn drawn_past(&self, front_psd: Vec<u8>, turned: Vec<Vec<u8>>) -> Vec<usize> {
        assert_eq!(front_psd, b"front");
        tokio::time::sleep(Duration::from_millis(CHECK)).await;
        let drawings: Vec<u8> = turned.iter().map(|png| png[0]).collect();
        self.note(format!("checked {drawings:?}"));
        self.sent_back.clone()
    }

    async fn draw_again(&self, index: usize) -> ApiResult<Vec<u8>> {
        self.note(format!("drawing {index} again"));
        tokio::time::sleep(Duration::from_millis(self.redraw)).await;
        if self.redraw_fails {
            self.note(format!("could not draw {index} again"));
            return Err(failed_at("redraw"));
        }
        self.note(format!("drawn {index} again"));
        Ok(vec![AGAIN + index as u8])
    }
}

fn notes(of: &[(u64, &str)]) -> Vec<(u64, String)> {
    of.iter()
        .map(|(at, what)| (*at, (*what).to_string()))
        .collect()
}

/// The four drawings, each kept in the cache as it comes in, as a job's are;
/// `fails` is the one that cannot be drawn.
fn drawings(
    cache: &TurnCache,
    fails: Option<usize>,
) -> Vec<impl Future<Output = ApiResult<Vec<u8>>>> {
    DRAWN
        .iter()
        .enumerate()
        .map(|(index, &wait)| async move {
            let name = format!("turned-{index}.png");
            let draw = async move {
                tokio::time::sleep(Duration::from_millis(wait)).await;
                if fails == Some(index) {
                    return Err(failed_at("drawing"));
                }
                Ok(vec![wait as u8])
            };
            kept_or_made(cache, &name, draw).await
        })
        .collect()
}

/// The front decomposition, in at `FRONT`.
async fn front() -> ApiResult<Vec<u8>> {
    tokio::time::sleep(Duration::from_millis(FRONT)).await;
    Ok(b"front".to_vec())
}

/// A job's drawings and decompositions made together, none of them failing but as `steps` has it.
async fn made(id: &str, cache: &TurnCache, steps: &Staged) -> ApiResult<(Vec<u8>, Vec<Turned>)> {
    turns_beside_front(id, cache, steps, drawings(cache, None), front()).await
}

/// What the cache has of a turn: its drawing, its decomposition with whether
/// that is checked, and whether the turn was drawn again.
type Kept = (Option<Vec<u8>>, Option<(Vec<u8>, bool)>, bool);

async fn kept(cache: &TurnCache, index: usize) -> Kept {
    (
        cache.read(&format!("turned-{index}.png")).await,
        kept_decomposition(cache, index + 1).await,
        cache
            .read(&format!("turned-{index}.redrawn"))
            .await
            .is_some(),
    )
}

/// A turn as drawn the first time, with its decomposition checked, not drawn again.
fn first_kept(index: usize) -> Kept {
    let drawing = DRAWN[index] as u8;
    (
        Some(vec![drawing]),
        Some((vec![drawing + 100], true)),
        false,
    )
}

/// A one-byte drawing and its one-byte decomposition.
fn turn(png: u8, psd: u8) -> Turned {
    (vec![png], vec![psd])
}

/// What the cache has of a turn `after` milliseconds from now.
async fn kept_after(cache: &TurnCache, index: usize, after: u64) -> Kept {
    tokio::time::sleep(Duration::from_millis(after)).await;
    kept(cache, index).await
}

/// The four as they come in and are decomposed with no turn sent back.
const UNTIL_CHECKED: [(u64, &str); 6] = [
    (10, "splitting 10"),
    (20, "splitting 20"),
    (30, "splitting 30"),
    (40, "splitting 40"),
    (220, "split 10"),
    (225, "checked [30, 10, 20, 40]"),
];

#[tokio::test(start_paused = true)]
async fn each_turned_drawing_is_decomposed_from_when_it_is_in_beside_the_front_one() {
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[], 0);
    let (front_psd, turned) = made("none", &cache, &steps).await.unwrap();
    assert_eq!(front_psd, b"front");
    // In the order asked for, each decomposed once.
    assert_eq!(
        turned,
        [turn(30, 130), turn(10, 110), turn(20, 120), turn(40, 140)]
    );
    // Each was begun the moment its drawing came in, long before the front
    // decomposition was in (200) and the drawings were checked against it.
    let mut did = notes(&UNTIL_CHECKED);
    did.extend(notes(&[
        (230, "split 20"),
        (240, "split 30"),
        (250, "split 40"),
    ]));
    assert_eq!(steps.did(), did);
    // Over with the last decomposition, where waiting for the check first ends at 435.
    assert_eq!(steps.now(), DRAWN[3] + DECOMPOSE);
    for index in 0..4 {
        assert_eq!(kept(&cache, index).await, first_kept(index));
    }
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn a_turn_sent_back_gives_up_the_decomposition_still_running_once_it_is_drawn_again() {
    let cache = TurnCache::scratch();
    // The last turn's decomposition would be in at 250; its redraw is in at 235.
    let steps = Staged::sending_back(&[3], 10);
    let (out, later) = tokio::join!(made("none", &cache, &steps), kept_after(&cache, 3, 260));
    let (_, turned) = out.unwrap();
    let again = turn(AGAIN + 3, AGAIN + 103);
    assert_eq!(turned[3], again);
    assert_eq!(turned[0], turn(30, 130));
    // The first drawing's decomposition never ends: the new drawing's is the only other one.
    let mut did = notes(&UNTIL_CHECKED);
    did.extend(notes(&[
        (225, "drawing 3 again"),
        (230, "split 20"),
        (235, "drawn 3 again"),
        (235, "splitting 93"),
        (240, "split 30"),
        (445, "split 93"),
    ]));
    assert_eq!(steps.did(), did);
    // Past the moment the one given up would have been in, the cache has the
    // new drawing and no decomposition beside it.
    assert_eq!(later, (Some(vec![AGAIN + 3]), None, true));
    assert_eq!(
        kept(&cache, 3).await,
        (Some(again.0), Some((again.1, true)), true)
    );
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn a_decomposition_in_and_kept_before_the_redraw_is_not_what_the_cache_ends_with() {
    // In before the check has its say: the second turn's is in at 220, the check at 225.
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[1], 10);
    let (out, before) = tokio::join!(made("none", &cache, &steps), kept_after(&cache, 1, 222));
    let (_, turned) = out.unwrap();
    assert_eq!(before, first_kept(1));
    let again = turn(AGAIN + 1, AGAIN + 101);
    assert_eq!(turned[1], again);
    let mut did = notes(&UNTIL_CHECKED);
    did.extend(notes(&[
        (225, "drawing 1 again"),
        (230, "split 20"),
        (235, "drawn 1 again"),
        (235, "splitting 91"),
        (240, "split 30"),
        (250, "split 40"),
        (445, "split 91"),
    ]));
    assert_eq!(steps.did(), did);
    assert_eq!(
        kept(&cache, 1).await,
        (Some(again.0), Some((again.1, true)), true)
    );
    cache.remove();

    // In while the turn is being drawn again: the first turn's is in at 240, its redraw at 265.
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[0], 40);
    let (out, before) = tokio::join!(made("none", &cache, &steps), kept_after(&cache, 0, 255));
    let (_, turned) = out.unwrap();
    assert_eq!(before, first_kept(0));
    let again = turn(AGAIN, AGAIN + 100);
    assert_eq!(turned[0], again);
    let mut did = notes(&UNTIL_CHECKED);
    did.extend(notes(&[
        (225, "drawing 0 again"),
        (230, "split 20"),
        (240, "split 30"),
        (250, "split 40"),
        (265, "drawn 0 again"),
        (265, "splitting 90"),
        (475, "split 90"),
    ]));
    assert_eq!(steps.did(), did);
    assert_eq!(
        kept(&cache, 0).await,
        (Some(again.0), Some((again.1, true)), true)
    );
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn a_turn_that_cannot_be_drawn_again_keeps_its_drawing_and_the_decomposition_under_way() {
    let cache = TurnCache::scratch();
    let steps = Staged {
        redraw_fails: true,
        ..Staged::sending_back(&[3], 10)
    };
    let (_, turned) = made("none", &cache, &steps).await.unwrap();
    assert_eq!(turned[3], turn(40, 140));
    // The decomposition begun at 40 went on through the redraw: none was made beside it.
    let mut did = notes(&UNTIL_CHECKED);
    did.extend(notes(&[
        (225, "drawing 3 again"),
        (230, "split 20"),
        (235, "could not draw 3 again"),
        (240, "split 30"),
        (250, "split 40"),
    ]));
    assert_eq!(steps.did(), did);
    assert_eq!(kept(&cache, 3).await, first_kept(3));
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn a_turn_drawn_again_already_is_not_drawn_yet_again() {
    let cache = TurnCache::scratch();
    cache.write("turned-3.redrawn", b"redrawn").await;
    let steps = Staged::sending_back(&[3], 10);
    let (_, turned) = made("none", &cache, &steps).await.unwrap();
    assert_eq!(turned[3], turn(40, 140));
    assert!(steps.did().iter().all(|(_, what)| !what.contains("again")));
    assert_eq!(steps.now(), DRAWN[3] + DECOMPOSE);
    assert_eq!(
        kept(&cache, 3).await,
        (Some(vec![40]), Some((vec![140], true)), true)
    );
    cache.remove();
}

#[tokio::test]
async fn a_check_that_fails_sends_no_turn_back() {
    // The sides a check names, by their place; one it does not know is left out.
    let sides = ["minus", "down", "sideways"].map(str::to_string);
    assert_eq!(turn_places(&sides), [1, 3]);
    assert!(turn_places(&[]).is_empty());

    // What is not a decomposition is refused before the Space is asked anything.
    let client = see_through::SeeThroughClient::offline();
    let cache = TurnCache::scratch();
    let generation = image_generation::ImageGenerationConfig {
        provider: "openai".into(),
        model: "m".into(),
        api_key: String::new(),
        base_url: String::new(),
    };
    let reference = image_generation::ImageReference {
        bytes: axum::body::Bytes::from_static(b"master"),
        media_type: "image/png".to_string(),
    };
    let turns = Turns {
        id: "none",
        user_id: 0,
        generation: &generation,
        reference: &reference,
        size: (1, 1),
        client: &client,
        cache: &cache,
        options: see_through::DecomposeOptions::default(),
    };
    let turned = vec![vec![1], vec![2], vec![3], vec![4]];
    let sent_back = TurnedSteps::drawn_past(&turns, b"no decomposition".to_vec(), turned).await;
    assert!(sent_back.is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_drawing_leaves_the_other_turns_decomposed_for_the_next_job() {
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[], 0);
    let out = turns_beside_front("none", &cache, &steps, drawings(&cache, Some(1)), front()).await;
    let (_, axum::Json(error)) = out.unwrap_err();
    assert_eq!(error["at"], "drawing");
    // The other three were decomposed to their end; nothing was checked.
    assert_eq!(
        steps.did(),
        notes(&[
            (20, "splitting 20"),
            (30, "splitting 30"),
            (40, "splitting 40"),
            (230, "split 20"),
            (240, "split 30"),
            (250, "split 40"),
        ])
    );
    for index in [0, 2, 3] {
        assert_eq!(kept(&cache, index).await, first_kept(index));
    }
    assert_eq!(kept(&cache, 1).await, (None, None, false));

    // The next job draws and decomposes only the missing one, and checks the four once.
    let steps = Staged::sending_back(&[], 0);
    let (_, turned) = made("none", &cache, &steps).await.unwrap();
    assert_eq!(
        steps.did(),
        notes(&[
            (10, "splitting 10"),
            (220, "split 10"),
            (225, "checked [30, 10, 20, 40]"),
        ])
    );
    assert_eq!(turned[1], turn(10, 110));
    assert_eq!(turned[3], turn(40, 140));

    // With everything kept, a job asks for the check alone.
    let steps = Staged::sending_back(&[], 0);
    made("none", &cache, &steps).await.unwrap();
    assert_eq!(steps.did(), notes(&[(225, "checked [30, 10, 20, 40]")]));
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn the_failure_given_is_the_first_in_order_and_what_was_decomposed_is_kept() {
    async fn failing_front() -> ApiResult<Vec<u8>> {
        tokio::time::sleep(Duration::from_millis(FRONT)).await;
        Err(failed_at("front"))
    }
    let at = |out: ApiResult<(Vec<u8>, Vec<Turned>)>| {
        let (_, axum::Json(error)) = out.unwrap_err();
        error["at"].as_str().map(str::to_string)
    };

    // The front decomposition fails: the turns' are seen to their end and kept, nothing is checked.
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[], 0);
    let out = turns_beside_front(
        "none",
        &cache,
        &steps,
        drawings(&cache, None),
        failing_front(),
    )
    .await;
    assert_eq!(at(out).as_deref(), Some("front"));
    assert_eq!(steps.now(), DRAWN[3] + DECOMPOSE);
    assert!(
        steps
            .did()
            .iter()
            .all(|(_, what)| !what.contains("checked"))
    );
    for index in 0..4 {
        assert_eq!(kept(&cache, index).await, first_kept(index));
    }
    cache.remove();

    // A drawing's failure comes before the front decomposition's.
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[], 0);
    let out = turns_beside_front(
        "none",
        &cache,
        &steps,
        drawings(&cache, Some(2)),
        failing_front(),
    )
    .await;
    assert_eq!(at(out).as_deref(), Some("drawing"));
    cache.remove();

    // A turn that cannot be decomposed fails the job once the others are in and kept.
    let cache = TurnCache::scratch();
    let steps = Staged {
        cannot_split: Some(20),
        ..Staged::sending_back(&[], 0)
    };
    let out = made("none", &cache, &steps).await;
    assert_eq!(at(out).as_deref(), Some("split"));
    assert_eq!(steps.now(), DRAWN[3] + DECOMPOSE);
    assert_eq!(kept(&cache, 2).await, (Some(vec![20]), None, false));
    assert_eq!(kept(&cache, 3).await, first_kept(3));
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn a_kept_decomposition_not_yet_checked_is_checked_and_not_made_again() {
    let cache = TurnCache::scratch();
    cache.write("turned-2.png", &[20]).await;
    keep_decomposition(&cache, 3, &[77], false).await;
    let steps = Staged::sending_back(&[], 0);
    let (_, turned) = made("none", &cache, &steps).await.unwrap();
    assert_eq!(turned[2], turn(20, 77));
    let did = steps.did();
    assert!(did.contains(&(0, "checked the kept one of 20".to_string())));
    assert!(did.iter().all(|(_, what)| what != "splitting 20"));
    assert_eq!(
        kept(&cache, 2).await,
        (Some(vec![20]), Some((vec![77], true)), false)
    );
    cache.remove();
}

#[tokio::test]
async fn a_turn_drawn_again_leaves_nothing_of_the_decomposition_it_replaces() {
    let cache = TurnCache::scratch();
    cache.write("turned-2.png", b"first").await;
    keep_decomposition(&cache, 3, b"of the first", true).await;
    assert_eq!(
        kept(&cache, 2).await,
        (
            Some(b"first".to_vec()),
            Some((b"of the first".to_vec(), true)),
            false
        )
    );
    keep_redrawn(&cache, 2, b"again").await;
    assert_eq!(kept(&cache, 2).await, (Some(b"again".to_vec()), None, true));
    // The new drawing's decomposition is not taken for checked on the old one's word.
    keep_decomposition(&cache, 3, b"of the second", false).await;
    assert_eq!(
        kept_decomposition(&cache, 3).await,
        Some((b"of the second".to_vec(), false))
    );
    cache.remove();
}

fn progress() -> (Stage, u8, u8) {
    let job = JOB.lock().unwrap();
    let job = job.as_ref().expect("a job");
    (job.stage, job.done, job.total)
}

/// The job's progress at each of `at`, milliseconds from now.
async fn progress_at(at: &[u64]) -> Vec<(Stage, u8, u8)> {
    let started = tokio::time::Instant::now();
    let mut seen = Vec::new();
    for &at in at {
        tokio::time::sleep_until(started + Duration::from_millis(at)).await;
        seen.push(progress());
    }
    seen
}

#[tokio::test(start_paused = true)]
async fn the_job_counts_drawings_then_the_decompositions_in_hand() {
    let _slot = JOB_TESTS.lock().await;
    let decomposing = |done: u8| (Stage::Decomposing, done, 5u8);

    // Drawings until the four are in; then decompositions, the front's (200)
    // and the turns' (220 to 250) as they come in.
    let cache = TurnCache::scratch();
    let (id, _) = claim_job(&Some("together-a".into()), "m", Stage::Generating, 4).unwrap();
    let steps = Staged::sending_back(&[], 0);
    let (out, seen) = tokio::join!(
        made(&id, &cache, &steps),
        progress_at(&[5, 35, 45, 210, 222, 245])
    );
    out.unwrap();
    assert_eq!(
        seen,
        [
            (Stage::Generating, 0, 4),
            (Stage::Generating, 3, 4),
            decomposing(0),
            decomposing(1),
            decomposing(2),
            decomposing(4),
        ]
    );
    assert_eq!(progress(), decomposing(5));
    update(&id, |job| job.stage = Stage::Done);

    // Everything kept: all five are in hand when the drawings are counted in,
    // and stay counted while the drawings are checked.
    let (id, _) = claim_job(&Some("together-b".into()), "m", Stage::Generating, 4).unwrap();
    let kept_front = async { ApiResult::Ok(b"front".to_vec()) };
    let steps = Staged::sending_back(&[], 0);
    let (out, seen) = tokio::join!(
        turns_beside_front(&id, &cache, &steps, drawings(&cache, None), kept_front),
        progress_at(&[5])
    );
    out.unwrap();
    assert_eq!(seen, [decomposing(5)]);
    assert_eq!(steps.now(), CHECK);
    update(&id, |job| job.stage = Stage::Done);
    cache.remove();

    // A turn drawn again: its first decomposition, in at 220, is counted until
    // the redraw is in (235), and the turn again only with the new drawing's (445).
    let cache = TurnCache::scratch();
    let (id, _) = claim_job(&Some("together-c".into()), "m", Stage::Generating, 4).unwrap();
    let steps = Staged::sending_back(&[1], 10);
    let (out, seen) = tokio::join!(
        made(&id, &cache, &steps),
        progress_at(&[222, 232, 237, 245, 300])
    );
    out.unwrap();
    assert_eq!(seen, [2, 3, 2, 3, 4].map(decomposing));
    assert_eq!(progress(), decomposing(5));
    update(&id, |job| job.stage = Stage::Done);
    cache.remove();

    // A drawing fails: the job stays at its drawings, whatever was decomposed beside them.
    let cache = TurnCache::scratch();
    let (id, _) = claim_job(&Some("together-d".into()), "m", Stage::Generating, 4).unwrap();
    let steps = Staged::sending_back(&[], 0);
    let out = turns_beside_front(&id, &cache, &steps, drawings(&cache, Some(1)), front()).await;
    assert!(out.is_err());
    assert_eq!(progress(), (Stage::Generating, 3, 4));
    update(&id, |job| job.stage = Stage::Done);
    cache.remove();
}

/// Looks into the cache every millisecond until `until`: what it found of a
/// turn's decomposition beside another drawing than the one it is of.
async fn mixed(cache: &TurnCache, until: u64) -> Vec<String> {
    let mut wrong = Vec::new();
    for at in 0..until {
        for index in 0..4 {
            let (png, psd, _) = kept(cache, index).await;
            let Some((psd, _)) = psd else { continue };
            match png {
                Some(png) if png[0].wrapping_add(100) == psd[0] => {}
                other => wrong.push(format!(
                    "at {at}: turn {index} has {other:?} beside {psd:?}"
                )),
            }
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    wrong
}

#[tokio::test(start_paused = true)]
async fn two_turns_sent_back_together_are_each_drawn_again_with_the_cache_never_mixed() {
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[0, 3], 10);
    let (out, wrong) = tokio::join!(made("none", &cache, &steps), mixed(&cache, 500));
    let (_, turned) = out.unwrap();
    assert_eq!(wrong, Vec::<String>::new());
    assert_eq!(
        turned,
        [
            turn(AGAIN, AGAIN + 100),
            turn(10, 110),
            turn(20, 120),
            turn(AGAIN + 3, AGAIN + 103)
        ]
    );
    for (index, png) in [(0, AGAIN), (1, 10), (2, 20), (3, AGAIN + 3)] {
        let (kept_png, kept_psd, _) = kept(&cache, index).await;
        assert_eq!(kept_png, Some(vec![png]));
        assert_eq!(kept_psd, Some((vec![png + 100], true)));
    }
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn a_turn_whose_first_decomposition_fails_stands_on_its_redraw_whichever_ends_first() {
    // The first decomposition fails at 250; the redraw is in before it (235) or after it (265).
    for redraw in [10, 40] {
        let cache = TurnCache::scratch();
        let steps = Staged {
            cannot_split: Some(40),
            ..Staged::sending_back(&[3], redraw)
        };
        let (out, wrong) = tokio::join!(made("none", &cache, &steps), mixed(&cache, 520));
        let (_, turned) = out.unwrap();
        assert_eq!(wrong, Vec::<String>::new());
        assert_eq!(turned[3], turn(AGAIN + 3, AGAIN + 103), "redraw {redraw}");
        cache.remove();
    }
    // Neither is made: the decomposition's failure is the job's.
    let cache = TurnCache::scratch();
    let steps = Staged {
        cannot_split: Some(40),
        redraw_fails: true,
        ..Staged::sending_back(&[3], 40)
    };
    let (_, axum::Json(error)) = made("none", &cache, &steps).await.unwrap_err();
    assert_eq!(error["at"], "split");
    cache.remove();
}

#[tokio::test(start_paused = true)]
async fn a_front_decomposition_in_last_is_counted_and_checked_against_like_any_other() {
    let _slot = JOB_TESTS.lock().await;
    async fn late_front() -> ApiResult<Vec<u8>> {
        tokio::time::sleep(Duration::from_millis(400)).await;
        Ok(b"front".to_vec())
    }
    let cache = TurnCache::scratch();
    let (id, _) = claim_job(&Some("together-e".into()), "m", Stage::Generating, 4).unwrap();
    let steps = Staged::sending_back(&[2], 10);
    let (out, wrong, seen) = tokio::join!(
        turns_beside_front(&id, &cache, &steps, drawings(&cache, None), late_front()),
        mixed(&cache, 700),
        progress_at(&[260, 410, 430, 440, 500])
    );
    let (_, turned) = out.unwrap();
    assert_eq!(wrong, Vec::<String>::new());
    assert_eq!(turned[2], turn(AGAIN + 2, AGAIN + 102));
    // The four turns' are in before the front one (400); the check, at 425,
    // sends one back, drawn again by 435 and decomposed by 645.
    let counted: Vec<u8> = seen.iter().map(|(_, done, _)| *done).collect();
    assert_eq!(counted, [4, 5, 5, 4, 4]);
    assert_eq!(progress(), (Stage::Decomposing, 5, 5));
    update(&id, |job| job.stage = Stage::Done);
    cache.remove();

    // Everything kept and checked by a first job; a second job's check sends a turn back.
    let cache = TurnCache::scratch();
    let steps = Staged::sending_back(&[], 0);
    made("none", &cache, &steps).await.unwrap();
    let steps = Staged::sending_back(&[1], 10);
    let (out, wrong) = tokio::join!(made("none", &cache, &steps), mixed(&cache, 500));
    let (_, turned) = out.unwrap();
    assert_eq!(wrong, Vec::<String>::new());
    assert_eq!(turned[1], turn(AGAIN + 1, AGAIN + 101));
    assert_eq!(
        steps.did(),
        notes(&[
            (225, "checked [30, 10, 20, 40]"),
            (225, "drawing 1 again"),
            (235, "drawn 1 again"),
            (235, "splitting 91"),
            (445, "split 91"),
        ])
    );
    assert_eq!(
        kept(&cache, 1).await,
        (Some(vec![AGAIN + 1]), Some((vec![AGAIN + 101], true)), true)
    );
    cache.remove();
}
