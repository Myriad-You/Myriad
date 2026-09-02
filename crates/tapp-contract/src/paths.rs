//! Path, id, and setting-value rules shared by install validation and CLI.

use std::path::{Component, Path as FsPath};

use crate::contract_rules::{
    ASSET_DIRECTORY, ASSET_FORBIDDEN_EXTENSIONS, INLINE_SCHEMA_ROOT_KEYS, MAX_DATA_EXCHANGE_ID_LEN,
    MAX_DATA_EXCHANGE_SCHEMA_BYTES, MAX_INLINE_SCHEMA_DEPTH, MAX_RESOURCE_PATH_LEN,
    MAX_SETTING_LABEL_LEN, MAX_SETTING_OPTIONS, MAX_SETTING_OPTION_VALUE_LEN, MAX_TAPP_ID_LEN,
    MAX_TAPP_SETTINGS, MAX_WIDGET_REFRESH_INTERVAL_SECONDS, MIN_WIDGET_REFRESH_INTERVAL_SECONDS,
    SEMVER_PREFIXES, SETTING_TYPES, WIDGET_SIZES,
};
use crate::manifest::{TappSettingDef, TappWidgetRefreshMode, TappWidgetRefreshPolicy};
use crate::storage::validate_storage_key;

pub fn valid_data_exchange_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DATA_EXCHANGE_ID_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub fn validate_inline_data_schema(schema: &serde_json::Value) -> Result<(), String> {
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
        if depth > MAX_INLINE_SCHEMA_DEPTH {
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
    if !INLINE_SCHEMA_ROOT_KEYS
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return Err(
            "Data Exchange schema must declare type, properties, enum, or const".to_string(),
        );
    }
    Ok(())
}

