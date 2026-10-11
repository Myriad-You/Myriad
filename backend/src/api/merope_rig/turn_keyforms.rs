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
//! 现在的拟合重做一次，照常导入。

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
    portrait_generation_config_error, portrait_generation_provider_error, require_merope_enabled,
    require_owner,
};
use crate::{
    middleware::auth::Claims,
    services::{image_generation, see_through},
};

const KEEP: &str = "Edit this exact illustration. Keep the canvas size, framing, body, shoulders, \
neck base, outfit, colors, line style and shading exactly the same and pixel-aligned with the \
original, on the same background.";
const HEAD_ALONE: &str = "The head stays the same size and stays attached at the same neck \
position; the hair and every hair accessory and earring move with the head and keep their exact \
design, every lock of hair the same lock seen from the new angle: the bangs are not restyled, \
tucked or parted anew, and hair covering an eye still covers it. Same facial expression and the \
same eye openness. Clean finished art in the original's style.";

/// The four turned drawings, in the Space's order: toward image right and
/// left (Myriad's angleX ±1), raised and lowered (angleY ±1).
fn turn_prompts() -> [String; 4] {
    let turn = |side: &str| {
        format!(
            "{KEEP} Change only the head: the character turns the head about 30 degrees toward \
             the viewer's {side}, a natural three-quarter view, the eyes looking where the face \
             points. {HEAD_ALONE}"
        )
    };
    let nod = |motion: &str| {
        format!(
            "{KEEP} Change only the head: the character {motion} about 30 degrees, facing \
             straight toward the viewer with no turn to either side and no tilt, the eyes looking \
             where the face points. {HEAD_ALONE}"
        )
    };
    [
        turn("right"),
        turn("left"),
        nod("tilts the head back, the face looking upward,"),
        nod("lowers the head, the face looking downward,"),
    ]
}

mod archive;
mod body_turn;
mod cache;
mod figure_picture;
mod hair_misfit;
mod judge;
use archive::{ArchiveFiles, ArchiveMeta, FigureFiles, FigureMeta};
use cache::{CACHE_KEEP, TurnCache, cache_root, prune_cache};

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
    if !payload.head {
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

    let (stage, total) = if outfit_id.is_some() {
        (Stage::Upscaling, 1)
    } else {
        (Stage::Generating, 4)
    };
    let (id, new) = claim_job(&outfit_id, &master.asset_id, stage, total)?;
    if !new {
        return Ok(Json(json!({ "jobId": id })));
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
        } else {
            run(&task_id, user_id, &generation, reference, &client, &cache).await
        };
        prune_cache(cache_root(), CACHE_KEEP).await;
        finish(&db, &task_id, result, meta).await;
    });
    Ok(Json(json!({ "jobId": id })))
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

