//! Unit tests for `head_first.rs`, with the body's keys (`body_turn`) made alongside as a job makes them.

use std::{sync::Mutex, time::Duration};

use serde_json::Value;

use super::super::{
    ApiError,
    body_turn::{BodySteps, Motion, keyed_behind},
    claim_job,
    figure::{alongside, plain},
    served::missing_job,
    the_job,
    together::JOB_TESTS,
};
use super::*;

/// A failure that tells which step it is of.
fn failed_at(step: &str) -> ApiError {
    (
        missing_job().0,
        axum::Json(serde_json::json!({ "at": step })),
    )
}

fn at_of<T>(out: ApiResult<T>) -> Option<String> {
    let (_, axum::Json(error)) = out.err()?;
    error["at"].as_str().map(str::to_string)
}

/// Nothing here waits longer than this: a wait that never ends fails its test.
const LIMIT: Duration = Duration::from_secs(3600);

/// How long the steps take here, milliseconds for a job's seconds, set so
/// that no two of them end at the same moment.
const HEAD_SPLIT: u64 = 250;
const HEAD_FIT: u64 = 200;
const UPPER: u64 = 220;
const TILE: [u64; 2] = [210, 215];
const STITCH: u64 = 60;
const BODY_DRAWN: [u64; 2] = [45, 40];
const BODY_SPLIT: [u64; 2] = [205, 195];
const BODY_FIT: u64 = 60;

/// How a staged head's work goes wrong.
#[derive(Clone, Copy, PartialEq)]
enum HeadFails {
    /// Its decompositions fail: it returns without a word of them.
    Split,
    /// Its fit fails, the decompositions in hand.
    Fit,
    /// It is done without ever saying its decompositions were in hand.
    Silent,
}

/// The steps a figure's job asks of the outside, as staged for a test.
struct Staged {
    started: tokio::time::Instant,
    id: String,
    /// How long the head's decompositions and its fit take.
    split: u64,
    fit: u64,
    head_fails: Option<HeadFails>,
    tile_fails: bool,
    /// What was begun and what ended, and when.
    did: Mutex<Vec<(u64, String)>>,
}

impl Staged {
    fn of(id: &str) -> Self {
        Self {
            started: tokio::time::Instant::now(),
            id: id.to_string(),
            split: HEAD_SPLIT,
            fit: HEAD_FIT,
            head_fails: None,
            tile_fails: false,
            did: Mutex::default(),
        }
    }

