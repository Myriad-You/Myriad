//! 拟合，以及拟合跟不上前发的那张转头图的重画。
//!
//! 生图模型偶尔把某一张转头图的刘海画变了，拟合出的关键形会让头发随转头变长
//! 变短（`hair_misfit`）。看得出来的地方是拟合报告里每一侧前发的轮廓距离：
//! 那一侧比对侧差很多。这个数只取决于正面和那一侧两份拆层，是拟合第一步算
//! 的；Space 的 `hair_fit` 只算这一步，十几秒，完整拟合要几分钟。
//!
//! 所以第一次拟合开始的同时，向 Space 问四个方向的这个数（限时）。看出要重画
//! 的那一侧，就在第一次拟合还在跑的时候把它重画、重拆，再问一次新拆层的数：
//! 没变好，第一次拟合的结果就是结果；变好了，等第一次拟合让出位置，用换过的
//! 那一侧再拟合一次，读这次拟合自己的报告和第一次的比，留好的。
//!
//! 提前这一步没留下重画时（Space 没有 `hair_fit`、限时内问不到、没有哪一侧要
//! 重画、这套图重画过、重画没变好），由 `redraw_misfit` 读第一次拟合自己的
//! 报告来定：报告和提前问到的数一致时它什么都不做。
//!
//! 第一次拟合失败时任务失败。那时重画还没开始，就不开始，也不写「重画过」的
//! 标记；已经在画、在拆，就等它做完，量出来更好的那张写进缓存留给重跑。

use std::time::Duration;

use futures::future::join_all;

use super::{
    ApiResult, Stage, TURN_SIDES, TURNED_REDOS, TurnCache, decompose_checked, draw_turn,
    drawing::redraw_misfit, fit, hair_misfit, turn_prompts, update,
};
use crate::services::{
    image_generation,
    see_through::{self, HairFit, SeeThroughError, TurnKeyformsOutput},
};

/// The five decompositions and the five pictures they were made of: the
/// front's, then the four turns' in `TURN_SIDES` order.
pub(super) type Sets = ([Vec<u8>; 5], [(Vec<u8>, String); 5]);

/// How long the front hair is waited for ahead of the fit: the four turns'
/// together, and the redrawn one's. A measure is 10 to 25 s on the Space,
/// four at once within 30. One that takes longer (a Space still starting, a
/// queue) is done without: the fit's report tells the same, only later, so
/// waiting on would cost the job time and going on costs it nothing.
const HAIR_FIT_WAIT: Duration = Duration::from_secs(120);

/// What the fit and the redraw beside it ask of the outside: the Space, the
/// image model, the cache and the job's progress. Apart, so the order of the
/// steps can be tested without them.
pub(super) trait Steps: Sync {
    /// The full fit of the four turns onto the front.
    fn fit(
        &self,
        decompositions: &[Vec<u8>; 5],
        pictures: &[(Vec<u8>, String); 5],
    ) -> impl Future<Output = ApiResult<TurnKeyformsOutput>> + Send;
    /// The front hair's outline distance of one turned decomposition, as the fit would report it.
    fn hair_fit(
        &self,
        front: Vec<u8>,
        turned: Vec<u8>,
    ) -> impl Future<Output = Result<HairFit, SeeThroughError>> + Send;
    /// How long `hair_fit` is waited for.
    fn hair_fit_wait(&self) -> Duration {
        HAIR_FIT_WAIT
    }
    /// Whether this set of drawings had its turn `index` drawn again for its hair already.
    fn retried(&self, index: usize) -> impl Future<Output = bool> + Send;
    /// Notes that it is, before it is drawn: a rerun never draws it yet again.
    fn mark_retried(&self, index: usize) -> impl Future<Output = ()> + Send;
    /// The turn `index` drawn once more.
    fn draw(&self, index: usize) -> impl Future<Output = ApiResult<Vec<u8>>> + Send;
    /// A turned drawing decomposed, redone when faulty as any turned one is.
    fn decompose(&self, png: &[u8]) -> impl Future<Output = ApiResult<Vec<u8>>> + Send;
    /// The drawing that won and its decomposition, made the set's own.
    fn keep(&self, index: usize, png: &[u8], psd: &[u8]) -> impl Future<Output = ()> + Send;
    /// The job is fitting once more.
    fn fitting_again(&self);
    /// The misfit turn told from the first fit's own report (`redraw_misfit`):
    /// nothing where the report has none or the set was retried already, else
    /// that turn drawn, decomposed and fitted again.
    fn by_report(
        &self,
        sets: Sets,
        first: TurnKeyformsOutput,
        redrawn: &mut Vec<String>,
    ) -> impl Future<Output = ApiResult<(Sets, TurnKeyformsOutput)>> + Send;
}

