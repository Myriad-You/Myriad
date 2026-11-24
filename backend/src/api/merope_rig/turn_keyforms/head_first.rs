//! 头部先拆：全身增强里，腰、腿两块和身体转身的拆层给头部的拆层让路。
//!
//! Space 上同一时刻真正在跑的调用有限，多出来的在 Space 里排队，先到先得。头部
//! 自己的五份拆层（上块正面和四张转头图）后面还接着几分钟的拟合，是整个任务里
//! 最长的一条路；腰、腿两块只有最后的拼接要用，身体的两份拆层后面只有一次很短的
//! 拟合，赶在头部拟合结束前后做完就不耽误什么。所以它们等头部的拆层全部到手再
//! 发出去；身体的两张图不占 GPU，照常马上画。头部不做关键形时没有要让的对象，
//! 三块和身体的拆层一起发。
//!
//! 「到手」由头部流程（`run`）手里的 `HeadSplit` 说：`turns_beside_front` 一返回就
//! 说，成了说到手，没成就丢掉它。`HeadSplit` 归 `run` 这个 future 所有，`run` 不管
//! 怎么结束（成功、失败、提前返回、整个被丢弃）它都跟着被丢掉，等的一方（`Way`）
//! 那时一定被叫醒，并且知道拆层不会来了：还没开始的拆层就不开始，不为注定失败的
//! 任务花钱。已经开始的照 `together` 的约定跑完进缓存。
//!
//! 代价：到手之后头部又重画的转头图（评判、拟合跟不上前发），它的拆层和这四份
//! 一起排队；头部拆层失败的任务，腰、腿两块留给下一次任务拆。

use futures::future::{FutureExt, Shared};
use tokio::sync::oneshot;

use super::{ApiResult, Made, Phase, Stage, all_or_first_error, internal_error, timed, update};

/// Held by the head's work (`run`), which says through it that its
/// decompositions are all in hand. Dropped without that word (they failed,
/// the work returned early or was dropped itself), it says they are not
/// coming: whoever waits on the `Way` is woken either way.
pub(super) struct HeadSplit(Option<oneshot::Sender<()>>);

impl HeadSplit {
    /// No one gives way: a bust's own job.
    pub(super) fn unwatched() -> Self {
        Self(None)
    }

    /// The head's decompositions are in hand, or (`in_hand` false) have failed.
    pub(super) fn settle(self, in_hand: bool) {
        if let (true, Some(told)) = (in_hand, self.0) {
            // Whoever waited may be gone with a failed job: no one to tell is no failure.
            let _ = told.send(());
        }
    }
}

/// What gives way to the head's decompositions waits on this.
pub(super) struct Way(Option<Shared<oneshot::Receiver<()>>>);

impl Way {
    /// Nothing to give way to: clear from the start.
    pub(super) fn open() -> Self {
        Self(None)
    }

    /// Waits until the head's decompositions are in hand. False when they are
    /// not coming: nothing more is begun, and paid for, for a job that has
    /// failed.
    pub(super) async fn clear(&self) -> bool {
        match &self.0 {
            None => true,
            Some(told) => told.clone().await.is_ok(),
        }
    }
}

/// The two ends of giving way to a head's decompositions: the head's work
/// takes the first, whatever gives way waits on the second.
pub(super) fn head_first() -> (HeadSplit, Way) {
    let (told, waited) = oneshot::channel();
    (HeadSplit(Some(told)), Way(Some(waited.shared())))
}

/// What a figure decomposed in tiles asks of the outside: the Space and the
/// image model. Apart, so the order of the steps can be tested without them.
pub(super) trait TileSteps: Sync {
    /// The head keyed as a bust on the upper tile (`run`), its front
    /// decomposition the tile's; `split` is `run`'s to settle.
    fn head(&self, split: HeadSplit) -> impl Future<Output = ApiResult<Made>> + Send;
    /// The upper tile decomposed and checked, for a head that is not keyed.
    fn upper(&self) -> impl Future<Output = ApiResult<Vec<u8>>> + Send;
    /// A tile without a head decomposed: the waist's (0) or the legs' (1).
    fn tile(&self, at: usize) -> impl Future<Output = ApiResult<Vec<u8>>> + Send;
    /// The tiles' decompositions (upper, waist, legs) stitched on the whole.
    fn stitch(&self, tiles: [Vec<u8>; 3]) -> impl Future<Output = ApiResult<Vec<u8>>> + Send;
    /// The stitched figure with the head's keys placed on it; plain when no
    /// head was keyed (`made` none).
    fn finish(
        &self,
        made: Option<Made>,
        stitched: Vec<u8>,
    ) -> impl Future<Output = ApiResult<Made>> + Send;
}

/// A figure's three tiles decomposed and stitched, its head keyed on the
/// upper one when `split` is there; `way` is then that split's other end, and
/// open otherwise.
///
/// A keyed head's work begins at once. The waist's tile and the legs' need
/// nothing of it or of each other and are only stitched in at the end: they
/// are decomposed together once the head's decompositions are in hand, beside
/// its fit, and not at all when those fail. With no head keyed there is
/// nothing to give way to: the three tiles are decomposed together.
///
/// The job counts a keyed head's stages as `run` reports them. The two tiles
/// behind it are not counted, so the head's count stays its own; waiting for
/// them is part of stitching. With no head keyed the three tiles are counted
/// as they come, after the whole.
///
/// What was begun is seen to its end, as `all_or_first_error` has it: tiles
/// under way when the head's fit fails are decomposed and kept for the next
/// job. The failure given is the head's before a tile's.
pub(super) async fn in_tiles(
    id: &str,
    steps: &impl TileSteps,
    split: Option<HeadSplit>,
    way: &Way,
) -> ApiResult<Made> {
    let keyed = split.is_some();
    let stitching = || {
        update(id, |job| {
            job.phase = Phase::Figure;
            job.stage = Stage::Stitching;
            job.done = 0;
            job.total = 1;
        })
    };
    let rest = async {
        if !way.clear().await {
            return Ok(None);
        }
        let tile = |at: usize| async move {
            let psd = steps.tile(at).await?;
            if !keyed {
                update(id, |job| job.done += 1);
            }
            ApiResult::Ok(psd)
        };
        let tiles = timed(id, "tiles", all_or_first_error([tile(0), tile(1)])).await?;
        ApiResult::Ok(Some(tiles))
    };
    let upper = async {
        match split {
            Some(split) => {
                update(id, |job| job.phase = Phase::Head);
                let made = steps.head(split).await?;
                stitching();
                ApiResult::Ok((made.files.decompositions[0].clone(), Some(made)))
            }
            None => {
                let psd = timed(id, "upper", steps.upper()).await?;
                update(id, |job| job.done += 1);
                Ok((psd, None))
            }
        }
    };
    let (upper, rest) = tokio::join!(upper, rest);
    let (upper, made) = upper?;
    let Some(rest) = rest? else {
        // A head keyed without a word of its decompositions: the tiles were
        // never begun, and there is nothing to stitch.
        return Err(internal_error(
            "a figure's head was keyed without its decompositions told in hand",
        ));
    };
    let [middle, lower]: [Vec<u8>; 2] = rest.try_into().expect("two tiles");
    stitching();
    let stitched = timed(id, "stitch", steps.stitch([upper, middle, lower])).await?;
    update(id, |job| job.done += 1);
    steps.finish(made, stitched).await
}

#[cfg(test)]
#[path = "head_first_tests.rs"]
mod tests;