/// A standing figure's head is a couple of hundred pixels on a decomposition's
/// canvas: too little for a figure's detail, too small to fit turn keys well.
/// Its picture is enlarged (the Space's `upscale`), decomposed whole, planned
/// into three tiles (`figure_plan`: head and chest framed as a bust, waist,
/// legs), each tile decomposed on its own canvas, and the tiles stitched on
/// the whole (`figure_stitch`). With the head keyed, the upper tile is keyed
/// as a bust (`run`, on its own cache, its front decomposition the tile's)
/// and the keys placed on the stitched figure (`figure_keys`); with the body
/// keyed, its turn and weight shift are made alongside (`body_turn`). A Space
/// without these endpoints, or a head big enough already, has the figure
/// decomposed whole.
#[allow(clippy::too_many_arguments)]
async fn run_figure(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    reference: image_generation::ImageReference,
    client: &see_through::SeeThroughClient,
    space_name: &str,
    keys: FigureKeys,
) -> ApiResult<Made> {
    let options = see_through::DecomposeOptions::default();
    update(id, |job| {
        job.stage = Stage::Upscaling;
        job.done = 0;
        job.total = 1;
    });
    let picture = enlarged(client, &reference).await;
    let cache = TurnCache::for_master(&picture.bytes, generation, space_name, options);
    update(id, |job| {
        job.stage = Stage::Decomposing;
        job.done = 0;
        job.total = if keys.head { 3 } else { 4 };
    });
    let whole = decomposition(
        client,
        &cache,
        0,
        &picture.bytes,
        &picture.media_type,
        options,
    )
    .await?;
    update(id, |job| job.done += 1);
    let drawn = (picture.bytes.to_vec(), picture.media_type.clone());
    let plan = client
        .figure_plan(whole.clone(), drawn.clone())
        .await
        .map_err(see_through_error)?;
    // The body's keys are made alongside the head's work: their drawings and
    // fit need only the figure's whole decomposition (body_turn).
    let body = async {
        if keys.body {
            Some(
                body_turn::body_turn_keys(
                    id, user_id, generation, &reference, &picture, &whole, client, &cache,
                )
                .await,
            )
        } else {
            None
        }
    };
    let Some(plan) = plan else {
        tracing::info!(job = %id, "figure decomposed whole");
        let head = async {
            if keys.head {
                update(id, |job| job.phase = Phase::Head);
                run(id, user_id, generation, picture.clone(), client, &cache).await
            } else {
                Ok(plain(whole.clone(), drawn.1.clone()))
            }
        };
        return with_body(id, head, body).await;
    };
    tracing::info!(job = %id, tiles = ?plan.tiles, "figure decomposed in tiles");
    let [upper, middle, lower] = plan.crops;
    let tiles = plan.tiles;
    let head = async {
        let middle = tile_decomposition(client, &cache, "middle", middle).await?;
        update(id, |job| job.done += 1);
        let lower = tile_decomposition(client, &cache, "lower", lower).await?;
        update(id, |job| job.done += 1);
        let upper = image_generation::ImageReference {
            bytes: axum::body::Bytes::from(upper),
            media_type: "image/png".to_string(),
        };
        let head_cache = TurnCache::for_master(&upper.bytes, generation, space_name, options);
        let (upper_psd, made) = if keys.head {
            // The bust's front decomposition is the upper tile's.
            update(id, |job| job.phase = Phase::Head);
            let made = run(id, user_id, generation, upper, client, &head_cache).await?;
            (made.files.decompositions[0].clone(), Some(made))
        } else {
            let psd = decomposition(
                client,
                &head_cache,
                0,
                &upper.bytes,
                &upper.media_type,
                options,
            )
            .await?;
            update(id, |job| job.done += 1);
            (psd, None)
        };
        prune_cache(cache_root(), CACHE_KEEP).await;
        update(id, |job| {
            job.phase = Phase::Figure;
            job.stage = Stage::Stitching;
            job.done = 0;
            job.total = 1;
        });
        let stitched = client
            .figure_stitch(
                drawn.clone(),
                whole.clone(),
                [upper_psd, middle, lower],
                &tiles,
            )
            .await
            .map_err(see_through_error)?;
        update(id, |job| job.done += 1);
        let Some(made) = made else {
            return Ok(plain(stitched, drawn.1.clone()));
        };
        let figure = FigureMeta {
            master: String::new(),
            master_media_type: drawn.1.clone(),
            crop_box: tiles.upper,
        };
        let kept = FigureFiles {
            master: drawn.0.clone(),
            decomposition: stitched,
        };
        put_back(client, made, figure, kept).await
    };
    with_body(id, head, body).await
}

/// The head's work with the body's keys made alongside (`alongside`) and
/// put beside the head's in its keys JSON.
async fn with_body(
    id: &str,
    head: impl Future<Output = ApiResult<Made>>,
    body: impl Future<Output = Option<body_turn::BodyKeys>>,
) -> ApiResult<Made> {
    let (made, body) = alongside(id, head, body).await;
    let mut made = made?;
    made.files.keyforms = body_turn::with_body_keys(made.files.keyforms, body.flatten().as_ref());
    Ok(made)
}

