//! 立绘的骨架识别：用 DWPose（全身 133 点：身体 17、脚 6、脸 68、双手 42）找出关节在
//! 主立绘上的位置，给形象绑定用。模型第一次用到时从作者仓库下载，按 SHA-256 校验；
//! 识别在一个临时子进程里做：子进程加载模型、识别一张图、交回结果后退出，模型的内存
//! 随进程一起还给系统，后端进程从不持有模型。同一张图只识别一次，结果按图片内容缓存；
//! 不同的图可以同时识别几张（`LOOKS_AT_ONCE`）。

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tract_onnx::prelude::*;

use crate::services::data_paths;

/// The whole-body model, distilled RTMPose-l at 288×384 (Apache-2.0).
const MODEL_URL: &str = "https://huggingface.co/yzd-v/DWPose/resolve/main/dw-ll_ucoco_384.onnx";
const MODEL_FILE: &str = "dw-ll_ucoco_384.onnx";
const MODEL_SHA256: &str = "724f4ff2439ed61afb86fb8a1951ec39c6220682803b4a8bd4f598cd913b1843";
/// Named in every result, so a cached result from another model is never reused.
pub const POSE_MODEL: &str = "dwpose-ll-ucoco-384";

const INPUT_WIDTH: usize = 288;
const INPUT_HEIGHT: usize = 384;
/// SimCC heads classify each axis at twice the input resolution.
const SIMCC_SPLIT: f32 = 2.0;
/// The person box is padded by this much before it is cropped, as in training.
const BOX_PADDING: f32 = 1.25;
const MEAN: [f32; 3] = [123.675, 116.28, 103.53];
const STD: [f32; 3] = [58.395, 57.12, 57.375];
/// A pixel is the figure when its darkest channel is below this, or it is opaque
/// on a transparent sheet: master portraits stand on a near-white backdrop.
const INK_THRESHOLD: u8 = 235;
pub const KEYPOINT_COUNT: usize = 133;

#[derive(Debug)]
pub enum PoseError {
    Model(String),
    Image(String),
    Empty,
    Inference(String),
}

impl std::fmt::Display for PoseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(message) => write!(formatter, "pose model is unavailable: {message}"),
            Self::Image(message) => write!(formatter, "image cannot be read: {message}"),
            Self::Empty => write!(formatter, "image has no figure"),
            Self::Inference(message) => write!(formatter, "pose inference failed: {message}"),
        }
    }
}

impl std::error::Error for PoseError {}

/// Where the model found each keypoint, in the image's own pixels, and how sure it is.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pose {
    pub model: String,
    pub width: u32,
    pub height: u32,
    /// `[x, y, score]` for each of the 133 COCO-WholeBody keypoints.
    pub keypoints: Vec<[f32; 3]>,
}

type Runnable = std::sync::Arc<TypedRunnableModel>;

/**
 * A few looks at a time. Each look runs in its own child process, which loads
 * the model, looks, and exits: an allocator keeps freed memory, so the model's
 * few hundred megabytes are never held by the server between imports. A set's
 * gesture arms are drawn together and come to be looked at within seconds of
 * each other; one at a time, the last of them waited a minute for its turn.
 * A child holds some 340 MB while it looks, so this many are about a gigabyte
 * at their peak, given back as each exits. Each child is its own process on
 * its own core: what it finds does not depend on how many look beside it.
 */
const LOOKS_AT_ONCE: usize = 3;
static LOOKING: Semaphore = Semaphore::const_new(LOOKS_AT_ONCE);

/// The images being looked at now. The same image asked about by several at
/// once is looked at by the first; the rest wait for it and read what it kept.
static SAME_IMAGE: Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>> =
    Mutex::new(BTreeMap::new());

/// One asker's place in an image's queue, given up when it is done.
struct SameImage {
    key: String,
    turn: Arc<tokio::sync::Mutex<()>>,
}

impl SameImage {
    fn of(key: &str) -> Self {
        let mut images = SAME_IMAGE.lock().unwrap_or_else(|error| error.into_inner());
        Self {
            key: key.to_string(),
            turn: images.entry(key.to_string()).or_default().clone(),
        }
    }
}

