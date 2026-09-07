//! Validated in-memory Tapp package (resources or archive) — pure domain.
//!
//! Install/update handlers stage packages to disk in the API layer. Validation,
//! resource overrides, widget-template binding, and declared-style content
//! presence rules live here so they are free of Axum/`StatusCode`.

use std::collections::HashMap;
use std::sync::Arc;

use myriad_tapp_contract::contract_rules::MAX_TAPP_MANIFEST_BYTES;
use myriad_tapp_contract::manifest::TappManifest;
use myriad_tapp_contract::validate::{validate_named_resource_keys, validate_tapp_manifest};

/// Widget id → size → HTML template content.
pub type WidgetTemplateContents = HashMap<String, HashMap<String, String>>;

/// Structured install payload (JSON install path / store download).
#[derive(Debug, Default, Clone)]
pub struct PreparedTappResources {
    /// 包内 `.js` 文件：相对路径 → 源码。必须覆盖 manifest 声明的每个层入口。
    pub modules: HashMap<String, String>,
    /// 作者样式，按层声明的路径写入。
    pub core_styles: Option<String>,
    pub page_styles: Option<String>,
    /// 作者 widget 样式：widget id → 内容。
    pub widget_styles: Option<HashMap<String, String>>,
    pub page_template: Option<String>,
    pub widget_templates: Option<WidgetTemplateContents>,
    /// 宿主预编译 Tailwind，写到固定路径，与作者样式是两条通道。
    pub generated_widget_css: Option<String>,
    pub generated_page_css: Option<String>,
    pub i18n: Option<HashMap<String, serde_json::Value>>,
    pub assets: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
enum PreparedTappPayload {
    Resources(Box<PreparedTappResources>),
    /// Shared archive bytes (MYR-025): `Arc` so package clones / spawn_blocking
    /// never duplicate the full zip unboundedly.
    Archive(Arc<Vec<u8>>),
}

/// Validated package ready for staging (resources map or raw .tapp archive).
#[derive(Debug, Clone)]
pub struct PreparedTappPackage {
    pub manifest: TappManifest,
    payload: PreparedTappPayload,
}

/// Domain validation failures for prepared packages (HTTP maps to 400).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageValidateError {
    IdMismatch,
    Manifest(String),
    WidgetTemplates(String),
    MissingLayerEntry { declared: String },
    MissingPageStyles { declared: String },
    MissingPageTemplate,
    MissingWidgetStyles { declared: String },
    NamedResource(String),
}

impl PackageValidateError {
    pub fn message(&self) -> String {
        match self {
            Self::IdMismatch => "manifest id does not match target tapp id".to_string(),
            Self::Manifest(msg) | Self::WidgetTemplates(msg) | Self::NamedResource(msg) => {
                msg.clone()
            }
            Self::MissingLayerEntry { declared } => {
                format!("Install package is missing source for declared layer entry {declared}")
            }
            Self::MissingPageStyles { declared } => {
                format!("Install package is missing content for declared page.styles={declared}")
            }
            Self::MissingPageTemplate => {
                "Install package is missing content for declared page.template".to_string()
            }
            Self::MissingWidgetStyles { declared } => format!(
                "Install package is missing content for declared widgets[].styles={declared}"
            ),
        }
    }

    pub fn status_hint(&self) -> u16 {
        400
    }
}

impl std::fmt::Display for PackageValidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for PackageValidateError {}

/// Errors while loading a .tapp archive into a prepared package (pre-stage).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageLoadError {
    InvalidArchive,
    ManifestNotFound,
    ManifestTooLarge,
    ManifestUnreadable,
    ManifestParse(String),
}

impl PackageLoadError {
    pub fn message(&self) -> String {
        match self {
            Self::InvalidArchive => "Invalid .tapp file format".to_string(),
            Self::ManifestNotFound => "manifest.json not found in .tapp file".to_string(),
            Self::ManifestTooLarge => {
                format!("manifest.json exceeds {MAX_TAPP_MANIFEST_BYTES} bytes")
            }
            Self::ManifestUnreadable => "Failed to read manifest.json".to_string(),
            Self::ManifestParse(message) => message.clone(),
        }
    }

