//! 并行：互不依赖的生图与拆层一起发出去，全部跑完再看结果。发出去的请求和
//! 一个一个做时逐个相同，只是不再排着队等；结果仍按原来的顺序交回。

use std::sync::atomic::{AtomicBool, Ordering};

use futures::future::join_all;

use super::{ApiResult, Stage, TurnCache, update};

/// Runs all of `work` at once and waits for every one, then gives their
/// values in the order asked for, or the first failure in that order.
/// Nothing is dropped half-way: a drawing or a decomposition already paid for
/// is finished and kept in the cache even when another one fails, so the next
/// job makes only what is missing.
pub(super) async fn all_or_first_error<T>(
    work: impl IntoIterator<Item = impl Future<Output = ApiResult<T>>>,
) -> ApiResult<Vec<T>> {
    join_all(work).await.into_iter().collect()
}

/// What the cache keeps under `name`, or what `make` makes, kept as soon as
/// it is made. `make` is not started when the cache has it.
pub(super) async fn kept_or_made(
    cache: &TurnCache,
    name: &str,
    make: impl Future<Output = ApiResult<Vec<u8>>>,
) -> ApiResult<Vec<u8>> {
    if let Some(kept) = cache.read(name).await {
        return Ok(kept);
    }
    let made = make.await?;
    cache.write(name, &made).await;
    Ok(made)
}

/// `work` with `beside` run alongside it and finished after it. Work that
/// failed does not wait: `beside` is dropped with it.
async fn finished_beside<T>(
    work: impl Future<Output = ApiResult<T>>,
    beside: impl Future<Output = ()>,
) -> ApiResult<T> {
    tokio::pin!(work);
    tokio::pin!(beside);
    let mut beside_done = false;
    let out = loop {
        tokio::select! {
            () = &mut beside, if !beside_done => beside_done = true,
            out = &mut work => break out,
        }
    };
    let out = out?;
    if !beside_done {
        beside.await;
    }
    Ok(out)
}

/// The turned drawings and the front decomposition, made together (the
/// decomposition needs only the master). The job counts drawings until all
/// are in, then decompositions of five: the front one counts as soon as it is
/// done, also when it was done before the drawings.
///
/// Each drawing is handed to `send_ahead` as it comes (drawn or kept), one
/// after another, while the front decomposition still runs: the uploads the
/// calls after it would start with are behind it by then. Sending is not part
/// of a drawing: the count and the switch to decompositions do not wait for
/// it. `send_ahead` tells whether it was sent; the first that was not ends
/// the sending, the ones after it left to the calls that carry them.
///
/// What is still being sent when all is made is waited for. While sending
/// goes well that is time the next call would spend sending the same bytes.
/// When it does not, it is at most the one attempt that failed (as long as a
/// Space that is not running is waited for, or an upload's time limit), not
/// one such wait for each drawing. Nothing is waited for when a drawing or
/// the decomposition failed.
pub(super) async fn drawn_beside_front<S: Future<Output = bool>>(
    id: &str,
    drawings: Vec<impl Future<Output = ApiResult<Vec<u8>>>>,
    front: impl Future<Output = ApiResult<Vec<u8>>>,
    send_ahead: impl Fn(Vec<u8>) -> S,
) -> ApiResult<(Vec<Vec<u8>>, Vec<u8>)> {
    let front_done = AtomicBool::new(false);
    let front_done = &front_done;
    let (came, mut coming) = tokio::sync::mpsc::unbounded_channel();
    // Owns `came`: no more are coming once the drawings are over, however they end.
    let drawings = async move {
        let came = &came;
        let drawn = all_or_first_error(drawings.into_iter().map(|drawing| async move {
            let png = drawing.await?;
            update(id, |job| job.done += 1);
            let _ = came.send(png.clone());
            ApiResult::Ok(png)
        }))
        .await?;
        update(id, |job| {
            job.stage = Stage::Decomposing;
            job.done = u8::from(front_done.load(Ordering::Relaxed));
            job.total = 5;
        });
        ApiResult::Ok(drawn)
    };
    let front = async {
        let psd = front.await?;
        front_done.store(true, Ordering::Relaxed);
        // While drawings are still counted, the switch to decompositions counts it.
        update(id, |job| {
            if job.stage == Stage::Decomposing {
                job.done += 1;
            }
        });
        ApiResult::Ok(psd)
    };
    let made = async {
        let (drawn, front) = tokio::join!(drawings, front);
        ApiResult::Ok((drawn?, front?))
    };
    let sending = async {
        while let Some(png) = coming.recv().await {
            if !send_ahead(png).await {
                // What kept this one from the Space would keep the others: no more are taken.
                coming.close();
                break;
            }
        }
    };
    finished_beside(made, sending).await
}

