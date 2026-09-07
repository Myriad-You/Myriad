//! Create/rename/mount/bind/image/network validators used by policy.

use std::path::Path;

use axum::http::Uri;
use bytes::Bytes;
use serde_json::{json, Value};

use super::{
    validate_identifier, GuardConfig, GuardState, SELF_UPDATE_EXHAUSTED_NAME,
    SELF_UPDATE_HELPER_NAME, SELF_UPDATE_RECOVERY_NAME, TRUSTED_UPDATER_REPOSITORY,
};

pub(crate) fn validate_container_create_name(uri: &Uri) -> std::result::Result<(), String> {
    let Some(name) = query_param(uri, "name") else {
        return Ok(());
    };
    let name = name.trim_start_matches('/');
    validate_identifier(name)?;
    if matches!(
        name,
        SELF_UPDATE_HELPER_NAME
            | SELF_UPDATE_RECOVERY_NAME
            | SELF_UPDATE_EXHAUSTED_NAME
            | "myriad-docker-guard"
            | "myriad-updater"
            | "myriad-updater-gateway"
    ) {
        return Err("container name is reserved for the trusted updater control plane".into());
    }
    Ok(())
}

pub(crate) fn validate_container_rename(state: &GuardState, uri: &Uri) -> std::result::Result<(), String> {
    let requested =
        query_param(uri, "name").ok_or_else(|| "container rename is missing name".to_string())?;
    let requested = requested.trim_start_matches('/');
    validate_identifier(requested)?;

    // Compose temporarily renames the old container to `<12 hex>_<original>` while replacing
    // it. Keep that one lifecycle operation, but do not expose arbitrary Docker renames.
    let base = requested
        .split_once('_')
        .filter(|(prefix, _)| prefix.len() == 12 && prefix.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|(_, base)| base)
        .unwrap_or(requested);
    let allowed = ["backend", "frontend", "postgres", "proxy", "updater"]
        .into_iter()
        .any(|service| {
            base == format!("{}-{service}-1", state.config.project)
                || base == format!("{}_{service}_1", state.config.project)
                || base == format!("{}-{service}", state.config.project)
                || base == format!("myriad-{service}")
        });
    if !allowed {
        return Err("container rename target is outside the Compose lifecycle allowlist".into());
    }
    Ok(())
}

