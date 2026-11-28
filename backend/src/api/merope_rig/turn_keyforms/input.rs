//! 任务的输入：从哪张图做、做成什么（`JobInput`），连同任务归谁（`rig_jobs::Owner`）存在任务
//! 记录上（`rig_jobs.pipeline_input`、`owner_scope`）。步骤只从这两样和站点配置的生图、拆层服务
//! 跑，不读设定。图是站点媒体里的一张（`PictureRef::SiteMedia`），站点设定的页面把当前主立绘
//! 写在这里。

use serde::Serialize;

use super::*;

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
}

impl PictureRef {
    /// What the picture is known by in what its jobs keep (an archive's
    /// `sourceMasterAssetId`) and what joins a job of the same picture.
    pub(super) fn name(&self) -> String {
        match self {
            Self::SiteMedia { url } => url.clone(),
        }
    }
}

/// The picture's bytes, as the job's steps take them.
pub(super) async fn load_picture(
    picture: &PictureRef,
) -> ApiResult<image_generation::ImageReference> {
    match picture {
        PictureRef::SiteMedia { url } => image_generation::load_local_reference(url)
            .await
            .map_err(|_| bad_request("The current master portrait is not available")),
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
