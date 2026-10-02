//! 立绘的骨架识别：用 DWPose（全身 133 点：身体 17、脚 6、脸 68、双手 42）找出关节在
//! 主立绘上的位置，给形象绑定用。模型第一次用到时从作者仓库下载，按 SHA-256 校验；
//! 同一张图只识别一次，结果按图片内容缓存。

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
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

type Runnable = Arc<TypedRunnableModel<TypedModel>>;

static MODEL: OnceCell<Runnable> = OnceCell::const_new();

fn models_dir() -> PathBuf {
    data_paths::paths().root.join("models")
}

fn cache_dir() -> PathBuf {
    data_paths::paths().root.join("merope/pose")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn model() -> Result<Runnable, PoseError> {
    MODEL
        .get_or_try_init(|| async {
            let path = ensure_model_file().await?;
            tokio::task::spawn_blocking(move || {
                let model = tract_onnx::onnx()
                    .model_for_path(&path)
                    .and_then(|model| {
                        model
                            .with_input_fact(0, f32::fact([1, 3, INPUT_HEIGHT, INPUT_WIDTH]).into())
                    })
                    .and_then(|model| model.into_optimized())
                    .and_then(|model| model.into_runnable())
                    .map_err(|error| PoseError::Model(error.to_string()))?;
                Ok(Arc::new(model))
            })
            .await
            .map_err(|error| PoseError::Model(error.to_string()))?
        })
        .await
        .cloned()
}

/// The model file, downloaded once and kept only if it is the pinned one.
async fn ensure_model_file() -> Result<PathBuf, PoseError> {
    let path = models_dir().join(MODEL_FILE);
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
    let key = hex(&Sha256::digest(bytes));
    let cached = cache_dir().join(format!("{key}.json"));
    if let Ok(saved) = tokio::fs::read(&cached).await
        && let Ok(pose) = serde_json::from_slice::<Pose>(&saved)
        && pose.model == POSE_MODEL
    {
        return Ok(pose);
    }
    let model = model().await?;
    let bytes = bytes.to_vec();
    let pose = tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes)
            .map_err(|error| PoseError::Image(error.to_string()))?
            .to_rgba8();
        run(&model, &image)
    })
    .await
    .map_err(|error| PoseError::Inference(error.to_string()))??;
    if tokio::fs::create_dir_all(cache_dir()).await.is_ok()
        && let Ok(json) = serde_json::to_vec(&pose)
        && let Err(error) = tokio::fs::write(&cached, json).await
    {
        tracing::warn!(%error, "failed to cache a pose");
    }
    Ok(pose)
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
            .to_array_view::<f32>()
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
}
