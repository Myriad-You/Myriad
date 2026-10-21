//! 转头关键形：照 Live2D 在角度 X / Y ±30 两端给每个部件定形状的做法，
//! 以立绘为参考生成头向画面右、左转和抬头、低头约 30° 的四张图（身体不动、
//! 与立绘逐像素对齐），连同立绘一起拆层，在 See-through Space 上把立绘的每个
//! 部件拟合到四张图的同一部件上，得到关键形；转头露出的后发、耳朵、脖子从
//! 四张图里烘焙进立绘的拆层。
//!
//! 整个过程要几十分钟（四次生图、五次拆层、一次拟合），在后台跑；前端轮询
//! 进度，完成后取回烘焙过的拆层与关键形，照常导入。同一时间只跑一个。
//!
//! 四张图与五份拆层按立绘内容存在磁盘上（`TurnCache`）：任务中途失败、后端
//! 重启，或同一张立绘再导入一次，已经花钱做完的生图与拆层直接取用，只把缺的
//! 补上；拟合（Space 上只用 CPU）每次重做，拟合改进后再导入一次就能用上。
//!
//! 站立全身的增强另外画整个人左右转身的两张图、拆两次层，在 Space 上拟合出
//! 身体各部件的转身关键形（`body_turn`），和头部关键形放在同一份 JSON 里。
//!
//! 每个完成的任务整套存档（`archive`）：立绘、四张图、五份拆层、关键形与烘焙后的
//! 拆层，每张立绘留最近两份。存档可以打包下载，也可以不再生图、拆层，只用 Space
//! 现在的拟合重做一次，照常导入。下载的包可以传回来（`upload`），同一张立绘的
//! 就存成它的存档。

use std::{
    io::Cursor,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    body::Body,
    extract::{Extension, Path, State},
    http::{HeaderValue, StatusCode, header},
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
    services::{image_generation, see_through},
};

mod archive;
mod body_turn;
mod cache;
mod drawing;
mod figure;
mod figure_picture;
mod hair_misfit;
mod judge;
mod served;
mod upload;
use archive::{ArchiveFiles, ArchiveMeta, FigureFiles, FigureMeta};
use cache::{CACHE_KEEP, TurnCache, cache_root, prune_cache};
#[cfg(test)]
use drawing::fit_to;
use drawing::{
    TRANSIENT_RETRIES, TURN_SIDES, TURNED_REDOS, decompose_checked, decompose_waiting,
    decomposition, draw_turn, redraw_bad_turns, redraw_misfit, retry_delay, transient,
    turn_prompts,
};
#[cfg(test)]
use figure::alongside;
use figure::{fit, plain, put_back, run_figure};
#[cfg(test)]
use served::missing_job;
use served::space_client;
pub use served::{
    delete_turn_archive, download_turn_archive, get_turn_keyforms, get_turn_keyforms_keys,
    get_turn_keyforms_psd, list_turn_archives, load_turn_archive, refit_turn_archive,
    running_turn_keyforms,
};
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
    /// None: the worn bust; Some: a full-body set.
    outfit_id: Option<String>,
    master_asset_id: String,
    stage: Stage,
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
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

fn update(id: &str, change: impl FnOnce(&mut Job)) {
    let mut job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(job) = job.as_mut().filter(|job| job.id == id) {
        change(job);
    }
}

fn slot_of(outfit_id: Option<&str>) -> MasterSlot<'_> {
    match outfit_id {
        Some(outfit) => MasterSlot::FullBody(outfit),
        None => MasterSlot::WornBust,
    }
}

/// The one job slot: a new job, or the one already running for the same
/// master (a reload, a second tab), or a conflict with another one.
fn claim_job(
    outfit_id: &Option<String>,
    master_asset_id: &str,
    stage: Stage,
    total: u8,
) -> Result<(String, bool), ApiError> {
    let mut job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(running) = job
        .as_ref()
        .filter(|job| !matches!(job.stage, Stage::Done | Stage::Failed))
    {
        if running.outfit_id == *outfit_id && running.master_asset_id == master_asset_id {
            return Ok((running.id.clone(), false));
        }
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Turn keys are already being made",
                "code": "turn_keyforms_in_progress"
            })),
        ));
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    *job = Some(Job {
        id: id.clone(),
        outfit_id: outfit_id.clone(),
        master_asset_id: master_asset_id.to_owned(),
        stage,
        phase: if outfit_id.is_some() {
            Phase::Figure
        } else {
            Phase::Bust
        },
        body: None,
        done: 0,
        total,
        error: None,
        psd: None,
        keyforms: None,
        archive_id: None,
        started: Instant::now(),
    });
    Ok((id, true))
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

