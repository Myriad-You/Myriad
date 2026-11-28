//! 转头关键形：照二维绑定里在转头两端给每个部件定形状的做法，
//! 以立绘为参考生成头向画面右、左转和抬头、低头约 30° 的四张图（身体不动、
//! 与立绘逐像素对齐），连同立绘一起拆层，在 See-through Space 上把立绘的每个
//! 部件拟合到四张图的同一部件上，得到关键形；转头露出的后发、耳朵、脖子从
//! 四张图里烘焙进立绘的拆层。
//!
//! 整个过程要几十分钟（四次生图、五次拆层、一次拟合），在后台跑；前端轮询
//! 进度，完成后取回烘焙过的拆层与关键形，照常导入。页面离开不影响任务：还在跑的、
//! 做完没被取走的、失败还没报过的，页面回来时接上（`pickup`）。每个任务是数据库里的
//! 一条记录（`services::rig_jobs`）：进度、每一步的耗时与重试、出错、结果放在哪、花费记在
//! 账上的哪个 id；页面读的都是记录（`live`）。同时跑的任务数有上限，默认一个；
//! 同一个调用方带同一个 `Idempotency-Key` 提交，拿到的是同一个任务；任务可以取消。
//! 跑任务的进程不在了，它的任务记成中断（租约过期），不自动重跑。
//! 任务里互不依赖的生图与拆层一起发出去（`together`），拟合仍是一次一个。
//! 转头图一画好就开始拆层，和正面拆层同时进行；正面拆层和四张图到齐后预检
//! 一次，画过头的那张重画一次再拆。
//! 拟合跟不上前发的那张转头图，在第一次拟合还在跑的时候就重画（`fit_turns`）。
//!
//! 四张图与五份拆层按立绘内容存在磁盘上（`TurnCache`）：任务中途失败、后端
//! 重启，或同一张立绘再导入一次，已经花钱做完的生图与拆层直接取用，只把缺的
//! 补上；拟合（Space 上只用 CPU）每次重做，拟合改进后再导入一次就能用上。
//!
//! 站立全身的增强另外画整个人左右转身的两张图、拆两次层，在 Space 上拟合出
//! 身体各部件的转身关键形（`body_turn`），和头部关键形放在同一份 JSON 里。
//! 全身的腰、腿两块和身体的拆层排在头部的五份拆层之后发出（`head_first`）。
//!
//! 任务从哪张图做、做成什么、归谁，都存在任务记录上（`input`）；步骤只从这些跑，做出来的
//! 东西放进任务主人的存储（站点的放在站点一直放的地方）。站点设定的页面是调用方之一：它把
//! 当前主立绘当成任务的图，任务结束时核对主立绘还是不是那一张。
//!
//! 任务的每一步结束时记一行耗时（`took`）：任务、步骤名、毫秒，不记内容。
//!
//! 每个完成的任务整套存档（`archive`）：立绘、四张图、五份拆层、关键形与烘焙后的
//! 拆层，每张立绘留最近两份。存档可以打包下载，也可以不再生图、拆层，只用 Space
//! 现在的拟合重做一次，照常导入。下载的包可以传回来（`upload`），同一张立绘的
//! 就存成它的存档。

use std::{
    io::Cursor,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    ApiError, ApiResult, bad_request,
    decompose::see_through_error,
    internal_error,
    master::{MasterSlot, require_master_match, valid_generation_fingerprint},
    not_found, portrait_generation_config_error, portrait_generation_provider_error,
    require_merope_enabled, require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{image_generation, rig_jobs, see_through},
};

