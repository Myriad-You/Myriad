//! 并行：互不依赖的生图与拆层一起发出去，全部跑完再看结果。发出去的请求和
//! 一个一个做时逐个相同，只是不再排着队等；结果仍按原来的顺序交回。
//!
//! 转头图一画好就开始拆层（`turns_beside_front`），和正面拆层、其余图的绘制
//! 同时进行。正面拆层和四张图到齐后预检一次（Space 的 `check_turns`）：画过头
//! 的那张重画一次，重画成了才放弃它手上那份拆层、改拆新图；其余的都接着用
//! 已经在拆的那份。代价是重画成了的那张，原图那份拆层白做。
//!
//! 缓存里一份拆层永远是它旁边那张图的：可能被中途放弃的拆层自己不写缓存，
//! 拆完由不会被放弃的一侧写；换图时先放弃在途的拆层，再清掉旧拆层、写新图。

use std::sync::atomic::{AtomicU8, Ordering};

use futures::future::{FutureExt, join_all};

use super::{
    ApiResult, Stage, TURN_SIDES, TurnCache,
    drawing::{keep_decomposition, keep_redrawn, kept_decomposition},
    update,
};

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

/// What decomposing the turned drawings beside the front one asks of the
/// outside: the Space and the image model. Apart, so the order of the steps
/// can be tested without them.
pub(super) trait TurnedSteps: Sync {
    /// A turned drawing decomposed and checked (`decompose_checked`), kept
    /// nowhere: its decomposition, and whether the check had its say. `kept`
    /// is a decomposition of this drawing the check has not had its say on.
    fn decompose(
        &self,
        png: &[u8],
        kept: Option<Vec<u8>>,
    ) -> impl Future<Output = ApiResult<(Vec<u8>, bool)>> + Send;
    /// The turned drawings, by their place among them, that are drawn past
    /// what keys of the front drawing can follow. None of them when that
    /// cannot be told: the turns are then fitted as drawn.
    fn drawn_past(
        &self,
        front_psd: Vec<u8>,
        turned: Vec<Vec<u8>>,
    ) -> impl Future<Output = Vec<usize>> + Send;
    /// The turn at `index` drawn once more.
    fn draw_again(&self, index: usize) -> impl Future<Output = ApiResult<Vec<u8>>> + Send;
}

/// A turned drawing as it stands in the end, and its decomposition.
pub(super) type Turned = (Vec<u8>, Vec<u8>);

/// The front decomposition and the turned drawings with theirs, all made
/// together: each turned drawing is decomposed from the moment it is in (drawn
/// or kept), beside the front decomposition and the drawings still being made.
///
/// With the front decomposition and every drawing in, the drawings are checked
/// against it once (`drawn_past`). A turn found drawn past what the front
/// drawing's keys can follow is drawn once more, unless this set's was drawn
/// again already (`turned-{index}.redrawn`). Its first decomposition goes on
/// meanwhile: a redraw that fails leaves the turn as it was, decomposition
/// and all. Only a redraw that is in gives the first decomposition up, still
/// running or done, and has the new drawing decomposed in its place: that one
/// decomposition is the cost of not waiting for the check.
///
/// The job counts drawings until all are in, then decompositions of five:
/// those in hand, the front's and the turns' alike, whenever they came in. A
/// decomposition given up for a redraw is taken off the count again.
///
/// Everything begun is seen to its end (`all_or_first_error`): a decomposition
/// paid for is in the cache for the next job whatever else failed. The failure
/// given is the first in this order: a drawing's, the front decomposition's, a
/// turn's own decomposition's.
pub(super) async fn turns_beside_front(
    id: &str,
    cache: &TurnCache,
    steps: &impl TurnedSteps,
    drawings: Vec<impl Future<Output = ApiResult<Vec<u8>>>>,
    front: impl Future<Output = ApiResult<Vec<u8>>>,
) -> ApiResult<(Vec<u8>, Vec<Turned>)> {
    let count = Count {
        id,
        in_hand: AtomicU8::new(0),
    };
    let count = &count;
    let front = async move {
        let psd = front.await?;
        count.one_more();
        ApiResult::Ok(psd)
    }
    .shared();
    let drawings: Vec<_> = drawings
        .into_iter()
        .map(|drawing| {
            async move {
                let png = drawing.await?;
                update(id, |job| job.done += 1);
                ApiResult::Ok(png)
            }
            .shared()
        })
        .collect();
    let drawn = {
        let drawings = drawings.clone();
        async move {
            let drawn = all_or_first_error(drawings).await?;
            count.decomposing();
            ApiResult::Ok(drawn)
        }
        .shared()
    };
    // Asked once, of all the drawings at once; a job that has failed asks nothing.
    let drawn_past = {
        let (drawn, front) = (drawn.clone(), front.clone());
        async move {
            match tokio::join!(drawn, front) {
                (Ok(turned), Ok(front_psd)) => steps.drawn_past(front_psd, turned).await,
                _ => Vec::new(),
            }
        }
        .shared()
    };
    let turns: Vec<_> = drawings
        .iter()
        .enumerate()
        .map(|(index, drawing)| {
            let turn = Turn {
                index,
                cache,
                steps,
                count,
            };
            turn.made(drawing.clone(), drawn_past.clone())
        })
        .collect();
    let (front, drawn, turned) = tokio::join!(front, drawn, join_all(turns));
    drawn?;
    let front = front?;
    let turned = turned.into_iter().collect::<ApiResult<Vec<_>>>()?;
    Ok((front, turned))
}

/// The job's count of decompositions while the turned drawings and the front
/// decomposition are made together.
struct Count<'a> {
    id: &'a str,
    /// Decompositions in hand, of the drawings as they stand.
    in_hand: AtomicU8,
}