pub(crate) fn validate_container_create(state: &GuardState, body: &Bytes) -> std::result::Result<(), String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| "containers/create body must be valid JSON".to_string())?;
    let labels = value
        .get("Labels")
        .and_then(Value::as_object)
        .ok_or_else(|| "managed container requires Compose labels".to_string())?;
    let project = labels
        .get("com.docker.compose.project")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if project != state.config.project {
        return Err("container project label is not allowed".into());
    }
    let service = labels
        .get("com.docker.compose.service")
        .and_then(Value::as_str)
        .ok_or_else(|| "container is missing Compose service label".to_string())?;
    let expected_repository =
        state.config.service_images.get(service).ok_or_else(|| {
            "Compose service is not managed by the generic updater API".to_string()
        })?;

    let image = value
        .get("Image")
        .and_then(Value::as_str)
        .ok_or_else(|| "container image is missing".to_string())?;
    if normalize_repository(image) != *expected_repository {
        return Err("container image does not match the fixed service repository".into());
    }
    if nonempty(value.get("Entrypoint")) || nonempty(value.get("Cmd")) {
        return Err("command or entrypoint overrides are not allowed".into());
    }

    let host = value
        .get("HostConfig")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let narrow_volume_init = is_narrow_backend_volume_init(&value, &host, service);
    if service == "backend-volume-init" && !narrow_volume_init {
        return Err("backend-volume-init is allowed only in the narrow root init mode".into());
    }
    if requests_root_user(&value) && !narrow_volume_init {
        return Err("explicit root user is allowed only for the backend volume initializer".into());
    }
    reject_true(&host, "Privileged")?;
    reject_true(&host, "PublishAllPorts")?;
    // PortBindings are forbidden for internal services. Proxy is the edge entry and must
    // publish host ports (80/443); validate those bindings separately below.
    let mut host_forbidden = vec![
        "CapAdd",
        "Devices",
        "DeviceRequests",
        "VolumesFrom",
        "Sysctls",
        "ContainerIDFile",
        "Runtime",
        "VolumeDriver",
        "CgroupParent",
        "Isolation",
        "Annotations",
        "Cgroup",
        "StorageOpt",
        "Links",
    ];
    if service != "proxy" {
        host_forbidden.push("PortBindings");
    }
    reject_nonempty_fields(&host, &host_forbidden)?;
    if let Some(log_config) = host.get("LogConfig").filter(|value| nonempty(Some(value))) {
        // Compose v5 sends a zero-value LogConfig (Type="") when the service has
        // no `logging:` block. The engine then applies the daemon default
        // (json-file). Rejecting the empty type blocked `compose run`.
        let log_type = log_config
            .get("Type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let type_ok = log_type.is_empty() || log_type == "json-file";
        let config_is_safe = log_config
            .get("Config")
            .and_then(Value::as_object)
            .is_none_or(|config| {
                config
                    .keys()
                    .all(|key| matches!(key.as_str(), "max-size" | "max-file"))
            });
        if !type_ok || !config_is_safe {
            return Err("HostConfig.LogConfig is outside the json-file allowlist".into());
        }
    }
    if service == "proxy" {
        authorize_proxy_port_bindings(host.get("PortBindings"))?;
    }
    for field in [
        "PidMode",
        "IpcMode",
        "UTSMode",
        "UsernsMode",
        "CgroupnsMode",
    ] {
        if nonempty(host.get(field)) {
            return Err(format!("HostConfig.{field} is not allowed"));
        }
    }
    if let Some(mode) = host.get("NetworkMode").and_then(Value::as_str) {
        if !mode.is_empty() && !matches!(mode, "default" | "bridge" | "none") {
            if mode != state.config.compose_network
                && mode != state.config.admin_network
                && mode != state.config.guard_network
            {
                return Err("host or foreign network mode is not allowed".into());
            }
            authorize_guard_network_attachment(service, mode, &state.config)?;
        }
    }
    if let Some(options) = host.get("SecurityOpt").and_then(Value::as_array) {
        if options.iter().any(|v| {
            !v.as_str()
                .is_some_and(|s| s == "no-new-privileges" || s == "no-new-privileges:true")
        }) {
            return Err("only no-new-privileges SecurityOpt is allowed".into());
        }
    }

    for bind in host
        .get("Binds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let bind = bind
            .as_str()
            .ok_or_else(|| "HostConfig.Binds must contain strings".to_string())?;
        validate_bind(state, service, bind)?;
    }
    for mount in host
        .get("Mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        validate_mount(state, service, mount)?;
    }
    for mount in value
        .get("Mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        validate_mount(state, service, mount)?;
    }

    if let Some(endpoints) = value
        .pointer("/NetworkingConfig/EndpointsConfig")
        .and_then(Value::as_object)
    {
        for (network, endpoint) in endpoints {
            if !is_allowlisted_network_name(network, &state.config) {
                return Err(format!("network {network} is outside the Myriad allowlist"));
            }
            authorize_guard_network_attachment(service, network, &state.config)?;
            validate_endpoint_settings(service, endpoint, &state.config)?;
        }
    }
    Ok(())
}

pub(crate) fn validate_endpoint_settings(
    service: &str,
    endpoint: &Value,
    config: &GuardConfig,
) -> std::result::Result<(), String> {
    let Some(object) = endpoint.as_object() else {
        return Err("network endpoint settings must be an object".into());
    };
    for (key, value) in object {
        if key != "Aliases" && nonempty(Some(value)) {
            return Err(format!("network endpoint field {key} is not allowed"));
        }
    }
    if let Some(aliases) = object.get("Aliases").and_then(Value::as_array) {
        let service_alias = service;
        let fixed_name = format!("{}-{service}", config.project);
        let generated_name = format!("{}-{service}-1", config.project);
        let legacy_generated_name = format!("{}_{service}_1", config.project);
        if aliases.iter().any(|alias| {
            !alias.as_str().is_some_and(|alias| {
                alias == service_alias
                    || alias == fixed_name
                    || alias == generated_name
                    || alias == legacy_generated_name
            })
        }) {
            return Err(
                "network aliases are outside the Compose service identity allowlist".into(),
            );
        }
    }
    Ok(())
}