impl Drop for SameImage {
    fn drop(&mut self) {
        let mut images = SAME_IMAGE.lock().unwrap_or_else(|error| error.into_inner());
        // This asker's handle goes first, while the list is held: what is left
        // is the list's own and those of the others waiting for this image.
        drop(std::mem::take(&mut self.turn));
        if images
            .get(&self.key)
            .is_some_and(|turn| Arc::strong_count(turn) == 1)
        {
            images.remove(&self.key);
        }
    }
}

/// The backend's own binary, started with this, is a one-shot pose worker.
pub const WORKER_ARG: &str = "--merope-pose-worker";
/// A look that has not finished by then has hung.
const WORKER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

fn models_dir() -> PathBuf {
    data_paths::paths().root.join("models")
}

fn cache_dir() -> PathBuf {
    data_paths::paths().root.join("merope/pose")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Loads the model from its file; it lives only as long as the caller keeps it.
fn load_model(path: &Path) -> Result<Runnable, PoseError> {
    tract_onnx::onnx()
        .model_for_path(path)
        .and_then(|model| {
            model.with_input_fact(0, f32::fact([1, 3, INPUT_HEIGHT, INPUT_WIDTH]).into())
        })
        .and_then(|model| model.into_optimized())
        .and_then(|model| model.into_runnable())
        .map_err(|error| PoseError::Model(error.to_string()))
}

/// The model file, downloaded once and kept only if it is the pinned one.
async fn ensure_model_file() -> Result<PathBuf, PoseError> {
    /// Several looks may find it missing at once; one of them fetches it.
    static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let path = models_dir().join(MODEL_FILE);
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(path);
    }
    let _fetching = FETCHING.lock().await;
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Ok(path);
    }
    tokio::fs::create_dir_all(models_dir())
        .await
        .map_err(|error| PoseError::Model(error.to_string()))?;
    tracing::info!(url = MODEL_URL, "downloading the pose model");
    let client = crate::services::http_client::get_long_running_client().await;
    let bytes = client
        .get(MODEL_URL)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|error| PoseError::Model(error.to_string()))?
        .bytes()
        .await
        .map_err(|error| PoseError::Model(error.to_string()))?;
    let digest = hex(&Sha256::digest(&bytes));
    if digest != MODEL_SHA256 {
        return Err(PoseError::Model(format!(
            "downloaded model does not match its pinned digest ({digest})"
        )));
    }
    let partial = path.with_extension("onnx.part");
    tokio::fs::write(&partial, &bytes)
        .await
        .map_err(|error| PoseError::Model(error.to_string()))?;
    tokio::fs::rename(&partial, &path)
        .await
        .map_err(|error| PoseError::Model(error.to_string()))?;
    Ok(path)
}

/// The figure's pose in this image, from the cache when it was seen before.
pub async fn estimate(bytes: &[u8]) -> Result<Pose, PoseError> {
    let look = async {
        let path = ensure_model_file().await?;
        look_in_child(&path, bytes).await
    };
    looked_once(&cache_dir(), &LOOKING, bytes, look).await
}

/// What `look` finds in this image, or what is kept in `cache` when it was
/// seen before. No more looks run at once than `looking` has room for, and
/// the same image is not looked at twice at once.
async fn looked_once(
    cache: &Path,
    looking: &Semaphore,
    bytes: &[u8],
    look: impl Future<Output = Result<Pose, PoseError>>,
) -> Result<Pose, PoseError> {
    let key = hex(&Sha256::digest(bytes));
    let cached = cache.join(format!("{key}.json"));
    if let Some(pose) = read_cached(&cached).await {
        return Ok(pose);
    }
    // Waited for before a place among the looks is taken, so the ones waiting
    // for the same image do not hold places other images could use.
    let same = SameImage::of(&key);
    let _first = same.turn.lock().await;
    let _looking = looking
        .acquire()
        .await
        .map_err(|error| PoseError::Model(error.to_string()))?;
    // Someone may have looked at the same image while this one waited.
    if let Some(pose) = read_cached(&cached).await {
        return Ok(pose);
    }
    let pose = look.await?;
    if tokio::fs::create_dir_all(cache).await.is_ok()
        && let Ok(json) = serde_json::to_vec(&pose)
        && let Err(error) = tokio::fs::write(&cached, json).await
    {
        tracing::warn!(%error, "failed to cache a pose");
    }
    Ok(pose)
}