/// The head's work and the body's turn, run together. The job reports the
/// head's stages; done first, it then waits on the body as fitting. A failed
/// head does not wait: the body is dropped with it.
async fn alongside<B: Future>(
    id: &str,
    head: impl Future<Output = ApiResult<Made>>,
    body: B,
) -> (ApiResult<Made>, Option<B::Output>) {
    tokio::pin!(head);
    tokio::pin!(body);
    let mut body_out = None;
    let head_out = loop {
        tokio::select! {
            out = &mut body, if body_out.is_none() => body_out = Some(out),
            out = &mut head => break out,
        }
    };
    if head_out.is_err() {
        return (head_out, None);
    }
    let body_out = match body_out {
        Some(out) => out,
        None => {
            update(id, |job| {
                job.phase = Phase::Body;
                job.stage = Stage::Fitting;
                job.done = 0;
                job.total = 1;
            });
            body.await
        }
    };
    (head_out, Some(body_out))
}

/// A plain figure: its decomposition, no keys (`null`), nothing kept.
fn plain(psd: Vec<u8>, media_type: String) -> Made {
    Made {
        files: ArchiveFiles {
            master: Vec::new(),
            turned: Default::default(),
            decompositions: Default::default(),
            keyforms: b"null".to_vec(),
            baked: psd,
            figure: None,
        },
        master_media_type: media_type,
        redrawn: Vec::new(),
        figure: None,
        keep: false,
    }
}

/// The figure's picture enlarged (kept by the master, `figure_picture`); the
/// master as it is when the Space cannot enlarge it.
async fn enlarged(
    client: &see_through::SeeThroughClient,
    reference: &image_generation::ImageReference,
) -> image_generation::ImageReference {
    let png = |bytes: Vec<u8>| image_generation::ImageReference {
        bytes: axum::body::Bytes::from(bytes),
        media_type: "image/png".to_string(),
    };
    if let Some(kept) = figure_picture::read(&reference.bytes).await {
        return png(kept);
    }
    match client
        .upscale((reference.bytes.to_vec(), reference.media_type.clone()))
        .await
    {
        Ok(Some(bytes)) => {
            figure_picture::write(&reference.bytes, &bytes).await;
            png(bytes)
        }
        Ok(None) => reference.clone(),
        Err(error) => {
            tracing::warn!(%error, "figure picture could not be enlarged; decomposed as drawn");
            reference.clone()
        }
    }
}

/// A tile without a head (waist, legs) decomposed once, kept in the figure's
/// cache. Not checked: the check is of a face.
async fn tile_decomposition(
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    name: &str,
    picture: Vec<u8>,
) -> ApiResult<Vec<u8>> {
    let file = format!("tile-{name}.psd");
    if let Some(psd) = cache.read(&file).await.filter(|psd| !psd.is_empty()) {
        return Ok(psd);
    }
    let psd = decompose_waiting(
        client,
        axum::body::Bytes::from(picture),
        "image/png",
        see_through::DecomposeOptions::default(),
    )
    .await?;
    cache.write(&file, &psd).await;
    Ok(psd)
}

/// The head's keys placed on the figure (`figure_keys`), with the figure's
/// own decomposition but the head's hair (`figure_hair`): the head's other
/// parts were only what the keys were fitted on.
async fn put_back(
    client: &see_through::SeeThroughClient,
    made: Made,
    figure: FigureMeta,
    kept: FigureFiles,
) -> ApiResult<Made> {
    let keyforms = client
        .figure_keys(
            (kept.master.clone(), figure.master_media_type.clone()),
            kept.decomposition.clone(),
            made.files.keyforms,
            figure.crop_box,
        )
        .await
        .map_err(see_through_error)?;
    // The keys were fitted on the head's hair as the head's decomposition
    // split it (side locks in the front hair, the front hair in locks): the
    // figure takes that hair. Should the Space not manage, the figure keeps
    // its own, as before.
    let kept = match client
        .figure_hair(
            kept.decomposition.clone(),
            made.files.baked.clone(),
            keyforms.clone(),
        )
        .await
    {
        Ok(decomposition) => FigureFiles {
            decomposition,
            ..kept
        },
        Err(error) => {
            tracing::warn!(%error, "the figure keeps its own hair");
            kept
        }
    };
    Ok(Made {
        files: ArchiveFiles {
            keyforms,
            baked: kept.decomposition.clone(),
            figure: Some(kept),
            ..made.files
        },
        figure: Some(figure),
        ..made
    })
}