fn requests_root_user(value: &Value) -> bool {
    value
        .get("User")
        .and_then(Value::as_str)
        .and_then(|user| user.split(':').next())
        .is_some_and(|user| matches!(user, "0" | "root"))
}

fn has_exact_env(value: &Value, expected: &str) -> bool {
    let key = expected
        .split_once('=')
        .map(|(key, _)| key)
        .unwrap_or(expected);
    let prefix = format!("{key}=");
    let mut matches = value
        .get("Env")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|entry| entry.starts_with(&prefix));
    matches.next() == Some(expected) && matches.next().is_none()
}

fn has_mount_target(value: &Value, host: &Value, expected: &str) -> bool {
    let bind_has_target = host
        .get("Binds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|bind| bind.split(':').nth(1) == Some(expected));
    let structured_has_target = [host.get("Mounts"), value.get("Mounts")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .any(|mount| {
            mount
                .get("Target")
                .or_else(|| mount.get("Destination"))
                .and_then(Value::as_str)
                == Some(expected)
        });
    bind_has_target || structured_has_target
}

/// The updater may run one disposable `backend-volume-init` container as root
/// solely to repair the two backend named volumes. Keep this exception narrower
/// than normal Compose service creation: dedicated service label, exact uid,
/// exact mode flag, one-off label, no network, no-new-privileges, and both
/// already-allowlisted volume targets are required.
fn is_narrow_backend_volume_init(value: &Value, host: &Value, service: &str) -> bool {
    service == "backend-volume-init"
        && value.get("User").and_then(Value::as_str) == Some("0:0")
        && has_exact_env(value, "MYRIAD_VOLUME_INIT_ONLY=true")
        && value
            .pointer("/Labels/com.docker.compose.oneoff")
            .and_then(Value::as_str)
            == Some("True")
        && host.get("AutoRemove").and_then(Value::as_bool) != Some(true)
        && host
            .pointer("/RestartPolicy/Name")
            .and_then(Value::as_str)
            .is_none_or(|name| name.is_empty() || name == "no")
        && host
            .get("SecurityOpt")
            .and_then(Value::as_array)
            .is_some_and(|options| {
                options.iter().any(|option| {
                    option.as_str().is_some_and(|option| {
                        matches!(option, "no-new-privileges" | "no-new-privileges:true")
                    })
                })
            })
        && host.get("NetworkMode").and_then(Value::as_str) == Some("none")
        && value
            .pointer("/NetworkingConfig/EndpointsConfig")
            .and_then(Value::as_object)
            .is_none_or(serde_json::Map::is_empty)
        && has_mount_target(value, host, "/app/cache")
        && has_mount_target(value, host, "/app/data")
}

fn is_allowlisted_network_name(name: &str, config: &GuardConfig) -> bool {
    // Keep in sync with `crate::docker::network_allowlist::NetworkAllowlist::contains`
    // (same three env-backed names). Preflight rejects updates before stop/snapshot when
    // compose would attach managed services outside this set.
    let name = name.trim_start_matches('/');
    name == config.compose_network || name == config.admin_network || name == config.guard_network
}

/// Exact service-to-network topology. In particular, a compromised updater may
/// not attach itself to the business network to reach Postgres/frontend peers.
pub(crate) fn authorize_guard_network_attachment(
    service: &str,
    network_name: &str,
    config: &GuardConfig,
) -> std::result::Result<(), String> {
    let allowed = match service {
        "postgres" | "frontend" => network_name == config.compose_network,
        "backend" | "proxy" => {
            network_name == config.compose_network || network_name == config.admin_network
        }
        "updater" => network_name == config.admin_network || network_name == config.guard_network,
        "backend-volume-init" => false,
        _ => false,
    };
    if !allowed {
        if network_name == config.guard_network && service != "updater" {
            return Err("only the updater service may attach to the docker-guard network".into());
        }
        return Err(format!(
            "service {service} may not attach to network {network_name}"
        ));
    }
    Ok(())
}

fn validate_bind(state: &GuardState, service: &str, bind: &str) -> std::result::Result<(), String> {
    let parts = bind.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return Err("invalid bind syntax".into());
    }
    let options = parts.get(2).copied().unwrap_or("");
    if !options.is_empty() {
        validate_mount_options(options)?;
    }
    validate_mount_pair(
        state,
        service,
        parts[0],
        parts[1],
        Path::new(parts[0]).is_absolute(),
        options.split(',').any(|option| option == "ro"),
    )
}

fn validate_mount(
    state: &GuardState,
    service: &str,
    mount: &Value,
) -> std::result::Result<(), String> {
    let kind = mount
        .get("Type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(kind, "bind" | "volume") {
        return Err("only bind and volume mounts are allowed".into());
    }
    if kind == "bind" {
        if let Some(propagation) = mount
            .pointer("/BindOptions/Propagation")
            .and_then(Value::as_str)
        {
            if !matches!(propagation, "" | "private" | "rprivate") {
                return Err("bind mount propagation is not allowed".into());
            }
        }
    }
    if kind == "volume"
        && (nonempty(mount.pointer("/VolumeOptions/DriverConfig"))
            || nonempty(mount.pointer("/VolumeOptions/Subpath")))
    {
        return Err("volume driver configuration or subpath is not allowed".into());
    }
    let source = mount
        .get("Source")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let target = mount
        .get("Target")
        .or_else(|| mount.get("Destination"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let read_only = mount
        .get("ReadOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    validate_mount_pair(state, service, source, target, kind == "bind", read_only)
}

fn validate_mount_pair(
    state: &GuardState,
    service: &str,
    source: &str,
    target: &str,
    host_bind: bool,
    read_only: bool,
) -> std::result::Result<(), String> {
    let root = state.host_compose_root.as_path();
    let source_path = Path::new(source);
    let exact_host_pair = |relative: &str, expected_target: &str| {
        source_path == root.join(relative) && target == expected_target
    };
    match service {
        "frontend" => Err("frontend container may not add mounts".into()),
        "backend" | "backend-volume-init" => {
            if host_bind || source_path.is_absolute() {
                return Err("backend host bind mounts are forbidden".into());
            }
            let allowed_name = source == format!("{}_backend_cache", state.config.project)
                || source == format!("{}_backend_data", state.config.project);
            if !allowed_name || !matches!(target, "/app/cache" | "/app/data") {
                return Err("backend named volume is outside the allowlist".into());
            }
            Ok(())
        }
        "postgres" => {
            if !host_bind || !exact_host_pair("pgdata", "/var/lib/postgresql") {
                return Err("postgres may only bind the project pgdata directory".into());
            }
            validate_visible_host_directory(state, "pgdata")
        }
        "updater" => {
            if !host_bind {
                return Err("updater mounts must be fixed host bind paths".into());
            }
            if exact_host_pair("", "/host/compose") {
                // Writable root is required for v0.3.37 SwapTag (sibling .env
                // tmp/bak). Official RO+file-bind stacks still match this pair.
                return validate_visible_host_directory(state, "");
            }
            if exact_host_pair(".env", "/host/compose/.env") {
                return validate_visible_host_path(state, ".env", false);
            }
            if exact_host_pair("state", "/host/compose/state") {
                return validate_visible_host_directory(state, "state");
            }
            if exact_host_pair("pgdata", "/host/compose/pgdata") {
                return validate_visible_host_directory(state, "pgdata");
            }
            if exact_host_pair("guard-policy", "/run/secrets") {
                if !read_only {
                    return Err("host Guard policy must be mounted read-only".into());
                }
                return validate_visible_host_directory(state, "guard-policy");
            }
            Err("updater host bind is outside the fixed deployment allowlist".into())
        }
        "proxy" => {
            // Proxy only needs the maintenance state file (read-only).
            if !host_bind || !exact_host_pair("state", "/state") {
                return Err("proxy may only bind the project state directory at /state".into());
            }
            validate_visible_host_directory(state, "state")
        }
        "docker-guard" => {
            if !host_bind {
                return Err("docker-guard mounts must be fixed host bind paths".into());
            }
            if source == "/var/run/docker.sock" && target == "/var/run/docker.sock" {
                return Ok(());
            }
            if exact_host_pair("", "/host/compose") {
                if !read_only {
                    return Err("docker-guard deployment root must be mounted read-only".into());
                }
                return validate_visible_host_directory(state, "");
            }
            if exact_host_pair("state", "/host/state") {
                return validate_visible_host_directory(state, "state");
            }
            if exact_host_pair("guard-policy", "/guard-policy") {
                return validate_visible_host_directory(state, "guard-policy");
            }
            Err("docker-guard host bind is outside the fixed deployment allowlist".into())
        }
        _ => Err("service mount policy is undefined".into()),
    }
}

/// Proxy may only publish container ports 80 and/or 443 to the host.
fn authorize_proxy_port_bindings(bindings: Option<&Value>) -> std::result::Result<(), String> {
    let Some(obj) = bindings.and_then(Value::as_object) else {
        return Ok(());
    };
    if obj.is_empty() {
        return Ok(());
    }
    for key in obj.keys() {
        // Docker API keys look like "80/tcp" or "443/tcp".
        let port = key.split('/').next().unwrap_or(key);
        if port != "80" && port != "443" {
            return Err(format!(
                "proxy may only publish ports 80/443, got HostConfig.PortBindings key {key}"
            ));
        }
    }
    Ok(())
}

fn validate_mount_options(options: &str) -> std::result::Result<(), String> {
    for option in options.split(',').filter(|value| !value.is_empty()) {
        if matches!(
            option,
            "shared" | "rshared" | "slave" | "rslave" | "unbindable" | "runbindable"
        ) {
            return Err("bind mount propagation is not allowed".into());
        }
        if !matches!(
            option,
            "ro" | "rw" | "z" | "Z" | "consistent" | "cached" | "delegated" | "nocopy"
        ) {
            return Err(format!("mount option {option} is not allowed"));
        }
    }
    Ok(())
}

fn validate_visible_host_directory(
    state: &GuardState,
    relative: &str,
) -> std::result::Result<(), String> {
    let mut visible = state.config.compose_dir.clone();
    let root_meta = std::fs::symlink_metadata(&visible)
        .map_err(|_| "deployment root is unavailable to docker guard".to_string())?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err("deployment root must be a real directory, not a symlink".into());
    }

    for component in Path::new(relative).components() {
        let std::path::Component::Normal(component) = component else {
            return Err("host bind path contains an invalid component".into());
        };
        visible.push(component);
        let metadata = std::fs::symlink_metadata(&visible)
            .map_err(|_| "host bind source does not exist".to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("host bind source may not contain symbolic links".into());
        }
        if !metadata.is_dir() {
            return Err("host bind source must be a directory".into());
        }
    }
    Ok(())
}

fn validate_visible_host_path(
    state: &GuardState,
    relative: &str,
    directory: bool,
) -> std::result::Result<(), String> {
    let path = state.config.compose_dir.join(relative);
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| "host bind source does not exist".to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("host bind source may not contain symbolic links".into());
    }
    if metadata.is_dir() != directory {
        return Err(if directory {
            "host bind source must be a directory".into()
        } else {
            "host bind source must be a regular file".into()
        });
    }
    Ok(())
}

pub(crate) fn validate_image_pull(
    state: &GuardState,
    uri: &Uri,
    body: &Bytes,
) -> std::result::Result<(), String> {
    if !body.is_empty() {
        return Err("image pull request body must be empty".into());
    }
    let mut has_tag = false;
    for (name, value) in url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()) {
        match name.as_ref() {
            "fromImage" => {}
            "tag" => {
                has_tag = true;
                validate_pull_tag(&value)?;
            }
            // Bollard 0.21 always serializes `platform` (empty = let the engine
            // choose). A non-empty value pins an architecture. Import/build
            // selectors such as fromSrc/repo are never part of a registry pull.
            "platform" => {}
            _ => return Err(format!("image pull query parameter {name} is forbidden")),
        }
    }
    if !has_tag {
        return Err("images/create requires an explicit tag".into());
    }
    let image = query_param(uri, "fromImage")
        .ok_or_else(|| "images/create requires fromImage".to_string())?;
    let repository = normalize_repository(&image);
    if repository == TRUSTED_UPDATER_REPOSITORY {
        return Err("TCB image pulls require the dedicated self-update endpoint".into());
    }
    if state.config.allowed_images.contains(&repository) {
        Ok(())
    } else {
        Err("image pull repository is not allowlisted".into())
    }
}

fn validate_pull_tag(tag: &str) -> std::result::Result<(), String> {
    let tag = tag.trim();
    if tag.is_empty()
        || tag.len() > 128
        || tag.eq_ignore_ascii_case("latest")
        || !tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("image pull tag has unsafe syntax".into());
    }
    Ok(())
}

