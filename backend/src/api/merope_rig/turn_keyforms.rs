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

use std::{
    io::Cursor,
    path::PathBuf,
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
use sha2::{Digest, Sha256};

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
design, every lock of hair the same lock seen from the new angle. Same facial expression. Clean \
finished art in the original's style.";

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

/// Bump when what a cached picture or decomposition means changes.
const CACHE_VERSION: &str = "turn-keyforms-cache-v1";
/// Masters whose pictures and decompositions are kept; older ones are removed.
const CACHE_KEEP: usize = 4;

/// The four turned drawings and five decompositions made for one master, on disk.
struct TurnCache {
    dir: PathBuf,
}

impl TurnCache {
    /// Keyed by everything that decides them: the master's pixels, how the
    /// drawings are asked for, and where and how they are decomposed.
    fn for_master(
        reference: &[u8],
        generation: &image_generation::ImageGenerationConfig,
        space: &str,
        options: see_through::DecomposeOptions,
    ) -> Self {
        Self::in_root(cache_root(), reference, generation, space, options)
    }

    fn in_root(
        root: PathBuf,
        reference: &[u8],
        generation: &image_generation::ImageGenerationConfig,
        space: &str,
        options: see_through::DecomposeOptions,
    ) -> Self {
        let mut hasher = Sha256::new();
        for part in [
            CACHE_VERSION,
            &generation.provider,
            &generation.model,
            space,
            see_through::CANVAS,
            &format!(
                "{}/{}/{}",
                options.resolution, options.seed, options.split_arms_and_legs
            ),
        ]
        .into_iter()
        .chain(turn_prompts().iter().map(String::as_str))
        {
            hasher.update((part.len() as u64).to_be_bytes());
            hasher.update(part.as_bytes());
        }
        hasher.update(reference);
        let key = hex::encode(hasher.finalize());
        Self {
            dir: root.join(&key[..32]),
        }
    }

    /// Marks the master as just used, so pruning keeps it even when nothing new is written.
    async fn touch(&self) {
        self.write("last-used", chrono::Utc::now().to_rfc3339().as_bytes())
            .await;
    }

    async fn read(&self, name: &str) -> Option<Vec<u8>> {
        tokio::fs::read(self.dir.join(name))
            .await
            .ok()
            .filter(|bytes| !bytes.is_empty())
    }

    /// Written whole or not at all; a cache that cannot be written only costs a redo.
    async fn write(&self, name: &str, bytes: &[u8]) {
        let result = async {
            tokio::fs::create_dir_all(&self.dir).await?;
            let partial = self.dir.join(format!(".{name}.partial"));
            tokio::fs::write(&partial, bytes).await?;
            tokio::fs::rename(&partial, self.dir.join(name)).await
        }
        .await;
        if let Err(error) = result {
            tracing::warn!(%error, dir = %self.dir.display(), name, "turn keyforms cache write failed");
        }
    }
}

fn cache_root() -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("merope/turn-keyforms")
}