mod archive;
mod body_turn;
mod cache;
mod claim;
mod ending;
mod drawing;
mod figure;
mod figure_picture;
mod fit_turns;
mod hair_misfit;
mod head_first;
mod input;
mod judge;
mod launch;
mod live;
mod pickup;
mod served;
mod together;
mod upload;
use archive::{ArchiveFiles, ArchiveMeta, FigureFiles, FigureMeta};
use cache::{CACHE_KEEP, TurnCache, cache_root, prune_cache};
#[cfg(test)]
use drawing::fit_to;
use drawing::{
    TRANSIENT_RETRIES, TURN_SIDES, TURNED_REDOS, decompose_checked, decompose_waiting,
    decomposition, draw_turn, retried, retried_when, transient, turn_prompts,
};
#[cfg(test)]
use figure::alongside;
pub(super) use figure::{enlarged_figure, super_resolved};
use figure::{fit, plain, put_back, run_figure};
pub use figure_picture::get_full_body_figure_picture;
use head_first::{HeadSplit, TileSteps, Way, head_first, in_tiles};
#[cfg(test)]
use claim::{claim_job, the_job};
use claim::{Claim, Submission, claim, claim_for, idempotency_key};
use ending::{Holds, cancelled, end, finish};
use input::{JobInput, Make, PictureRef, Providers, load_picture};
#[cfg(test)]
use launch::start_input;
use launch::{Lineage, start_claimed};
use pickup::{Asked, Beside};
use served::{missing_job, space_client};
pub use served::{
    cancel_turn_keyforms, delete_turn_archive, download_turn_archive, get_turn_keyforms,
    get_turn_keyforms_keys, get_turn_keyforms_psd, get_turn_keyforms_record, list_turn_archives,
    load_turn_archive, refit_turn_archive, running_turn_keyforms,
};
use together::{all_or_first_error, kept_or_made, turns_beside_front};
pub use upload::{
    UPLOAD_PIECE_MAX, finish_full_body_turn_archive_upload, finish_turn_archive_upload,
    put_turn_archive_piece,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// A standing figure's picture enlarged before it is decomposed in tiles.
    Upscaling,
    Generating,
    Decomposing,
    Fitting,
    /// A standing figure's tiles stitched into one decomposition.
    Stitching,
    Done,
    Failed,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Upscaling => "upscaling",
            Self::Generating => "generating",
            Self::Decomposing => "decomposing",
            Self::Fitting => "fitting",
            Self::Stitching => "stitching",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// Which part of a job's work its stage belongs to, so a page can tell where
/// the whole stands (a figure's decomposing and its head's are both
/// `decomposing`) and how long is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The worn bust's turn keys.
    Bust,
    /// A standing figure: enlarging, decomposing whole and in tiles, stitching.
    Figure,
    /// A standing figure's head, keyed as a bust.
    Head,
    /// A standing figure's head done, its body's keys still being made.
    Body,
    /// A kept job fitted again.
    Refit,
    /// A kept job handed over as it was.
    Load,
    /// The worn bust split once, no keys: the base its redrawn expressions go on.
    Plain,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bust => "bust",
            Self::Figure => "figure",
            Self::Head => "head",
            Self::Body => "body",
            Self::Refit => "refit",
            Self::Load => "load",
            Self::Plain => "plain",
        }
    }
}

/// Where a standing figure's body keys stand, made alongside its head's.
#[derive(Clone, Copy, Debug)]
struct BodyProgress {
    /// "turn" or "weight".
    motion: &'static str,
    stage: Stage,
    done: u8,
    total: u8,
}

struct Job {
    id: String,
    stage: Stage,
    /// The stage a page is shown while the job's record has not ended: the
    /// last one before done or failed, which only the record says.
    shown: Stage,
    phase: Phase,
    /// A standing figure's body keys, made alongside: none until they start.
    body: Option<BodyProgress>,
    done: u8,
    total: u8,
    error: Option<Value>,
    psd: Option<Arc<Vec<u8>>>,
    keyforms: Option<Arc<Vec<u8>>>,
    /// The archive the finished job was kept as.
    archive_id: Option<String>,
    started: Instant,
    /// The steps begun, in order (`timed`, `took`).
    steps: Vec<live::Step>,
    /// Where the steps run.
    targets: live::Targets,
    /// Bumped by every change, so that only what changed is written.
    version: u64,
    cancel: Arc<tokio::sync::Notify>,
    /// Its work is over and its end is being written.
    ending: bool,
    /// Whose it is: its paid calls' ledger subject, where what it makes is kept.
    owner: rig_jobs::Owner,
    /// What its result must still be true of at its end.
    holds: Holds,
}