/// Runs one look in a child process: the image in on stdin, the pose out on stdout.
async fn look_in_child(model: &Path, bytes: &[u8]) -> Result<Pose, PoseError> {
    use tokio::io::AsyncWriteExt;

    let program = std::env::current_exe().map_err(|error| PoseError::Model(error.to_string()))?;
    let mut child = tokio::process::Command::new(program)
        .arg(WORKER_ARG)
        .arg(model)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| PoseError::Model(error.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| PoseError::Model("pose worker has no stdin".into()))?;
    let image = bytes.to_vec();
    let feed = tokio::spawn(async move {
        let _ = stdin.write_all(&image).await;
    });
    let output = tokio::time::timeout(WORKER_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| PoseError::Inference("pose worker timed out".into()))?
        .map_err(|error| PoseError::Inference(error.to_string()))?;
    let _ = feed.await;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(match message.strip_prefix("empty:") {
            Some(_) => PoseError::Empty,
            None => match message.strip_prefix("image:") {
                Some(reason) => PoseError::Image(reason.trim().to_string()),
                None => PoseError::Inference(message),
            },
        });
    }
    serde_json::from_slice(&output.stdout).map_err(|error| PoseError::Inference(error.to_string()))
}

/// The worker's whole life: read one image from stdin, write its pose to
/// stdout as JSON, and exit. Failures go to stderr, tagged by kind.
pub fn worker_main() -> anyhow::Result<()> {
    use std::io::{Read, Write};

    let model = std::env::args()
        .nth(2)
        .ok_or_else(|| anyhow::anyhow!("pose worker needs the model path"))?;
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes)?;
    let looked = image::load_from_memory(&bytes)
        .map_err(|error| PoseError::Image(error.to_string()))
        .and_then(|image| {
            let image = image.to_rgba8();
            run(&load_model(Path::new(&model))?, &image)
        });
    match looked {
        Ok(pose) => {
            std::io::stdout().write_all(&serde_json::to_vec(&pose)?)?;
            Ok(())
        }
        Err(error) => {
            let tag = match error {
                PoseError::Empty => "empty:",
                PoseError::Image(_) => "image:",
                _ => "",
            };
            eprintln!("{tag} {error}");
            std::process::exit(1);
        }
    }
}

async fn read_cached(path: &Path) -> Option<Pose> {
    let saved = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice::<Pose>(&saved)
        .ok()
        .filter(|pose| pose.model == POSE_MODEL)
}

/// The square-ish box the figure is cropped to, in image pixels: its content,
/// padded and widened or heightened to the model's 3:4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PersonBox {
    pub center_x: f32,
    pub center_y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn person_box(image: &image::RgbaImage) -> Option<PersonBox> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, pixel) in image.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 16 || r.min(g).min(b) >= INK_THRESHOLD {
            continue;
        }
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let mut width = (x1 - x0) as f32 * BOX_PADDING;
    let mut height = (y1 - y0) as f32 * BOX_PADDING;
    let aspect = INPUT_WIDTH as f32 / INPUT_HEIGHT as f32;
    if width / height > aspect {
        height = width / aspect;
    } else {
        width = height * aspect;
    }
    Some(PersonBox {
        center_x: (x0 + x1) as f32 / 2.0,
        center_y: (y0 + y1) as f32 / 2.0,
        width,
        height,
    })
}

/// The model's input: the box resampled to 288×384, white outside the image,
/// normalized as in training.
fn input_tensor(image: &image::RgbaImage, frame: PersonBox) -> Tensor {
    let left = frame.center_x - frame.width / 2.0;
    let top = frame.center_y - frame.height / 2.0;
    let step_x = frame.width / INPUT_WIDTH as f32;
    let step_y = frame.height / INPUT_HEIGHT as f32;
    tract_ndarray::Array4::<f32>::from_shape_fn(
        (1, 3, INPUT_HEIGHT, INPUT_WIDTH),
        |(_, c, v, u)| {
            let value = sample(image, left + u as f32 * step_x, top + v as f32 * step_y, c);
            (value - MEAN[c]) / STD[c]
        },
    )
    .into()
}