pub(crate) fn validate_image_tag(
    state: &GuardState,
    encoded_source: &str,
    uri: &Uri,
) -> std::result::Result<(), String> {
    let source = decode_path_segment(encoded_source);
    let repo = query_param(uri, "repo").ok_or_else(|| "image tag requires repo".to_string())?;
    let source_repository = normalize_repository(&source);
    if source_repository == TRUSTED_UPDATER_REPOSITORY {
        return Err("TCB image tagging is forbidden on the generic updater API".into());
    }
    if !state.config.allowed_images.contains(&source_repository) {
        return Err("image tag source repository is not allowlisted".into());
    }
    let target_repository = normalize_repository(&repo);
    if target_repository == TRUSTED_UPDATER_REPOSITORY {
        return Err("TCB image tagging is forbidden on the generic updater API".into());
    }
    if !state.config.allowed_images.contains(&target_repository) {
        return Err("image tag target repository is not allowlisted".into());
    }
    if source_repository != target_repository {
        return Err("cross-repository image tagging is forbidden".into());
    }
    Ok(())
}

/// Returns the Compose service name when the container is a managed project member.
pub(crate) fn managed_project_service(inspect: &Value, config: &GuardConfig) -> Option<String> {
    let project = inspect
        .pointer("/Config/Labels/com.docker.compose.project")
        .and_then(Value::as_str)?;
    let service = inspect
        .pointer("/Config/Labels/com.docker.compose.service")
        .and_then(Value::as_str)?;
    if project == config.project
        && matches!(
            service,
            "backend" | "backend-volume-init" | "frontend" | "postgres" | "proxy" | "updater"
        )
    {
        Some(service.to_string())
    } else {
        None
    }
}