impl Job {
    fn new(
        id: String,
        phase: Phase,
        stage: Stage,
        total: u8,
        targets: live::Targets,
        owner: rig_jobs::Owner,
        holds: Holds,
    ) -> Self {
        Self {
            id,
            stage,
            shown: if matches!(stage, Stage::Done | Stage::Failed) {
                Stage::Fitting
            } else {
                stage
            },
            phase,
            body: None,
            done: 0,
            total,
            error: None,
            psd: None,
            keyforms: None,
            archive_id: None,
            started: Instant::now(),
            steps: Vec::new(),
            targets,
            version: 0,
            cancel: live::cancel_notifier(),
            ending: false,
            owner,
            holds,
        }
    }

    fn ended(&self) -> bool {
        matches!(self.stage, Stage::Done | Stage::Failed)
    }
}

fn update(id: &str, change: impl FnOnce(&mut Job)) {
    update_quietly(id, |job| {
        change(job);
        if !job.ended() {
            job.shown = job.stage;
        }
        job.version += 1;
    });
}

/// `update`, as nothing to write again.
fn update_quietly(id: &str, change: impl FnOnce(&mut Job)) {
    let mut jobs = live::JOBS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(job) = jobs.get_mut(id) {
        change(job);
    }
}

/// One line as a step of a job ends: the job, the step and how long it took.
/// Nothing of what the step was given or made. The step is kept with its job.
fn took(id: &str, step: &'static str, began: Instant) {
    let ms = began.elapsed().as_millis() as u64;
    tracing::info!(job = %id, step, ms, "turn keyforms step ended");
    update(id, |job| {
        let seq = job.step_began(step, began);
        job.step_ended(seq, true);
    });
}

/// Whether a step's output is a failure.
trait StepOutcome {
    fn ok(&self) -> bool;
}

impl<T, E> StepOutcome for Result<T, E> {
    fn ok(&self) -> bool {
        self.is_ok()
    }
}

impl StepOutcome for image_generation::ImageReference {
    fn ok(&self) -> bool {
        true
    }
}

/// `work` run to its end as a step of the job, with its line however it
/// ended; the calls it tries again are counted on it (`live::note_retry`).
async fn timed<T: StepOutcome>(id: &str, step: &'static str, work: impl Future<Output = T>) -> T {
    let began = Instant::now();
    let mut seq = None;
    update(id, |job| seq = Some(job.step_began(step, began)));
    let out = match seq {
        Some(seq) => live::as_step(id, seq, work).await,
        None => work.await,
    };
    let ms = began.elapsed().as_millis() as u64;
    tracing::info!(job = %id, step, ms, "turn keyforms step ended");
    if let Some(seq) = seq {
        let ok = out.ok();
        update(id, |job| job.step_ended(seq, ok));
    }
    out
}

fn slot_of(outfit_id: Option<&str>) -> MasterSlot<'_> {
    match outfit_id {
        Some(outfit) => MasterSlot::FullBody(outfit),
        None => MasterSlot::WornBust,
    }
}