/// The Space's fit of the four turned decompositions onto the front one,
/// matched against the five pictures; passing failures are tried again.
async fn fit(
    client: &see_through::SeeThroughClient,
    decompositions: &[Vec<u8>; 5],
    pictures: &[(Vec<u8>, String); 5],
) -> ApiResult<see_through::TurnKeyformsOutput> {
    let [front, turned @ ..] = decompositions.clone();
    let mut attempt = 0;
    loop {
        match client
            .turn_keyforms(front.clone(), turned.clone(), pictures.clone())
            .await
        {
            Ok(output) => return Ok(output),
            Err(error) if attempt < TRANSIENT_RETRIES && transient(&error) => {
                attempt += 1;
                tracing::warn!(%error, attempt, "turn keyforms fit failed; retrying");
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Err(error) => return Err(see_through_error(error)),
        }
    }
}

/// One turned drawing of the master, at the master's size.
async fn draw_turn(
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    prompt: &str,
    reference: &image_generation::ImageReference,
    width: u32,
    height: u32,
) -> ApiResult<Vec<u8>> {
    let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
        user_id,
        "merope",
        "turn-reference",
        image_generation::generate_image(generation, prompt, width, height, Some(reference)),
    )
    .await
    .map_err(portrait_generation_provider_error)?;
    let (bytes, _) = image_generation::load_generated_bytes(generated)
        .await
        .map_err(portrait_generation_provider_error)?;
    // The provider may answer at its own size; lined up means the master's.
    fit_to(&bytes, width, height)
}

/// The sides of the turned drawings, in their order.
const TURN_SIDES: [&str; 4] = ["plus", "minus", "up", "down"];

/// Draws again, once each, the turned drawings the Space's `check_turns`
/// finds redrawn past what keys of the front drawing can follow (a restyled
/// fringe, a moved clip). A check or a drawing that fails keeps what there is:
/// the turn is then fitted as drawn.
#[allow(clippy::too_many_arguments)]
async fn redraw_bad_turns(
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    reference: &image_generation::ImageReference,
    (width, height): (u32, u32),
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    front_psd: &[u8],
    mut turned: Vec<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let pictures: [(Vec<u8>, String); 5] =
        std::iter::once((reference.bytes.to_vec(), reference.media_type.clone()))
            .chain(
                turned
                    .iter()
                    .map(|png| (png.clone(), "image/png".to_string())),
            )
            .collect::<Vec<_>>()
            .try_into()
            .expect("five pictures");
    let check = match client
        .check_turned_pictures(front_psd.to_vec(), pictures)
        .await
    {
        Ok(check) => check,
        Err(error) => {
            tracing::warn!(%error, "turned drawings check failed; keeping the drawings");
            return turned;
        }
    };
    let prompts = turn_prompts();
    for side in &check.bad {
        let Some(index) = TURN_SIDES.iter().position(|known| known == side) else {
            continue;
        };
        let redrawn = format!("turned-{index}.redrawn");
        if cache.read(&redrawn).await.is_some() {
            continue;
        }
        tracing::info!(side, residual = ?check.residual, "turned drawing drawn again");
        match draw_turn(
            user_id,
            generation,
            &prompts[index],
            reference,
            width,
            height,
        )
        .await
        {
            Ok(png) => {
                cache.write(&format!("turned-{index}.png"), &png).await;
                // Its decomposition was of the drawing it replaces (an empty entry reads as none).
                cache
                    .write(&format!("decomposition-{}.psd", index + 1), b"")
                    .await;
                cache
                    .write(&format!("decomposition-{}.checked", index + 1), b"")
                    .await;
                cache.write(&redrawn, b"redrawn").await;
                turned[index] = png;
            }
            Err(error) => tracing::warn!(side, ?error, "turned drawing could not be drawn again"),
        }
    }
    turned
}

