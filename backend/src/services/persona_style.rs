//! Where her portrait style comes from: this build's own recipe (`Builtin`),
//! or a recipe service the site owner configured (`Remote`).
//!
//! A recipe is only style: the style words (core `StyleRecipe`), at most
//! one style reference image, and the provenance the contract records
//! (core `RenderingProvenance`). The site builds every prompt itself with
//! core's `*_with` builders, so the identity is normalised and screened
//! here, and the contract's identity section describes what the prompt
//! asks for. The site also draws: image provider, keys, cost ledger, lease
//! and storage stay here. A recipe service answers `GET {url}/v1/recipe`
//! and `GET {url}/v1/info` (wire schema 1, see `remote.rs`).
//!
//! A configured service that cannot answer is an error, never a quiet
//! return to the builtin recipe: the owner chose that style, and a portrait
//! in another one would replace hers without saying so. A stored URL that
//! no longer passes the URL policy is an error the same way.

use std::{sync::Mutex, time::Duration};

use axum::body::Bytes;
use myriad_merope::{
    RenderingProvenance, StyleRecipe, build_character_visual_edit_prompt_with,
    build_character_visual_prompt_with, build_full_body_design_prompt_with,
    build_full_body_portrait_prompt_with,
};
use serde_json::{Value, json};

use crate::{config::DynamicConfig, services::image_generation::ImageReference};

mod remote;

pub use remote::{RemoteRecipe, origin_of, sanitize_service_url};

/// The bundled rendering-technique reference of this build's own recipe.
const BUILTIN_STYLE_REFERENCE_BYTES: &[u8] =
    include_bytes!("../../assets/merope/style-reference.png");

/// A probe result is shown for this long, then the status asks for a new one.
const PROBE_SHOWN_FOR: Duration = Duration::from_secs(300);

/// The picture a prompt is built for.
#[derive(Debug, Clone, Copy)]
pub enum Drawing<'a> {
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

impl Drawing<'_> {
    /// Bust and full body from the design are drawn against the style
    /// reference; an edit and a redraw keep the portrait they start from,
    /// which the caller attaches.
    pub fn uses_style_reference(&self) -> bool {
        matches!(self, Self::Bust { .. } | Self::FullBodyDesign { .. })
    }
}

/// One recipe: style words, at most one reference, and its provenance.
#[derive(Debug, Clone)]
pub struct Recipe {
    pub style: StyleRecipe,
    /// The style reference; `rendering.style_reference_sha256` is its hash
    /// (checked for a remote recipe before it is returned).
    pub reference: Option<ImageReference>,
    pub rendering: RenderingProvenance,
    pub source: SourceKind,
}

impl Recipe {
    /// The prompt for `drawing`, built with core's builders from this
    /// recipe's words.
    pub fn prompt(&self, drawing: Drawing<'_>) -> String {
        match drawing {
            Drawing::Bust {
                name,
                visual_profile,
                notes,
            } => build_character_visual_prompt_with(name, visual_profile, notes, &self.style),
            Drawing::BustEdit { notes } => build_character_visual_edit_prompt_with(notes, &self.style),
            Drawing::FullBodyDesign {
                name,
                visual_profile,
            } => build_full_body_design_prompt_with(name, visual_profile, &self.style),
            Drawing::FullBodyFromBust => build_full_body_portrait_prompt_with(&self.style),
        }
    }

    /// The style references sent with `drawing`: this recipe's one, when the
    /// drawing is in its style.
    pub fn style_references(&self, drawing: Drawing<'_>) -> Vec<ImageReference> {
        if drawing.uses_style_reference() {
            self.reference.iter().cloned().collect()
        } else {
            Vec::new()
        }
    }
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
    /// A service URL is set without a token.
    NotConfigured,
    /// The stored URL, or where its host resolves, fails the URL policy.
    InvalidUrl,
    Unavailable,
    Unauthorized,
    InvalidResponse(&'static str),
}

impl std::fmt::Display for PersonaStyleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => {
                formatter.write_str("the portrait style service has no token configured")
            }
            Self::InvalidUrl => {
                formatter.write_str("the portrait style service URL fails the URL policy")
            }
            Self::Unavailable => formatter.write_str("the portrait style service is unavailable"),
            Self::Unauthorized => {
                formatter.write_str("the portrait style service rejected the site's token")
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
            Self::InvalidUrl => "persona_style_url_invalid",
            Self::Unavailable => "persona_style_unavailable",
            Self::Unauthorized => "persona_style_unauthorized",
            Self::InvalidResponse(_) => "persona_style_invalid_response",
        }
    }
}

fn stored_url(dynamic: &DynamicConfig) -> Option<&str> {
    dynamic
        .merope_persona_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
}