/// Everything a job made, to keep and to hand to the importer.
struct Made {
    files: ArchiveFiles,
    master_media_type: String,
    redrawn: Vec<String>,
    /// A standing figure keyed by its head.
    figure: Option<FigureMeta>,
    /// Kept in the archive: a job with turn keys. A plain figure's
    /// decomposition is handed out only.
    keep: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnKeyformsRequest {
    source_master_asset_id: String,
    #[serde(default)]
    source_generation_fingerprint: Option<String>,
    /// A figure's head keyed for its turn; a bust's always is.
    #[serde(default = "keyed")]
    head: bool,
    /// A figure's body keyed for its turn and weight shift.
    #[serde(default = "keyed")]
    body: bool,
    /// The bust split once without keys, as a job a reload picks up.
    #[serde(default)]
    plain: bool,
}

fn keyed() -> bool {
    true
}

/// What a figure's job keys: its head's turn, its body's turn and weight
/// shift, each made only when chosen. Neither is not a job: a figure
/// without keys is an uploaded PSD.
#[derive(Clone, Copy, Debug)]
struct FigureKeys {
    head: bool,
    body: bool,
}

pub async fn start_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(beside): Query<Beside>,
    headers: HeaderMap,
    Json(payload): Json<TurnKeyformsRequest>,
) -> ApiResult<Json<Value>> {
    if !payload.head && !payload.plain {
        return Err(bad_request("A bust's enhancement keys its head"));
    }
    start(db, &claims, None, payload, beside, &headers).await
}

pub async fn start_full_body_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    Query(beside): Query<Beside>,
    headers: HeaderMap,
    Json(payload): Json<TurnKeyformsRequest>,
) -> ApiResult<Json<Value>> {
    if !payload.head && !payload.body {
        return Err(bad_request("Choose the head, the body or both to key"));
    }
    if payload.plain {
        return Err(bad_request("A set is split plain by keying neither part"));
    }
    start(db, &claims, Some(outfit_id), payload, beside, &headers).await
}

async fn start(
    db: DatabaseConnection,
    claims: &Claims,
    outfit_id: Option<String>,
    payload: TurnKeyformsRequest,
    beside: Beside,
    headers: &HeaderMap,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    let user_id = require_owner(claims, &db).await?;
    start_as(db, user_id, outfit_id, payload, beside, headers).await
}

/// A start asked by the owner `user_id`: its key, its master, and the job
/// claimed and set running from the master's picture.
async fn start_as(
    db: DatabaseConnection,
    user_id: i32,
    outfit_id: Option<String>,
    payload: TurnKeyformsRequest,
    beside: Beside,
    headers: &HeaderMap,
) -> ApiResult<Json<Value>> {
    let idempotency_key = idempotency_key(headers)?;
    let fingerprint = payload
        .source_generation_fingerprint
        .as_deref()
        .map(str::to_ascii_lowercase);
    if fingerprint
        .as_deref()
        .is_some_and(|value| !valid_generation_fingerprint(value))
    {
        return Err(bad_request(
            "Source generation fingerprint must be a SHA-256 hex digest",
        ));
    }
    let plain = outfit_id.is_none() && payload.plain;
    let phase = if outfit_id.is_some() {
        Phase::Figure
    } else if plain {
        Phase::Plain
    } else {
        Phase::Bust
    };
    let submission = Submission {
        user_id,
        idempotency_key,
        outfit_id: outfit_id.clone(),
        source_master_asset_id: payload.source_master_asset_id.clone(),
        source_generation_fingerprint: fingerprint.clone(),
        asked: Asked::started(outfit_id.is_some(), &payload, beside),
    };
    if let Some(id) = submission.given_before(&db, phase).await? {
        return Ok(Json(json!({ "jobId": id })));
    }
    let master = require_master_match(
        &db,
        slot_of(outfit_id.as_deref()),
        &payload.source_master_asset_id,
        fingerprint.as_deref(),
    )
    .await?;
    let dynamic = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    let generation = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    let picture = PictureRef::SiteMedia {
        url: master.asset_id,
    };
    load_picture(&rig_jobs::Storage::site(), &picture).await?;
    let (client, space_name) = space_client().await?;
    let providers = Providers {
        generation,
        client,
        space_name,
    };
    let lineage = Lineage {
        outfit_id,
        fingerprint,
    };
    let (id, _) = start_claimed(db, submission.claim(phase), providers, lineage).await?;
    Ok(Json(json!({ "jobId": id })))
}