/// Draws once more the turn whose front hair the fit could not follow
/// (hair_misfit), decomposes and fits it again, and keeps whichever fit
/// follows that turn's front hair better. Once per drawing set: a cache
/// marker keeps a rerun from drawing it yet again.
#[allow(clippy::too_many_arguments)]
async fn redraw_misfit(
    id: &str,
    user_id: i32,
    generation: &image_generation::ImageGenerationConfig,
    reference: &image_generation::ImageReference,
    (width, height): (u32, u32),
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    options: see_through::DecomposeOptions,
    (decompositions, pictures, output): (
        [Vec<u8>; 5],
        [(Vec<u8>, String); 5],
        see_through::TurnKeyformsOutput,
    ),
    redrawn: &mut Vec<String>,
) -> ApiResult<(
    [Vec<u8>; 5],
    [(Vec<u8>, String); 5],
    see_through::TurnKeyformsOutput,
)> {
    let Some(index) = hair_misfit::misfit_side(&output.keyforms) else {
        return Ok((decompositions, pictures, output));
    };
    let side = TURN_SIDES[index];
    let marker = format!("turned-{index}.hair-retried");
    if cache.read(&marker).await.is_some() {
        return Ok((decompositions, pictures, output));
    }
    cache.write(&marker, b"retried").await;
    let before = hair_misfit::hair_outline(&output.keyforms, side).unwrap_or(f64::INFINITY);
    tracing::info!(
        side,
        before,
        "turned drawing's front hair not followed; drawing it again"
    );
    let png = match draw_turn(
        user_id,
        generation,
        &turn_prompts()[index],
        reference,
        width,
        height,
    )
    .await
    {
        Ok(png) => png,
        Err(error) => {
            tracing::warn!(side, ?error, "turned drawing could not be drawn again");
            return Ok((decompositions, pictures, output));
        }
    };
    update(id, |job| {
        job.stage = Stage::Decomposing;
        job.done = 0;
        job.total = 1;
    });
    // Decomposed apart from the cached one, which stays the drawing's until this one wins.
    let bytes = axum::body::Bytes::from(png.clone());
    let psd = decompose_checked(client, &bytes, "image/png", options, &TURNED_REDOS, None)
        .await?
        .0;
    let mut retried_decompositions = decompositions.clone();
    retried_decompositions[index + 1] = psd.clone();
    let mut retried_pictures = pictures.clone();
    retried_pictures[index + 1] = (png.clone(), "image/png".to_string());
    update(id, |job| {
        job.stage = Stage::Fitting;
        job.done = 0;
        job.total = 1;
    });
    let retried = fit(client, &retried_decompositions, &retried_pictures).await?;
    let after = hair_misfit::hair_outline(&retried.keyforms, side).unwrap_or(f64::INFINITY);
    if after >= before {
        tracing::info!(
            side,
            before,
            after,
            "the turn drawn again was no better; kept the first"
        );
        return Ok((decompositions, pictures, output));
    }
    tracing::info!(
        side,
        before,
        after,
        "the turn drawn again is followed better; kept it"
    );
    cache.write(&format!("turned-{index}.png"), &png).await;
    cache
        .write(&format!("decomposition-{}.psd", index + 1), &psd)
        .await;
    cache
        .write(&format!("decomposition-{}.checked", index + 1), b"checked")
        .await;
    cache
        .write(&format!("turned-{index}.redrawn"), b"redrawn")
        .await;
    if !redrawn.iter().any(|known| known == side) {
        redrawn.push(side.to_string());
    }
    Ok((retried_decompositions, retried_pictures, retried))
}

/// A drawing's decomposition, checked (decompose_checked) and kept in the cache.
async fn decomposition(
    client: &see_through::SeeThroughClient,
    cache: &TurnCache,
    index: usize,
    bytes: &axum::body::Bytes,
    media_type: &str,
    options: see_through::DecomposeOptions,
) -> ApiResult<Vec<u8>> {
    let name = format!("decomposition-{index}.psd");
    let checked = format!("decomposition-{index}.checked");
    let cached = cache.read(&name).await;
    let redos: &[(bool, u16)] = if index == 0 {
        &FRONT_REDOS
    } else {
        &TURNED_REDOS
    };
    Ok(match cached {
        Some(psd) if cache.read(&checked).await.is_some() => psd,
        cached => {
            let (psd, settled) =
                decompose_checked(client, bytes, media_type, options, redos, cached).await?;
            cache.write(&name, &psd).await;
            if settled {
                cache.write(&checked, b"checked").await;
            }
            psd
        }
    })
}