/// Bilinear sample of one channel, over white where the image has nothing.
fn sample(image: &image::RgbaImage, x: f32, y: f32, channel: usize) -> f32 {
    let (width, height) = (image.width() as i64, image.height() as i64);
    let at = |px: i64, py: i64| -> f32 {
        if px < 0 || py < 0 || px >= width || py >= height {
            return 255.0;
        }
        let pixel = image.get_pixel(px as u32, py as u32).0;
        let alpha = pixel[3] as f32 / 255.0;
        pixel[channel] as f32 * alpha + 255.0 * (1.0 - alpha)
    };
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = (x - fx, y - fy);
    let (ix, iy) = (fx as i64, fy as i64);
    let top = at(ix, iy) * (1.0 - tx) + at(ix + 1, iy) * tx;
    let bottom = at(ix, iy + 1) * (1.0 - tx) + at(ix + 1, iy + 1) * tx;
    top * (1.0 - ty) + bottom * ty
}

fn run(model: &Runnable, image: &image::RgbaImage) -> Result<Pose, PoseError> {
    let frame = person_box(image).ok_or(PoseError::Empty)?;
    let outputs = model
        .run(tvec!(input_tensor(image, frame).into()))
        .map_err(|error| PoseError::Inference(error.to_string()))?;
    let view = |index: usize| {
        outputs
            .get(index)
            .ok_or_else(|| PoseError::Inference("missing SimCC head".into()))?
            .to_plain_array_view::<f32>()
            .map_err(|error| PoseError::Inference(error.to_string()))
            .map(|view| view.to_owned())
    };
    let (simcc_x, simcc_y) = (view(0)?, view(1)?);
    let keypoints = decode(
        simcc_x.as_slice().unwrap_or_default(),
        simcc_y.as_slice().unwrap_or_default(),
        frame,
    )?;
    Ok(Pose {
        model: POSE_MODEL.to_string(),
        width: image.width(),
        height: image.height(),
        keypoints,
    })
}