fn stored_token(dynamic: &DynamicConfig) -> Option<&str> {
    dynamic
        .merope_persona_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// The source the configuration names. No URL is `Builtin`; a URL that
/// fails the policy is `InvalidUrl` and a URL without a token is
/// `NotConfigured`, never `Builtin`.
pub fn configured(dynamic: &DynamicConfig) -> Result<StyleSource, PersonaStyleError> {
    let Some(url) = stored_url(dynamic) else {
        return Ok(StyleSource::Builtin);
    };
    let url = sanitize_service_url(url)
        .filter(|url| !url.is_empty())
        .ok_or(PersonaStyleError::InvalidUrl)?;
    let token = stored_token(dynamic).ok_or(PersonaStyleError::NotConfigured)?;
    RemoteRecipe::new(&url, token).map(StyleSource::Remote)
}

/// The recipe to draw with now.
pub async fn recipe(source: &StyleSource) -> Result<Recipe, PersonaStyleError> {
    match source {
        StyleSource::Builtin => Ok(builtin()),
        StyleSource::Remote(remote) => remote.recipe().await,
    }
}

/// This build's own recipe: core's builtin words, the bundled image and
/// core's builtin provenance.
pub fn builtin() -> Recipe {
    Recipe {
        style: StyleRecipe::builtin(),
        reference: Some(builtin_style_reference()),
        rendering: RenderingProvenance::builtin(),
        source: SourceKind::Builtin,
    }
}

pub fn builtin_style_reference() -> ImageReference {
    ImageReference::new(Bytes::from_static(BUILTIN_STYLE_REFERENCE_BYTES), "image/png")
        .expect("the bundled style reference is a PNG")
}

/// The last probe of a recipe service, for the origin it went to.
#[derive(Debug, Clone)]
struct Probe {
    origin: String,
    at: std::time::Instant,
    at_wall: chrono::DateTime<chrono::Utc>,
    answer: Result<RenderingProvenance, PersonaStyleError>,
}

static LAST_PROBE: Mutex<Option<Probe>> = Mutex::new(None);

fn last_probe_for(origin: &str) -> Option<Probe> {
    LAST_PROBE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
        .filter(|probe| probe.origin == origin && probe.at.elapsed() < PROBE_SHOWN_FOR)
}

/// What a probe shows: only whether it answered, and the versions it draws
/// with. Every failure other than a refused token reads as unavailable, so
/// the status says no more about an internal host than that.
fn shown_probe(probe: &Probe) -> Value {
    match &probe.answer {
        Ok(rendering) => json!({
            "at": probe.at_wall.to_rfc3339(),
            "available": true,
            "recipe": rendering.recipe,
            "visualSchoolVersion": rendering.visual_school_version,
            "styleReferenceSha256": rendering.style_reference_sha256,
            "fullBodyInstructionVersion": rendering.full_body_instruction_version,
        }),
        Err(error) => json!({
            "at": probe.at_wall.to_rfc3339(),
            "available": false,
            "error": match error {
                PersonaStyleError::Unauthorized => "persona_style_unauthorized",
                _ => "persona_style_unavailable",
            },
        }),
    }
}

/// What the owner sees about the style source, without any outbound call:
/// which source, whether it is configured, and the last probe of it (for
/// this origin, within five minutes) if there was one. Never the token.
pub fn status(dynamic: &DynamicConfig) -> Value {
    let token_configured = stored_token(dynamic).is_some();
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
        Ok(StyleSource::Remote(remote)) => json!({
            "source": "remote",
            "configured": true,
            "urlConfigured": true,
            "tokenConfigured": true,
            "probe": last_probe_for(remote.origin()).as_ref().map(shown_probe),
        }),
        Err(error) => json!({
            "source": "remote",
            "configured": false,
            "urlConfigured": true,
            "tokenConfigured": token_configured,
            "available": false,
            "error": error.code(),
        }),
    }
}

/// Forgets the last probe: the settings it was made under changed.
pub fn forget_probe() {
    *LAST_PROBE.lock().unwrap_or_else(|error| error.into_inner()) = None;
}

/// Asks the configured service what it draws with now (`GET /v1/info`),
/// remembers the answer for its origin, and returns the status. With no
/// usable remote configuration nothing is sent.
pub async fn probe(dynamic: &DynamicConfig) -> Value {
    if let Ok(StyleSource::Remote(remote)) = configured(dynamic) {
        let answer = remote.info().await;
        *LAST_PROBE.lock().unwrap_or_else(|error| error.into_inner()) = Some(Probe {
            origin: remote.origin().to_string(),
            at: std::time::Instant::now(),
            at_wall: crate::clock::now(),
            answer,
        });
    }
    status(dynamic)
}

/// The token is bound to the service's origin: a change of URL to another
/// origin (or to none) clears the stored token unless the same change
/// supplies one. Used by the owner's settings call and by settings restore
/// (`api::config::bind_restored_persona_style_token`).
/// `current` is the stored URL; `url` and `token` are the incoming values
/// (`None` = unchanged). Returns the token to store, if it changes.
pub fn token_after_url_change(
    current: Option<&str>,
    url: Option<&str>,
    token: Option<&str>,
) -> Option<String> {
    if let Some(token) = token {
        return Some(token.to_string());
    }
    let url = url?;
    let before = current.and_then(origin_of);
    let after = origin_of(url);
    (before.is_none() || before != after).then(String::new)
}

#[cfg(test)]
#[path = "persona_style_tests.rs"]
pub(crate) mod tests;
