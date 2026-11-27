//! A recipe service over HTTP, wire schema 1.
//!
//! `GET {url}/v1/recipe`, `Authorization: Bearer {token}`:
//!
//! ```text
//! {schema: 1,
//!  recipe: {id, visualSchoolVersion, fullBodyInstructionVersion,
//!           style: {school, referenceRole?, priority, editFinish, redrawFinish}},
//!  references: [{mimeType, sha256, data}],        // at most one
//!  rendering: {recipe, visualSchoolVersion, styleReferenceSha256?,
//!              fullBodyInstructionVersion}}
//! ```
//!
//! `GET {url}/v1/info` (same token) answers `{schema: 1, recipe: {id,
//! visualSchoolVersion, fullBodyInstructionVersion}, rendering}`. `data` is
//! standard base64. Nothing about the design is sent.
//!
//! Everything the service sends is bounded and checked before anything is
//! drawn: the style passes core's `StyleRecipe::check`; at most one
//! reference of at most 8 MiB whose hash matches and which is an image; a
//! reference role exactly when there is a reference; the provenance passes
//! core's `RenderingProvenance::check` (so it is not `builtin`) and equals
//! what the recipe head and the reference say.
//!
//! Where the request goes: the host is resolved first and refused if any
//! address is link-local, unspecified or multicast. A host that resolves
//! only to loopback or private addresses (or a single-label name) is
//! reached directly at the addresses just checked; any other host is
//! reached through the site's outbound proxy when one is configured, and
//! otherwise directly at the checked addresses. Plain `http` is only for
//! the first kind. Redirects are not followed, so the token only goes to
//! the configured origin.

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use base64::Engine as _;
use myriad_merope::{RenderingProvenance, StyleRecipe};
use reqwest::{
    StatusCode,
    header::{AUTHORIZATION, HeaderValue},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::{Host, Url};

use super::{PersonaStyleError, Recipe, SourceKind};
use crate::services::image_generation::ImageReference;

pub(super) const WIRE_SCHEMA: u32 = 1;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RECIPE_TIMEOUT: Duration = Duration::from_secs(30);
const INFO_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const MAX_REFERENCE_BYTES: usize = 8 * 1024 * 1024;
/// One reference in base64 and the words, with room to spare.
pub(super) const MAX_RESPONSE_BYTES: usize = 12 * 1024 * 1024;
const MAX_INFO_BYTES: usize = 16 * 1024;
const MAX_URL_CHARS: usize = 512;
const MAX_TOKEN_CHARS: usize = 512;

/// A configured recipe service. `Debug` never shows the token.
#[derive(Clone)]
pub struct RemoteRecipe {
    base: Url,
    origin: String,
    token: String,
}

impl std::fmt::Debug for RemoteRecipe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteRecipe")
            .field("origin", &self.origin)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Link-local (cloud metadata lives there), unspecified and multicast
/// addresses are never a recipe service.
fn refused_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_link_local() || ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast()
        }
        IpAddr::V6(ip) => {
            ip.is_unicast_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|ip| refused_address(IpAddr::V4(ip)))
        }
    }
}

/// Loopback or private: the service runs beside the site.
fn local_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|ip| local_address(IpAddr::V4(ip)))
        }
    }
}

fn single_label(name: &str) -> bool {
    name.eq_ignore_ascii_case("localhost") || !name.contains('.')
}

/// The policy for the recipe service URL, used when it is saved, restored
/// from a backup, and read back: `https`, or `http` only to a loopback or
/// private address or a single-label host; never a link-local,
/// unspecified or multicast address; no credentials, query or fragment.
/// Stored without a trailing `/`; empty clears it.
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
    let local = match url.host()? {
        Host::Ipv4(ip) => {
            let ip = IpAddr::V4(ip);
            if refused_address(ip) {
                return None;
            }
            local_address(ip)
        }
        Host::Ipv6(ip) => {
            let ip = IpAddr::V6(ip);
            if refused_address(ip) {
                return None;
            }
            local_address(ip)
        }
        Host::Domain(name) => single_label(name),
    };
    match url.scheme() {
        "https" => {}
        "http" if local => {}
        _ => return None,
    }
    Some(url.as_str().trim_end_matches('/').to_string())
}

/// How a request reaches the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// Straight to the addresses just resolved and checked.
    Direct,
    /// Through the site's outbound proxy.
    Proxy,
}

