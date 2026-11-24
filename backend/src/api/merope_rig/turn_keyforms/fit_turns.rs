//! 拟合，以及「拟合跟不上前发的那张转头图重画一次」提前到拟合旁边做。
//!
//! 原来是：完整拟合一次（几分钟）→ 从它的报告里看出某一侧的前发轮廓比对侧差
//! 很多 → 重画那张、重拆 → 再完整拟合一次 → 比前后两次那一侧的轮廓，留好的。
//! 报告里那个轮廓数只取决于正面和那一侧两份拆层，是拟合最开始那一步算的；
//! Space 的 `hair_fit` 只算这一步，十几秒。
//!
//! 现在第一次拟合开始的同时，先向 Space 问四个方向的这个数；看出要重画的那
//! 一侧，就在第一次拟合还在跑的时候把它重画、重拆，再问一次新的那份。没变好
//! 就用第一次拟合的结果，省掉整次第二次拟合；变好了才等第一次拟合让出位置，
//! 用换过的那一侧再拟合一次，之后照原来的办法比较并留下好的。
//!
//! 提前这一步什么都没做成时（Space 没有 `hair_fit`、问不到、没有哪一侧要
//! 重画、这套图已经重画过）就照原来的流程走（`redraw_misfit`），判定仍然读
//! 第一次拟合自己的报告。

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
    /// Whether this set of drawings had its turn `index` drawn again for its hair before.
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
    /// What was done before there was a `hair_fit`, from the first fit's own
    /// report on: nothing where no turn's hair is a misfit or the set was
    /// retried before, else that turn drawn, decomposed and fitted again.
    fn as_before(
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
    /// The outline distance of the drawing it would replace.
    before: f64,
    /// Its own, when the Space measured it (then less than `before`); none
    /// when it could not be asked: only fitting with it will tell.
    after: Option<f64>,
}

