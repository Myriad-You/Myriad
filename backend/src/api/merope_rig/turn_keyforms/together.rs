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

/// The turned drawings and the front decomposition, made together (the
/// decomposition needs only the master). The job counts drawings until all
/// are in, then decompositions of five: the front one counts as soon as it is
/// done, also when it was done before the drawings.
pub(super) async fn drawn_beside_front(
    id: &str,
    drawings: Vec<impl Future<Output = ApiResult<Vec<u8>>>>,
    front: impl Future<Output = ApiResult<Vec<u8>>>,
) -> ApiResult<(Vec<Vec<u8>>, Vec<u8>)> {
    let front_done = AtomicBool::new(false);
    let front_done = &front_done;
    let drawings = async {
        let drawn = all_or_first_error(drawings.into_iter().map(|drawing| async move {
            let png = drawing.await?;
            update(id, |job| job.done += 1);
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
    let (drawn, front) = tokio::join!(drawings, front);
    Ok((drawn?, front?))
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

    use super::super::{ApiError, JOB, claim_job, served::missing_job};
    use super::*;

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
        let (drawn, psd) = drawn_beside_front(&id, Vec::from(drawings), front)
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
        drawn_beside_front(&id, Vec::from(drawings), front)
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
        assert!(drawn_beside_front(&id, drawings, front).await.is_err());
        assert_eq!(seen.lock().unwrap().len(), 3);
        assert_eq!(progress(), (Stage::Generating, 1, 4));
        update(&id, |job| job.stage = Stage::Done);
    }
}