    fn now(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn note(&self, what: impl Into<String>) {
        let at = self.now();
        self.did.lock().unwrap().push((at, what.into()));
    }

    /// When `what` was noted.
    fn at(&self, what: &str) -> Option<u64> {
        let did = self.did.lock().unwrap();
        did.iter()
            .find(|(_, noted)| noted == what)
            .map(|(at, _)| *at)
    }

    /// Whether anything noted has `part` in it.
    fn did_any(&self, part: &str) -> bool {
        let did = self.did.lock().unwrap();
        did.iter().any(|(_, noted)| noted.contains(part))
    }

    async fn takes(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    fn counts(&self, stage: Stage, done: u8, total: u8) {
        update(&self.id, |job| {
            job.stage = stage;
            job.done = done;
            job.total = total;
        });
    }
}

impl TileSteps for Staged {
    /// Counts its stages as `run` does: drawings, decompositions, the fit.
    async fn head(&self, split: HeadSplit) -> ApiResult<Made> {
        self.note("head begun");
        self.counts(Stage::Generating, 0, 4);
        self.takes(self.split).await;
        if self.head_fails == Some(HeadFails::Split) {
            return Err(failed_at("head split"));
        }
        self.counts(Stage::Decomposing, 5, 5);
        self.note("head in hand");
        if self.head_fails == Some(HeadFails::Silent) {
            drop(split);
        } else {
            split.settle(true);
        }
        self.counts(Stage::Fitting, 0, 1);
        self.takes(self.fit).await;
        if self.head_fails == Some(HeadFails::Fit) {
            return Err(failed_at("head fit"));
        }
        self.note("head fitted");
        let mut made = plain(b"head".to_vec(), "image/png".to_string());
        made.files.decompositions[0] = b"upper".to_vec();
        made.files.keyforms = b"{}".to_vec();
        Ok(made)
    }

    async fn upper(&self) -> ApiResult<Vec<u8>> {
        self.note("splitting upper");
        self.takes(UPPER).await;
        self.note("split upper");
        Ok(b"upper".to_vec())
    }

    async fn tile(&self, at: usize) -> ApiResult<Vec<u8>> {
        self.note(format!("splitting tile {at}"));
        self.takes(TILE[at]).await;
        self.note(format!("split tile {at}"));
        if self.tile_fails && at == 0 {
            return Err(failed_at("tile"));
        }
        Ok(vec![at as u8])
    }

    async fn stitch(&self, tiles: [Vec<u8>; 3]) -> ApiResult<Vec<u8>> {
        assert_eq!(tiles, [b"upper".to_vec(), vec![0], vec![1]]);
        self.note("stitching");
        self.takes(STITCH).await;
        Ok(b"stitched".to_vec())
    }

    async fn finish(&self, made: Option<Made>, stitched: Vec<u8>) -> ApiResult<Made> {
        self.note("finished");
        let mut made = made.unwrap_or_else(|| plain(Vec::new(), "image/png".to_string()));
        made.files.baked = stitched;
        Ok(made)
    }
}

impl BodySteps for Staged {
    async fn draw(&self, side: &'static str) -> ApiResult<Vec<u8>> {
        self.note(format!("drawing body {side}"));
        self.takes(BODY_DRAWN[usize::from(side == "left")]).await;
        Ok(side.as_bytes().to_vec())
    }

    async fn decompose(&self, side: &'static str, png: &[u8]) -> ApiResult<Vec<u8>> {
        assert_eq!(png, side.as_bytes());
        self.note(format!("splitting body {side}"));
        self.takes(BODY_SPLIT[usize::from(side == "left")]).await;
        Ok(png.to_vec())
    }

    async fn keys(&self, turned: [Vec<u8>; 2], pictures: [Vec<u8>; 2]) -> ApiResult<Value> {
        assert_eq!(turned, pictures);
        self.note("fitting body");
        self.takes(BODY_FIT).await;
        Ok(serde_json::json!({ "turned": ["right", "left"] }))
    }
}

/// What the body's side of a job gives: its keys, none when it gave way to a
/// head that failed; nothing at all when the job failed before it was done.
type BodyOut = Option<ApiResult<Option<Value>>>;

/// A figure's tiles and its body's keys made as a job makes them, the head
/// keyed or not, within `LIMIT`.
async fn made(id: &str, steps: &Staged, keyed: bool) -> (ApiResult<Made>, BodyOut) {
    let (split, way) = if keyed {
        let (split, way) = head_first();
        (Some(split), way)
    } else {
        (None, Way::open())
    };
    let body = keyed_behind(id, Motion::Turn, steps, &way);
    let job = alongside(id, in_tiles(id, steps, split, &way), body);
    tokio::time::timeout(LIMIT, job)
        .await
        .expect("nothing waits for ever")
}

/// Whether the way cleared, waited for within `LIMIT`.
async fn cleared(way: &Way) -> bool {
    tokio::time::timeout(LIMIT, way.clear())
        .await
        .expect("nothing waits for ever")
}

type Progress = (Phase, Stage, u8, u8);

/// The job's phase and count, and its body's stage and count.
fn progress() -> (Progress, Option<(Stage, u8, u8)>) {
    the_job(|job| {
        (
            (job.phase, job.stage, job.done, job.total),
            job.body.map(|body| (body.stage, body.done, body.total)),
        )
    })
}

/// The job's progress at each of `at`, milliseconds from now.
async fn progress_at(at: &[u64]) -> Vec<(Progress, Option<(Stage, u8, u8)>)> {
    let started = tokio::time::Instant::now();
    let mut seen = Vec::new();
    for &at in at {
        tokio::time::sleep_until(started + Duration::from_millis(at)).await;
        seen.push(progress());
    }
    seen
}

/// A figure's job as `run_figure` leaves it for its tiles: the whole
/// decomposition counted, one of `total`.
fn claimed(outfit: &str, total: u8) -> String {
    let (id, _) = claim_job(&Some(outfit.into()), "m", Stage::Decomposing, total).unwrap();
    update(&id, |job| job.done = 1);
    id
}

#[tokio::test(start_paused = true)]
async fn the_way_clears_when_the_heads_decompositions_are_in_hand_and_never_leaves_a_wait_hanging()
{
    // In hand: everyone waiting is told, whenever they came to wait.
    let (split, way) = head_first();
    let tell = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        split.settle(true);
    };
    let (first, second, ()) = tokio::join!(cleared(&way), cleared(&way), tell);
    assert!(first && second);
    assert!(cleared(&way).await, "told once, clear from then on");

    // Failed, or gone without a word: the wait ends, and nothing is to be begun.
    let (split, way) = head_first();
    split.settle(false);
    assert!(!cleared(&way).await);
    let (split, way) = head_first();
    let gone = async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        drop(split);
    };
    let (in_hand, ()) = tokio::join!(cleared(&way), gone);
    assert!(!in_hand);