/// How a faulty decomposition is redone, in turn (hair pass, seed). The
/// front one is the rig's own layers: other seeds first, as a decomposition
/// with the hair from a pass of its own keeps the picture less closely at
/// rest. A turned one only guides the fit: the hair pass first, which mended
/// every turned one so far where new seeds did not.
const FRONT_REDOS: [(bool, u16); 3] = [(false, 7), (false, 1234), (true, 42)];
const TURNED_REDOS: [(bool, u16); 2] = [(true, 42), (true, 7)];

/// A decomposition the fit can key from. One the Space's check finds faulty
/// (the face took in the hair, the headwear the outfit) is redone (`redos`);
/// when none passes, the least faulty. Settled when the check had its say (a
/// check that could not run is asked again next time).
async fn decompose_checked(
    client: &see_through::SeeThroughClient,
    image: &axum::body::Bytes,
    media_type: &str,
    options: see_through::DecomposeOptions,
    redos: &[(bool, u16)],
    mut cached: Option<Vec<u8>>,
) -> ApiResult<(Vec<u8>, bool)> {
    let mut best: Option<(f64, Vec<u8>)> = None;
    for &(hair_pass, seed) in std::iter::once(&(options.hair_pass, options.seed)).chain(redos) {
        let psd = match cached.take() {
            Some(psd) => psd,
            None => {
                let options = see_through::DecomposeOptions {
                    hair_pass,
                    seed,
                    ..options
                };
                decompose_waiting(client, image.clone(), media_type, options).await?
            }
        };
        let check = match client.check_decomposition(psd.clone()).await {
            Ok(check) => check,
            Err(error) => {
                tracing::warn!(%error, "decomposition check failed; keeping the decomposition");
                return Ok((psd, false));
            }
        };
        if check.ok {
            // A Space without the check passes everything unmeasured.
            return Ok((psd, check.badness.is_some()));
        }
        tracing::warn!(seed, hair_pass, faults = ?check.faults, badness = ?check.badness, "faulty decomposition");
        let fault = check.badness.unwrap_or(f64::INFINITY);
        if best.as_ref().is_none_or(|(least, _)| fault < *least) {
            best = Some((fault, psd));
        }
    }
    Ok((best.expect("at least one decomposition").1, true))
}

/// A decomposition, waiting its turn while another one holds the Space.
async fn decompose_waiting(
    client: &see_through::SeeThroughClient,
    image: axum::body::Bytes,
    media_type: &str,
    options: see_through::DecomposeOptions,
) -> ApiResult<Vec<u8>> {
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    let mut attempt = 0;
    loop {
        match client.decompose(image.clone(), media_type, options).await {
            Ok(output) => return Ok(output.psd),
            Err(see_through::SeeThroughError::Busy) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            Err(error) if attempt < TRANSIENT_RETRIES && transient(&error) => {
                attempt += 1;
                tracing::warn!(%error, attempt, "decomposition failed; retrying");
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Err(error) => return Err(see_through_error(error)),
        }
    }
}

/// Tries after a passing failure (a dropped connection, a time-out, the Space restarting).
const TRANSIENT_RETRIES: u32 = 2;

/// Failures that may pass by themselves. Quota, a refusal, bad input or a
/// paused Space do not; trying again would only spend the quota.
fn transient(error: &see_through::SeeThroughError) -> bool {
    use see_through::SeeThroughError::*;
    match error {
        Timeout | Transport(_) | Waking => true,
        Upstream { status, .. } => *status >= 500 || *status == 408,
        _ => false,
    }
}

fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(20 * u64::from(attempt))
}