pub fn is_safe_path_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TAPP_ID_LEN
        && value != "."
        && value != ".."
        && !value.starts_with('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

pub fn validate_tapp_id(tapp_id: &str) -> Result<(), String> {
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

pub fn validate_resource_path(path: &str) -> Result<(), String> {
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

pub fn is_valid_widget_size(size: &str) -> bool {
    WIDGET_SIZES.contains(&size)
}

pub fn tapp_setting_value_is_valid(setting: &TappSettingDef, value: &serde_json::Value) -> bool {
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

pub fn validate_tapp_settings(settings: &[TappSettingDef], scope: &str) -> Result<(), String> {
    if settings.len() > MAX_TAPP_SETTINGS {
        return Err(format!(
            "{scope} accepts at most {MAX_TAPP_SETTINGS} settings"
        ));
    }
    let mut keys = std::collections::HashSet::new();
    for setting in settings {
        if validate_storage_key(&setting.key).is_err()
            || !keys.insert(setting.key.as_str())
            || setting.label.is_empty()
            || setting.label.len() > MAX_SETTING_LABEL_LEN
            || !SETTING_TYPES.contains(&setting.setting_type.as_str())
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
                .is_none_or(|options| options.is_empty() || options.len() > MAX_SETTING_OPTIONS)
        {
            return Err(format!(
                "Select {scope} setting {} requires 1-{MAX_SETTING_OPTIONS} options",
                setting.key
            ));
        }
        if let Some(options) = &setting.options {
            let mut values = std::collections::HashSet::new();
            if setting.setting_type != "select"
                || options.iter().any(|option| {
                    option.value.is_empty()
                        || option.value.len() > MAX_SETTING_OPTION_VALUE_LEN
                        || option.label.is_empty()
                        || option.label.len() > MAX_SETTING_LABEL_LEN
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

pub fn validate_widget_refresh_policy(
    policy: &TappWidgetRefreshPolicy,
    widget_id: &str,
) -> Result<(), String> {
    match policy.mode {
        TappWidgetRefreshMode::Event if policy.interval_seconds.is_some() => Err(format!(
            "Event-driven Widget {widget_id} cannot declare intervalSeconds"
        )),
        TappWidgetRefreshMode::Event => Ok(()),
        TappWidgetRefreshMode::Interval
            if !policy.interval_seconds.is_some_and(|seconds| {
                (MIN_WIDGET_REFRESH_INTERVAL_SECONDS..=MAX_WIDGET_REFRESH_INTERVAL_SECONDS)
                    .contains(&seconds)
            }) =>
        {
            Err(format!(
                "Interval Widget {widget_id} requires intervalSeconds between {MIN_WIDGET_REFRESH_INTERVAL_SECONDS} and {MAX_WIDGET_REFRESH_INTERVAL_SECONDS}"
            ))
        }
        TappWidgetRefreshMode::Interval => Ok(()),
    }
}

pub fn parse_system_version(value: &str) -> Result<semver::Version, String> {
    let normalized = SEMVER_PREFIXES
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
        .unwrap_or(value);
    semver::Version::parse(normalized)
        .map_err(|_| format!("Invalid minSystemVersion: {value}; expected semantic version"))
}

pub fn validate_resource_extension(path: &str, extension: &str, field: &str) -> Result<(), String> {
    if !path.ends_with(extension) {
        return Err(format!("Tapp {field} must reference a {extension} file"));
    }
    Ok(())
}

pub fn validate_asset_path(path: &str) -> Result<(), String> {
    validate_resource_path(path)?;
    let prefix = format!("{ASSET_DIRECTORY}/");
    if !path.starts_with(&prefix) || path == ASSET_DIRECTORY || path.ends_with('/') {
        return Err(format!(
            "Tapp asset path must be a file under {ASSET_DIRECTORY}/: {path}"
        ));
    }
    if ASSET_FORBIDDEN_EXTENSIONS
        .iter()
        .any(|extension| path.ends_with(extension))
    {
        return Err(format!(
            "Tapp asset path must not be a script or HTML entry: {path}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::TappSettingOption;
    use serde_json::json;

    #[test]
    fn widget_size_consults_widget_sizes() {
        for size in WIDGET_SIZES {
            assert!(is_valid_widget_size(size), "{size}");
        }
        assert!(!is_valid_widget_size("5x5"));
        assert!(!is_valid_widget_size("2x2x2"));
    }

    #[test]
    fn resource_path_rejects_absolute_and_escape_components() {
        assert!(validate_resource_path("page/state.js").is_ok());
        assert!(validate_resource_path("/abs.js").is_err());
        assert!(validate_resource_path("../escape.js").is_err());
        assert!(validate_resource_path("").is_err());
        assert!(validate_resource_path("a\\b.js").is_err());
    }

    #[test]
    fn asset_path_must_live_under_assets_and_not_be_entrypoint() {
        assert!(validate_asset_path("assets/icon.png").is_ok());
        assert!(validate_asset_path("assets/").is_err());
        assert!(validate_asset_path("icon.png").is_err());
        assert!(validate_asset_path("assets/main.js").is_err());
        assert!(validate_asset_path("assets/page.html").is_err());
    }

    #[test]
    fn data_exchange_id_and_inline_schema_rules() {
        assert!(valid_data_exchange_id("share.profile"));
        assert!(!valid_data_exchange_id(""));
        assert!(!valid_data_exchange_id("has space"));

        assert!(validate_inline_data_schema(&json!({"type": "object"})).is_ok());
        assert!(validate_inline_data_schema(&json!({"$ref": "#/x"})).is_err());
        assert!(validate_inline_data_schema(&json!({})).is_err());
        assert!(validate_inline_data_schema(&json!("string")).is_err());
    }

    #[test]
    fn tapp_id_requires_safe_component_starting_alphanumeric() {
        assert!(validate_tapp_id("com.myriad.safe-app_2").is_ok());
        assert!(validate_tapp_id(".hidden").is_err());
        assert!(validate_tapp_id("../x").is_err());
        assert!(validate_tapp_id("").is_err());
        assert!(is_safe_path_component("page"));
        assert!(!is_safe_path_component(".."));
        assert!(!is_safe_path_component(".hidden"));
    }

    #[test]
    fn setting_value_respects_type_and_number_bounds() {
        let number = TappSettingDef {
            key: "volume".into(),
            label: "Volume".into(),
            setting_type: "number".into(),
            description: None,
            default_value: None,
            options: None,
            min: Some(0.0),
            max: Some(100.0),
            step: Some(1.0),
            placeholder: None,
        };
        assert!(tapp_setting_value_is_valid(&number, &json!(75)));
        assert!(!tapp_setting_value_is_valid(&number, &json!(101)));
        assert!(!tapp_setting_value_is_valid(&number, &json!("75")));

        let select = TappSettingDef {
            key: "theme".into(),
            label: "Theme".into(),
            setting_type: "select".into(),
            description: None,
            default_value: None,
            options: Some(vec![TappSettingOption {
                value: "dark".into(),
                label: "Dark".into(),
            }]),
            min: None,
            max: None,
            step: None,
            placeholder: None,
        };
        assert!(tapp_setting_value_is_valid(&select, &json!("dark")));
        assert!(!tapp_setting_value_is_valid(&select, &json!("system")));
    }

    #[test]
    fn parse_system_version_strips_optional_prefix() {
        assert_eq!(
            parse_system_version("v1.2.3").unwrap(),
            semver::Version::new(1, 2, 3)
        );
        assert_eq!(
            parse_system_version("0.3.18").unwrap(),
            semver::Version::new(0, 3, 18)
        );
        assert!(parse_system_version("not-a-version").is_err());
    }

    #[test]
    fn widget_refresh_policy_uses_interval_bounds() {
        let event = TappWidgetRefreshPolicy {
            mode: TappWidgetRefreshMode::Event,
            interval_seconds: None,
            refresh_on_visible: true,
        };
        assert!(validate_widget_refresh_policy(&event, "w").is_ok());
        let event_with_interval = TappWidgetRefreshPolicy {
            mode: TappWidgetRefreshMode::Event,
            interval_seconds: Some(30),
            refresh_on_visible: true,
        };
        assert!(validate_widget_refresh_policy(&event_with_interval, "w").is_err());

        let interval = TappWidgetRefreshPolicy {
            mode: TappWidgetRefreshMode::Interval,
            interval_seconds: Some(MIN_WIDGET_REFRESH_INTERVAL_SECONDS),
            refresh_on_visible: true,
        };
        assert!(validate_widget_refresh_policy(&interval, "w").is_ok());
        let too_short = TappWidgetRefreshPolicy {
            mode: TappWidgetRefreshMode::Interval,
            interval_seconds: Some(MIN_WIDGET_REFRESH_INTERVAL_SECONDS - 1),
            refresh_on_visible: true,
        };
        assert!(validate_widget_refresh_policy(&too_short, "w").is_err());
    }

    #[test]
    fn resource_extension_and_js_file() {
        assert!(validate_resource_extension("core.js", ".js", "core.entry").is_ok());
        assert!(validate_resource_extension("core.css", ".js", "core.entry").is_err());
    }

    fn toggle(key: &str) -> TappSettingDef {
        TappSettingDef {
            key: key.into(),
            label: "On".into(),
            setting_type: "toggle".into(),
            description: None,
            default_value: Some(json!(true)),
            options: None,
            min: None,
            max: None,
            step: None,
            placeholder: None,
        }
    }

    #[test]
    fn tapp_settings_table_uses_storage_key_and_setting_types() {
        assert!(validate_tapp_settings(&[toggle("theme")], "Tapp").is_ok());
        assert!(validate_tapp_settings(&[toggle(".hidden")], "Tapp").is_err());
        assert!(validate_tapp_settings(&[toggle("a"), toggle("a")], "Tapp").is_err());
        let mut unknown = toggle("ok");
        unknown.setting_type = "secret".into();
        assert!(validate_tapp_settings(&[unknown], "Tapp").is_err());
        let select = TappSettingDef {
            key: "mode".into(),
            label: "Mode".into(),
            setting_type: "select".into(),
            description: None,
            default_value: None,
            options: None,
            min: None,
            max: None,
            step: None,
            placeholder: None,
        };
        assert!(validate_tapp_settings(&[select], "Tapp")
            .unwrap_err()
            .contains("1-100 options"));
    }
}