/// A turn drawn again beside the first fit, worth fitting with.
struct Again {
    index: usize,
    png: Vec<u8>,
    psd: Vec<u8>,
    /// The outline distance of the drawing it would replace, as measured ahead of the fit.
    before: f64,
    /// Its own, when the Space measured it (then less than `before`); none
    /// when it could not be asked: only fitting with it will tell.
    after: Option<f64>,
}

/// The four turns fitted onto the front, with the turn whose front hair the
/// fit cannot follow drawn once more and kept when it is followed better.
/// The redraw is begun beside the first fit when the Space can tell which
/// turn it is ahead of the fit's report (`hair_fit`), and told from that
/// report otherwise (`redraw_misfit`): either way a fit's own report decides
/// which drawing is kept.
pub(super) async fn fit_turns(
    steps: &impl Steps,
    (mut decompositions, mut pictures): Sets,
    redrawn: &mut Vec<String>,
) -> ApiResult<(Sets, TurnKeyformsOutput)> {
    let (first, again) = first_fit(steps, &decompositions, &pictures).await?;
    let Some(again) = again else {
        // With nothing drawn ahead the report is read as if nothing was asked
        // ahead: where it agrees with what was measured, this does nothing.
        return steps
            .by_report((decompositions, pictures), first, redrawn)
            .await;
    };
    let Again {
        index,
        png,
        psd,
        before: measured,
        ..
    } = again;
    let side = TURN_SIDES[index];
    let replaced = (
        std::mem::replace(&mut decompositions[index + 1], psd),
        std::mem::replace(&mut pictures[index + 1], (png, "image/png".to_string())),
    );
    steps.fitting_again();
    let second = steps.fit(&decompositions, &pictures).await?;
    // The two fits' own reports have the last word, whatever was measured ahead of them.
    let before = hair_misfit::hair_outline(&first.keyforms, side).unwrap_or(measured);
    let after = hair_misfit::hair_outline(&second.keyforms, side).unwrap_or(f64::INFINITY);
    if after >= before {
        tracing::info!(
            side,
            before,
            after,
            "the turn drawn again was no better; kept the first"
        );
        (decompositions[index + 1], pictures[index + 1]) = replaced;
        return Ok(((decompositions, pictures), first));
    }
    tracing::info!(
        side,
        before,
        after,
        "the turn drawn again is followed better; kept it"
    );
    steps
        .keep(index, &pictures[index + 1].0, &decompositions[index + 1])
        .await;
    if !redrawn.iter().any(|known| known == side) {
        redrawn.push(side.to_string());
    }
    Ok(((decompositions, pictures), second))
}

/// The first fit, and beside it the misfit turn drawn again when the Space
/// can tell which it is before the fit is over.
///
/// A fit that fails fails the job. Nothing that costs is begun for a job
/// that has failed: while the front hair is still being measured, the failure
/// is returned at once and no turn is marked as drawn again. A redraw under
/// way by then is paid for: it is seen to its end, and one the Space measured
/// as followed better is made the set's own, so a rerun fits with it (the
/// marker keeps that rerun from drawing another).
async fn first_fit(
    steps: &impl Steps,
    decompositions: &[Vec<u8>; 5],
    pictures: &[(Vec<u8>, String); 5],
) -> ApiResult<(TurnKeyformsOutput, Option<Again>)> {
    let fit = steps.fit(decompositions, pictures);
    tokio::pin!(fit);
    let measuring = misfit_ahead(steps, decompositions);
    tokio::pin!(measuring);
    let mut first = None;
    let misfit = loop {
        tokio::select! {
            out = &mut fit, if first.is_none() => first = Some(out?),
            misfit = &mut measuring => break misfit,
        }
    };
    let again = match misfit {
        None => None,
        Some((index, before)) => {
            let redrawing = drawn_again(steps, &decompositions[0], index, before);
            if first.is_some() {
                redrawing.await?
            } else {
                // Both to their end: the fit holds its place on the Space
                // until it is over, and the redraw is paid for.
                let (out, again) = tokio::join!(&mut fit, redrawing);
                match out {
                    Ok(out) => first = Some(out),
                    Err(error) => {
                        if let Ok(Some(again)) = &again
                            && again.after.is_some()
                        {
                            steps.keep(again.index, &again.png, &again.psd).await;
                        }
                        return Err(error);
                    }
                }
                again?
            }
        }
    };
    let first = match first {
        Some(first) => first,
        None => fit.await?,
    };
    Ok((first, again))
}