    pub fn status_hint(&self) -> u16 {
        400
    }
}

impl std::fmt::Display for PackageLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for PackageLoadError {}

/// Non-empty string content (empty strings treated as missing).
pub fn nonempty_content(value: Option<&String>) -> Option<&str> {
    value
        .map(String::as_str)
        .filter(|content| !content.is_empty())
}

/// Manifest template path for a widget size, when declared.
pub fn widget_template_path<'a>(
    manifest: &'a TappManifest,
    widget_id: &str,
    size: &str,
) -> Option<&'a str> {
    manifest
        .widgets
        .as_ref()
        .into_iter()
        .flatten()
        .find(|widget| widget.id == widget_id)
        .and_then(|widget| widget.templates.as_ref())
        .and_then(|templates| templates.get(size))
        .map(String::as_str)
}

/// Ensure provided template contents match Manifest widget ids/sizes/paths.
pub fn validate_widget_template_contents(
    manifest: &TappManifest,
    contents: &WidgetTemplateContents,
) -> Result<(), String> {
    for (widget_id, templates) in contents {
        let widget = manifest
            .widgets
            .as_ref()
            .and_then(|widgets| widgets.iter().find(|widget| widget.id == *widget_id))
            .ok_or_else(|| format!("Widget template references unknown Widget: {widget_id}"))?;
        for size in templates.keys() {
            if !widget.sizes.contains(size)
                || widget_template_path(manifest, widget_id, size).is_none()
            {
                return Err(format!(
                    "Widget template content has no matching Manifest path: {widget_id}/{size}"
                ));
            }
        }
    }
    Ok(())
}

/// Reject oversized manifest payloads before full parse work.
pub fn check_manifest_byte_size(size: u64) -> Result<(), PackageLoadError> {
    if size > MAX_TAPP_MANIFEST_BYTES {
        Err(PackageLoadError::ManifestTooLarge)
    } else {
        Ok(())
    }
}

/// Parse manifest.json text into a typed manifest.
/// 层契约之前的顶层字段。命中任何一个说明这是旧格式包，报错要指向新格式，
/// 而不是把 serde 的 `unknown field` 原样丢给用户。
const PRE_LAYER_MANIFEST_FIELDS: &[&str] = &[
    "main",
    "hasPage",
    "cssMode",
    "styles",
    "widgetStyles",
    "pageStyles",
    "pageTemplate",
    "pageModules",
];

pub fn parse_manifest_json(content: &str) -> Result<TappManifest, PackageLoadError> {
    match serde_json::from_str(content) {
        Ok(manifest) => Ok(manifest),
        Err(error) => {
            let legacy = serde_json::from_str::<serde_json::Value>(content)
                .ok()
                .and_then(|value| {
                    let object = value.as_object()?;
                    let found: Vec<&str> = PRE_LAYER_MANIFEST_FIELDS
                        .iter()
                        .copied()
                        .filter(|field| object.contains_key(*field))
                        .collect();
                    (!found.is_empty()).then(|| found.join(", "))
                });
            if let Some(fields) = legacy.as_deref() {
                tracing::error!(%error, %fields, "tapp manifest uses the pre-layer format");
                return Err(PackageLoadError::ManifestParse(
                    "manifest.json uses the pre-layer format".to_string(),
                ));
            }
            tracing::error!(%error, "invalid tapp manifest.json");
            Err(PackageLoadError::ManifestParse(
                "Invalid manifest.json".to_string(),
            ))
        }
    }
}

impl PreparedTappPackage {
    pub fn from_resources(manifest: TappManifest, resources: PreparedTappResources) -> Self {
        Self {
            manifest,
            payload: PreparedTappPayload::Resources(Box::new(resources)),
        }
    }