/// Each keypoint is the peak of its row in both SimCC heads, mapped from the
/// crop back to the image; its score is the lower of the two peaks.
pub fn decode(
    simcc_x: &[f32],
    simcc_y: &[f32],
    frame: PersonBox,
) -> Result<Vec<[f32; 3]>, PoseError> {
    let columns = (INPUT_WIDTH as f32 * SIMCC_SPLIT) as usize;
    let rows = (INPUT_HEIGHT as f32 * SIMCC_SPLIT) as usize;
    if simcc_x.len() != KEYPOINT_COUNT * columns || simcc_y.len() != KEYPOINT_COUNT * rows {
        return Err(PoseError::Inference("unexpected SimCC shape".into()));
    }
    let peak = |values: &[f32]| {
        values
            .iter()
            .enumerate()
            .fold((0usize, f32::MIN), |best, (index, &value)| {
                if value > best.1 { (index, value) } else { best }
            })
    };
    Ok((0..KEYPOINT_COUNT)
        .map(|k| {
            let (lx, sx) = peak(&simcc_x[k * columns..(k + 1) * columns]);
            let (ly, sy) = peak(&simcc_y[k * rows..(k + 1) * rows]);
            let u = lx as f32 / SIMCC_SPLIT / INPUT_WIDTH as f32;
            let v = ly as f32 / SIMCC_SPLIT / INPUT_HEIGHT as f32;
            [
                frame.center_x + (u - 0.5) * frame.width,
                frame.center_y + (v - 0.5) * frame.height,
                sx.min(sy),
            ]
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn the_person_box_is_the_figure_padded_to_the_models_aspect() {
        let mut image = image::RgbaImage::from_pixel(200, 400, image::Rgba([255, 255, 255, 255]));
        for y in 100..300 {
            for x in 80..120 {
                image.put_pixel(x, y, image::Rgba([40, 40, 40, 255]));
            }
        }
        let frame = person_box(&image).unwrap();
        assert_eq!((frame.center_x, frame.center_y), (100.0, 200.0));
        assert_eq!(frame.height, 250.0);
        assert!((frame.width / frame.height - 0.75).abs() < 1e-6);
        let blank = image::RgbaImage::from_pixel(10, 10, image::Rgba([250, 250, 250, 255]));
        assert_eq!(person_box(&blank), None);
    }

    #[test]
    fn keypoints_map_from_the_crop_back_to_the_image() {
        let frame = PersonBox {
            center_x: 500.0,
            center_y: 800.0,
            width: 300.0,
            height: 400.0,
        };
        let mut x = vec![0.0; KEYPOINT_COUNT * 576];
        let mut y = vec![0.0; KEYPOINT_COUNT * 768];
        // Keypoint 0 peaks at the crop centre, keypoint 1 at its top-left corner.
        x[288] = 0.9;
        y[384] = 0.8;
        x[576] = 0.5;
        y[768] = 0.7;
        let points = decode(&x, &y, frame).unwrap();
        assert_eq!(points[0], [500.0, 800.0, 0.8]);
        assert_eq!(points[1], [350.0, 600.0, 0.5]);
        assert!(decode(&x[1..], &y, frame).is_err());
    }

    fn temp_cache() -> PathBuf {
        std::env::temp_dir().join(format!("pose-cache-test-{}", uuid::Uuid::new_v4().simple()))
    }

    fn pose_of(width: u32) -> Pose {
        Pose {
            model: POSE_MODEL.to_string(),
            width,
            height: 2,
            keypoints: vec![[1.0, 2.0, 0.5]],
        }
    }

    /// A stuck look fails its test instead of hanging it.
    async fn within_a_while<T>(work: impl Future<Output = T>) -> T {
        tokio::time::timeout(std::time::Duration::from_secs(20), work)
            .await
            .expect("the looks did not finish")
    }

    #[tokio::test]
    async fn the_same_image_asked_about_at_once_is_looked_at_once() {
        let cache = temp_cache();
        let looking = Semaphore::new(3);
        let image = uuid::Uuid::new_v4().simple().to_string().into_bytes();
        let looks = &AtomicUsize::new(0);
        // Each look takes a while, so the ones asking beside it have come by then.
        let look = || async move {
            looks.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
            Ok::<_, PoseError>(pose_of(7))
        };
        let found = within_a_while(futures::future::join_all(
            (0..5).map(|_| looked_once(&cache, &looking, &image, look())),
        ))
        .await;
        assert_eq!(looks.load(Ordering::SeqCst), 1);
        assert!(
            found
                .iter()
                .all(|pose| pose.as_ref().ok() == Some(&pose_of(7)))
        );
        // Kept by the image's content: asked again later, nothing looks.
        let again = looked_once(&cache, &looking, &image, look()).await.unwrap();
        assert_eq!((again, looks.load(Ordering::SeqCst)), (pose_of(7), 1));
        // Nobody waits for it any more, so its queue is gone.
        let key = hex(&Sha256::digest(&image));
        assert!(!SAME_IMAGE.lock().unwrap().contains_key(&key));
        let _ = tokio::fs::remove_dir_all(&cache).await;
    }

    #[tokio::test]
    async fn different_images_are_looked_at_together_up_to_the_room_there_is() {
        let cache = temp_cache();
        let looking = Semaphore::new(2);
        let (now, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
        // A look ends only once another is looking beside it: one at a time would never finish.
        let beside = tokio::sync::Barrier::new(2);
        let images: Vec<Vec<u8>> = (0..4u8).map(|i| vec![i; 8]).collect();
        let found = within_a_while(futures::future::join_all(images.iter().enumerate().map(
            |(i, image)| {
                let (now, most, beside) = (&now, &most, &beside);
                looked_once(&cache, &looking, image, async move {
                    most.fetch_max(now.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                    beside.wait().await;
                    now.fetch_sub(1, Ordering::SeqCst);
                    Ok(pose_of(i as u32))
                })
            },
        )))
        .await;
        assert_eq!(most.load(Ordering::SeqCst), 2);
        for (i, pose) in found.into_iter().enumerate() {
            assert_eq!(pose.unwrap(), pose_of(i as u32));
        }
        let _ = tokio::fs::remove_dir_all(&cache).await;
    }

    #[tokio::test]
    async fn a_look_that_failed_keeps_nothing_and_the_next_asker_looks_again() {
        let cache = temp_cache();
        let looking = Semaphore::new(1);
        let failed = looked_once(&cache, &looking, b"image", async { Err(PoseError::Empty) }).await;
        assert!(matches!(failed, Err(PoseError::Empty)));
        let found = looked_once(&cache, &looking, b"image", async { Ok(pose_of(3)) }).await;
        assert_eq!(found.unwrap(), pose_of(3));
        let _ = tokio::fs::remove_dir_all(&cache).await;
    }
}