/// The turn whose front hair is a misfit and its outline distance, measured
/// ahead of the fit. None when the Space has no `hair_fit`, when the four
/// measures are not all had in time, when no turn is far worse than its
/// opposite, or when this set had that turn drawn again already. Nothing is
/// drawn, marked or paid for here.
async fn misfit_ahead(steps: &impl Steps, decompositions: &[Vec<u8>; 5]) -> Option<(usize, f64)> {
    let front = &decompositions[0];
    let asked: Vec<_> = decompositions[1..]
        .iter()
        .map(|turned| steps.hair_fit(front.clone(), turned.clone()))
        .collect();
    let Ok(answers) = tokio::time::timeout(steps.hair_fit_wait(), join_all(asked)).await else {
        tracing::warn!("front hair not measured in time ahead of the fit; told from its report");
        return None;
    };
    let mut outlines = [None; 4];
    for (outline, answer) in outlines.iter_mut().zip(answers) {
        match answer {
            Ok(HairFit::Outline(measured)) => *outline = measured,
            Ok(HairFit::Unsupported) => return None,
            Err(error) => {
                tracing::warn!(%error, "front hair could not be measured ahead of the fit; told from its report");
                return None;
            }
        }
    }
    let index = hair_misfit::misfit_of(outlines)?;
    if steps.retried(index).await {
        return None;
    }
    Some((index, outlines[index].unwrap_or(f64::INFINITY)))
}

/// The misfit turn `index` drawn once more, decomposed and measured. None
/// when it could not be drawn or was measured as no better: the first fit
/// then stands. Marked as drawn again before it is drawn.
async fn drawn_again(
    steps: &impl Steps,
    front: &[u8],
    index: usize,
    before: f64,
) -> ApiResult<Option<Again>> {
    steps.mark_retried(index).await;
    let side = TURN_SIDES[index];
    tracing::info!(
        side,
        before,
        "turned drawing's front hair not followed; drawing it again beside the fit"
    );
    let png = match steps.draw(index).await {
        Ok(png) => png,
        Err(error) => {
            tracing::warn!(side, ?error, "turned drawing could not be drawn again");
            return Ok(None);
        }
    };
    // Decomposed apart from the cached one, which stays the drawing's until this one wins.
    let psd = steps.decompose(&png).await?;
    let measuring = steps.hair_fit(front.to_vec(), psd.clone());
    let after = match tokio::time::timeout(steps.hair_fit_wait(), measuring).await {
        // No front hair fitted on it reads as the report's missing figure does: no better.
        Ok(Ok(HairFit::Outline(after))) => Some(after.unwrap_or(f64::INFINITY)),
        Ok(Ok(HairFit::Unsupported)) => None,
        Ok(Err(error)) => {
            tracing::warn!(side, %error, "the turn drawn again could not be measured; fitting with it to tell");
            None
        }
        Err(_) => {
            tracing::warn!(
                side,
                "the turn drawn again not measured in time; fitting with it to tell"
            );
            None
        }
    };
    if after.is_some_and(|after| after >= before) {
        tracing::info!(
            side,
            before,
            ?after,
            "the turn drawn again was no better; the first fit stands"
        );
        return Ok(None);
    }
    Ok(Some(Again {
        index,
        png,
        psd,
        before,
        after,
    }))
}