impl Count<'_> {
    /// A decomposition is in. While drawings are still counted, the switch to
    /// decompositions counts it.
    fn one_more(&self) {
        self.in_hand.fetch_add(1, Ordering::Relaxed);
        update(self.id, |job| {
            if job.stage == Stage::Decomposing {
                job.done += 1;
            }
        });
    }

    /// One that was counted is given up: its drawing was drawn again.
    fn one_less(&self) {
        self.in_hand.fetch_sub(1, Ordering::Relaxed);
        update(self.id, |job| {
            if job.stage == Stage::Decomposing {
                job.done = job.done.saturating_sub(1);
            }
        });
    }

    /// The drawings are all in: the job counts decompositions from here.
    fn decomposing(&self) {
        update(self.id, |job| {
            job.stage = Stage::Decomposing;
            job.done = self.in_hand.load(Ordering::Relaxed);
            job.total = 5;
        });
    }
}

/// How the decomposition of a turn's first drawing ended.
enum First {
    /// The drawing stands, with this decomposition of it.
    Stands(Vec<u8>),
    /// The turn was drawn again, and this is the new drawing. The first
    /// one's decomposition is given up: nothing of it is running any more.
    Redrawn(Vec<u8>),
}

/// One turned drawing on its way to its decomposition.
struct Turn<'a, S> {
    /// Its place among the turned drawings.
    index: usize,
    cache: &'a TurnCache,
    steps: &'a S,
    count: &'a Count<'a>,
}

impl<S: TurnedSteps> Turn<'_, S> {
    /// The turn's drawing as it stands in the end, and its decomposition.
    async fn made(
        self,
        drawing: impl Future<Output = ApiResult<Vec<u8>>>,
        drawn_past: impl Future<Output = Vec<usize>>,
    ) -> ApiResult<Turned> {
        let png = drawing.await?;
        let first = self.first(&png, drawn_past).await?;
        let again = match first {
            First::Stands(psd) => return Ok((png, psd)),
            First::Redrawn(again) => again,
        };
        // No decomposition of the first drawing is running by now: whatever
        // is written from here on is the new drawing's.
        keep_redrawn(self.cache, self.index, &again).await;
        let made = self.steps.decompose(&again, None).await;
        let made = made.map(|(psd, settled)| (psd, Some(settled)));
        let psd = self.in_hand(made).await?;
        Ok((again, psd))
    }

    /// The first drawing's decomposition, begun at once and run beside the
    /// check of the drawn turns and, for a turn the check sends back, beside
    /// its redraw.
    async fn first(
        &self,
        png: &[u8],
        drawn_past: impl Future<Output = Vec<usize>>,
    ) -> ApiResult<First> {
        let decomposing = self.decomposed(png);
        tokio::pin!(decomposing);
        tokio::pin!(drawn_past);
        // Kept and counted the moment it is in, whatever else is still awaited.
        let mut made = None;
        let drawn_past = loop {
            tokio::select! {
                biased;
                out = &mut decomposing, if made.is_none() => made = Some(self.in_hand(out).await),
                places = &mut drawn_past => break places,
            }
        };
        if drawn_past.contains(&self.index) && !self.drawn_again().await {
            let side = TURN_SIDES.get(self.index).copied().unwrap_or_default();
            tracing::info!(side, "turned drawing drawn again");
            let redrawing = self.steps.draw_again(self.index);
            tokio::pin!(redrawing);
            let again = loop {
                tokio::select! {
                    biased;
                    out = &mut decomposing, if made.is_none() => made = Some(self.in_hand(out).await),
                    drawn = &mut redrawing => break drawn,
                }
            };
            match again {
                Ok(again) => {
                    if matches!(made, Some(Ok(_))) {
                        self.count.one_less();
                    }
                    return Ok(First::Redrawn(again));
                }
                Err(error) => {
                    tracing::warn!(side, ?error, "turned drawing could not be drawn again");
                }
            }
        }
        let psd = match made {
            Some(out) => out,
            None => {
                let out = decomposing.await;
                self.in_hand(out).await
            }
        }?;
        Ok(First::Stands(psd))
    }

    /// The drawing's decomposition: the one the cache has checked, else made,
    /// with whether its check had its say (none for the cache's own). Nothing
    /// is written here. This is dropped half-way when the turn is drawn again,
    /// and a write under way then could still land in the cache after the new
    /// drawing's entries.
    async fn decomposed(&self, png: &[u8]) -> ApiResult<(Vec<u8>, Option<bool>)> {
        let kept = match kept_decomposition(self.cache, self.index + 1).await {
            Some((psd, true)) => return Ok((psd, None)),
            kept => kept.map(|(psd, _)| psd),
        };
        let (psd, settled) = self.steps.decompose(png, kept).await?;
        Ok((psd, Some(settled)))
    }

    /// A decomposition that is in: kept in the cache when it was made just
    /// now, and counted. Run to its end wherever it is called from.
    async fn in_hand(&self, made: ApiResult<(Vec<u8>, Option<bool>)>) -> ApiResult<Vec<u8>> {
        let (psd, settled) = made?;
        if let Some(settled) = settled {
            keep_decomposition(self.cache, self.index + 1, &psd, settled).await;
        }
        self.count.one_more();
        Ok(psd)
    }

    /// Whether this set's drawing of the turn is one drawn again already.
    async fn drawn_again(&self) -> bool {
        let marker = format!("turned-{}.redrawn", self.index);
        self.cache.read(&marker).await.is_some()
    }
}

/// Tests that claim the one job slot take this first, so they do not find it taken by each other.
#[cfg(test)]
pub(super) static JOB_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
#[path = "together_tests.rs"]
mod tests;
