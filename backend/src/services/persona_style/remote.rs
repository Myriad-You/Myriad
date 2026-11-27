//! A recipe service over HTTP, wire schema 1.
//!
//! `POST {url}/v1/recipe`, `Authorization: Bearer {token}`:
//!
//! ```text
//! request  {schema: 1, kind: "bust", name, visualProfile, notes?}
//!        | {schema: 1, kind: "bustEdit", notes}
//!        | {schema: 1, kind: "fullBodyDesign", name, visualProfile}
//!        | {schema: 1, kind: "fullBodyFromBust"}
//! response {schema: 1, prompt, references: [{mimeType, sha256, data}],
//!           rendering: {recipe, visualSchoolVersion, styleReferenceSha256?,
//!                       fullBodyInstructionVersion}}
//! ```
//!
//! `GET {url}/v1/info` (same token) answers `{schema: 1, rendering}`: what a
//! bust drawn now would record. `data` is standard base64.
//!
//! Everything the service sends is bounded and checked before anything is
//! drawn: the prompt, at most two references of at most 8 MiB each whose
//! hashes match, a rendering provenance that names its recipe, and a
//! reference hash that is the first reference's. Redirects are not followed,
//! so the token only ever goes to the configured origin.

use std::{sync::OnceLock, time::Duration};

use base64::Engine as _;
use myriad_merope::RenderingProvenance;
use reqwest::{
    StatusCode,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::{Host, Url};

use super::{PersonaStyleError, Recipe, RecipeRequest, SourceKind};
use crate::services::image_generation::ImageReference;

pub(super) const WIRE_SCHEMA: u32 = 1;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RECIPE_TIMEOUT: Duration = Duration::from_secs(30);
const INFO_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const MAX_RESPONSE_BYTES: usize = 24 * 1024 * 1024;
pub(super) const MAX_REFERENCES: usize = 2;
pub(super) const MAX_REFERENCE_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_PROMPT_CHARS: usize = 32_000;
const MAX_URL_CHARS: usize = 512;
const MAX_TOKEN_CHARS: usize = 512;

/// A configured recipe service. `Debug` never shows the token.
#[derive(Clone)]
pub struct RemoteRecipe {
    base: Url,
    token: String,
}

impl std::fmt::Debug for RemoteRecipe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteRecipe")
            .field("base", &self.base.as_str())
            .field("token", &"<redacted>")
            .finish()
    }
}

/// The policy for the recipe service URL, used when it is saved, restored
/// from a backup, and read back: `https`, or `http` only to a loopback,
/// private or link-local address or a single-label host (a container on
/// the same network), so the bearer token never crosses the internet in
/// clear text. No credentials, query or fragment. Private addresses are
/// allowed on purpose (the service usually runs beside the site); only the
/// owner sets this, redirects are not followed, and nothing the service
/// sends is passed back unchecked. Stored without a trailing `/`; empty
/// clears it.
pub fn sanitize_service_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Some(String::new());
    }
    if raw.chars().count() > MAX_URL_CHARS {
        return None;
    }
    let url = Url::parse(raw).ok()?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let host = url.host()?;
    match url.scheme() {
        "https" => {}
        "http" if host_is_local(&host) => {}
        _ => return None,
    }
    Some(url.as_str().trim_end_matches('/').to_string())
}

fn host_is_local(host: &Host<&str>) -> bool {
    match host {
        Host::Ipv4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Host::Ipv6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost") || !name.contains('.'),
    }
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            // The service runs beside the site; the site's outbound proxy is
            // for the internet.
            .no_proxy()
            .build()
            .expect("persona style client")
    })
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum WireRequest<'a> {
    #[serde(rename_all = "camelCase")]
    Bust {
        schema: u32,
        name: &'a str,
        visual_profile: &'a Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        notes: Option<&'a str>,
    },
    BustEdit {
        schema: u32,
        notes: &'a str,
    },
    #[serde(rename_all = "camelCase")]
    FullBodyDesign {
        schema: u32,
        name: &'a str,
        visual_profile: &'a Value,
    },
    FullBodyFromBust {
        schema: u32,
    },
}