/// Keeps the masters touched last; a master in use is touched as it is read or written.
async fn prune_cache(root: PathBuf, keep: usize) {
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return;
    };
    let mut dirs = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(meta) = entry.metadata().await
            && meta.is_dir()
        {
            dirs.push((meta.modified().ok(), entry.path()));
        }
    }
    dirs.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, dir) in dirs.into_iter().skip(keep) {
        if let Err(error) = tokio::fs::remove_dir_all(&dir).await {
            tracing::warn!(%error, dir = %dir.display(), "turn keyforms cache prune failed");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Generating,
    Decomposing,
    Fitting,
    Done,
    Failed,
}

impl Stage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Generating => "generating",
            Self::Decomposing => "decomposing",
            Self::Fitting => "fitting",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

struct Job {
    id: String,
    /// None: the worn bust; Some: a full-body set.
    outfit_id: Option<String>,
    master_asset_id: String,
    stage: Stage,
    done: u8,
    total: u8,
    error: Option<Value>,
    psd: Option<Arc<Vec<u8>>>,
    keyforms: Option<Arc<Vec<u8>>>,
    started: Instant,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);

fn update(id: &str, change: impl FnOnce(&mut Job)) {
    let mut job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(job) = job.as_mut().filter(|job| job.id == id) {
        change(job);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnKeyformsRequest {
    source_master_asset_id: String,
    #[serde(default)]
    source_generation_fingerprint: Option<String>,
}

pub async fn start_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<TurnKeyformsRequest>,
) -> ApiResult<Json<Value>> {
    start(db, &claims, None, payload).await
}

pub async fn start_full_body_turn_keyforms(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(outfit_id): Path<String>,
    Json(payload): Json<TurnKeyformsRequest>,
) -> ApiResult<Json<Value>> {
    start(db, &claims, Some(outfit_id), payload).await
}

async fn start(
    db: DatabaseConnection,
    claims: &Claims,
    outfit_id: Option<String>,
    payload: TurnKeyformsRequest,
) -> ApiResult<Json<Value>> {
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
    let slot = match outfit_id.as_deref() {
        Some(outfit) => MasterSlot::FullBody(outfit),
        None => MasterSlot::WornBust,
    };
    let master = require_master_match(
        &db,
        slot,
        &payload.source_master_asset_id,
        fingerprint.as_deref(),
    )
    .await?;
    let (token, space, dynamic) = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        (
            see_through::configured_hf_token(&config),
            see_through::configured_space(&config),
            config.clone(),
        )
    };
    let token =
        token.ok_or_else(|| see_through_error(see_through::SeeThroughError::NotConfigured))?;
    let space_name = space.name().to_owned();
    let generation = image_generation::config_from_dynamic(&dynamic)
        .map_err(portrait_generation_config_error)?;
    let reference = image_generation::load_local_reference(&master.asset_id)
        .await
        .map_err(|_| bad_request("The current master portrait is not available"))?;
    let client = see_through::SeeThroughClient::new(token, space)
        .await
        .map_err(see_through_error)?;
    let cache = TurnCache::for_master(
        &reference.bytes,
        &generation,
        &space_name,
        see_through::DecomposeOptions::default(),
    );

    let id = uuid::Uuid::new_v4().simple().to_string();
    {
        let mut job = JOB.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(running) = job
            .as_ref()
            .filter(|job| !matches!(job.stage, Stage::Done | Stage::Failed))
        {
            // The same master asked again (a reload, a second tab): follow the job already running.
            if running.outfit_id == outfit_id && running.master_asset_id == master.asset_id {
                return Ok(Json(json!({ "jobId": running.id })));
            }
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Turn keys are already being made",
                    "code": "turn_keyforms_in_progress"
                })),
            ));
        }
        *job = Some(Job {
            id: id.clone(),
            outfit_id: outfit_id.clone(),
            master_asset_id: master.asset_id.clone(),
            stage: Stage::Generating,
            done: 0,
            total: 4,
            error: None,
            psd: None,
            keyforms: None,
            started: Instant::now(),
        });
    }
    tracing::info!(job = %id, outfit = ?outfit_id, "turn keyforms started");
    let task_id = id.clone();
    tokio::spawn(async move {
        let result = run(&task_id, user_id, &generation, reference, &client, &cache).await;
        prune_cache(cache_root(), CACHE_KEEP).await;
        let still_current = {
            let slot = match outfit_id.as_deref() {
                Some(outfit) => MasterSlot::FullBody(outfit),
                None => MasterSlot::WornBust,
            };
            require_master_match(
                &db,
                slot,
                &payload.source_master_asset_id,
                fingerprint.as_deref(),
            )
            .await
        };
        match (result, still_current) {
            (Ok((psd, keyforms)), Ok(_)) => update(&task_id, |job| {
                job.stage = Stage::Done;
                job.psd = Some(Arc::new(psd));
                job.keyforms = Some(Arc::new(keyforms));
                tracing::info!(job = %job.id, seconds = job.started.elapsed().as_secs(), "turn keyforms done");
            }),
            (Err((_, Json(error))), _) | (_, Err((_, Json(error)))) => update(&task_id, |job| {
                tracing::warn!(job = %job.id, ?error, "turn keyforms failed");
                job.stage = Stage::Failed;
                job.error = Some(error);
            }),
        }
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
) -> ApiResult<(Vec<u8>, Vec<u8>)> {
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
        let generated = crate::services::ai_cost_ledger::with_site_ai_ledger(
            user_id,
            "merope",
            "turn-reference",
            image_generation::generate_image(generation, &prompt, width, height, Some(&reference)),
        )
        .await
        .map_err(portrait_generation_provider_error)?;
        let (bytes, _) = image_generation::load_generated_bytes(generated)
            .await
            .map_err(portrait_generation_provider_error)?;
        // The provider may answer at its own size; lined up means the master's.
        let png = fit_to(&bytes, width, height)?;
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
    let mut decompositions = Vec::with_capacity(5);
    let front = (reference.bytes.clone(), reference.media_type.clone());
    let inputs: Vec<_> = std::iter::once(front)
        .chain(
            turned
                .into_iter()
                .map(|png| (axum::body::Bytes::from(png), "image/png".to_string())),
        )
        .collect();
    for (index, (bytes, media_type)) in inputs.iter().enumerate() {
        let name = format!("decomposition-{index}.psd");
        let psd = match cache.read(&name).await {
            Some(psd) => psd,
            None => {
                let psd = decompose_waiting(client, bytes.clone(), media_type, options).await?;
                cache.write(&name, &psd).await;
                psd
            }
        };
        decompositions.push(psd);
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
    let mut decompositions = decompositions.into_iter();
    let front = decompositions.next().expect("front decomposition");
    let turned: [Vec<u8>; 4] = [
        decompositions.next().expect("right"),
        decompositions.next().expect("left"),
        decompositions.next().expect("up"),
        decompositions.next().expect("down"),
    ];
    let mut attempt = 0;
    let output = loop {
        match client
            .turn_keyforms(front.clone(), turned.clone(), pictures.clone())
            .await
        {
            Ok(output) => break output,
            Err(error) if attempt < TRANSIENT_RETRIES && transient(&error) => {
                attempt += 1;
                tracing::warn!(%error, attempt, "turn keyforms fit failed; retrying");
                tokio::time::sleep(retry_delay(attempt)).await;
            }
            Err(error) => return Err(see_through_error(error)),
        }
    };
    Ok((output.psd, output.keyforms))
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
        "done": job.done,
        "total": job.total,
        "seconds": job.started.elapsed().as_secs(),
        "error": job.error,
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

fn missing_job() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "No such turn keys job", "code": "turn_keyforms_missing" })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn generation(model: &str) -> image_generation::ImageGenerationConfig {
        image_generation::ImageGenerationConfig {
            provider: "openai".into(),
            model: model.into(),
            api_key: "secret".into(),
            base_url: String::new(),
        }
    }

    #[tokio::test]
    async fn a_master_keeps_its_drawings_and_decompositions_across_jobs() {
        let root = scratch_dir();
        let options = see_through::DecomposeOptions::default();
        let cache = TurnCache::in_root(root.clone(), b"master", &generation("m"), "a/b", options);
        assert!(cache.read("turned-0.png").await.is_none());
        cache.write("turned-0.png", b"png").await;
        // The same master, asked for the same way, finds it; another master, model or Space does not.
        let again = TurnCache::in_root(root.clone(), b"master", &generation("m"), "a/b", options);
        assert_eq!(
            again.read("turned-0.png").await.as_deref(),
            Some(&b"png"[..])
        );
        for other in [
            TurnCache::in_root(root.clone(), b"other", &generation("m"), "a/b", options),
            TurnCache::in_root(root.clone(), b"master", &generation("n"), "a/b", options),
            TurnCache::in_root(root.clone(), b"master", &generation("m"), "a/c", options),
        ] {
            assert!(other.read("turned-0.png").await.is_none());
        }
        // The key never carries the provider's secret.
        assert!(!cache.dir.to_string_lossy().contains("secret"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn pruning_keeps_the_masters_used_last() {
        let root = scratch_dir();
        let options = see_through::DecomposeOptions::default();
        let caches: Vec<_> = (0..3u8)
            .map(|n| TurnCache::in_root(root.clone(), &[n], &generation("m"), "a/b", options))
            .collect();
        for cache in &caches {
            cache.touch().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        caches[0].touch().await;
        prune_cache(root.clone(), 2).await;
        assert!(caches[0].dir.exists());
        assert!(!caches[1].dir.exists());
        assert!(caches[2].dir.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    fn scratch_dir() -> PathBuf {
        std::env::temp_dir().join(format!("turn-cache-test-{}", uuid::Uuid::new_v4().simple()))
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
        assert!(!transient(&Rejected));
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