/// Resized to exactly `width` x `height`, as PNG.
fn fit_to(bytes: &[u8], width: u32, height: u32) -> ApiResult<Vec<u8>> {
    let image = image::load_from_memory(bytes).map_err(internal_error)?;
    let image = if image.width() == width && image.height() == height {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Lanczos3)
    };
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(internal_error)?;
    Ok(png)
}

pub async fn get_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = job
        .as_ref()
        .filter(|job| job.id == id)
        .ok_or_else(missing_job)?;
    Ok(Json(json!({
        "jobId": job.id,
        "outfitId": job.outfit_id,
        "sourceMasterAssetId": job.master_asset_id,
        "stage": job.stage.as_str(),
        "phase": job.phase.as_str(),
        "body": job.body.map(|body| json!({
            "motion": body.motion,
            "stage": body.stage.as_str(),
            "done": body.done,
            "total": body.total,
        })),
        "done": job.done,
        "total": job.total,
        "seconds": job.started.elapsed().as_secs(),
        "error": job.error,
        "archiveId": job.archive_id,
    })))
}

pub async fn get_turn_keyforms_psd(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let psd = finished(&db, &claims, &id, |job| job.psd.clone()).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/vnd.adobe.photoshop"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .header(
            "x-see-through-canvas",
            HeaderValue::from_static(see_through::CANVAS),
        )
        .body(Body::from(psd.as_ref().clone()))
        .map_err(internal_error)
}

pub async fn get_turn_keyforms_keys(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let keys = finished(&db, &claims, &id, |job| job.keyforms.clone()).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .body(Body::from(keys.as_ref().clone()))
        .map_err(internal_error)
}

async fn finished(
    db: &DatabaseConnection,
    claims: &Claims,
    id: &str,
    pick: impl FnOnce(&Job) -> Option<Arc<Vec<u8>>>,
) -> ApiResult<Arc<Vec<u8>>> {
    require_merope_enabled().await?;
    require_owner(claims, db).await?;
    let job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let job = job
        .as_ref()
        .filter(|job| job.id == id)
        .ok_or_else(missing_job)?;
    pick(job).ok_or_else(|| {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "The turn keys are not ready",
                "code": "turn_keyforms_not_ready"
            })),
        )
    })
}

/// The configured See-through Space, and its name.
async fn space_client() -> ApiResult<(see_through::SeeThroughClient, String)> {
    let (token, space) = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        (
            see_through::configured_hf_token(&config),
            see_through::configured_space(&config),
        )
    };
    let token =
        token.ok_or_else(|| see_through_error(see_through::SeeThroughError::NotConfigured))?;
    let space_name = space.name().to_owned();
    let client = see_through::SeeThroughClient::new(token, space)
        .await
        .map_err(see_through_error)?;
    Ok((client, space_name))
}

/// The kept jobs, newest first, each with whether its master is still the
/// current one of its slot (only those can be fitted again and imported).
pub async fn list_turn_archives(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let mut archives = Vec::new();
    for meta in archive::list().await {
        let current = require_master_match(
            &db,
            slot_of(meta.outfit_id.as_deref()),
            &meta.source_master_asset_id,
            meta.source_generation_fingerprint.as_deref(),
        )
        .await
        .is_ok();
        let mut entry = serde_json::to_value(&meta).map_err(internal_error)?;
        entry["current"] = json!(current);
        archives.push(entry);
    }
    Ok(Json(
        json!({ "archives": archives, "keepPerMaster": archive::KEEP_PER_MASTER }),
    ))
}

/// A kept job as one zip.
pub async fn download_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    archive::read_meta(&id).await.ok_or_else(missing_archive)?;
    let name = id.clone();
    let zip = tokio::task::spawn_blocking(move || archive::zip_blocking(&name))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/zip"),
        )
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("attachment; filename=\"turn-keys-{id}.zip\""))
                .map_err(internal_error)?,
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, private"),
        )
        .body(Body::from(zip))
        .map_err(internal_error)
}

pub async fn delete_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    if !archive::delete(&id).await.map_err(internal_error)? {
        return Err(missing_archive());
    }
    Ok(Json(json!({ "deleted": id })))
}