    // Nothing to give way to.
    assert!(cleared(&Way::open()).await);
    HeadSplit::unwatched().settle(true);
}

#[tokio::test(start_paused = true)]
async fn the_tiles_and_the_bodys_decompositions_begin_once_the_heads_are_in_hand() {
    let _slot = JOB_TESTS.lock().await;
    let id = claimed("head-first-a", 1);
    let steps = Staged::of(&id);
    let ((head, body), seen) = tokio::join!(
        made(&id, &steps, true),
        progress_at(&[100, 300, 452, 462, 470])
    );
    let head = head.unwrap();
    assert_eq!(head.files.baked, b"stitched");
    assert_eq!(head.files.keyforms, b"{}");
    assert!(matches!(body, Some(Ok(Some(_)))));

    // The head's work and the body's drawings begin at once.
    for begun in ["head begun", "drawing body right", "drawing body left"] {
        assert_eq!(steps.at(begun), Some(0), "{begun}");
    }
    // Every other decomposition begins the moment the head's are in hand, none before.
    assert_eq!(steps.at("head in hand"), Some(HEAD_SPLIT));
    for begun in [
        "splitting tile 0",
        "splitting tile 1",
        "splitting body right",
        "splitting body left",
    ] {
        assert_eq!(steps.at(begun), Some(HEAD_SPLIT), "{begun}");
    }
    assert!(
        !steps.did_any("upper"),
        "the upper tile is the head's front decomposition"
    );
    // The fit is done at 450, the tiles are in at 460 and 465: the stitch waits for both.
    assert_eq!(steps.at("head fitted"), Some(HEAD_SPLIT + HEAD_FIT));
    assert_eq!(steps.at("stitching"), Some(HEAD_SPLIT + TILE[1]));
    assert_eq!(steps.now(), HEAD_SPLIT + TILE[1] + STITCH);

    // The job counts the head's stages, then stitching from the head's end:
    // a tile coming in (460) changes nothing. The body waits as decomposing.
    let stitching = (Phase::Figure, Stage::Stitching, 0, 1);
    assert_eq!(
        seen,
        [
            (
                (Phase::Head, Stage::Generating, 0, 4),
                Some((Stage::Decomposing, 0, 2))
            ),
            (
                (Phase::Head, Stage::Fitting, 0, 1),
                Some((Stage::Decomposing, 0, 2))
            ),
            (stitching, Some((Stage::Decomposing, 1, 2))),
            (stitching, Some((Stage::Fitting, 0, 1))),
            (stitching, Some((Stage::Fitting, 0, 1))),
        ]
    );
    assert_eq!(
        progress(),
        (
            (Phase::Figure, Stage::Stitching, 1, 1),
            Some((Stage::Done, 1, 1))
        )
    );
    update(&id, |job| job.stage = Stage::Done);
}

#[tokio::test(start_paused = true)]
async fn tiles_in_before_the_heads_fit_ends_leave_its_count_alone_and_are_stitched_after_it() {
    let _slot = JOB_TESTS.lock().await;
    let id = claimed("head-first-b", 1);
    let steps = Staged {
        fit: 300,
        ..Staged::of(&id)
    };
    let ((head, _), seen) = tokio::join!(made(&id, &steps, true), progress_at(&[470, 560]));
    head.unwrap();
    assert_eq!(steps.at("split tile 1"), Some(HEAD_SPLIT + TILE[1]));
    assert_eq!(steps.at("head fitted"), Some(HEAD_SPLIT + 300));
    assert_eq!(steps.at("stitching"), Some(HEAD_SPLIT + 300));
    assert_eq!(seen[0].0, (Phase::Head, Stage::Fitting, 0, 1));
    assert_eq!(seen[1].0, (Phase::Figure, Stage::Stitching, 0, 1));
    update(&id, |job| job.stage = Stage::Done);
}

