//! 任务的输入：从哪张图做、做成什么（`JobInput`），连同任务归谁（`rig_jobs::Owner`）存在任务
//! 记录上（`rig_jobs.pipeline_input`、`owner_scope`）。步骤只从这两样和站点配置的生图、拆层服务
//! 跑，不读设定。图有两种：站点媒体里的一张（`PictureRef::SiteMedia`），只给站点自己的任务，
//! 站点设定的页面把当前主立绘写在这里；调用方自己带来的一张，先照不可信的上传检查一遍，再按
//! 内容存进主人的存储（`supply_picture`、`SuppliedPicture`）。

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

/// The shortest side a supplied picture may have: the decompositions and
/// keys are made on a canvas this picture fills.
pub(super) const MIN_SUPPLIED_EDGE: u32 = 512;

/// A picture a caller supplied and `supply_picture` kept: the only way a
/// job's input names one. It is never read from what a caller sends, and it
/// cannot name the site's media.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SuppliedPicture {
    sha256: String,
    media_type: String,
}

impl From<SuppliedPicture> for PictureRef {
    fn from(picture: SuppliedPicture) -> Self {
        PictureRef::Supplied {
            sha256: picture.sha256,
            media_type: picture.media_type,
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
/// (`supplied_picture`), and kept in the owner's storage by its content: a
/// file already there with the same content is the picture kept. It takes
/// the whole body in memory and decodes it: whatever hands it a body caps
/// the body's size before reading it and how many run at once.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the route that takes a caller's own picture is not in this build"
    )
)]
pub(super) async fn supply_picture(
    storage: &Storage,
    bytes: axum::body::Bytes,
) -> ApiResult<SuppliedPicture> {
    let reference = supplied_picture(bytes).await?;
    let sha256 = sha256_hex(&reference.bytes);
    let path = supplied_path(storage, &sha256, &reference.media_type)
        .ok_or_else(|| internal_error("a checked picture has no place to be kept"))?;
    let kept = tokio::fs::read(&path)
        .await
        .is_ok_and(|bytes| sha256_hex(&bytes) == sha256);
    if !kept {
        // Written under a name of its own, so that two supplying the same
        // picture at once never write into one another's file.
        let partial = supplied_root(storage).join(format!(
            ".{sha256}.{}.partial",
            uuid::Uuid::new_v4().simple()
        ));
        let written = async {
            tokio::fs::create_dir_all(supplied_root(storage)).await?;
            tokio::fs::write(&partial, &reference.bytes).await?;
            tokio::fs::rename(&partial, &path).await
        }
        .await;
        if let Err(error) = written {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(internal_error(error));
        }
    }
    Ok(SuppliedPicture {
        sha256,
        media_type: reference.media_type,
    })
}

/// A supplied picture taken as an uploaded portrait is: at most
/// `MAX_PORTRAIT_BYTES`, a PNG, JPEG or WebP by its content
/// (`uploaded_portrait_reference`), within the media library's edge, pixel
/// and decode limits (`media::validate_bytes`), decoded whole within them
/// (`media::decode_image`), and no side shorter than `MIN_SUPPLIED_EDGE`.
/// It is taken as it is: not trimmed, not looked at by a model.
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
    let (width, height) = tokio::task::spawn_blocking(move || {
        media::decode_image(&whole).map(|picture| (picture.width(), picture.height()))
    })
    .await
    .map_err(internal_error)?
    .map_err(|error| bad_request(&error.to_string()))?;
    if width.min(height) < MIN_SUPPLIED_EDGE {
        return Err(bad_request("Picture's shorter side is under 512 pixels"));
    }
    Ok(reference)
}

/// The picture's bytes, as the job's steps take them. The site's media is
/// read only for the site's own jobs.
pub(super) async fn load_picture(
    owner: &rig_jobs::Owner,
    picture: &PictureRef,
) -> ApiResult<image_generation::ImageReference> {
    match picture {
        PictureRef::SiteMedia { url } => {
            if !matches!(owner, rig_jobs::Owner::Site { .. }) {
                return Err(bad_request(
                    "Only the site's own jobs read the site's media",
                ));
            }
            image_generation::load_local_reference(url)
                .await
                .map_err(|_| bad_request("The current master portrait is not available"))
        }
        PictureRef::Supplied { sha256, media_type } => {
            let missing = || bad_request("The supplied picture is not available");
            let path = supplied_path(&owner.storage(), sha256, media_type).ok_or_else(missing)?;
            let bytes = tokio::fs::read(path).await.map_err(|_| missing())?;
            if sha256_hex(&bytes) != *sha256 {
                return Err(missing());
            }
            image_generation::ImageReference::new(bytes, media_type.clone()).map_err(|_| missing())
        }
    }
}

/// How long a supplied picture no job names is kept before it may go: long
/// enough for the start that supplied it to claim its job.
const UNNAMED_KEPT: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// The owner's supplied pictures that no job of its record names any more
/// (its jobs ended and past their retention, `rig_jobs::RETENTION`), and
/// that no start has supplied within `UNNAMED_KEPT`, removed.
pub(super) async fn prune_supplied(db: &DatabaseConnection, owner: &rig_jobs::Owner) {
    let named: Vec<String> = match rig_jobs::pipeline_inputs(db, &owner.scope()).await {
        Ok(inputs) => inputs
            .into_iter()
            .filter_map(|input| match serde_json::from_value(input) {
                Ok(JobInput::Picture {
                    picture: PictureRef::Supplied { sha256, .. },
                    ..
                }) => Some(sha256),
                _ => None,
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "supplied pictures could not be listed; none removed");
            return;
        }
    };
    let Ok(mut entries) = tokio::fs::read_dir(supplied_root(&owner.storage())).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((sha256, _)) = name.split_once('.') else {
            continue;
        };
        if sha256.is_empty() || named.iter().any(|kept| kept == sha256) {
            continue;
        }
        // A file's own time: how long ago it was supplied.
        #[allow(clippy::disallowed_methods)]
        let old = entry
            .metadata()
            .await
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| std::time::SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > UNNAMED_KEPT);
        if old {
            let _ = tokio::fs::remove_file(entry.path()).await;
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