/// Ends a job: kept in the archive and handed out when the master is still
/// the one it was made for, else failed.
async fn finish(
    db: &DatabaseConnection,
    task_id: &str,
    result: ApiResult<Made>,
    meta: ArchiveMeta,
) {
    let still_current = require_master_match(
        db,
        slot_of(meta.outfit_id.as_deref()),
        &meta.source_master_asset_id,
        meta.source_generation_fingerprint.as_deref(),
    )
    .await;
    match (result, still_current) {
        (Ok(made), Ok(_)) => {
            let meta = ArchiveMeta {
                master_media_type: made.master_media_type.clone(),
                redrawn: made.redrawn.clone(),
                figure: made.figure.clone(),
                body_motions: made
                    .figure
                    .is_some()
                    .then(|| body_turn::motions_of(&made.files.keyforms)),
                ..meta
            };
            let archive_id = match made.keep.then_some(()) {
                Some(()) => match archive::save(meta, &made.files).await {
                    Ok(id) => Some(id),
                    Err(error) => {
                        tracing::warn!(%error, job = %task_id, "turn keys could not be archived");
                        None
                    }
                },
                None => None,
            };
            let ArchiveFiles {
                keyforms, baked, ..
            } = made.files;
            update(task_id, |job| {
                job.stage = Stage::Done;
                job.psd = Some(Arc::new(baked));
                job.keyforms = Some(Arc::new(keyforms));
                job.archive_id = archive_id;
                tracing::info!(job = %job.id, seconds = job.started.elapsed().as_secs(), archive = ?job.archive_id, "turn keyforms done");
            });
        }
        (Err((_, Json(error))), _) | (_, Err((_, Json(error)))) => update(task_id, |job| {
            tracing::warn!(job = %job.id, ?error, "turn keyforms failed");
            job.stage = Stage::Failed;
            job.error = Some(error);
        }),
    }
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
    Json(payload): Json<TurnKeyformsRequest>,
) -> ApiResult<Json<Value>> {
    if !payload.head && !payload.plain {
        return Err(bad_request("A bust's enhancement keys its head"));
    }
    start(db, &claims, None, payload).await
}

pub async fn start_full_body_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    Json(payload): Json<TurnKeyformsRequest>,
) -> ApiResult<Json<Value>> {
    if !payload.head && !payload.body {
        return Err(bad_request("Choose the head, the body or both to key"));
    }
    if payload.plain {
        return Err(bad_request("A set is split plain by keying neither part"));
    }
    start(db, &claims, Some(outfit_id), payload).await
}

/// A full-body set's picture as its tiles were cut from (enlarged), for the
/// importer to check its decomposition against; 404 while there is none.
pub async fn get_full_body_figure_picture(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
) -> ApiResult<Response> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let master = super::master::current_master(&db, MasterSlot::FullBody(&outfit_id))
        .await?
        .ok_or_else(missing_picture)?;
    let reference = image_generation::load_local_reference(&master.asset_id)
        .await
        .map_err(|_| missing_picture())?;
    let picture = figure_picture::read(&reference.bytes)
        .await
        .ok_or_else(missing_picture)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("image/png"))
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .body(Body::from(picture))
        .map_err(internal_error)
}

fn missing_picture() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(
            json!({ "error": "No enlarged picture for this set", "code": "figure_picture_missing" }),
        ),
    )
}