    /// Build from already-read archive bytes + parsed manifest (caller validates zip layout).
    ///
    /// Bytes are stored behind an [`Arc`] so staging / concurrent install paths
    /// can share one buffer instead of cloning the full zip (MYR-025).
    pub fn from_archive_parts(
        manifest: TappManifest,
        file_data: Vec<u8>,
        current_system_version: &semver::Version,
    ) -> Result<Self, PackageValidateError> {
        let package = Self {
            manifest,
            payload: PreparedTappPayload::Archive(Arc::new(file_data)),
        };
        package.validate(None, current_system_version)?;
        Ok(package)
    }

    pub fn apply_resource_overrides(
        &mut self,
        i18n: Option<HashMap<String, serde_json::Value>>,
        modules: Option<HashMap<String, String>>,
        assets: Option<HashMap<String, String>>,
    ) {
        if let PreparedTappPayload::Resources(resources) = &mut self.payload {
            resources.i18n = i18n.or(resources.i18n.take());
            if let Some(modules) = modules {
                resources.modules = modules;
            }
            resources.assets = assets.or(resources.assets.take());
        }
    }

    pub fn resources(&self) -> Option<&PreparedTappResources> {
        match &self.payload {
            PreparedTappPayload::Resources(resources) => Some(resources),
            PreparedTappPayload::Archive(_) => None,
        }
    }

    /// Cheap share of archive bytes for spawn_blocking extract (no full clone).
    pub fn archive_arc(&self) -> Option<Arc<Vec<u8>>> {
        match &self.payload {
            PreparedTappPayload::Archive(bytes) => Some(Arc::clone(bytes)),
            PreparedTappPayload::Resources(_) => None,
        }
    }

