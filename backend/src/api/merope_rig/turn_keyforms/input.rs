//! 任务的输入：从哪张图做、做成什么（`JobInput`），连同任务归谁（`rig_jobs::Owner`）存在任务
//! 记录上（`rig_jobs.pipeline_input`、`owner_scope`）。步骤只从这两样和站点配置的生图、拆层服务
//! 跑，不读设定。站点设定的页面把当前主立绘写成站点媒体里的那张图（`PictureRef::SiteMedia`）；
//! 调用方自己带来的图先照不可信的上传检查一遍，再按内容存进主人的存储
//! （`supply_picture`、`PictureRef::Supplied`）。

use serde::Serialize;

use super::super::portrait_upload::{MAX_PORTRAIT_BYTES, uploaded_portrait_reference};
use super::*;
use crate::services::{media, rig_jobs::Storage};

/// What a job's steps run from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "from",
    deny_unknown_fields
)]
pub(super) enum JobInput {
    /// A picture, made into `make`.
    Picture { picture: PictureRef, make: Make },
    /// A kept job of the owner's: handed over as it was, or fitted again.
    Kept { archive_id: String, refit: bool },
}

impl JobInput {
    pub(super) fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("a job input is plain data")
    }
}

/// What is made of a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "shape",
    deny_unknown_fields
)]
pub(super) enum Make {
    /// A bust, its head keyed for its turn.
    Bust,
    /// A bust split once, no keys.
    Plain,
    /// A standing figure: its head's turn, and its body's turn and weight
    /// shift, each keyed when chosen.
    Figure { head: bool, body: bool },
}

impl Make {
    pub(super) fn phase(self) -> Phase {
        match self {
            Self::Bust => Phase::Bust,
            Self::Plain => Phase::Plain,
            Self::Figure { .. } => Phase::Figure,
        }
    }

    /// The stage a job of it starts at, and how many of its first stage there are.
    pub(super) fn first_stage(self) -> (Stage, u8) {
        match self {
            Self::Figure { .. } => (Stage::Upscaling, 1),
            Self::Plain => (Stage::Decomposing, 1),
            Self::Bust => (Stage::Generating, 4),
        }
    }
}

/// The picture a job works from: a reference, never the bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind",
    deny_unknown_fields
)]
pub(super) enum PictureRef {
    /// A picture in the site's media, by the URL it is served at: the site
    /// persona's master.
    SiteMedia { url: String },
    /// A picture a caller supplied, kept in the job owner's storage by its
    /// content (`supply_picture`).
    Supplied { sha256: String, media_type: String },
}

impl PictureRef {
    /// What the picture is known by in what its jobs keep (an archive's
    /// `sourceMasterAssetId`) and what joins a job of the same picture.
    pub(super) fn name(&self) -> String {
        match self {
            Self::SiteMedia { url } => url.clone(),
            Self::Supplied { sha256, .. } => format!("supplied:{sha256}"),
        }
    }
}

/// A picture's content, as its supplied name has it.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Where an owner keeps the pictures supplied for its jobs.
fn supplied_root(storage: &Storage) -> std::path::PathBuf {
    storage.root().join("rig-inputs")
}

/// The file a supplied picture is kept in; None for a name that is not a
/// SHA-256 hex digest of a picture of a type taken.
fn supplied_path(storage: &Storage, sha256: &str, media_type: &str) -> Option<std::path::PathBuf> {
    let ext = match media_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => return None,
    };
    let digest = sha256.len() == 64
        && sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    digest.then(|| supplied_root(storage).join(format!("{sha256}.{ext}")))
}

/// A picture a caller supplies, checked as any uploaded portrait is
/// (`supplied_picture`), and kept in the owner's storage by its content.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no handler of this build takes a caller's own picture"
    )
)]
pub(super) async fn supply_picture(
    storage: &Storage,
    bytes: axum::body::Bytes,
) -> ApiResult<PictureRef> {
    let reference = supplied_picture(bytes).await?;
    let sha256 = sha256_hex(&reference.bytes);
    let path = supplied_path(storage, &sha256, &reference.media_type)
        .ok_or_else(|| internal_error("a checked picture has no place to be kept"))?;
    let partial = path.with_extension("partial");
    async {
        tokio::fs::create_dir_all(supplied_root(storage)).await?;
        tokio::fs::write(&partial, &reference.bytes).await?;
        tokio::fs::rename(&partial, &path).await
    }
    .await
    .map_err(internal_error)?;
    Ok(PictureRef::Supplied {
        sha256,
        media_type: reference.media_type,
    })
}

/// A supplied picture taken as an uploaded portrait is: at most
/// `MAX_PORTRAIT_BYTES`, a PNG, JPEG or WebP by its content
/// (`uploaded_portrait_reference`), within the media library's edge, pixel
/// and decode limits (`media::validate_bytes`), and decoded whole within
/// them (`media::decode_image`).
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "no handler of this build takes a caller's own picture"
    )
)]
pub(super) async fn supplied_picture(
    bytes: axum::body::Bytes,
) -> ApiResult<image_generation::ImageReference> {
    if bytes.len() > MAX_PORTRAIT_BYTES {
        return Err(bad_request("Picture exceeds 10 MB"));
    }
    let reference = uploaded_portrait_reference(bytes)?;
    media::validate_bytes(&reference.bytes, &reference.media_type, MAX_PORTRAIT_BYTES)
        .map_err(|error| bad_request(&error.to_string()))?;
    let whole = reference.bytes.clone();
    tokio::task::spawn_blocking(move || media::decode_image(&whole).map(|_| ()))
        .await
        .map_err(internal_error)?
        .map_err(|error| bad_request(&error.to_string()))?;
    Ok(reference)
}

/// The picture's bytes, as the job's steps take them.
pub(super) async fn load_picture(
    storage: &Storage,
    picture: &PictureRef,
) -> ApiResult<image_generation::ImageReference> {
    match picture {
        PictureRef::SiteMedia { url } => image_generation::load_local_reference(url)
            .await
            .map_err(|_| bad_request("The current master portrait is not available")),
        PictureRef::Supplied { sha256, media_type } => {
            let missing = || bad_request("The supplied picture is not available");
            let path = supplied_path(storage, sha256, media_type).ok_or_else(missing)?;
            let bytes = tokio::fs::read(path).await.map_err(|_| missing())?;
            if sha256_hex(&bytes) != *sha256 {
                return Err(missing());
            }
            image_generation::ImageReference::new(bytes, media_type.clone()).map_err(|_| missing())
        }
    }
}

/// What a job's steps call out to, as the site's settings have them: the
/// image provider, and the See-through Space with its name.
pub(super) struct Providers {
    pub(super) generation: image_generation::ImageGenerationConfig,
    pub(super) client: see_through::SeeThroughClient,
    pub(super) space_name: String,
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