async fn start(
    db: DatabaseConnection,
    claims: &Claims,
    outfit_id: Option<String>,
    payload: TurnKeyformsRequest,
) -> ApiResult<Json<Value>> {
    let keys = FigureKeys {
        head: payload.head,
        body: payload.body,
    };
    require_merope_enabled().await?;
    let user_id = require_owner(claims, &db).await?;
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
    let reference = image_generation::load_local_reference(&master.asset_id)
        .await
        .map_err(|_| bad_request("The current master portrait is not available"))?;
    let (client, space_name) = space_client().await?;
    let cache = TurnCache::for_master(
        &reference.bytes,
        &generation,
        &space_name,
        see_through::DecomposeOptions::default(),
    );

    let plain = outfit_id.is_none() && payload.plain;
    let (stage, total) = if outfit_id.is_some() {
        (Stage::Upscaling, 1)
    } else if plain {
        (Stage::Decomposing, 1)
    } else {
        (Stage::Generating, 4)
    };
    let (id, new) = claim_job(&outfit_id, &master.asset_id, stage, total)?;
    if !new {
        return Ok(Json(json!({ "jobId": id })));
    }
    if plain {
        update(&id, |job| job.phase = Phase::Plain);
    }
    tracing::info!(job = %id, outfit = ?outfit_id, "turn keyforms started");
    let meta = ArchiveMeta {
        id: archive::new_id(),
        created_at: chrono::Utc::now(),
        outfit_id: outfit_id.clone(),
        source_master_asset_id: payload.source_master_asset_id.clone(),
        source_generation_fingerprint: fingerprint.clone(),
        parent: None,
        master: String::new(),
        master_media_type: String::new(),
        generation_provider: generation.provider.clone(),
        generation_model: generation.model.clone(),
        prompts: turn_prompts().to_vec(),
        space: space_name.clone(),
        redrawn: Vec::new(),
        bytes: 0,
        figure: None,
        body_motions: None,
    };
    let task_id = id.clone();
    let standing = outfit_id.is_some();
    tokio::spawn(async move {
        let result = if standing {
            run_figure(
                &task_id,
                user_id,
                &generation,
                reference,
                &client,
                &space_name,
                keys,
            )
            .await
        } else if plain {
            run_plain(&task_id, reference, &client, &cache).await
        } else {
            run(&task_id, user_id, &generation, reference, &client, &cache).await
        };
        prune_cache(cache_root(), CACHE_KEEP).await;
        finish(&db, &task_id, result, meta).await;
    });
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

/// Four turned drawings, five decompositions, one fit.
async fn run(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    reference: image_generation::ImageReference,
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
) -> ApiResult<Made> {
    let (width, height) = image::ImageReader::new(Cursor::new(reference.bytes.as_ref()))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .ok_or_else(|| bad_request("The current master portrait is not available"))?;
    cache.touch().await;
    let mut turned = Vec::with_capacity(4);
    for (index, prompt) in turn_prompts().into_iter().enumerate() {
        let name = format!("turned-{index}.png");
        if let Some(png) = cache.read(&name).await {
            turned.push(png);
            update(id, |job| job.done += 1);
            continue;
        }
        let png = draw_turn(user_id, generation, &prompt, &reference, width, height).await?;
        cache.write(&name, &png).await;
        turned.push(png);
        update(id, |job| job.done += 1);
    }

    update(id, |job| {
        job.stage = Stage::Decomposing;
        job.done = 0;
        job.total = 5;
    });
    let options = see_through::DecomposeOptions::default();
    let front = (reference.bytes.clone(), reference.media_type.clone());
    let front_psd = decomposition(client, cache, 0, &front.0, &front.1, options).await?;
    update(id, |job| job.done += 1);
    // A turn drawn past what the front drawing's keys can follow is drawn once more.
    let turned = redraw_bad_turns(
        user_id,
        generation,
        &reference,
        (width, height),
        client,
        cache,
        &front_psd,
        turned,
    )
    .await;
    let inputs: Vec<_> = std::iter::once(front)
        .chain(
            turned
                .into_iter()
                .map(|png| (axum::body::Bytes::from(png), "image/png".to_string())),
        )
        .collect();
    let mut decompositions = Vec::with_capacity(5);
    decompositions.push(front_psd);
    for (index, (bytes, media_type)) in inputs.iter().enumerate().skip(1) {
        decompositions.push(decomposition(client, cache, index, bytes, media_type, options).await?);
        update(id, |job| job.done += 1);
    }
    // The fit also matches its keys against the drawings themselves.
    let pictures: [(Vec<u8>, String); 5] = inputs
        .into_iter()
        .map(|(bytes, media_type)| (bytes.to_vec(), media_type))
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
    let judged = judge::judge_turns(
        id,
        user_id,
        generation,
        &reference,
        (width, height),
        client,
        cache,
        options,
        &mut decompositions,
        &mut pictures,
        &mut redrawn,
    )
    .await?;
    update(id, |job| job.stage = Stage::Fitting);
    let output = fit(client, &decompositions, &pictures).await?;
    let (decompositions, pictures, output) = redraw_misfit(
        id,
        user_id,
        generation,
        &reference,
        (width, height),
        client,
        cache,
        options,
        (decompositions, pictures, output),
        &mut redrawn,
    )
    .await?;
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
        let (id, _) = claim_job(&Some("alongside-a".into()), "m", Stage::Decomposing, 3).unwrap();
        // The head is done first: the job then waits on the body as fitting.
        let body = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            7
        };
        let (head, body) = alongside(&id, async { Ok(made()) }, body).await;
        assert!(head.is_ok());
        assert_eq!(body, Some(7));
        let stage = JOB.lock().unwrap().as_ref().map(|job| job.stage);
        assert_eq!(stage, Some(Stage::Fitting));
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
