//! Validation and bounded-resource rules for Tapp package manifests.

use super::{
    valid_agent_name, valid_event_topic, validate_storage_key, TappAiContextSource,
    TappAiOperation, TappAiOutputFormat, TappManifest, TappPermission, TappSettingDef,
    TappWidgetRefreshMode, TappWidgetRefreshPolicy,
};
use std::path::{Component, Path as FsPath};

pub(crate) const MAX_TAPP_ID_LEN: usize = 128;
pub(crate) const MAX_RESOURCE_PATH_LEN: usize = 256;
pub(crate) const MAX_TAPP_ARCHIVE_BYTES: usize = 25 * 1024 * 1024;
pub(crate) const MAX_TAPP_ARCHIVE_FILES: usize = 512;
pub(crate) const MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
pub(crate) const MAX_TAPP_RESOURCE_BYTES: u64 = 25 * 1024 * 1024;
/// Declared package assets (textures, audio, wasm, levels). Binary allowed.
pub(crate) const MAX_TAPP_ASSETS: usize = 64;
pub(crate) const MAX_TAPP_ASSET_BYTES: u64 = 5 * 1024 * 1024;
pub(crate) const MAX_TAPP_ASSETS_TOTAL_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const MAX_TAPP_MANIFEST_BYTES: u64 = 256 * 1024;
pub(crate) const MAX_WIDGETS_PER_TAPP: usize = 64;
pub(crate) const MAX_DATA_EXCHANGE_DECLARATIONS: usize = 32;
pub(crate) const MAX_DATA_EXCHANGE_ID_LEN: usize = 128;
pub(crate) const MAX_DATA_EXCHANGE_SCHEMA_BYTES: usize = 64 * 1024;
pub(crate) const MAX_DATA_EXCHANGE_RESPONSE_BYTES: usize = 512 * 1024;
pub(crate) const MAX_AGENT_SCHEMA_RESOURCE_BYTES: usize = 64 * 1024;
pub(crate) const MAX_TAPP_I18N_FILES: usize = 32;
pub(crate) const MAX_TAPP_I18N_RESOURCE_BYTES: usize = 1024 * 1024;