impl<'a> From<RecipeRequest<'a>> for WireRequest<'a> {
    fn from(request: RecipeRequest<'a>) -> Self {
        let schema = WIRE_SCHEMA;
        match request {
            RecipeRequest::Bust {
                name,
                visual_profile,
                notes,
            } => Self::Bust {
                schema,
                name,
                visual_profile,
                notes,
            },
            RecipeRequest::BustEdit { notes } => Self::BustEdit { schema, notes },
            RecipeRequest::FullBodyDesign {
                name,
                visual_profile,
            } => Self::FullBodyDesign {
                schema,
                name,
                visual_profile,
            },
            RecipeRequest::FullBodyFromBust => Self::FullBodyFromBust { schema },
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireRecipe {
    schema: u32,
    prompt: String,
    references: Vec<WireReference>,
    rendering: RenderingProvenance,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireReference {
    mime_type: String,
    sha256: String,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireInfo {
    schema: u32,
    rendering: RenderingProvenance,
}

impl RemoteRecipe {
    pub fn new(url: &str, token: &str) -> Result<Self, PersonaStyleError> {
        let url = sanitize_service_url(url)
            .filter(|url| !url.is_empty())
            .ok_or(PersonaStyleError::NotConfigured)?;
        let token = token.trim();
        if token.is_empty()
            || token.chars().count() > MAX_TOKEN_CHARS
            || !token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(PersonaStyleError::NotConfigured);
        }
        // `Url::join` replaces the last segment unless the base ends in `/`.
        let base = Url::parse(&format!("{url}/")).map_err(|_| PersonaStyleError::NotConfigured)?;
        Ok(Self {
            base,
            token: token.to_string(),
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url, PersonaStyleError> {
        self.base
            .join(path)
            .map_err(|_| PersonaStyleError::NotConfigured)
    }

    fn authorization(&self) -> Result<HeaderValue, PersonaStyleError> {
        let mut value = HeaderValue::from_str(&format!("Bearer {}", self.token))
            .map_err(|_| PersonaStyleError::NotConfigured)?;
        value.set_sensitive(true);
        Ok(value)
    }

    pub(super) async fn recipe(
        &self,
        request: RecipeRequest<'_>,
    ) -> Result<Recipe, PersonaStyleError> {
        let response = client()
            .post(self.endpoint("v1/recipe")?)
            .header(AUTHORIZATION, self.authorization()?)
            .timeout(RECIPE_TIMEOUT)
            .json(&WireRequest::from(request))
            .send()
            .await
            .map_err(transport_error)?;
        let body = read_answer(response).await?;
        let wire: WireRecipe = serde_json::from_slice(&body)
            .map_err(|_| PersonaStyleError::InvalidResponse("not a schema 1 recipe"))?;
        checked_recipe(wire, &request)
    }

    /// What a bust drawn now would record.
    pub async fn info(&self) -> Result<RenderingProvenance, PersonaStyleError> {
        let response = client()
            .get(self.endpoint("v1/info")?)
            .header(AUTHORIZATION, self.authorization()?)
            .timeout(INFO_TIMEOUT)
            .send()
            .await
            .map_err(transport_error)?;
        let body = read_answer(response).await?;
        let wire: WireInfo = serde_json::from_slice(&body)
            .map_err(|_| PersonaStyleError::InvalidResponse("not a schema 1 info"))?;
        if wire.schema != WIRE_SCHEMA {
            return Err(PersonaStyleError::InvalidResponse("unsupported schema"));
        }
        checked_rendering(wire.rendering)
    }
}

fn transport_error(error: reqwest::Error) -> PersonaStyleError {
    // The error's own text names the URL; only its kind is logged.
    tracing::warn!(
        timeout = error.is_timeout(),
        connect = error.is_connect(),
        "persona style service unreachable"
    );
    PersonaStyleError::Unavailable
}

async fn read_answer(response: reqwest::Response) -> Result<Vec<u8>, PersonaStyleError> {
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(
            status = status.as_u16(),
            "persona style service answered with an error"
        );
        return Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => PersonaStyleError::Unauthorized,
            StatusCode::BAD_REQUEST
            | StatusCode::PAYLOAD_TOO_LARGE
            | StatusCode::UNPROCESSABLE_ENTITY => PersonaStyleError::Rejected,
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                PersonaStyleError::Unavailable
            }
            _ => PersonaStyleError::InvalidResponse("unexpected status"),
        });
    }
    crate::services::outbound_security::read_limited_body(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|_| PersonaStyleError::InvalidResponse("answer too large or cut off"))
}

fn checked_rendering(
    rendering: RenderingProvenance,
) -> Result<RenderingProvenance, PersonaStyleError> {
    if rendering.recipe.is_none() {
        return Err(PersonaStyleError::InvalidResponse(
            "rendering does not name its recipe",
        ));
    }
    rendering
        .check()
        .map_err(|_| PersonaStyleError::InvalidResponse("rendering out of bounds"))?;
    Ok(rendering)
}

fn checked_recipe(
    wire: WireRecipe,
    request: &RecipeRequest<'_>,
) -> Result<Recipe, PersonaStyleError> {
    if wire.schema != WIRE_SCHEMA {
        return Err(PersonaStyleError::InvalidResponse("unsupported schema"));
    }
    let prompt = wire.prompt.trim();
    if prompt.is_empty()
        || prompt.chars().count() > MAX_PROMPT_CHARS
        || prompt
            .chars()
            .any(|character| character.is_control() && character != '\n')
    {
        return Err(PersonaStyleError::InvalidResponse("prompt out of bounds"));
    }
    let rendering = checked_rendering(wire.rendering)?;
    if wire.references.len() > MAX_REFERENCES {
        return Err(PersonaStyleError::InvalidResponse("too many references"));
    }
    if !request.draws_in_recipe_style() && !wire.references.is_empty() {
        // An edit or a redraw keeps the portrait it starts from, which the
        // site attaches itself.
        return Err(PersonaStyleError::InvalidResponse(
            "references for a picture that keeps its own",
        ));
    }
    let mut references = Vec::with_capacity(wire.references.len());
    let mut hashes = Vec::with_capacity(wire.references.len());
    for reference in wire.references {
        if reference.data.len() > MAX_REFERENCE_BYTES.div_ceil(3) * 4 {
            return Err(PersonaStyleError::InvalidResponse("reference too large"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(reference.data.as_bytes())
            .map_err(|_| PersonaStyleError::InvalidResponse("reference is not base64"))?;
        if bytes.is_empty() || bytes.len() > MAX_REFERENCE_BYTES {
            return Err(PersonaStyleError::InvalidResponse("reference too large"));
        }
        let sha256 = hex::encode(Sha256::digest(&bytes));
        if sha256 != reference.sha256 {
            return Err(PersonaStyleError::InvalidResponse(
                "reference hash does not match",
            ));
        }
        let image = ImageReference::new(bytes, reference.mime_type)
            .map_err(|_| PersonaStyleError::InvalidResponse("reference is not an image"))?;
        references.push(image);
        hashes.push(sha256);
    }
    if request.draws_in_recipe_style() {
        // The contract records the first reference; it must be the one sent.
        if rendering.style_reference_sha256.as_ref() != hashes.first() {
            return Err(PersonaStyleError::InvalidResponse(
                "rendering does not record the reference sent",
            ));
        }
    }
    Ok(Recipe {
        prompt: prompt.to_string(),
        references,
        rendering,
        source: SourceKind::Remote,
    })
}