pub(crate) fn allowlisted_network_name(inspect: &Value, config: &GuardConfig) -> Option<String> {
    let name = inspect.get("Name").and_then(Value::as_str)?;
    if is_allowlisted_network_name(name, config) {
        Some(name.to_string())
    } else {
        None
    }
}

fn normalize_repository(image: &str) -> String {
    let image = image.trim().split('@').next().unwrap_or(image.trim());
    let (prefix, last) = image.rsplit_once('/').unwrap_or(("", image));
    let name = last.split_once(':').map(|(name, _)| name).unwrap_or(last);
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

fn query_param(uri: &Uri, name: &str) -> Option<String> {
    url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn decode_path_segment(value: &str) -> String {
    let encoded = format!("value={value}");
    url::form_urlencoded::parse(encoded.as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default()
}

fn reject_true(value: &Value, field: &str) -> std::result::Result<(), String> {
    if value.get(field).and_then(Value::as_bool) == Some(true) {
        Err(format!("HostConfig.{field}=true is not allowed"))
    } else {
        Ok(())
    }
}

fn reject_nonempty_fields(value: &Value, fields: &[&str]) -> std::result::Result<(), String> {
    for field in fields {
        if nonempty(value.get(*field)) {
            return Err(format!("HostConfig.{field} is not allowed"));
        }
    }
    Ok(())
}

fn nonempty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(v)) => !v.is_empty(),
        Some(Value::Object(v)) => !v.is_empty(),
        Some(Value::Number(v)) => v.as_i64().unwrap_or_default() != 0,
        Some(Value::Bool(true)) => true,
    }
}