pub(crate) fn valid_data_exchange_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DATA_EXCHANGE_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub(crate) fn validate_inline_data_schema(schema: &serde_json::Value) -> Result<(), String> {
    let object = schema
        .as_object()
        .ok_or_else(|| "Data Exchange schema must be an inline JSON object".to_string())?;
    let encoded = serde_json::to_vec(schema)
        .map_err(|_| "Data Exchange schema cannot be serialized".to_string())?;
    if encoded.len() > MAX_DATA_EXCHANGE_SCHEMA_BYTES {
        return Err(format!(
            "Data Exchange schema is too large (max {MAX_DATA_EXCHANGE_SCHEMA_BYTES} bytes)"
        ));
    }

    fn reject_refs(value: &serde_json::Value, depth: usize) -> Result<(), String> {
        if depth > 32 {
            return Err("Data Exchange schema nesting is too deep".to_string());
        }
        match value {
            serde_json::Value::Object(map) => {
                if map.contains_key("$ref") {
                    return Err("Data Exchange schema does not support $ref".to_string());
                }
                for child in map.values() {
                    reject_refs(child, depth + 1)?;
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    reject_refs(child, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    reject_refs(schema, 0)?;
    if !object.contains_key("type")
        && !object.contains_key("properties")
        && !object.contains_key("enum")
        && !object.contains_key("const")
    {
        return Err(
            "Data Exchange schema must declare type, properties, enum, or const".to_string(),
        );
    }
    Ok(())
}

pub(crate) fn is_safe_path_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TAPP_ID_LEN
        && value != "."
        && value != ".."
        && !value.starts_with('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

pub(crate) fn validate_tapp_id(tapp_id: &str) -> Result<(), String> {
    if tapp_id.len() > MAX_TAPP_ID_LEN
        || !tapp_id
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
        || !is_safe_path_component(tapp_id)
    {
        return Err(
            "Invalid Tapp id: use 1-128 ASCII letters, numbers, dots, underscores, or hyphens"
                .to_string(),
        );
    }
    Ok(())
}

pub(crate) fn validate_resource_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.len() > MAX_RESOURCE_PATH_LEN
        || path.contains('\\')
        || FsPath::new(path).is_absolute()
    {
        return Err(format!("Invalid Tapp resource path: {path}"));
    }

    let mut saw_component = false;
    for component in FsPath::new(path).components() {
        match component {
            Component::Normal(value) => {
                let value = value
                    .to_str()
                    .ok_or_else(|| format!("Invalid Tapp resource path: {path}"))?;
                if !is_safe_path_component(value) {
                    return Err(format!("Invalid Tapp resource path: {path}"));
                }
                saw_component = true;
            }
            _ => return Err(format!("Invalid Tapp resource path: {path}")),
        }
    }

    if !saw_component {
        return Err(format!("Invalid Tapp resource path: {path}"));
    }
    Ok(())
}

pub(crate) fn is_valid_widget_size(size: &str) -> bool {
    matches!(
        size,
        "1x1" | "1x2" | "2x1" | "2x2" | "2x3" | "3x2" | "4x1" | "4x2" | "2x4" | "3x3" | "4x4"
    )
}

pub(crate) fn tapp_setting_value_is_valid(
    setting: &TappSettingDef,
    value: &serde_json::Value,
) -> bool {
    match setting.setting_type.as_str() {
        "toggle" => value.is_boolean(),
        "input" | "color" => value.is_string(),
        "select" => value.as_str().is_some_and(|value| {
            setting
                .options
                .as_ref()
                .is_some_and(|options| options.iter().any(|option| option.value == value))
        }),
        "number" => value.as_f64().is_some_and(|value| {
            value.is_finite()
                && setting.min.is_none_or(|min| value >= min)
                && setting.max.is_none_or(|max| value <= max)
        }),
        _ => false,
    }
}

pub(crate) fn validate_tapp_settings(
    settings: &[TappSettingDef],
    scope: &str,
) -> Result<(), String> {
    if settings.len() > 64 {
        return Err(format!("{scope} accepts at most 64 settings"));
    }
    let mut keys = std::collections::HashSet::new();
    for setting in settings {
        if validate_storage_key(&setting.key).is_err()
            || !keys.insert(setting.key.as_str())
            || setting.label.is_empty()
            || setting.label.len() > 255
            || !matches!(
                setting.setting_type.as_str(),
                "toggle" | "select" | "input" | "number" | "color"
            )
        {
            return Err(format!(
                "Invalid or duplicate {scope} setting: {}",
                setting.key
            ));
        }
        if setting.setting_type == "select"
            && setting
                .options
                .as_ref()
                .is_none_or(|options| options.is_empty() || options.len() > 100)
        {
            return Err(format!(
                "Select {scope} setting {} requires 1-100 options",
                setting.key
            ));
        }
        if let Some(options) = &setting.options {
            let mut values = std::collections::HashSet::new();
            if setting.setting_type != "select"
                || options.iter().any(|option| {
                    option.value.is_empty()
                        || option.value.len() > 255
                        || option.label.is_empty()
                        || option.label.len() > 255
                        || !values.insert(option.value.as_str())
                })
            {
                return Err(format!(
                    "Invalid options for {scope} setting: {}",
                    setting.key
                ));
            }
        }
        let has_numeric_constraints =
            setting.min.is_some() || setting.max.is_some() || setting.step.is_some();
        if (has_numeric_constraints && setting.setting_type != "number")
            || (setting.placeholder.is_some() && setting.setting_type != "input")
        {
            return Err(format!(
                "Incompatible fields for {scope} setting: {}",
                setting.key
            ));
        }
        if setting
            .min
            .zip(setting.max)
            .is_some_and(|(min, max)| min > max)
            || setting.step.is_some_and(|step| step <= 0.0)
        {
            return Err(format!(
                "Invalid numeric range for {scope} setting: {}",
                setting.key
            ));
        }
        if let Some(default) = &setting.default_value {
            if !tapp_setting_value_is_valid(setting, default) {
                return Err(format!(
                    "Invalid defaultValue for {scope} setting: {}",
                    setting.key
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_widget_refresh_policy(
    policy: &TappWidgetRefreshPolicy,
    widget_id: &str,
) -> Result<(), String> {
    match policy.mode {
        TappWidgetRefreshMode::Event if policy.interval_seconds.is_some() => Err(format!(
            "Event-driven Widget {widget_id} cannot declare intervalSeconds"
        )),
        TappWidgetRefreshMode::Event => Ok(()),
        TappWidgetRefreshMode::Interval
            if !matches!(policy.interval_seconds, Some(15..=86_400)) =>
        {
            Err(format!(
                "Interval Widget {widget_id} requires intervalSeconds between 15 and 86400"
            ))
        }
        TappWidgetRefreshMode::Interval => Ok(()),
    }
}

pub(crate) fn parse_system_version(value: &str) -> Result<semver::Version, String> {
    let normalized = value.strip_prefix('v').unwrap_or(value);
    semver::Version::parse(normalized)
        .map_err(|_| format!("Invalid minSystemVersion: {value}; expected semantic version"))
}

pub(crate) fn validate_http_url(value: &str, field: &str) -> Result<(), String> {
    if value.len() > 2_048 {
        return Err(format!("Tapp {field} is too long"));
    }
    let parsed = reqwest::Url::parse(value).map_err(|_| format!("Invalid Tapp {field}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("Tapp {field} must be an HTTP(S) URL"));
    }
    Ok(())
}

pub(crate) fn validate_resource_extension(
    path: &str,
    extension: &str,
    field: &str,
) -> Result<(), String> {
    if !path.ends_with(extension) {
        return Err(format!("Tapp {field} must reference a {extension} file"));
    }
    Ok(())
}

pub(crate) fn validate_asset_path(path: &str) -> Result<(), String> {
    validate_resource_path(path)?;
    if !path.starts_with("assets/") || path == "assets" || path.ends_with('/') {
        return Err(format!(
            "Tapp asset path must be a file under assets/: {path}"
        ));
    }
    // Reject nested path escape already handled by validate_resource_path.
    // Disallow treating runtime entrypoints as assets.
    if path.ends_with(".js") || path.ends_with(".html") {
        return Err(format!(
            "Tapp asset path must not be a script or HTML entry: {path}"
        ));
    }
    Ok(())
}

pub(crate) fn guess_asset_mime_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".wav") {
        "audio/wav"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".wasm") {
        "application/wasm"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".txt") || lower.ends_with(".csv") {
        "text/plain"
    } else if lower.ends_with(".bin") {
        "application/octet-stream"
    } else if lower.ends_with(".glb") {
        "model/gltf-binary"
    } else if lower.ends_with(".gltf") {
        "model/gltf+json"
    } else {
        "application/octet-stream"
    }
}

pub(crate) fn decode_asset_base64(value: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let trimmed = value.trim();
    // Allow optional data-URL prefix: data:<mime>;base64,<payload>
    let payload = trimmed
        .split_once("base64,")
        .map(|(_, data)| data)
        .unwrap_or(trimmed);
    STANDARD
        .decode(payload.trim())
        .map_err(|_| "Invalid asset base64 encoding".to_string())
}

pub(crate) fn validate_tapp_manifest(manifest: &TappManifest) -> Result<(), String> {
    validate_tapp_id(&manifest.id)?;
    if manifest.name.trim().is_empty() || manifest.name.len() > 255 {
        return Err("Tapp name must contain 1-255 characters".to_string());
    }
    semver::Version::parse(&manifest.version)
        .map_err(|_| "Tapp version must be valid semantic version".to_string())?;
    if manifest.category.is_none() {
        return Err("Tapp category is required".to_string());
    }
    if manifest
        .description
        .as_ref()
        .is_some_and(|description| description.len() > 2_000)
    {
        return Err("Tapp description must not exceed 2000 characters".to_string());
    }
    if manifest
        .icon
        .as_ref()
        .is_some_and(|icon| icon.len() > 2_048)
    {
        return Err("Tapp icon must not exceed 2048 characters".to_string());
    }
    if manifest
        .icon_svg
        .as_ref()
        .is_some_and(|icon| icon.len() > 65_536)
    {
        return Err("Tapp iconSvg must not exceed 64 KiB".to_string());
    }
    if manifest.theme_color.as_ref().is_some_and(|color| {
        color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err("Tapp themeColor must use #RRGGBB format".to_string());
    }
    for (field, value) in [
        ("homepage", manifest.homepage.as_deref()),
        ("repository", manifest.repository.as_deref()),
    ] {
        if let Some(value) = value {
            validate_http_url(value, field)?;
        }
    }
    validate_resource_path(&manifest.main)?;
    validate_resource_extension(&manifest.main, ".js", "main")?;
    if let Some(required) = manifest.min_system_version.as_deref() {
        let required = parse_system_version(required)?;
        let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
            .expect("backend package version must be valid semver");
        if current < required {
            return Err(format!(
                "Tapp requires Myriad {required} or newer; current version is {current}"
            ));
        }
    }
    if let Some(author) = &manifest.author {
        if author.name.trim().is_empty() || author.name.len() > 255 {
            return Err("Tapp author.name must contain 1-255 characters".to_string());
        }
        if author.email.as_ref().is_some_and(|email| {
            email.len() > 320 || email.chars().any(char::is_whitespace) || !email.contains('@')
        }) {
            return Err("Invalid Tapp author.email".to_string());
        }
        if let Some(url) = &author.url {
            validate_http_url(url, "author.url")?;
        }
    }
    if manifest.permissions.len() > 64 {
        return Err("Tapp permissions accepts at most 64 entries".to_string());
    }
    let mut permissions = std::collections::HashSet::new();
    for permission in &manifest.permissions {
        if TappPermission::from_str(permission).is_none()
            || !permissions.insert(permission.as_str())
        {
            return Err(format!(
                "Unknown or duplicate Tapp permission: {permission}"
            ));
        }
    }
    if manifest
        .css_mode
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "unified" | "separated"))
    {
        return Err("Tapp cssMode must be unified or separated".to_string());
    }

    for (field, path, extension) in [
        ("styles", manifest.styles.as_deref(), ".css"),
        ("widgetStyles", manifest.widget_styles.as_deref(), ".css"),
        ("pageStyles", manifest.page_styles.as_deref(), ".css"),
        ("pageTemplate", manifest.page_template.as_deref(), ".html"),
    ] {
        let Some(path) = path else { continue };
        validate_resource_path(path)?;
        validate_resource_extension(path, extension, field)?;
    }

    if let Some(modules) = &manifest.page_modules {
        if modules.len() > 64 {
            return Err("Tapp pageModules accepts at most 64 entries".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for module in modules {
            if !is_safe_path_component(module)
                || !module.ends_with(".js")
                || !seen.insert(module.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate page module filename: {module}; expected a .js file relative to page/"
                ));
            }
        }
    }

    if let Some(requirements) = &manifest.background_requirements {
        if requirements.len() > 16 {
            return Err("Tapp backgroundRequirements accepts at most 16 entries".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for requirement in requirements {
            if !matches!(
                requirement.as_str(),
                "media" | "sync" | "notification" | "scheduler" | "event-listener" | "realtime"
            ) || !seen.insert(requirement.as_str())
            {
                return Err(format!(
                    "Unknown or duplicate background requirement: {requirement}"
                ));
            }
        }
    }

    if let Some(settings) = &manifest.settings {
        validate_tapp_settings(settings, "Tapp")?;
    }

    if let Some(assets) = &manifest.assets {
        if assets.len() > MAX_TAPP_ASSETS {
            return Err(format!(
                "Tapp assets accepts at most {MAX_TAPP_ASSETS} entries"
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for path in assets {
            validate_asset_path(path)?;
            if !seen.insert(path.as_str()) {
                return Err(format!("Duplicate Tapp asset path: {path}"));
            }
        }
    }

    if let Some(widgets) = &manifest.widgets {
        if !widgets.is_empty()
            && !manifest
                .permissions
                .iter()
                .any(|permission| permission == "widget:register")
        {
            return Err("Tapp widgets require widget:register permission".to_string());
        }
        if widgets.len() > MAX_WIDGETS_PER_TAPP {
            return Err(format!("Too many Widgets (max {MAX_WIDGETS_PER_TAPP})"));
        }
        let mut widget_ids = std::collections::HashSet::new();
        for widget in widgets {
            if !is_safe_path_component(&widget.id) || !widget_ids.insert(widget.id.as_str()) {
                return Err(format!("Invalid or duplicate Widget ID: {}", widget.id));
            }
            if widget.name.is_empty() || widget.name.len() > 255 {
                return Err(format!("Invalid Widget name: {}", widget.id));
            }
            if widget.sizes.is_empty()
                || widget.sizes.len() > 10
                || widget.sizes.iter().any(|size| !is_valid_widget_size(size))
                || !widget.sizes.contains(&widget.default_size)
            {
                return Err(format!("Invalid Widget sizes: {}", widget.id));
            }
            validate_tapp_settings(&widget.settings, &format!("Widget {}", widget.id))?;
            if let Some(policy) = &widget.refresh_policy {
                validate_widget_refresh_policy(policy, &widget.id)?;
            }
            if let Some(templates) = &widget.templates {
                for (size, path) in templates {
                    if !is_valid_widget_size(size) || !widget.sizes.contains(size) {
                        return Err(format!(
                            "Widget template uses an undeclared size {size}: {}",
                            widget.id
                        ));
                    }
                    validate_resource_path(path)?;
                    validate_resource_extension(path, ".html", "Widget template")?;
                }
            }
        }
    }

    if let Some(apis) = &manifest.apis {
        if apis.len() > 64 {
            return Err("Tapp apis accepts at most 64 entries".to_string());
        }
        for (name, api) in apis {
            if !valid_agent_name(name) {
                return Err(format!("Invalid Tapp API name: {name}"));
            }
            if api.cache_ttl > 86_400 {
                return Err(format!(
                    "Tapp API {name} cacheTtl must not exceed 86400 seconds"
                ));
            }
            if serde_json::to_string(api).is_ok_and(|encoded| encoded.contains("{{secrets.")) {
                return Err(format!(
                    "Tapp API {name} cannot reference host secret templates"
                ));
            }
            match api.api_type.as_str() {
                "http" => {
                    if api.endpoint.is_none() {
                        return Err(format!("HTTP Tapp API {name} requires endpoint"));
                    }
                    if api.builtin.is_some() {
                        return Err(format!("HTTP Tapp API {name} cannot declare builtin"));
                    }
                    if !manifest
                        .permissions
                        .iter()
                        .any(|permission| permission == "network:fetch")
                    {
                        return Err(format!("HTTP Tapp API {name} requires network:fetch"));
                    }
                }
                "builtin" => {
                    let Some(builtin) = api.builtin.as_deref() else {
                        return Err(format!("Builtin Tapp API {name} requires builtin"));
                    };
                    if !matches!(builtin, "geo" | "ai:chat" | "ai:generate") {
                        return Err(format!("Unknown builtin Tapp API: {builtin}"));
                    }
                    if api.endpoint.is_some()
                        || api.headers.is_some()
                        || api.body.is_some()
                        || api.spoof.is_some()
                        || api.inject.is_some()
                    {
                        return Err(format!("Builtin Tapp API {name} contains HTTP-only fields"));
                    }
                    let required_permission = match builtin {
                        "ai:chat" => Some("ai:chat"),
                        "ai:generate" => Some("ai:generate"),
                        _ => None,
                    };
                    if required_permission.is_some_and(|required| {
                        !manifest
                            .permissions
                            .iter()
                            .any(|permission| permission == required)
                    }) {
                        return Err(format!(
                            "Builtin Tapp API {name} requires permission {}",
                            required_permission.expect("checked permission")
                        ));
                    }
                    let required_operation = match builtin {
                        "ai:chat" => Some(TappAiOperation::Chat),
                        "ai:generate" => Some(TappAiOperation::Generate),
                        _ => None,
                    };
                    if let Some(operation) = required_operation {
                        let ai = manifest.ai.as_ref().ok_or_else(|| {
                            format!(
                                "Builtin Tapp API {name} requires a protocolVersion 2 AI declaration"
                            )
                        })?;
                        if ai.protocol_version != 2 || !ai.operations.contains(&operation) {
                            return Err(format!(
                                "Builtin Tapp API {name} requires the matching AI operation"
                            ));
                        }
                        if !ai.output_formats.contains(&TappAiOutputFormat::Text) {
                            return Err(format!("Builtin Tapp API {name} requires AI text output"));
                        }
                    }
                }
                other => return Err(format!("Unknown Tapp API type: {other}")),
            }
            if api.method.len() > 16 || api.method.parse::<reqwest::Method>().is_err() {
                return Err(format!("Invalid HTTP method for Tapp API {name}"));
            }
            if let Some(inject) = &api.inject {
                if inject.len() > 32 {
                    return Err(format!("Tapp API {name} inject accepts at most 32 aliases"));
                }
                for (alias, template) in inject {
                    if !valid_agent_name(alias)
                        || ["user.", "geo.", "secrets.", "params."]
                            .iter()
                            .any(|prefix| alias.starts_with(prefix))
                    {
                        return Err(format!(
                            "Invalid or reserved inject alias for Tapp API {name}: {alias}"
                        ));
                    }
                    if template.is_empty() || template.len() > 2_048 {
                        return Err(format!(
                            "Invalid inject template for Tapp API {name}: {alias}"
                        ));
                    }
                }
            }
        }
    }

    if let Some(exchange) = &manifest.data_exchange {
        if exchange.exports.len() > MAX_DATA_EXCHANGE_DECLARATIONS
            || exchange.imports.len() > MAX_DATA_EXCHANGE_DECLARATIONS
        {
            return Err(format!(
                "Too many Data Exchange declarations (max {MAX_DATA_EXCHANGE_DECLARATIONS} per direction)"
            ));
        }

        let mut export_ids = std::collections::HashSet::new();
        for export in &exchange.exports {
            if !valid_data_exchange_id(&export.id) || !export_ids.insert(export.id.as_str()) {
                return Err(format!(
                    "Invalid or duplicate Data Exchange export id: {}",
                    export.id
                ));
            }
            if export.max_bytes == 0 || export.max_bytes > MAX_DATA_EXCHANGE_RESPONSE_BYTES {
                return Err(format!(
                    "Data Exchange export {} maxBytes must be between 1 and {MAX_DATA_EXCHANGE_RESPONSE_BYTES}",
                    export.id
                ));
            }
            if export
                .max_records
                .is_some_and(|limit| limit == 0 || limit > 10_000)
            {
                return Err(format!(
                    "Data Exchange export {} maxRecords must be between 1 and 10000",
                    export.id
                ));
            }
            if export
                .description
                .as_ref()
                .is_some_and(|description| description.len() > 500)
            {
                return Err(format!(
                    "Data Exchange export {} description is too long",
                    export.id
                ));
            }
            validate_inline_data_schema(&export.schema)?;
        }

        let mut imports = std::collections::HashSet::new();
        for import in &exchange.imports {
            validate_tapp_id(&import.tapp_id)?;
            if !valid_data_exchange_id(&import.export_id)
                || !imports.insert((import.tapp_id.as_str(), import.export_id.as_str()))
            {
                return Err(format!(
                    "Invalid or duplicate Data Exchange import: {} / {}",
                    import.tapp_id, import.export_id
                ));
            }
        }
    }

    if let Some(ai) = &manifest.ai {
        if ai.protocol_version != 2 {
            return Err("Tapp AI protocolVersion must be 2".to_string());
        }
        if ai.operations.is_empty() || ai.operations.len() > 4 {
            return Err("Tapp AI operations must contain 1-4 entries".to_string());
        }
        if ai.output_formats.is_empty() || ai.output_formats.len() > 3 {
            return Err("Tapp AI outputFormats must contain 1-3 entries".to_string());
        }

        let mut operations = std::collections::HashSet::new();
        for operation in &ai.operations {
            if !operations.insert(*operation) {
                return Err("Tapp AI operations contains duplicates".to_string());
            }
            let permission = operation.permission();
            if !manifest.permissions.iter().any(|value| value == permission) {
                return Err(format!(
                    "Tapp AI operation requires manifest permission {permission}"
                ));
            }
        }

        let mut context_sources = std::collections::HashSet::new();
        if ai.context_sources.len() > 4
            || ai
                .context_sources
                .iter()
                .any(|source| !context_sources.insert(*source))
        {
            return Err(
                "Tapp AI contextSources contains duplicates or too many entries".to_string(),
            );
        }
        if context_sources.contains(&TappAiContextSource::Platform)
            && !manifest
                .permissions
                .iter()
                .any(|value| value == "platform:read")
        {
            return Err("Tapp AI platform context requires platform:read".to_string());
        }
        if context_sources.contains(&TappAiContextSource::Report)
            && !manifest
                .permissions
                .iter()
                .any(|value| value == "report:read")
        {
            return Err("Tapp AI report context requires report:read".to_string());
        }

        let mut output_formats = std::collections::HashSet::new();
        if ai
            .output_formats
            .iter()
            .any(|format| !output_formats.insert(*format))
        {
            return Err("Tapp AI outputFormats contains duplicates".to_string());
        }
        if operations.contains(&TappAiOperation::Image)
            && !output_formats.contains(&TappAiOutputFormat::Image)
        {
            return Err("Tapp AI image operation requires image output format".to_string());
        }
    }

    if let Some(events) = &manifest.events {
        if events.publish.len() > 100 || events.subscribe.len() > 100 {
            return Err("Tapp events publish/subscribe accept at most 100 topics".to_string());
        }
        let publish_prefix = format!("tapp.{}.", manifest.id);
        let mut publish_topics = std::collections::HashSet::new();
        for topic in &events.publish {
            if !valid_event_topic(topic)
                || !topic.starts_with(&publish_prefix)
                || !publish_topics.insert(topic.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Tapp event publish topic: {topic}"
                ));
            }
        }
        let mut subscribe_topics = std::collections::HashSet::new();
        for topic in &events.subscribe {
            if !valid_event_topic(topic)
                || (!topic.starts_with("tapp.") && !topic.starts_with("system."))
                || !subscribe_topics.insert(topic.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Tapp event subscribe topic: {topic}"
                ));
            }
        }
        let declares =
            |permission: &str| manifest.permissions.iter().any(|value| value == permission);
        if !events.publish.is_empty() && !declares("event:publish") {
            return Err("Tapp event publish topics require event:publish".to_string());
        }
        if !events.subscribe.is_empty() && !declares("event:subscribe") {
            return Err("Tapp event subscribe topics require event:subscribe".to_string());
        }
    }

    if let Some(agent) = &manifest.agent {
        if agent.protocol_version != 2 {
            return Err("Tapp agent protocolVersion must be 2".to_string());
        }
        if agent.interactions.is_empty() || agent.interactions.len() > 32 {
            return Err("Tapp agent interactions must contain 1-32 entries".to_string());
        }
        let mut interaction_types = std::collections::HashSet::new();
        for interaction in &agent.interactions {
            if !valid_agent_name(&interaction.interaction_type)
                || !interaction_types.insert(interaction.interaction_type.as_str())
            {
                return Err(format!(
                    "Invalid or duplicate Agent interaction type: {}",
                    interaction.interaction_type
                ));
            }
            for schema in [
                interaction.input_schema.as_deref(),
                interaction.result_schema.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                validate_resource_path(schema)?;
                if !schema.ends_with(".json") {
                    return Err(format!("Agent schema must be a JSON resource: {schema}"));
                }
            }
        }
        if agent.intents.len() > 16 {
            return Err("Tapp agent intents accepts at most 16 entries".to_string());
        }
        let mut intents = std::collections::HashSet::new();
        for intent in &agent.intents {
            if !matches!(
                intent.as_str(),
                "ui.open" | "report.create" | "dataExchange.request"
            ) || !intents.insert(intent.as_str())
            {
                return Err(format!("Invalid or duplicate Agent intent: {intent}"));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_named_resource_keys<'a>(
    keys: impl IntoIterator<Item = &'a String>,
    kind: &str,
) -> Result<(), String> {
    for key in keys {
        if !is_safe_path_component(key) {
            return Err(format!("Invalid {kind}: {key}"));
        }
    }
    Ok(())
}