/// A kept job handed to the importer again as it was: a finished job made of
/// its decomposition and keys, fetched as any job. For a job whose page went
/// away before it was imported; its master must still be the slot's current one.
pub async fn load_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let kept = archive::read_meta(&id).await.ok_or_else(missing_archive)?;
    require_master_match(
        &db,
        slot_of(kept.outfit_id.as_deref()),
        &kept.source_master_asset_id,
        kept.source_generation_fingerprint.as_deref(),
    )
    .await?;
    let (baked, keyforms) = archive::read_result(&kept).await.map_err(internal_error)?;
    let (job_id, new) = claim_job(
        &kept.outfit_id,
        &kept.source_master_asset_id,
        Stage::Done,
        1,
    )?;
    if new {
        update(&job_id, |job| {
            job.phase = Phase::Load;
            job.done = 1;
            job.psd = Some(Arc::new(baked));
            job.keyforms = Some(Arc::new(keyforms));
            job.archive_id = Some(kept.id.clone());
        });
        tracing::info!(job = %job_id, archive = %kept.id, "turn keyforms loaded from an archive");
    }
    Ok(Json(json!({ "jobId": job_id })))
}

/// Fits a kept job again with the Space's current fit: no drawing, no
/// decomposition. Its master must still be the slot's current one, as the
/// import requires; the result is kept as a new archive and fetched as any job.
pub async fn refit_turn_archive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    require_merope_enabled().await?;
    require_owner(&claims, &db).await?;
    let kept = archive::read_meta(&id).await.ok_or_else(missing_archive)?;
    require_master_match(
        &db,
        slot_of(kept.outfit_id.as_deref()),
        &kept.source_master_asset_id,
        kept.source_generation_fingerprint.as_deref(),
    )
    .await?;
    let files = archive::read_files(&kept).await.map_err(internal_error)?;
    let (client, space_name) = space_client().await?;
    let (job_id, new) = claim_job(
        &kept.outfit_id,
        &kept.source_master_asset_id,
        Stage::Fitting,
        1,
    )?;
    if !new {
        return Ok(Json(json!({ "jobId": job_id })));
    }
    update(&job_id, |job| job.phase = Phase::Refit);
    tracing::info!(job = %job_id, archive = %kept.id, "turn keyforms fitted again from an archive");
    let meta = ArchiveMeta {
        id: archive::new_id(),
        created_at: chrono::Utc::now(),
        parent: Some(kept.id.clone()),
        space: space_name,
        bytes: 0,
        ..kept.clone()
    };
    let task_id = job_id.clone();
    tokio::spawn(async move {
        let pictures: [(Vec<u8>, String); 5] =
            std::iter::once((files.master.clone(), kept.master_media_type.clone()))
                .chain(
                    files
                        .turned
                        .iter()
                        .map(|png| (png.clone(), "image/png".to_string())),
                )
                .collect::<Vec<_>>()
                .try_into()
                .expect("five pictures");
        let figure = files.figure;
        // The body's keys are not fitted again: they go along as they were.
        let body = body_turn::body_of(&files.keyforms);
        let result = fit(&client, &files.decompositions, &pictures)
            .await
            .map(|output| Made {
                files: ArchiveFiles {
                    keyforms: output.keyforms,
                    baked: output.psd,
                    figure: None,
                    ..files
                },
                master_media_type: kept.master_media_type.clone(),
                redrawn: kept.redrawn.clone(),
                figure: None,
                keep: true,
            });
        // A figure keyed by its head goes back into the figure it was kept with.
        let result = match (result, kept.figure.clone(), figure) {
            (Ok(made), Some(meta), Some(files)) => put_back(&client, made, meta, files).await,
            (result, _, _) => result,
        }
        .map(|mut made| {
            made.files.keyforms = body_turn::with_body_keys(made.files.keyforms, body.as_ref());
            made
        });
        finish(&db, &task_id, result, meta).await;
    });
    Ok(Json(json!({ "jobId": job_id })))
}

fn missing_archive() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "No such turn keys archive", "code": "turn_archive_missing" })),
    )
}

fn missing_job() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "No such turn keys job", "code": "turn_keyforms_missing" })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