    /// Validate manifest + structured resource completeness for install.
    pub fn validate(
        &self,
        expected_tapp_id: Option<&str>,
        current_system_version: &semver::Version,
    ) -> Result<(), PackageValidateError> {
        if expected_tapp_id.is_some_and(|expected| expected != self.manifest.id) {
            return Err(PackageValidateError::IdMismatch);
        }
        validate_tapp_manifest(&self.manifest, current_system_version)
            .map_err(PackageValidateError::Manifest)?;

        let Some(resources) = self.resources() else {
            return Ok(());
        };

        if let Some(templates) = &resources.widget_templates {
            validate_widget_template_contents(&self.manifest, templates)
                .map_err(PackageValidateError::WidgetTemplates)?;
        }

        // 声明了层入口就必须带上对应源码，否则 staging 之后 validate_installed_resources
        // 只会报一句「文件缺失」，看不出是 payload 少给了内容。
        for entry in self.manifest.layer_entries() {
            if !resources.modules.contains_key(entry) {
                return Err(PackageValidateError::MissingLayerEntry {
                    declared: entry.to_string(),
                });
            }
        }

        // 声明了层样式/模板同理。
        if let Some(declared) = self
            .manifest
            .page
            .as_ref()
            .and_then(|page| page.styles.as_deref())
        {
            if nonempty_content(resources.page_styles.as_ref()).is_none() {
                return Err(PackageValidateError::MissingPageStyles {
                    declared: declared.to_string(),
                });
            }
        }
        if self
            .manifest
            .page
            .as_ref()
            .is_some_and(|page| page.template.is_some())
            && nonempty_content(resources.page_template.as_ref()).is_none()
        {
            return Err(PackageValidateError::MissingPageTemplate);
        }
        if let Some(widgets) = &self.manifest.widgets {
            for widget in widgets {
                let Some(declared) = widget.styles.as_deref() else {
                    continue;
                };
                let content = resources
                    .widget_styles
                    .as_ref()
                    .and_then(|styles| styles.get(&widget.id));
                if nonempty_content(content).is_none() {
                    return Err(PackageValidateError::MissingWidgetStyles {
                        declared: declared.to_string(),
                    });
                }
            }
        }

        validate_named_resource_keys(
            resources
                .i18n
                .as_ref()
                .into_iter()
                .flat_map(|translations| translations.keys()),
            "i18n language code",
        )
        .map_err(PackageValidateError::NamedResource)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn current_system() -> semver::Version {
        semver::Version::new(0, 4, 0)
    }

    fn base_manifest() -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.prepared",
            "name": "Prepared package",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "category": "media",
            "permissions": []
        }))
        .unwrap()
    }

    #[test]
    fn validation_rejects_target_id_mismatch() {
        let package = PreparedTappPackage::from_resources(
            base_manifest(),
            PreparedTappResources {
                modules: HashMap::from([("core.js".to_string(), "export {};".to_string())]),
                ..PreparedTappResources::default()
            },
        );
        assert_eq!(
            package
                .validate(Some("com.example.other"), &current_system())
                .unwrap_err(),
            PackageValidateError::IdMismatch
        );
        assert!(package
            .validate(Some("com.example.prepared"), &current_system())
            .is_ok());
    }

    #[test]
    fn archive_payload_shares_bytes_via_arc_without_full_clone() {
        // MYR-025: package clone / extract should share one zip buffer.
        let bytes = vec![1u8, 2, 3, 4, 5];
        let package = PreparedTappPackage::from_archive_parts(
            base_manifest(),
            bytes.clone(),
            &current_system(),
        )
        .unwrap();
        let a = package.archive_arc().expect("archive");
        let b = package.archive_arc().expect("archive");
        assert_eq!(a.as_slice(), bytes.as_slice());
        assert!(Arc::ptr_eq(&a, &b));
        let cloned = package.clone();
        let c = cloned.archive_arc().expect("archive");
        assert!(Arc::ptr_eq(&a, &c));
        // package + cloned payloads + a + b + c
        assert_eq!(Arc::strong_count(&a), 5);
    }

    #[test]
    fn explicit_resource_overrides_replace_store_resources() {
        let mut original_i18n = HashMap::new();
        original_i18n.insert("en-US".to_string(), json!({ "title": "Store" }));
        let mut package = PreparedTappPackage::from_resources(
            base_manifest(),
            PreparedTappResources {
                modules: HashMap::from([("core.js".to_string(), "export {};".to_string())]),
                i18n: Some(original_i18n),
                ..PreparedTappResources::default()
            },
        );
        let mut override_i18n = HashMap::new();
        override_i18n.insert("en-US".to_string(), json!({ "title": "Override" }));
        package.apply_resource_overrides(Some(override_i18n), None, None);
        assert_eq!(
            package.resources().unwrap().i18n.as_ref().unwrap()["en-US"]["title"],
            "Override"
        );
    }

    /// 旧格式包不再能安装。对外只说是格式问题，字段名和文档路径留在日志里。
    #[test]
    fn legacy_manifest_error_points_at_the_layer_contract() {
        let error = parse_manifest_json(
            r#"{
                "id": "com.example.legacy",
                "name": "Legacy",
                "version": "1.0.0",
                "category": "utility",
                "main": "main.js",
                "hasPage": true,
                "permissions": []
            }"#,
        )
        .expect_err("pre-layer manifest must not parse");
        let message = error.message();
        assert!(message.contains("pre-layer format"), "got: {message}");
        assert!(!message.contains("main"), "got: {message}");
        assert!(!message.contains("hasPage"), "got: {message}");
        assert!(!message.contains("unknown field"), "got: {message}");
        assert!(!message.contains("TAPP_FILE_FORMAT"), "got: {message}");
    }

    /// 结构性错误也不把 serde 的列号/期望 token 抛给用户。
    #[test]
    fn non_legacy_parse_errors_stay_stable() {
        let error = parse_manifest_json("{ not json").expect_err("must not parse");
        assert_eq!(error.message(), "Invalid manifest.json");
    }

    /// 声明了层入口却不带源码：必须在 staging 之前就报清楚，不能等落盘后
    /// 变成一句「文件缺失」。
    #[test]
    fn validate_rejects_missing_layer_entry_source() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.missing-entry",
            "name": "Missing entry",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "page": { "entry": "page/index.js" },
            "category": "utility",
            "permissions": []
        }))
        .unwrap();
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([("core.js".to_string(), "export {};".to_string())]),
                ..PreparedTappResources::default()
            },
        );
        let err = package.validate(None, &current_system()).unwrap_err();
        assert!(err.message().contains("page/index.js"));
        assert_eq!(err.status_hint(), 400);
    }

    #[test]
    fn validate_rejects_missing_page_styles_content_with_clear_error() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.missing-page-css",
            "name": "Missing page css",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "page": { "entry": "page/index.js", "styles": "page.css" },
            "category": "game",
            "permissions": []
        }))
        .unwrap();
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([
                    ("core.js".to_string(), "export {};".to_string()),
                    ("page/index.js".to_string(), "export {};".to_string()),
                ]),
                ..PreparedTappResources::default()
            },
        );
        let err = package.validate(None, &current_system()).unwrap_err();
        let msg = err.message();
        assert!(msg.contains("page.styles"));
        assert!(msg.contains("page.css"));
        assert_eq!(err.status_hint(), 400);
    }

    /// 宿主预编译的 Tailwind 是另一条通道，不能拿它顶替作者声明的层样式。
    #[test]
    fn validate_rejects_empty_page_styles_content() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.empty-page-css",
            "name": "Empty page css",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "page": { "entry": "page/index.js", "styles": "styles/page.css" },
            "category": "utility",
            "permissions": []
        }))
        .unwrap();
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([
                    ("core.js".to_string(), "export {};".to_string()),
                    ("page/index.js".to_string(), "export {};".to_string()),
                ]),
                page_styles: Some(String::new()),
                generated_page_css: Some(".from-host {}".to_string()),
                ..PreparedTappResources::default()
            },
        );
        let msg = package
            .validate(None, &current_system())
            .unwrap_err()
            .message();
        assert!(msg.contains("page.styles"));
        assert!(msg.contains("styles/page.css"));
    }

    #[test]
    fn validate_rejects_missing_widget_styles_content() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.missing-widget-css",
            "name": "Missing widget css",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "category": "utility",
            "permissions": ["widget:register"],
            "widgets": [{
                "id": "card",
                "name": "Card",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "entry": "widget.js",
                "styles": "widget-card.css"
            }]
        }))
        .unwrap();
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([
                    ("core.js".to_string(), "export {};".to_string()),
                    ("widget.js".to_string(), "export {};".to_string()),
                ]),
                ..PreparedTappResources::default()
            },
        );
        let msg = package
            .validate(None, &current_system())
            .unwrap_err()
            .message();
        assert!(msg.contains("widgets[].styles"));
        assert!(msg.contains("widget-card.css"));
    }

    #[test]
    fn check_manifest_byte_size_enforces_contract_limit() {
        assert!(check_manifest_byte_size(1).is_ok());
        assert_eq!(
            check_manifest_byte_size(MAX_TAPP_MANIFEST_BYTES + 1).unwrap_err(),
            PackageLoadError::ManifestTooLarge
        );
    }

    #[test]
    fn widget_template_path_reads_manifest_declaration() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.widgets",
            "name": "Widgets",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "category": "utility",
            "permissions": ["widget:register"],
            "widgets": [{
                "id": "card",
                "name": "Card",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "templates": { "2x2": "templates/card-2x2.html" }
            }]
        }))
        .unwrap();
        assert_eq!(
            widget_template_path(&manifest, "card", "2x2"),
            Some("templates/card-2x2.html")
        );
        assert!(widget_template_path(&manifest, "card", "4x4").is_none());
    }
}