/// A job's own steps.
pub(super) struct Turns<'a> {
    pub(super) id: &'a str,
    pub(super) user_id: i32,
    pub(super) generation: &'a image_generation::ImageGenerationConfig,
    pub(super) reference: &'a image_generation::ImageReference,
    pub(super) size: (u32, u32),
    pub(super) client: &'a see_through::SeeThroughClient,
    pub(super) cache: &'a TurnCache,
    pub(super) options: see_through::DecomposeOptions,
}

impl Steps for Turns<'_> {
    async fn fit(
        &self,
        decompositions: &[Vec<u8>; 5],
        pictures: &[(Vec<u8>, String); 5],
    ) -> ApiResult<TurnKeyformsOutput> {
        fit(self.client, decompositions, pictures).await
    }

    async fn hair_fit(&self, front: Vec<u8>, turned: Vec<u8>) -> Result<HairFit, SeeThroughError> {
        self.client.hair_fit(front, turned).await
    }

    async fn retried(&self, index: usize) -> bool {
        let marker = format!("turned-{index}.hair-retried");
        self.cache.read(&marker).await.is_some()
    }

    async fn mark_retried(&self, index: usize) {
        let marker = format!("turned-{index}.hair-retried");
        self.cache.write(&marker, b"retried").await;
    }

    async fn draw(&self, index: usize) -> ApiResult<Vec<u8>> {
        let (width, height) = self.size;
        draw_turn(
            self.user_id,
            self.generation,
            &turn_prompts()[index],
            self.reference,
            width,
            height,
        )
        .await
    }

    async fn decompose(&self, png: &[u8]) -> ApiResult<Vec<u8>> {
        let bytes = axum::body::Bytes::copy_from_slice(png);
        let (psd, _) = decompose_checked(
            self.client,
            &bytes,
            "image/png",
            self.options,
            &TURNED_REDOS,
            None,
        )
        .await?;
        Ok(psd)
    }

    async fn keep(&self, index: usize, png: &[u8], psd: &[u8]) {
        let cache = self.cache;
        cache.write(&format!("turned-{index}.png"), png).await;
        cache
            .write(&format!("decomposition-{}.psd", index + 1), psd)
            .await;
        cache
            .write(&format!("decomposition-{}.checked", index + 1), b"checked")
            .await;
        cache
            .write(&format!("turned-{index}.redrawn"), b"redrawn")
            .await;
    }

    fn fitting_again(&self) {
        update(self.id, |job| {
            job.stage = Stage::Fitting;
            job.done = 0;
            job.total = 1;
        });
    }

    async fn by_report(
        &self,
        (decompositions, pictures): Sets,
        first: TurnKeyformsOutput,
        redrawn: &mut Vec<String>,
    ) -> ApiResult<(Sets, TurnKeyformsOutput)> {
        let (decompositions, pictures, output) = redraw_misfit(
            self.id,
            self.user_id,
            self.generation,
            self.reference,
            self.size,
            self.client,
            self.cache,
            self.options,
            (decompositions, pictures, first),
            redrawn,
        )
        .await?;
        Ok(((decompositions, pictures), output))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::super::served::missing_job;
    use super::*;

    /// The steps as a job of the site took them (2026-11-24), in milliseconds
    /// for its seconds, and what each is to answer.
    struct Staged {
        started: tokio::time::Instant,
        /// What `hair_fit` says of each of the four turns.
        outlines: [Option<f64>; 4],
        /// What a fit's report says of them, where it is not the same.
        reported: Option<[Option<f64>; 4]>,
        /// What both say of the turn drawn again.
        again: Option<f64>,
        /// How long the first fit takes, the four measures, the redraw's measure.
        first_fit: u64,
        hair_fit: u64,
        again_hair_fit: u64,
        /// What `hair_fit` cannot do: not on the Space, fails for the four, fails for the redraw.
        unsupported: bool,
        unmeasured: bool,
        again_unmeasured: bool,
        draw_fails: bool,
        first_fit_fails: bool,
        retried_already: bool,
        /// What was done, and when it ended.
        done: Mutex<Vec<(u64, String)>>,
    }

    const FIT: u64 = 200;
    const HAIR_FIT: u64 = 20;
    const HAIR_FIT_WAIT: u64 = 120;
    const DRAW: u64 = 30;
    const DECOMPOSE: u64 = 215;
    /// The turn drawn again, and its decomposition.
    const AGAIN_PNG: u8 = 8;
    const AGAIN_PSD: u8 = 9;

    impl Staged {
        fn of(outlines: [f64; 4], again: f64) -> Self {
            Self {
                started: tokio::time::Instant::now(),
                outlines: outlines.map(Some),
                reported: None,
                again: Some(again),
                first_fit: FIT,
                hair_fit: HAIR_FIT,
                again_hair_fit: HAIR_FIT,
                unsupported: false,
                unmeasured: false,
                again_unmeasured: false,
                draw_fails: false,
                first_fit_fails: false,
                retried_already: false,
                done: Mutex::default(),
            }
        }

        fn now(&self) -> u64 {
            self.started.elapsed().as_millis() as u64
        }

        async fn took(&self, milliseconds: u64, what: impl Into<String>) {
            tokio::time::sleep(Duration::from_millis(milliseconds)).await;
            self.done.lock().unwrap().push((self.now(), what.into()));
        }

        fn did(&self) -> Vec<(u64, String)> {
            self.done.lock().unwrap().clone()
        }

        fn names(&self) -> Vec<String> {
            self.did().into_iter().map(|(_, what)| what).collect()
        }

        /// Whether a decomposition is the redraw's; else its one byte is its turn's place.
        fn outline_of(&self, psd: &[u8], outlines: [Option<f64>; 4]) -> Option<f64> {
            match psd[0] {
                AGAIN_PSD => self.again,
                place => outlines[usize::from(place) - 1],
            }
        }
    }

    impl Steps for Staged {
        async fn fit(
            &self,
            decompositions: &[Vec<u8>; 5],
            _: &[(Vec<u8>, String); 5],
        ) -> ApiResult<TurnKeyformsOutput> {
            let turns: Vec<u8> = decompositions[1..].iter().map(|psd| psd[0]).collect();
            let first = !turns.contains(&AGAIN_PSD);
            let takes = if first { self.first_fit } else { FIT };
            self.took(takes, format!("fit {turns:?}")).await;
            if first && self.first_fit_fails {
                return Err(missing_job());
            }
            let reported = self.reported.unwrap_or(self.outlines);
            let mut hair = serde_json::Map::new();
            for (side, psd) in TURN_SIDES.iter().zip(&decompositions[1..]) {
                if let Some(outline) = self.outline_of(psd, reported) {
                    hair.insert((*side).to_string(), json!({ "outline": outline }));
                }
            }
            let keys = json!({ "fit": { "front-hair": hair }, "turns": turns });
            Ok(TurnKeyformsOutput {
                keyforms: serde_json::to_vec(&keys).unwrap(),
                psd: Vec::new(),
            })
        }

        async fn hair_fit(&self, _: Vec<u8>, turned: Vec<u8>) -> Result<HairFit, SeeThroughError> {
            let again = turned[0] == AGAIN_PSD;
            let takes = if again {
                self.again_hair_fit
            } else {
                self.hair_fit
            };
            self.took(takes, format!("hair {}", turned[0])).await;
            if self.unsupported {
                return Ok(HairFit::Unsupported);
            }
            if (again && self.again_unmeasured) || (!again && self.unmeasured) {
                return Err(SeeThroughError::Timeout);
            }
            Ok(HairFit::Outline(self.outline_of(&turned, self.outlines)))
        }

        fn hair_fit_wait(&self) -> Duration {
            Duration::from_millis(HAIR_FIT_WAIT)
        }

        async fn retried(&self, _: usize) -> bool {
            self.retried_already
        }

        async fn mark_retried(&self, index: usize) {
            self.took(0, format!("marked {index}")).await;
        }

        async fn draw(&self, index: usize) -> ApiResult<Vec<u8>> {
            self.took(DRAW, format!("drawn {index}")).await;
            if self.draw_fails {
                return Err(missing_job());
            }
            Ok(vec![AGAIN_PNG])
        }

        async fn decompose(&self, png: &[u8]) -> ApiResult<Vec<u8>> {
            assert_eq!(png, [AGAIN_PNG]);
            self.took(DECOMPOSE, "decomposed").await;
            Ok(vec![AGAIN_PSD])
        }

        async fn keep(&self, index: usize, png: &[u8], psd: &[u8]) {
            assert_eq!((png, psd), (&[AGAIN_PNG][..], &[AGAIN_PSD][..]));
            self.took(0, format!("kept {index}")).await;
        }

        fn fitting_again(&self) {
            let at = self.now();
            self.done.lock().unwrap().push((at, "fitting again".into()));
        }

        /// As when the first fit's report has no turn to draw again.
        async fn by_report(
            &self,
            sets: Sets,
            first: TurnKeyformsOutput,
            _: &mut Vec<String>,
        ) -> ApiResult<(Sets, TurnKeyformsOutput)> {
            self.took(0, "by report").await;
            Ok((sets, first))
        }
    }

    fn sets() -> Sets {
        (
            [0, 1, 2, 3, 4].map(|place| vec![place]),
            [10, 11, 12, 13, 14].map(|place| (vec![place], "image/png".to_string())),
        )
    }

    /// The sets as they end, the turns the keys were fitted with, and how long it all took.
    async fn fitted(steps: &Staged, redrawn: &mut Vec<String>) -> (Sets, Vec<u64>, u64) {
        let (sets, output) = fit_turns(steps, sets(), redrawn).await.unwrap();
        let keys: serde_json::Value = serde_json::from_slice(&output.keyforms).unwrap();
        let turns = keys["turns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|turn| turn.as_u64().unwrap())
            .collect();
        (sets, turns, steps.now())
    }

    /// The left turn's fringe grew (2026-10-10): far worse than its opposite.
    const MISFIT: [f64; 4] = [13.4, 25.5, 2.7, 4.6];

    #[tokio::test(start_paused = true)]
    async fn with_no_misfit_turn_the_first_fit_is_all_there_is() {
        let steps = Staged::of([3.0, 4.0, 2.7, 4.6], 1.0);
        let mut redrawn = Vec::new();
        let (kept, turns, took) = fitted(&steps, &mut redrawn).await;
        assert_eq!(turns, [1, 2, 3, 4]);
        assert_eq!(took, FIT);
        assert_eq!(kept.0, sets().0);
        assert!(redrawn.is_empty());
        // The four were asked beside the fit; then the fit's own report is read.
        assert_eq!(
            steps.did(),
            [
                (20, "hair 1".to_string()),
                (20, "hair 2".into()),
                (20, "hair 3".into()),
                (20, "hair 4".into()),
                (200, "fit [1, 2, 3, 4]".into()),
                (200, "by report".into()),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_redraw_that_is_no_better_costs_no_second_fit() {
        let steps = Staged::of(MISFIT, 26.0);
        let mut redrawn = Vec::new();
        let (kept, turns, took) = fitted(&steps, &mut redrawn).await;
        // The first fit's keys and the drawings as they were.
        assert_eq!(turns, [1, 2, 3, 4]);
        assert_eq!(kept.0, sets().0);
        assert_eq!(kept.1, sets().1);
        assert!(redrawn.is_empty());
        // Marked before it was drawn; drawn, decomposed and measured while the fit ran,
        // and over 285 after the start, where one after another takes 665.
        assert_eq!(
            steps.did()[4..],
            [
                (20, "marked 1".to_string()),
                (50, "drawn 1".into()),
                (200, "fit [1, 2, 3, 4]".into()),
                (265, "decomposed".into()),
                (285, "hair 9".into()),
                (285, "by report".into()),
            ]
        );
        assert_eq!(took, HAIR_FIT + DRAW + DECOMPOSE + HAIR_FIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_redraw_that_is_followed_better_is_fitted_with_once_the_first_fit_is_over() {
        let steps = Staged::of(MISFIT, 5.0);
        let mut redrawn = vec!["up".to_string()];
        let (kept, turns, took) = fitted(&steps, &mut redrawn).await;
        // The second fit's keys, the redraw in the left turn's place, kept as the set's own.
        assert_eq!(turns, [1, 9, 3, 4]);
        assert_eq!(
            kept.0,
            [vec![0], vec![1], vec![AGAIN_PSD], vec![3], vec![4]]
        );
        assert_eq!(kept.1[2], (vec![AGAIN_PNG], "image/png".to_string()));
        assert_eq!(kept.1[1].0, [11]);
        assert_eq!(redrawn, ["up", "minus"]);
        assert_eq!(
            steps.did()[6..],
            [
                (200, "fit [1, 2, 3, 4]".to_string()),
                (265, "decomposed".into()),
                (285, "hair 9".into()),
                (285, "fitting again".into()),
                (485, "fit [1, 9, 3, 4]".into()),
                (485, "kept 1".into()),
            ]
        );
        // Where one after another takes 665.
        assert_eq!(took, 485);
        assert!(!steps.names().contains(&"by report".to_string()));
    }

    #[tokio::test(start_paused = true)]
    async fn the_fits_own_reports_decide_whatever_was_measured_ahead() {
        // Measured ahead as a misfit made better (25.5 to 10); the first fit's report
        // has that turn at 9, and the second has the redraw at 10: no better.
        let steps = Staged {
            reported: Some([13.4, 9.0, 2.7, 4.6].map(Some)),
            ..Staged::of(MISFIT, 10.0)
        };
        let mut redrawn = Vec::new();
        let (kept, turns, _) = fitted(&steps, &mut redrawn).await;
        assert_eq!(turns, [1, 2, 3, 4]);
        assert_eq!(kept.0, sets().0);
        assert_eq!(kept.1, sets().1);
        assert!(redrawn.is_empty());
        assert!(steps.names().contains(&"fit [1, 9, 3, 4]".to_string()));
        assert!(!steps.names().contains(&"kept 1".to_string()));
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_measure_ahead_of_the_fit_its_report_tells() {
        // No `hair_fit` on the Space, one that fails, and a set whose turn was drawn again already.
        let unsupported = Staged {
            unsupported: true,
            ..Staged::of(MISFIT, 5.0)
        };
        let unmeasured = Staged {
            unmeasured: true,
            ..Staged::of(MISFIT, 5.0)
        };
        let retried = Staged {
            retried_already: true,
            ..Staged::of(MISFIT, 5.0)
        };
        for steps in [unsupported, unmeasured, retried] {
            let began = steps.now();
            let (_, turns, took) = fitted(&steps, &mut Vec::new()).await;
            assert_eq!(turns, [1, 2, 3, 4]);
            // Nothing drawn or marked: the first fit, then its report.
            assert_eq!(
                steps.names()[4..],
                ["fit [1, 2, 3, 4]".to_string(), "by report".into()]
            );
            assert_eq!(took - began, FIT);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_measure_that_is_slow_is_done_without_and_never_waited_out() {
        // The four not in after 120 (a Space still starting, a queue): the fit is not held
        // past its own end, nothing is drawn, its report tells.
        let slow = Staged {
            hair_fit: 5000,
            ..Staged::of(MISFIT, 5.0)
        };
        let (_, turns, took) = fitted(&slow, &mut Vec::new()).await;
        assert_eq!(turns, [1, 2, 3, 4]);
        assert_eq!(took, FIT);
        assert_eq!(
            slow.did(),
            [
                (200, "fit [1, 2, 3, 4]".to_string()),
                (200, "by report".into())
            ]
        );
        // A fit over before the four are given up on waits for no more than that.
        let quick_fit = Staged {
            hair_fit: 5000,
            first_fit: 50,
            ..Staged::of(MISFIT, 5.0)
        };
        let (_, _, took) = fitted(&quick_fit, &mut Vec::new()).await;
        assert_eq!(took, HAIR_FIT_WAIT);
        // The redraw's measure not in after 120: fitted with to tell, as when it fails.
        let slow_again = Staged {
            again_hair_fit: 5000,
            ..Staged::of(MISFIT, 5.0)
        };
        let mut redrawn = Vec::new();
        let (_, turns, took) = fitted(&slow_again, &mut redrawn).await;
        assert_eq!(turns, [1, 9, 3, 4]);
        assert_eq!(redrawn, ["minus"]);
        assert_eq!(took, HAIR_FIT + DRAW + DECOMPOSE + HAIR_FIT_WAIT + FIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_redraw_that_cannot_be_drawn_or_measured_ends_as_the_report_has_it() {
        // Not drawn: the first fit stands.
        let undrawn = Staged {
            draw_fails: true,
            ..Staged::of(MISFIT, 5.0)
        };
        let (_, turns, _) = fitted(&undrawn, &mut Vec::new()).await;
        assert_eq!(turns, [1, 2, 3, 4]);
        assert!(undrawn.names().contains(&"marked 1".to_string()));
        assert!(!undrawn.names().contains(&"decomposed".to_string()));
        // Drawn but not measured: fitted with to tell, and the report has it better.
        let unmeasured = Staged {
            again_unmeasured: true,
            ..Staged::of(MISFIT, 5.0)
        };
        let mut redrawn = Vec::new();
        let (_, turns, _) = fitted(&unmeasured, &mut redrawn).await;
        assert_eq!(turns, [1, 9, 3, 4]);
        assert_eq!(redrawn, ["minus"]);
        // Not measured, and the fit with it reports it no better: the first fit's keys, the
        // drawings as they were, nothing kept.
        let no_better = Staged {
            again_unmeasured: true,
            ..Staged::of(MISFIT, 26.0)
        };
        let mut redrawn = Vec::new();
        let (kept, turns, _) = fitted(&no_better, &mut redrawn).await;
        assert_eq!(turns, [1, 2, 3, 4]);
        assert_eq!(kept.0, sets().0);
        assert_eq!(kept.1, sets().1);
        assert!(redrawn.is_empty());
        assert!(!no_better.names().contains(&"kept 1".to_string()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_first_fit_that_fails_before_the_redraw_begins_fails_the_job_at_once() {
        // Failed while the front hair was still being measured: nothing marked, drawn or waited for.
        let early = Staged {
            first_fit: 10,
            first_fit_fails: true,
            ..Staged::of(MISFIT, 5.0)
        };
        assert!(fit_turns(&early, sets(), &mut Vec::new()).await.is_err());
        assert_eq!(early.did(), [(10, "fit [1, 2, 3, 4]".to_string())]);
        assert_eq!(early.now(), 10);
        // Failed with no turn to draw again: only the fit's failure.
        let none = Staged {
            first_fit_fails: true,
            ..Staged::of([3.0, 4.0, 2.7, 4.6], 1.0)
        };
        assert!(fit_turns(&none, sets(), &mut Vec::new()).await.is_err());
        assert_eq!(none.names()[4..], ["fit [1, 2, 3, 4]".to_string()]);
        assert_eq!(none.now(), FIT);
    }

    #[tokio::test(start_paused = true)]
    async fn a_first_fit_that_fails_with_the_redraw_under_way_lets_it_end() {
        // Measured better: the set's own for the rerun, then the fit's failure.
        let better = Staged {
            first_fit_fails: true,
            ..Staged::of(MISFIT, 5.0)
        };
        assert!(fit_turns(&better, sets(), &mut Vec::new()).await.is_err());
        assert_eq!(
            better.did()[4..],
            [
                (20, "marked 1".to_string()),
                (50, "drawn 1".into()),
                (200, "fit [1, 2, 3, 4]".into()),
                (265, "decomposed".into()),
                (285, "hair 9".into()),
                (285, "kept 1".into()),
            ]
        );
        // No better, or not measured: seen to its end, not kept.
        for (again, again_unmeasured) in [(26.0, false), (5.0, true)] {
            let steps = Staged {
                first_fit_fails: true,
                again_unmeasured,
                ..Staged::of(MISFIT, again)
            };
            assert!(fit_turns(&steps, sets(), &mut Vec::new()).await.is_err());
            assert!(steps.names().contains(&"hair 9".to_string()));
            assert!(!steps.names().contains(&"kept 1".to_string()));
            assert!(!steps.names().iter().any(|what| what.contains("9, 3, 4")));
        }
    }
}