/// The route for a service whose host resolved to `addresses`: refused if
/// any address is link-local, unspecified, multicast or broadcast (or none
/// came back); direct when the service is beside the site (every address
/// loopback or private, or a single-label host name); otherwise through the
/// site's outbound proxy when one is configured, else direct. Plain `http`
/// only for a service beside the site.
pub(super) fn route(
    scheme: &str,
    single_label_host: bool,
    addresses: &[IpAddr],
    proxy_configured: bool,
) -> Result<Route, PersonaStyleError> {
    if addresses.is_empty() || addresses.iter().any(|ip| refused_address(*ip)) {
        return Err(PersonaStyleError::InvalidUrl);
    }
    let beside = single_label_host || addresses.iter().all(|ip| local_address(*ip));
    if scheme == "http" && !beside {
        return Err(PersonaStyleError::InvalidUrl);
    }
    Ok(if !beside && proxy_configured {
        Route::Proxy
    } else {
        Route::Direct
    })
}

/// `scheme://host:port` of a URL, the unit a token is bound to.
pub fn origin_of(url: &str) -> Option<String> {
    let url = Url::parse(url.trim()).ok()?;
    let origin = url.origin();
    origin
        .is_tuple()
        .then(|| origin.ascii_serialization())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireHead {
    id: String,
    visual_school_version: String,
    full_body_instruction_version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireRecipeHead {
    id: String,
    visual_school_version: String,
    full_body_instruction_version: String,
    style: StyleRecipe,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireRecipe {
    schema: u32,
    recipe: WireRecipeHead,
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
    recipe: WireHead,
    rendering: RenderingProvenance,
}

impl RemoteRecipe {
    pub fn new(url: &str, token: &str) -> Result<Self, PersonaStyleError> {
        let url = sanitize_service_url(url)
            .filter(|url| !url.is_empty())
            .ok_or(PersonaStyleError::InvalidUrl)?;
        let token = token.trim();
        if token.is_empty()
            || token.chars().count() > MAX_TOKEN_CHARS
            || !token.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(PersonaStyleError::NotConfigured);
        }
        let origin = origin_of(&url).ok_or(PersonaStyleError::InvalidUrl)?;
        // `Url::join` replaces the last segment unless the base ends in `/`.
        let base = Url::parse(&format!("{url}/")).map_err(|_| PersonaStyleError::InvalidUrl)?;
        Ok(Self {
            base,
            origin,
            token: token.to_string(),
        })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// A client for this origin, after the host has been resolved and
    /// checked (see the module notes).
    async fn client(&self) -> Result<reqwest::Client, PersonaStyleError> {
        let host = self
            .base
            .host_str()
            .ok_or(PersonaStyleError::InvalidUrl)?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = self
            .base
            .port_or_known_default()
            .ok_or(PersonaStyleError::InvalidUrl)?;
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|_| PersonaStyleError::Unavailable)?
            .collect();
        let ips: Vec<IpAddr> = addresses.iter().map(SocketAddr::ip).collect();
        let single_label_host = matches!(self.base.host(), Some(Host::Domain(name)) if single_label(name));
        let proxy = crate::services::http_client::ProxyConfig::from_dynamic_config().await;
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT);
        let builder = match route(
            self.base.scheme(),
            single_label_host,
            &ips,
            proxy.should_use_proxy(),
        )? {
            Route::Proxy => crate::services::http_client::apply_proxy(builder, &proxy).map_err(
                |_| {
                    tracing::error!("persona style: the site's outbound proxy is unusable");
                    PersonaStyleError::Unavailable
                },
            )?,
            Route::Direct => builder.no_proxy().resolve_to_addrs(&host, &addresses),
        };
        builder.build().map_err(|_| PersonaStyleError::Unavailable)
    }

    fn authorization(&self) -> Result<HeaderValue, PersonaStyleError> {
        let mut value = HeaderValue::from_str(&format!("Bearer {}", self.token))
            .map_err(|_| PersonaStyleError::NotConfigured)?;
        value.set_sensitive(true);
        Ok(value)
    }

    async fn get(
        &self,
        path: &str,
        timeout: Duration,
        cap: usize,
    ) -> Result<Vec<u8>, PersonaStyleError> {
        let endpoint = self
            .base
            .join(path)
            .map_err(|_| PersonaStyleError::InvalidUrl)?;
        let response = self
            .client()
            .await?
            .get(endpoint)
            .header(AUTHORIZATION, self.authorization()?)
            .timeout(timeout)
            .send()
            .await
            .map_err(transport_error)?;
        read_answer(response, cap).await
    }

    pub(super) async fn recipe(&self) -> Result<Recipe, PersonaStyleError> {
        let body = self
            .get("v1/recipe", RECIPE_TIMEOUT, MAX_RESPONSE_BYTES)
            .await?;
        let wire: WireRecipe = serde_json::from_slice(&body)
            .map_err(|_| PersonaStyleError::InvalidResponse("not a schema 1 recipe"))?;
        checked_recipe(wire)
    }

    /// What a portrait drawn now would record.
    pub async fn info(&self) -> Result<RenderingProvenance, PersonaStyleError> {
        let body = self.get("v1/info", INFO_TIMEOUT, MAX_INFO_BYTES).await?;
        let wire: WireInfo = serde_json::from_slice(&body)
            .map_err(|_| PersonaStyleError::InvalidResponse("not a schema 1 info"))?;
        if wire.schema != WIRE_SCHEMA {
            return Err(PersonaStyleError::InvalidResponse("unsupported schema"));
        }
        let rendering = checked_rendering(wire.rendering)?;
        if rendering.recipe.as_deref() != Some(wire.recipe.id.as_str())
            || rendering.visual_school_version != wire.recipe.visual_school_version
            || rendering.full_body_instruction_version != wire.recipe.full_body_instruction_version
        {
            return Err(PersonaStyleError::InvalidResponse(
                "rendering does not match the recipe",
            ));
        }
        Ok(rendering)
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

async fn read_answer(
    response: reqwest::Response,
    cap: usize,
) -> Result<Vec<u8>, PersonaStyleError> {
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(
            status = status.as_u16(),
            "persona style service answered with an error"
        );
        return Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => PersonaStyleError::Unauthorized,
            status if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS => {
                PersonaStyleError::Unavailable
            }
            _ => PersonaStyleError::InvalidResponse("unexpected status"),
        });
    }
    crate::services::outbound_security::read_limited_body(response, cap)
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

fn checked_recipe(wire: WireRecipe) -> Result<Recipe, PersonaStyleError> {
    if wire.schema != WIRE_SCHEMA {
        return Err(PersonaStyleError::InvalidResponse("unsupported schema"));
    }
    let style = wire.recipe.style;
    style
        .check()
        .map_err(|_| PersonaStyleError::InvalidResponse("style out of bounds"))?;
    if wire.references.len() > 1 {
        return Err(PersonaStyleError::InvalidResponse("more than one reference"));
    }
    let mut reference = None;
    let mut reference_sha256 = None;
    if let Some(sent) = wire.references.into_iter().next() {
        if sent.data.len() > MAX_REFERENCE_BYTES.div_ceil(3) * 4 {
            return Err(PersonaStyleError::InvalidResponse("reference too large"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(sent.data.as_bytes())
            .map_err(|_| PersonaStyleError::InvalidResponse("reference is not base64"))?;
        if bytes.is_empty() || bytes.len() > MAX_REFERENCE_BYTES {
            return Err(PersonaStyleError::InvalidResponse("reference too large"));
        }
        let sha256 = hex::encode(Sha256::digest(&bytes));
        if sha256 != sent.sha256 {
            return Err(PersonaStyleError::InvalidResponse(
                "reference hash does not match",
            ));
        }
        reference = Some(
            ImageReference::new(bytes, sent.mime_type)
                .map_err(|_| PersonaStyleError::InvalidResponse("reference is not an image"))?,
        );
        reference_sha256 = Some(sha256);
    }
    if style.reference_role.is_some() != reference.is_some() {
        return Err(PersonaStyleError::InvalidResponse(
            "reference role without a reference, or a reference without a role",
        ));
    }
    let rendering = checked_rendering(wire.rendering)?;
    let expected = RenderingProvenance {
        recipe: Some(wire.recipe.id),
        visual_school_version: wire.recipe.visual_school_version,
        style_reference_sha256: reference_sha256,
        full_body_instruction_version: wire.recipe.full_body_instruction_version,
    };
    if rendering != expected {
        return Err(PersonaStyleError::InvalidResponse(
            "rendering does not match the recipe and its reference",
        ));
    }
    Ok(Recipe {
        style,
        reference,
        rendering,
        source: SourceKind::Remote,
    })
}