/// Tests that claim the one job slot take this first, so they do not find it taken by each other.
#[cfg(test)]
pub(super) static JOB_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use super::super::{Ahead, ApiError, JOB, claim_job, served::missing_job};
    use super::*;
    use crate::services::see_through;

    fn failed() -> ApiError {
        missing_job()
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

    fn progress() -> (Stage, u8, u8) {
        let job = JOB.lock().unwrap();
        let job = job.as_ref().expect("a job");
        (job.stage, job.done, job.total)
    }

    type Seen = Arc<Mutex<Vec<(Stage, u8, u8)>>>;

    /// A unit that takes `wait` and notes the job's progress as it ends.
    async fn unit(wait: u64, seen: Seen, out: ApiResult<Vec<u8>>) -> ApiResult<Vec<u8>> {
        tokio::time::sleep(Duration::from_millis(wait)).await;
        seen.lock().unwrap().push(progress());
        out
    }

    #[tokio::test(start_paused = true)]
    async fn the_job_counts_drawings_then_decompositions_with_the_front_one_made_beside() {
        let _slot = JOB_TESTS.lock().await;
        let seen = Seen::default();

        // The drawings are in first: the job is at decomposing, none done, while the front one runs.
        let (id, _) = claim_job(&Some("together-a".into()), "m", Stage::Generating, 4).unwrap();
        let drawings = [30, 10, 20, 40].map(|wait| unit(wait, seen.clone(), Ok(vec![wait as u8])));
        let front = unit(90, seen.clone(), Ok(b"front".to_vec()));
        let (drawn, psd) = drawn_beside_front(&id, Vec::from(drawings), front, unsent)
            .await
            .unwrap();
        assert_eq!(drawn, [vec![30], vec![10], vec![20], vec![40]]);
        assert_eq!(psd, b"front");
        assert_eq!(
            *seen.lock().unwrap(),
            [
                (Stage::Generating, 0, 4),
                (Stage::Generating, 1, 4),
                (Stage::Generating, 2, 4),
                (Stage::Generating, 3, 4),
                (Stage::Decomposing, 0, 5),
            ]
        );
        assert_eq!(progress(), (Stage::Decomposing, 1, 5));
        update(&id, |job| job.stage = Stage::Done);

        // The front decomposition is in first: it is not counted as a drawing, and is there at the switch.
        seen.lock().unwrap().clear();
        let (id, _) = claim_job(&Some("together-b".into()), "m", Stage::Generating, 4).unwrap();
        let drawings = [30, 40].map(|wait| unit(wait, seen.clone(), Ok(Vec::new())));
        let front = unit(10, seen.clone(), Ok(Vec::new()));
        drawn_beside_front(&id, Vec::from(drawings), front, unsent)
            .await
            .unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            [
                (Stage::Generating, 0, 4),
                (Stage::Generating, 0, 4),
                (Stage::Generating, 1, 4),
            ]
        );
        assert_eq!(progress(), (Stage::Decomposing, 1, 5));
        update(&id, |job| job.stage = Stage::Done);

        // A drawing fails: the front decomposition is still finished, and the failure is the drawing's.
        seen.lock().unwrap().clear();
        let (id, _) = claim_job(&Some("together-c".into()), "m", Stage::Generating, 4).unwrap();
        let drawings = vec![
            unit(10, seen.clone(), Err(failed())),
            unit(20, seen.clone(), Ok(Vec::new())),
        ];
        let front = unit(60, seen.clone(), Ok(Vec::new()));
        assert!(
            drawn_beside_front(&id, drawings, front, unsent)
                .await
                .is_err()
        );
        assert_eq!(seen.lock().unwrap().len(), 3);
        assert_eq!(progress(), (Stage::Generating, 1, 4));
        update(&id, |job| job.stage = Stage::Done);
    }

    /// Nothing needs sending ahead.
    async fn unsent(_: Vec<u8>) -> bool {
        true
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_drawing_that_cannot_be_sent_ahead_ends_the_sending() {
        let _slot = JOB_TESTS.lock().await;
        let seen = Seen::default();
        let tried = Arc::new(Mutex::new(Vec::new()));
        let started = tokio::time::Instant::now();
        let (id, _) = claim_job(&Some("together-g".into()), "m", Stage::Generating, 4).unwrap();
        let drawings = [30, 10, 20, 40].map(|wait| unit(wait, seen.clone(), Ok(vec![wait as u8])));
        let front = unit(60, seen.clone(), Ok(b"front".to_vec()));
        // The Space cannot be reached: each try is given up after 100.
        let send_ahead = |png: Vec<u8>| {
            let tried = tried.clone();
            async move {
                tried.lock().unwrap().push(png);
                tokio::time::sleep(Duration::from_millis(100)).await;
                false
            }
        };
        let (drawn, psd) = drawn_beside_front(&id, Vec::from(drawings), front, send_ahead)
            .await
            .unwrap();
        // Only the first to come was tried; the others are left to the calls that carry them.
        assert_eq!(*tried.lock().unwrap(), [vec![10]]);
        // The job's drawings, their order and its progress are as without sending ahead.
        assert_eq!(drawn, [vec![30], vec![10], vec![20], vec![40]]);
        assert_eq!(psd, b"front");
        assert_eq!(
            *seen.lock().unwrap(),
            [
                (Stage::Generating, 0, 4),
                (Stage::Generating, 1, 4),
                (Stage::Generating, 2, 4),
                (Stage::Generating, 3, 4),
                (Stage::Decomposing, 0, 5),
            ]
        );
        assert_eq!(progress(), (Stage::Decomposing, 1, 5));
        // The one try, begun at 10, was waited for; four more would have run to 510.
        assert_eq!(started.elapsed(), Duration::from_millis(110));
        update(&id, |job| job.stage = Stage::Done);
    }

    #[tokio::test(start_paused = true)]
    async fn drawings_are_sent_ahead_as_they_come_without_holding_the_count() {
        let _slot = JOB_TESTS.lock().await;
        let seen = Seen::default();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let started = tokio::time::Instant::now();
        let (id, _) = claim_job(&Some("together-d".into()), "m", Stage::Generating, 4).unwrap();
        let drawings = [30, 10, 20, 40].map(|wait| unit(wait, seen.clone(), Ok(vec![wait as u8])));
        let front = unit(60, seen.clone(), Ok(b"front".to_vec()));
        // Each takes 50 to send, one after another: the last is sent at 210, long after all is made.
        let send_ahead = |png: Vec<u8>| {
            let sent = sent.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                sent.lock().unwrap().push((png, progress()));
                true
            }
        };
        let (drawn, psd) = drawn_beside_front(&id, Vec::from(drawings), front, send_ahead)
            .await
            .unwrap();
        // The result is in the order asked for; what was sent, in the order it came.
        assert_eq!(drawn, [vec![30], vec![10], vec![20], vec![40]]);
        assert_eq!(psd, b"front");
        let sent = sent.lock().unwrap().clone();
        let pictures: Vec<_> = sent.iter().map(|(png, _)| png.clone()).collect();
        assert_eq!(pictures, [vec![10], vec![20], vec![30], vec![40]]);
        // The count never waited on a send: the drawings were counted as they came, the
        // switch to decompositions was at 40, and the front one was counted at 60, while
        // the first send ended at 60 and the others after.
        assert_eq!(
            *seen.lock().unwrap(),
            [
                (Stage::Generating, 0, 4),
                (Stage::Generating, 1, 4),
                (Stage::Generating, 2, 4),
                (Stage::Generating, 3, 4),
                (Stage::Decomposing, 0, 5),
            ]
        );
        let at_each_send: Vec<_> = sent.iter().map(|(_, progress)| *progress).collect();
        assert_eq!(at_each_send[1..], [(Stage::Decomposing, 1, 5); 3]);
        // What was still being sent when all was made was waited for.
        assert_eq!(started.elapsed(), Duration::from_millis(210));
        update(&id, |job| job.stage = Stage::Done);
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_job_does_not_wait_for_what_is_being_sent_ahead() {
        let _slot = JOB_TESTS.lock().await;
        let seen = Seen::default();
        let started = tokio::time::Instant::now();
        let (id, _) = claim_job(&Some("together-e".into()), "m", Stage::Generating, 4).unwrap();
        let drawings = vec![unit(10, seen.clone(), Ok(vec![1]))];
        let front = unit(30, seen.clone(), Err(failed()));
        // A send that never ends.
        let stuck = |_: Vec<u8>| std::future::pending::<bool>();
        assert!(
            drawn_beside_front(&id, drawings, front, stuck)
                .await
                .is_err()
        );
        assert_eq!(started.elapsed(), Duration::from_millis(30));
        update(&id, |job| job.stage = Stage::Done);
    }

    #[tokio::test]
    async fn a_drawing_that_cannot_be_sent_ahead_is_still_the_jobs() {
        let _slot = JOB_TESTS.lock().await;
        let seen = Seen::default();
        let (id, _) = claim_job(&Some("together-f".into()), "m", Stage::Generating, 4).unwrap();
        // Neither the master nor the drawings are pictures: every send is refused
        // before the Space is asked anything.
        let client = see_through::SeeThroughClient::offline();
        let master = (axum::body::Bytes::new(), "image/gif".to_string());
        let ahead = Ahead::new(&client, &master);
        // The master cannot be sent: nor is a drawing tried after it.
        assert!(!ahead.send(Vec::new()).await);
        let drawings = [2, 1].map(|wait| unit(wait, seen.clone(), Ok(Vec::new())));
        let front = unit(3, seen.clone(), Ok(b"front".to_vec()));
        let (drawn, psd) =
            drawn_beside_front(&id, Vec::from(drawings), front, |png| ahead.send(png))
                .await
                .unwrap();
        assert_eq!(drawn, [Vec::<u8>::new(), Vec::new()]);
        assert_eq!(psd, b"front");
        assert_eq!(progress(), (Stage::Decomposing, 1, 5));
        update(&id, |job| job.stage = Stage::Done);
    }
}