/// The bust split once, no keys, nothing kept: the front decomposition a
/// keyed job of the same portrait would make (and reuse) first.
async fn run_plain(
    id: &str,
    reference: image_generation::ImageReference,
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
) -> ApiResult<Made> {
    cache.touch().await;
    let options = see_through::DecomposeOptions::default();
    let psd = decomposition(
        client,
        cache,
        0,
        &reference.bytes,
        &reference.media_type,
        options,
    )
    .await?;
    update(id, |job| job.done = 1);
    Ok(plain(psd, reference.media_type))
}

/// Four turned drawings, five decompositions, one fit. `split` is told when
/// the five decompositions are in hand, for what gives way to them
/// (`head_first`); it is this work's own, so that however the work ends
/// without them (failed, returned early, dropped), `split` goes with it and
/// tells that too.
async fn run(
    id: &str,
    generation: &image_generation::ImageGenerationConfig,
    reference: image_generation::ImageReference,
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    split: HeadSplit,
) -> ApiResult<Made> {
    let (width, height) = image::ImageReader::new(Cursor::new(reference.bytes.as_ref()))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .ok_or_else(|| bad_request("The current master portrait is not available"))?;
    cache.touch().await;
    update(id, |job| {
        job.stage = Stage::Generating;
        job.done = 0;
        job.total = 4;
    });
    let options = see_through::DecomposeOptions::default();
    let front = (reference.bytes.clone(), reference.media_type.clone());
    // The four drawings and the front decomposition need only the master: made together.
    let prompts = turn_prompts();
    let reference = &reference;
    let drawings: Vec<_> = prompts
        .iter()
        .enumerate()
        .map(|(index, prompt)| async move {
            let name = format!("turned-{index}.png");
            let draw = draw_turn(generation, prompt, reference, width, height);
            kept_or_made(cache, &name, draw).await
        })
        .collect();
    let turns = fit_turns::Turns {
        id,
        generation,
        reference,
        size: (width, height),
        client,
        cache,
        options,
    };
    // Each turned drawing is decomposed as it comes, beside the front one; a turn drawn
    // past what the front drawing's keys can follow is drawn once more.
    let began = Instant::now();
    let front_psd = decomposition(client, cache, 0, &front.0, &front.1, options);
    let beside = turns_beside_front(id, cache, &turns, drawings, front_psd).await;
    took(id, "head-decompositions", began);
    split.settle(beside.is_ok());
    let (front_psd, turned) = beside?;
    let (turned, turned_psds): (Vec<_>, Vec<_>) = turned.into_iter().unzip();
    let decompositions: Vec<_> = std::iter::once(front_psd).chain(turned_psds).collect();
    // The fit also matches its keys against the drawings themselves.
    let pictures: [(Vec<u8>, String); 5] = std::iter::once((front.0.to_vec(), front.1))
        .chain(turned.into_iter().map(|png| (png, "image/png".to_string())))
        .collect::<Vec<_>>()
        .try_into()
        .expect("five pictures");

    update(id, |job| {
        job.stage = Stage::Fitting;
        job.done = 0;
        job.total = 1;
    });
    let mut redrawn = Vec::new();
    for (index, side) in TURN_SIDES.iter().enumerate() {
        if cache
            .read(&format!("turned-{index}.redrawn"))
            .await
            .is_some()
        {
            redrawn.push((*side).to_string());
        }
    }
    let mut decompositions: [Vec<u8>; 5] = decompositions.try_into().expect("five decompositions");
    let mut pictures = pictures;
    // A turn that redrew what a turn keeps is drawn once more before the fit.
    let judging = judge::judge_turns(
        id,
        generation,
        reference,
        (width, height),
        client,
        cache,
        options,
        &mut decompositions,
        &mut pictures,
        &mut redrawn,
    );
    let judged = timed(id, "head-judging", judging).await?;
    update(id, |job| job.stage = Stage::Fitting);
    // The fit, with the turn whose front hair it cannot follow drawn again beside it.
    let fitting = fit_turns::fit_turns(&turns, (decompositions, pictures), &mut redrawn);
    let ((decompositions, pictures), output) = timed(id, "head-fit", fitting).await?;
    let output = see_through::TurnKeyformsOutput {
        keyforms: judge::with_judgements(output.keyforms, &judged),
        ..output
    };
    let [front, turned @ ..] = pictures;
    let master_media_type = front.1.clone();
    Ok(Made {
        files: ArchiveFiles {
            master: front.0,
            turned: turned.map(|(png, _)| png),
            decompositions,
            keyforms: output.keyforms,
            baked: output.psd,
            figure: None,
        },
        master_media_type,
        redrawn,
        figure: None,
        keep: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bust_can_ask_for_a_plain_split() {
        let asked: TurnKeyformsRequest = serde_json::from_value(json!({
            "sourceMasterAssetId": "/media/assets/m/portrait.png",
            "plain": true
        }))
        .expect("plain is a known field");
        assert!(asked.plain);
        let keyed: TurnKeyformsRequest =
            serde_json::from_value(json!({ "sourceMasterAssetId": "m" })).unwrap();
        assert!(!keyed.plain && keyed.head);
        assert_eq!(Phase::Plain.as_str(), "plain");
    }

    fn made() -> Made {
        plain(Vec::new(), "image/png".to_string())
    }

    #[tokio::test]
    async fn the_body_turn_runs_alongside_the_head_and_is_waited_for() {
        let _slot = together::JOB_TESTS.lock().await;
        let (id, _) = claim_job(&Some("alongside-a".into()), "m", Stage::Decomposing, 3).unwrap();
        // The head is done first: the job then waits on the body as fitting.
        let body = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            7
        };
        let (head, body) = alongside(&id, async { Ok(made()) }, body).await;
        assert!(head.is_ok());
        assert_eq!(body, Some(7));
        assert_eq!(the_job(|job| job.stage), Stage::Fitting);
        update(&id, |job| job.stage = Stage::Done);
        // The body is done first: kept until the head is.
        let head = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            Ok(made())
        };
        let (head, body) = alongside(&id, head, async { 8 }).await;
        assert!(head.is_ok());
        assert_eq!(body, Some(8));
        // A failed head does not wait for the body.
        let failed = async { Err::<Made, ApiError>(missing_job()) };
        let (head, body) = alongside(&id, failed, std::future::pending::<u8>()).await;
        assert!(head.is_err());
        assert_eq!(body, None);
        update(&id, |job| job.stage = Stage::Done);
    }

    #[test]
    fn prompts_turn_right_left_then_raise_and_lower() {
        let prompts = turn_prompts();
        assert!(prompts[0].contains("viewer's right"));
        assert!(prompts[1].contains("viewer's left"));
        assert!(prompts[2].contains("upward"));
        assert!(prompts[3].contains("downward"));
        for prompt in &prompts {
            assert!(prompt.contains("pixel-aligned"));
            assert!(prompt.len() < 2000);
        }
    }

    #[test]
    fn only_passing_failures_are_tried_again() {
        use see_through::SeeThroughError::*;
        assert!(transient(&Timeout));
        assert!(transient(&Transport("reset".into())));
        assert!(transient(&Waking));
        assert!(transient(&Upstream {
            stage: "upload",
            status: 503
        }));
        assert!(!transient(&Upstream {
            stage: "upload",
            status: 404
        }));
        assert!(!transient(&Quota));
        assert!(!transient(&Rejected(None)));
        assert!(!transient(&Authentication));
        assert!(!transient(&SpaceUnavailable("PAUSED".into())));
    }

    #[test]
    fn a_turned_drawing_comes_back_at_the_master_size() {
        let image = image::RgbaImage::new(64, 80);
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let fitted = fit_to(&png, 60, 75).unwrap();
        let back = image::load_from_memory(&fitted).unwrap();
        assert_eq!((back.width(), back.height()), (60, 75));
    }
}
