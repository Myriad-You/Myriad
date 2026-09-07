//! Single auditable Docker API policy decision.
//!
//! [`classify_request`] is the only allow/deny matcher. Do not split those
//! arms across files; enforcement after a [`Decision`] belongs in `forward`.

use axum::http::{Method, Uri};
use bytes::Bytes;
use serde_json::Value;

use super::validate::{
    validate_container_create, validate_container_create_name, validate_container_rename,
    validate_image_pull, validate_image_tag,
};
use super::{strip_api_version, validate_identifier, GuardState};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Decision {
    Allow,
    ProjectContainer(String),
    ProjectNetworkMutation {
        network: String,
        container: String,
        endpoint: Option<Value>,
    },
}

pub(crate) fn classify_request(
    state: &GuardState,
    method: &Method,
    uri: &Uri,
    body: &Bytes,
) -> std::result::Result<Decision, String> {
    let path = strip_api_version(uri.path());
    let segments = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if matches!((method, path), (&Method::GET | &Method::HEAD, "/_ping"))
        || (*method == Method::GET && matches!(path, "/version" | "/info" | "/events"))
    {
        return Ok(Decision::Allow);
    }
    // Docker accepts slashes inside the image-name path parameter, and Bollard sends them
    // unescaped (`/images/docker.io/org/image:tag/tag`). Match the bounded prefix/suffix and
    // still enforce the source and target repository allowlists below.
    if *method == Method::POST {
        if let Some(source) = path
            .strip_prefix("/images/")
            .and_then(|value| value.strip_suffix("/tag"))
            .filter(|value| !value.is_empty())
        {
            validate_image_tag(state, source, uri)?;
            return Ok(Decision::Allow);
        }
    }

    match segments.as_slice() {
        ["containers", "json"] if *method == Method::GET => Ok(Decision::Allow),
        ["containers", "create"] if *method == Method::POST => {
            validate_container_create_name(uri)?;
            validate_container_create(state, body)?;
            Ok(Decision::Allow)
        }
        ["containers", id] if *method == Method::DELETE => {
            validate_identifier(id)?;
            Ok(Decision::ProjectContainer((*id).to_string()))
        }
        ["containers", id, "rename"] if *method == Method::POST => {
            validate_identifier(id)?;
            validate_container_rename(state, uri)?;
            Ok(Decision::ProjectContainer((*id).to_string()))
        }
        ["containers", id, action]
            if (*method == Method::GET
                && matches!(*action, "json" | "logs" | "stats" | "top" | "changes"))
                || (*method == Method::POST
                    && matches!(*action, "start" | "stop" | "restart" | "kill" | "wait")) =>
        {
            validate_identifier(id)?;
            Ok(Decision::ProjectContainer((*id).to_string()))
        }
        ["images", "create"] if *method == Method::POST => {
            validate_image_pull(state, uri, body)?;
            Ok(Decision::Allow)
        }
        ["images", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["distribution", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["networks", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["networks", network, action]
            if *method == Method::POST && matches!(*action, "connect" | "disconnect") =>
        {
            validate_identifier(network)?;
            let value: Value = serde_json::from_slice(body)
                .map_err(|_| "network mutation body must be JSON".to_string())?;
            let container = value
                .get("Container")
                .and_then(Value::as_str)
                .ok_or_else(|| "network mutation is missing Container".to_string())?;
            validate_identifier(container)?;
            Ok(Decision::ProjectNetworkMutation {
                network: (*network).to_string(),
                container: container.to_string(),
                endpoint: value.get("EndpointConfig").cloned(),
            })
        }
        ["volumes", ..] if *method == Method::GET => Ok(Decision::Allow),
        ["system", ..] if *method == Method::GET => Ok(Decision::Allow),
        _ => Err(format!(
            "Docker API operation is not allowed: {method} {path}"
        )),
    }
}