/// The four turns fitted onto the front, with the turn whose front hair the
/// fit cannot follow drawn once more and kept when it is followed better
/// (`redraw_misfit`), that redraw begun beside the first fit instead of after
/// it. The sets and the keys are what `fit` then `redraw_misfit` gave.
pub(super) async fn fit_turns(
    steps: &impl Steps,
    (mut decompositions, mut pictures): Sets,
    redrawn: &mut Vec<String>,
) -> ApiResult<(Sets, TurnKeyformsOutput)> {
    // Both to their end, whatever the other comes to: the fit holds its place
    // on the Space until it is over, and a redraw already paid for is measured.
    let (first, again) = tokio::join!(
        steps.fit(&decompositions, &pictures),
        drawn_again(steps, &decompositions)
    );
    let first = match first {
        Ok(first) => first,
        Err(error) => {
            // No fit to go on with. A redraw the Space measured as followed
            // better is the set's from now on, so a rerun fits with it; the
            // marker already keeps that rerun from drawing another.
            if let Ok(Some(again)) = &again
                && again.after.is_some()
            {
                steps.keep(again.index, &again.png, &again.psd).await;
            }
            return Err(error);
        }
    };
    let Some(again) = again? else {
        return steps
            .as_before((decompositions, pictures), first, redrawn)
            .await;
    };
    let Again {
        index,
        png,
        psd,
        before,
        ..
    } = again;
    let side = TURN_SIDES[index];
    let replaced = (
        std::mem::replace(&mut decompositions[index + 1], psd),
        std::mem::replace(&mut pictures[index + 1], (png, "image/png".to_string())),
    );
    steps.fitting_again();
    let second = steps.fit(&decompositions, &pictures).await?;
    // As ever, the fit's own report has the last word.
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

/// What can be told and done of the misfit turn before the first fit is
/// over. None: nothing was (no `hair_fit` on the Space, a measure that could
/// not be had, no turn far worse than its opposite, a set retried before),
/// or the turn drawn again was no better or could not be drawn; the first
/// fit then stands, as `as_before` will find.
async fn drawn_again(
    steps: &impl Steps,
    decompositions: &[Vec<u8>; 5],
) -> ApiResult<Option<Again>> {
    let front = &decompositions[0];
    let asked: Vec<_> = decompositions[1..]
        .iter()
        .map(|turned| steps.hair_fit(front.clone(), turned.clone()))
        .collect();
    let mut outlines = [None; 4];
    for (outline, measured) in outlines.iter_mut().zip(join_all(asked).await) {
        match measured {
            Ok(HairFit::Outline(measured)) => *outline = measured,
            Ok(HairFit::Unsupported) => return Ok(None),
            Err(error) => {
                tracing::warn!(%error, "front hair could not be measured ahead of the fit; told from its report");
                return Ok(None);
            }
        }
    }
    let Some(index) = hair_misfit::misfit_of(outlines) else {
        return Ok(None);
    };
    if steps.retried(index).await {
        return Ok(None);
    }
    steps.mark_retried(index).await;
    let side = TURN_SIDES[index];
    let before = outlines[index].unwrap_or(f64::INFINITY);
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
    let after = match steps.hair_fit(front.clone(), psd.clone()).await {
        // No front hair fitted on it reads as the report's missing figure does: no better.
        Ok(HairFit::Outline(after)) => Some(after.unwrap_or(f64::INFINITY)),
        Ok(HairFit::Unsupported) => None,
        Err(error) => {
            tracing::warn!(side, %error, "the turn drawn again could not be measured; fitting with it to tell");
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

    async fn as_before(
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
    use std::{sync::Mutex, time::Duration};

    use serde_json::json;

    use super::super::served::missing_job;
    use super::*;

    /// The steps as a job of the site took them (2026-11-24), in milliseconds
    /// for its seconds, and what each is to answer.
    struct Staged {
        started: tokio::time::Instant,
        /// What `hair_fit` and a fit's report say of each of the four turns.
        outlines: [Option<f64>; 4],
        /// And of the turn drawn again.
        again: Option<f64>,
        /// What `hair_fit` cannot do: not on the Space, fails for the four, fails for the redraw.
        unsupported: bool,
        unmeasured: bool,
        again_unmeasured: bool,
        draw_fails: bool,
        first_fit_fails: bool,
        retried_before: bool,
        /// What was done, and when it ended.
        done: Mutex<Vec<(u64, String)>>,
    }

    const FIT: u64 = 200;
    const HAIR_FIT: u64 = 20;
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
                again: Some(again),
                unsupported: false,
                unmeasured: false,
                again_unmeasured: false,
                draw_fails: false,
                first_fit_fails: false,
                retried_before: false,
                done: Mutex::default(),
            }
        }

        async fn took(&self, milliseconds: u64, what: impl Into<String>) {
            tokio::time::sleep(Duration::from_millis(milliseconds)).await;
            let at = self.started.elapsed().as_millis() as u64;
            self.done.lock().unwrap().push((at, what.into()));
        }

        fn did(&self) -> Vec<(u64, String)> {
            self.done.lock().unwrap().clone()
        }

        fn names(&self) -> Vec<String> {
            self.did().into_iter().map(|(_, what)| what).collect()
        }

        /// A decomposition's outline: the turn it is of (its one byte is its place), or the redraw's.
        fn outline_of(&self, psd: &[u8]) -> Option<f64> {
            match psd[0] {
                AGAIN_PSD => self.again,
                place => self.outlines[usize::from(place) - 1],
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
            self.took(FIT, format!("fit {turns:?}")).await;
            if self.first_fit_fails && !turns.contains(&AGAIN_PSD) {
                return Err(missing_job());
            }
            let mut hair = serde_json::Map::new();
            for (side, psd) in TURN_SIDES.iter().zip(&decompositions[1..]) {
                if let Some(outline) = self.outline_of(psd) {
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
            self.took(HAIR_FIT, format!("hair {}", turned[0])).await;
            if self.unsupported {
                return Ok(HairFit::Unsupported);
            }
            let again = turned[0] == AGAIN_PSD;
            if (again && self.again_unmeasured) || (!again && self.unmeasured) {
                return Err(SeeThroughError::Timeout);
            }
            Ok(HairFit::Outline(self.outline_of(&turned)))
        }

        async fn retried(&self, _: usize) -> bool {
            self.retried_before
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
            let at = self.started.elapsed().as_millis() as u64;
            self.done.lock().unwrap().push((at, "fitting again".into()));
        }

        /// As when the first fit's report finds nothing to draw again.
        async fn as_before(
            &self,
            sets: Sets,
            first: TurnKeyformsOutput,
            _: &mut Vec<String>,
        ) -> ApiResult<(Sets, TurnKeyformsOutput)> {
            self.took(0, "as before").await;
            Ok((sets, first))
        }
    }

    fn sets() -> Sets {
        (
            [0, 1, 2, 3, 4].map(|place| vec![place]),
            [10, 11, 12, 13, 14].map(|place| (vec![place], "image/png".to_string())),
        )
    }

    /// The turns the keys were fitted with, and how long it all took.
    async fn fitted(steps: &Staged, redrawn: &mut Vec<String>) -> (Sets, Vec<u64>, u64) {
        let (sets, output) = fit_turns(steps, sets(), redrawn).await.unwrap();
        let keys: serde_json::Value = serde_json::from_slice(&output.keyforms).unwrap();
        let turns = keys["turns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|turn| turn.as_u64().unwrap())
            .collect();
        (sets, turns, steps.started.elapsed().as_millis() as u64)
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
        // The four were asked beside the fit; then the fit's own report is read, as before.
        assert_eq!(
            steps.did(),
            [
                (20, "hair 1".to_string()),
                (20, "hair 2".into()),
                (20, "hair 3".into()),
                (20, "hair 4".into()),
                (200, "fit [1, 2, 3, 4]".into()),
                (200, "as before".into()),
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
        // and over 285 after the start, where one after another took 665.
        assert_eq!(
            steps.did()[4..],
            [
                (20, "marked 1".to_string()),
                (50, "drawn 1".into()),
                (200, "fit [1, 2, 3, 4]".into()),
                (265, "decomposed".into()),
                (285, "hair 9".into()),
                (285, "as before".into()),
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
        // Where one after another took 665.
        assert_eq!(took, 485);
        assert!(!steps.names().contains(&"as before".to_string()));
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_measure_ahead_of_the_fit_all_is_as_before() {
        // No `hair_fit` on the Space, one that fails, and a set whose turn was drawn again before.
        let unsupported = Staged {
            unsupported: true,
            ..Staged::of(MISFIT, 5.0)
        };
        let unmeasured = Staged {
            unmeasured: true,
            ..Staged::of(MISFIT, 5.0)
        };
        let retried = Staged {
            retried_before: true,
            ..Staged::of(MISFIT, 5.0)
        };
        for steps in [unsupported, unmeasured, retried] {
            let (_, turns, took) = fitted(&steps, &mut Vec::new()).await;
            assert_eq!(turns, [1, 2, 3, 4]);
            // Nothing drawn or marked: the first fit, then what was done before.
            assert_eq!(
                steps.names()[4..],
                ["fit [1, 2, 3, 4]".to_string(), "as before".into()]
            );
            assert_eq!(took, steps.did()[4].0);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_redraw_that_cannot_be_drawn_or_measured_ends_as_it_did_before() {
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
        let disagreeing = Staged {
            again_unmeasured: true,
            ..Staged::of(MISFIT, 26.0)
        };
        let mut redrawn = Vec::new();
        let (kept, turns, _) = fitted(&disagreeing, &mut redrawn).await;
        assert_eq!(turns, [1, 2, 3, 4]);
        assert_eq!(kept.0, sets().0);
        assert_eq!(kept.1, sets().1);
        assert!(redrawn.is_empty());
        assert!(!disagreeing.names().contains(&"kept 1".to_string()));
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_first_fit_fails_the_job_once_the_redraw_is_measured_and_kept() {
        let steps = Staged {
            first_fit_fails: true,
            ..Staged::of(MISFIT, 5.0)
        };
        assert!(fit_turns(&steps, sets(), &mut Vec::new()).await.is_err());
        // The redraw went to its end and, measured better, is the set's for the rerun.
        assert_eq!(
            steps.did()[6..],
            [
                (200, "fit [1, 2, 3, 4]".to_string()),
                (265, "decomposed".into()),
                (285, "hair 9".into()),
                (285, "kept 1".into()),
            ]
        );
        // One that was no better is not kept.
        let steps = Staged {
            first_fit_fails: true,
            ..Staged::of(MISFIT, 26.0)
        };
        assert!(fit_turns(&steps, sets(), &mut Vec::new()).await.is_err());
        assert!(!steps.names().contains(&"kept 1".to_string()));
    }
}