#[tokio::test(start_paused = true)]
async fn when_the_heads_decompositions_fail_nothing_behind_them_is_begun_and_the_job_fails_at_once()
{
    let _slot = JOB_TESTS.lock().await;
    let id = claimed("head-first-c", 1);
    let steps = Staged {
        head_fails: Some(HeadFails::Split),
        ..Staged::of(&id)
    };
    let (head, body) = made(&id, &steps, true).await;
    assert_eq!(at_of(head).as_deref(), Some("head split"));
    assert!(body.is_none());
    assert_eq!(steps.now(), HEAD_SPLIT);
    // The drawings, begun at once, are all the body had made.
    assert_eq!(steps.at("drawing body left"), Some(0));
    assert!(!steps.did_any("splitting") && !steps.did_any("stitching"));
    assert_eq!(progress().1, Some((Stage::Decomposing, 0, 2)));
    update(&id, |job| job.stage = Stage::Done);

    // A head that returns before anything is made: the same, with no wait at all.
    let steps = Staged {
        split: 0,
        head_fails: Some(HeadFails::Split),
        ..Staged::of("none")
    };
    let (head, _) = made("none", &steps, true).await;
    assert_eq!(at_of(head).as_deref(), Some("head split"));
    assert_eq!(steps.now(), 0);
    assert!(!steps.did_any("splitting"));

    // The body's keys by themselves, the head gone while its drawings are made.
    let steps = Staged::of("none");
    let (split, way) = head_first();
    let gone = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(split);
    };
    let keys = keyed_behind("none", Motion::Turn, &steps, &way);
    let (keys, ()) = tokio::time::timeout(LIMIT, async { tokio::join!(keys, gone) })
        .await
        .expect("nothing waits for ever");
    assert!(matches!(keys, Ok(None)));
    assert_eq!(steps.now(), BODY_DRAWN[0]);
    assert!(!steps.did_any("splitting"));
}

#[tokio::test(start_paused = true)]
async fn what_was_begun_is_finished_when_the_heads_fit_or_a_tile_fails() {
    // The fit fails at 450: the tiles under way are decomposed to their end (465).
    let steps = Staged {
        head_fails: Some(HeadFails::Fit),
        ..Staged::of("none")
    };
    let (head, body) = made("none", &steps, true).await;
    assert_eq!(at_of(head).as_deref(), Some("head fit"));
    assert!(body.is_none());
    assert_eq!(steps.at("split tile 1"), Some(HEAD_SPLIT + TILE[1]));
    assert_eq!(steps.now(), HEAD_SPLIT + TILE[1]);
    assert!(!steps.did_any("stitching"));

    // A tile fails: the head's work is seen to its end, nothing is stitched.
    let steps = Staged {
        tile_fails: true,
        ..Staged::of("none")
    };
    let (head, _) = made("none", &steps, true).await;
    assert_eq!(at_of(head).as_deref(), Some("tile"));
    assert_eq!(steps.at("head fitted"), Some(HEAD_SPLIT + HEAD_FIT));
    assert_eq!(steps.now(), HEAD_SPLIT + TILE[1]);
    assert!(!steps.did_any("stitching"));

    // A head done without a word of its decompositions fails the job; it does not hang it.
    let steps = Staged {
        head_fails: Some(HeadFails::Silent),
        ..Staged::of("none")
    };
    let (head, body) = made("none", &steps, true).await;
    assert!(head.is_err());
    assert!(body.is_none());
    assert!(!steps.did_any("splitting") && !steps.did_any("stitching"));
}

#[tokio::test(start_paused = true)]
async fn with_no_head_keyed_the_three_tiles_and_the_bodys_decompositions_go_together() {
    let _slot = JOB_TESTS.lock().await;
    let id = claimed("head-first-d", 4);
    let steps = Staged::of(&id);
    let ((head, body), seen) = tokio::join!(
        made(&id, &steps, false),
        progress_at(&[100, 212, 217, 222, 290])
    );
    let head = head.unwrap();
    assert_eq!(head.files.baked, b"stitched");
    assert_eq!(head.files.keyforms, b"null");
    assert!(matches!(body, Some(Ok(Some(_)))));
    for begun in ["splitting upper", "splitting tile 0", "splitting tile 1"] {
        assert_eq!(steps.at(begun), Some(0), "{begun}");
    }
    assert!(!steps.did_any("head"));
    // The body's decompositions follow its drawings, with nothing to wait for.
    assert_eq!(steps.at("splitting body right"), Some(BODY_DRAWN[0]));
    assert_eq!(steps.at("stitching"), Some(UPPER));
    // The whole, then the three tiles as they come; the body's fit (310) is waited for last.
    let decomposing = |done: u8| (Phase::Figure, Stage::Decomposing, done, 4u8);
    assert_eq!(
        seen.iter().map(|seen| seen.0).collect::<Vec<_>>(),
        [
            decomposing(1),
            decomposing(2),
            decomposing(3),
            (Phase::Figure, Stage::Stitching, 0, 1),
            (Phase::Body, Stage::Fitting, 0, 1),
        ]
    );
    assert_eq!(steps.now(), BODY_DRAWN[0] + BODY_SPLIT[0] + BODY_FIT);
    update(&id, |job| job.stage = Stage::Done);
}
