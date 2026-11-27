//! Where her portrait style comes from: this build's own recipe (`Builtin`),
//! or a recipe service the site owner configured (`Remote`).
//!
//! A recipe is the prompt, the style reference images and the rendering
//! provenance one picture is drawn with. The site still draws it: the image
//! provider, its keys, the cost ledger, the lease and storage stay here. A
//! recipe service only answers `POST {url}/v1/recipe` (wire schema 1, see
//! `remote.rs`).
//!
//! A configured service that cannot answer is an error, never a quiet return
//! to the builtin recipe: the owner chose that style, and a portrait in
//! another one would replace hers without saying so.

use axum::body::Bytes;
use myriad_merope::{
    RenderingProvenance, build_character_visual_edit_prompt, build_character_visual_prompt,
    build_full_body_design_prompt, build_full_body_portrait_prompt,
};
use serde_json::{Value, json};

use crate::{config::DynamicConfig, services::image_generation::ImageReference};

mod remote;

pub use remote::{RemoteRecipe, sanitize_service_url};

/// The bundled rendering-technique reference of this build's own recipe.
const BUILTIN_STYLE_REFERENCE_BYTES: &[u8] =
    include_bytes!("../../assets/merope/style-reference.png");

/// Bust and full-body-from-design draw in the recipe's style; an edit and a
/// full-body redraw keep the style of the portrait they start from, which the
/// site attaches itself.
#[derive(Debug, Clone, Copy)]
pub enum RecipeRequest<'a> {
    Bust {
        name: &'a str,
        visual_profile: &'a Value,
        notes: Option<&'a str>,
    },
    BustEdit {
        notes: &'a str,
    },
    FullBodyDesign {
        name: &'a str,
        visual_profile: &'a Value,
    },
    FullBodyFromBust,
}

impl RecipeRequest<'_> {
    fn draws_in_recipe_style(&self) -> bool {
        matches!(self, Self::Bust { .. } | Self::FullBodyDesign { .. })
    }
}

/// What one picture is drawn with.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub prompt: String,
    /// Style references, in order. Empty for an edit or a redraw from a bust.
    pub references: Vec<ImageReference>,
    pub rendering: RenderingProvenance,
    pub source: SourceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Builtin,
    Remote,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Remote => "remote",
        }
    }
}

/// Where recipes come from, as the site configuration says.
#[derive(Debug, Clone)]
pub enum StyleSource {
    Builtin,
    Remote(RemoteRecipe),
}

/// Why no recipe came. `Display` is safe to show and log: it never carries
/// the service URL, the token or anything the service sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersonaStyleError {
    NotConfigured,
    Unavailable,
    Unauthorized,
    Rejected,
    InvalidResponse(&'static str),
}

impl std::fmt::Display for PersonaStyleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => {
                formatter.write_str("the portrait style service is configured incompletely")
            }
            Self::Unavailable => formatter.write_str("the portrait style service is unavailable"),
            Self::Unauthorized => {
                formatter.write_str("the portrait style service rejected the site's token")
            }
            Self::Rejected => {
                formatter.write_str("the portrait style service declined this design")
            }
            Self::InvalidResponse(what) => write!(
                formatter,
                "the portrait style service sent an unusable answer: {what}"
            ),
        }
    }
}

impl std::error::Error for PersonaStyleError {}

impl PersonaStyleError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured => "persona_style_not_configured",
            Self::Unavailable => "persona_style_unavailable",
            Self::Unauthorized => "persona_style_unauthorized",
            Self::Rejected => "persona_style_rejected",
            Self::InvalidResponse(_) => "persona_style_invalid_response",
        }
    }
}

/// The source the configuration names. A URL without a token, or one that
/// no longer passes the URL policy, is `NotConfigured`, not `Builtin`.
pub fn configured(dynamic: &DynamicConfig) -> Result<StyleSource, PersonaStyleError> {
    let Some(url) = dynamic
        .merope_persona_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    else {
        return Ok(StyleSource::Builtin);
    };
    let token = dynamic
        .merope_persona_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or(PersonaStyleError::NotConfigured)?;
    RemoteRecipe::new(url, token).map(StyleSource::Remote)
}

/// The recipe for one picture.
pub async fn recipe(
    source: &StyleSource,
    request: RecipeRequest<'_>,
) -> Result<Recipe, PersonaStyleError> {
    match source {
        StyleSource::Builtin => builtin(request),
        StyleSource::Remote(remote) => remote.recipe(request).await,
    }
}

/// This build's own recipe: the core prompt builders and the bundled image.
pub fn builtin(request: RecipeRequest<'_>) -> Result<Recipe, PersonaStyleError> {
    let prompt = match request {
        RecipeRequest::Bust {
            name,
            visual_profile,
            notes,
        } => build_character_visual_prompt(name, visual_profile, notes),
        RecipeRequest::BustEdit { notes } => build_character_visual_edit_prompt(notes),
        RecipeRequest::FullBodyDesign {
            name,
            visual_profile,
        } => build_full_body_design_prompt(name, visual_profile),
        RecipeRequest::FullBodyFromBust => build_full_body_portrait_prompt(),
    };
    let references = if request.draws_in_recipe_style() {
        vec![builtin_style_reference().map_err(|_| PersonaStyleError::NotConfigured)?]
    } else {
        Vec::new()
    };
    Ok(Recipe {
        prompt,
        references,
        rendering: RenderingProvenance::builtin(),
        source: SourceKind::Builtin,
    })
}

pub fn builtin_style_reference()
-> Result<ImageReference, crate::services::image_generation::ImageGenerationError> {
    ImageReference::new(
        Bytes::from_static(BUILTIN_STYLE_REFERENCE_BYTES),
        "image/png",
    )
}

/// What the owner sees about the style source: which one, whether it can
/// answer now, and the versions it draws with. Never the URL's secret or
/// the token.
pub async fn status(dynamic: &DynamicConfig) -> Value {
    let token_configured = dynamic
        .merope_persona_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty());
    let url_configured = dynamic
        .merope_persona_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    match configured(dynamic) {
        Ok(StyleSource::Builtin) => {
            let builtin = RenderingProvenance::builtin();
            json!({
                "source": "builtin",
                "configured": true,
                "urlConfigured": false,
                "tokenConfigured": token_configured,
                "available": true,
                "recipe": Value::Null,
                "visualSchoolVersion": builtin.visual_school_version,
                "styleReferenceSha256": builtin.style_reference_sha256,
                "fullBodyInstructionVersion": builtin.full_body_instruction_version,
            })
        }
        Ok(StyleSource::Remote(remote)) => match remote.info().await {
            Ok(rendering) => json!({
                "source": "remote",
                "configured": true,
                "urlConfigured": true,
                "tokenConfigured": true,
                "available": true,
                "recipe": rendering.recipe,
                "visualSchoolVersion": rendering.visual_school_version,
                "styleReferenceSha256": rendering.style_reference_sha256,
                "fullBodyInstructionVersion": rendering.full_body_instruction_version,
            }),
            Err(error) => json!({
                "source": "remote",
                "configured": true,
                "urlConfigured": true,
                "tokenConfigured": true,
                "available": false,
                "error": error.code(),
            }),
        },
        Err(error) => json!({
            "source": "remote",
            "configured": false,
            "urlConfigured": url_configured,
            "tokenConfigured": token_configured,
            "available": false,
            "error": error.code(),
        }),
    }
}

#[cfg(test)]
#[path = "persona_style_tests.rs"]
pub(crate) mod tests;
