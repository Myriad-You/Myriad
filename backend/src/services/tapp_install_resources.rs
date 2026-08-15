//! Declared install-resource checks for staged Tapp packages.
//!
//! Pure path enumeration and content rules live here so post-stage validation
//! does not own contract messages only in the API layer. The API still resolves
//! sandbox paths (canonicalize / symlink rejection) and performs file IO.

use std::collections::HashSet;

use myriad_tapp_contract::manifest::TappManifest;

use crate::services::tapp_validation::{
    is_safe_path_component, validate_asset_path, validate_inline_data_schema,
    MAX_AGENT_SCHEMA_RESOURCE_BYTES, MAX_TAPP_ARCHIVE_FILES, MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES,
    MAX_TAPP_ASSETS_TOTAL_BYTES, MAX_TAPP_ASSET_BYTES, MAX_TAPP_GAME_ASSETS_TOTAL_BYTES,
    MAX_TAPP_GAME_ASSET_BYTES, MAX_TAPP_I18N_FILES,
    MAX_TAPP_I18N_RESOURCE_BYTES, MAX_TAPP_RESOURCE_BYTES,
};

/// Kind of declared install resource used when reading/validating bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredResourceKind {
    /// UTF-8 text entrypoint / CSS / HTML / widget templates.
    Text,
    /// Page module under `page/` (same UTF-8 rule; distinct missing-file copy).
    PageModule,
    /// Agent interaction JSON schema (size-bounded + subset schema rules).
    AgentSchema,
    /// Binary-allowed package asset under `assets/`.
    Asset,
}

/// One relative path the install stage must materialize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredInstallResource {
    pub relative: String,
    pub kind: DeclaredResourceKind,
}

/// Collect every declared path that must exist after install staging.
///
/// Order is stable for diagnostics: main/styles/templates, then page modules,
/// agent schemas, then assets. i18n is directory-scanned separately.
pub fn collect_declared_install_resources(manifest: &TappManifest) -> Vec<DeclaredInstallResource> {
    let mut resources = Vec::new();

    resources.push(DeclaredInstallResource {
        relative: manifest.main.clone(),
        kind: DeclaredResourceKind::Text,
    });
    for optional in [
        manifest.styles.as_deref(),
        manifest.widget_styles.as_deref(),
        manifest.page_styles.as_deref(),
        manifest.page_template.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        resources.push(DeclaredInstallResource {
            relative: optional.to_string(),
            kind: DeclaredResourceKind::Text,
        });
    }
    if let Some(widgets) = &manifest.widgets {
        for widget in widgets {
            if let Some(templates) = &widget.templates {
                for path in templates.values() {
                    resources.push(DeclaredInstallResource {
                        relative: path.clone(),
                        kind: DeclaredResourceKind::Text,
                    });
                }
            }
        }
    }
    if let Some(modules) = &manifest.page_modules {
        for module in modules {
            resources.push(DeclaredInstallResource {
                relative: format!("page/{module}"),
                kind: DeclaredResourceKind::PageModule,
            });
        }
    }
    if let Some(agent) = &manifest.agent {
        for interaction in &agent.interactions {
            for relative in [
                interaction.input_schema.as_deref(),
                interaction.result_schema.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                resources.push(DeclaredInstallResource {
                    relative: relative.to_string(),
                    kind: DeclaredResourceKind::AgentSchema,
                });
            }
        }
    }
    if let Some(assets) = &manifest.assets {
        for relative in assets {
            resources.push(DeclaredInstallResource {
                relative: relative.clone(),
                kind: DeclaredResourceKind::Asset,
            });
        }
    }

    resources
}

/// Error when a declared path fails basic existence / sandbox file checks.
pub fn invalid_declared_path(relative: &str) -> String {
    format!("Declared Tapp resource has invalid path: {relative}")
}

pub fn missing_after_install(relative: &str) -> String {
    format!(
        "Declared Tapp resource is missing after install (expected regular file): {relative}. \
If this is page.css, the install payload likely omitted pageStyles/pageCss content for cssMode=separated."
    )
}

pub fn not_regular_in_sandbox(relative: &str) -> String {
    format!("Declared Tapp resource is not a regular in-sandbox file: {relative}")
}

pub fn not_regular_file(relative: &str) -> String {
    format!("Declared Tapp resource is not a regular file: {relative}")
}

pub fn resource_not_found(relative: &str) -> String {
    format!("Declared Tapp resource not found: {relative}")
}

pub fn agent_schema_not_regular(relative: &str) -> String {
    format!("Declared Agent schema is not a regular file: {relative}")
}

pub fn agent_schema_not_found(relative: &str) -> String {
    format!("Declared Agent schema not found: {relative}")
}

pub fn asset_not_regular(relative: &str) -> String {
    format!("Declared Tapp asset is not a regular file: {relative}")
}

pub fn asset_not_found(relative: &str) -> String {
    format!("Declared Tapp asset not found: {relative}")
}

/// Validate UTF-8 text declared resources (main/css/html/page modules).
pub fn validate_text_resource_bytes(relative: &str, bytes: &[u8]) -> Result<(), String> {
    std::str::from_utf8(bytes)
        .map(|_| ())
        .map_err(|_| format!("Declared Tapp resource is not UTF-8 text: {relative}"))
}

/// Validate agent schema file bytes (size + JSON subset).
pub fn validate_agent_schema_bytes(relative: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_AGENT_SCHEMA_RESOURCE_BYTES {
        return Err(format!(
            "Agent schema exceeds {MAX_AGENT_SCHEMA_RESOURCE_BYTES} bytes: {relative}"
        ));
    }
    let schema = serde_json::from_slice::<serde_json::Value>(bytes)
        .map_err(|_| format!("Agent schema is not valid JSON: {relative}"))?;
    validate_inline_data_schema(&schema)
        .map_err(|error| format!("Invalid Agent schema {relative}: {error}"))
}

/// Size budget for declared package assets.
#[derive(Debug, Clone, Copy)]
pub struct AssetBudget {
    pub max_each: u64,
    pub max_total: u64,
}

impl AssetBudget {
    pub fn standard() -> Self {
        Self {
            max_each: MAX_TAPP_ASSET_BYTES,
            max_total: MAX_TAPP_ASSETS_TOTAL_BYTES,
        }
    }

    pub fn for_manifest(manifest: &TappManifest) -> Self {
        if manifest.uses_game_asset_limits() {
            Self {
                max_each: MAX_TAPP_GAME_ASSET_BYTES,
                max_total: MAX_TAPP_GAME_ASSETS_TOTAL_BYTES,
            }
        } else {
            Self::standard()
        }
    }
}

/// Validate one package asset size and running total.
///
/// Returns the updated total after adding this asset.
pub fn validate_asset_resource_bytes(
    relative: &str,
    size: u64,
    total_so_far: u64,
) -> Result<u64, String> {
    validate_asset_resource_bytes_with(relative, size, total_so_far, AssetBudget::standard())
}

pub fn validate_asset_resource_bytes_with(
    relative: &str,
    size: u64,
    total_so_far: u64,
    budget: AssetBudget,
) -> Result<u64, String> {
    validate_asset_path(relative)?;
    if size > budget.max_each {
        return Err(format!(
            "Tapp asset exceeds {} bytes: {relative}",
            budget.max_each
        ));
    }
    let total = total_so_far
        .checked_add(size)
        .ok_or_else(|| "Tapp assets total size overflow".to_string())?;
    if total > budget.max_total {
        return Err(format!(
            "Tapp assets total size exceeds {} bytes",
            budget.max_total
        ));
    }
    Ok(total)
}

/// Validate an i18n directory entry filename (must be `{locale}.json` + safe).
pub fn validate_i18n_filename(filename: &str) -> Result<&str, String> {
    let Some(locale) = filename.strip_suffix(".json") else {
        return Err(format!(
            "Tapp i18n resource must be a JSON file: {filename}"
        ));
    };
    if !is_safe_path_component(filename) || !is_safe_path_component(locale) {
        return Err(format!("Invalid Tapp i18n resource: {filename}"));
    }
    Ok(locale)
}

/// Validate i18n file count against contract limit.
pub fn validate_i18n_file_count(count: usize) -> Result<(), String> {
    if count > MAX_TAPP_I18N_FILES {
        return Err(format!(
            "Tapp i18n accepts at most {MAX_TAPP_I18N_FILES} locale files"
        ));
    }
    Ok(())
}

/// Validate one i18n locale file body (size + JSON object).
pub fn validate_i18n_file_bytes(filename: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_TAPP_I18N_RESOURCE_BYTES {
        return Err(format!(
            "Tapp i18n resource exceeds {MAX_TAPP_I18N_RESOURCE_BYTES} bytes: {filename}"
        ));
    }
    let value = serde_json::from_slice::<serde_json::Value>(bytes)
        .map_err(|_| format!("Tapp i18n resource is not valid JSON: {filename}"))?;
    if !value.is_object() {
        return Err(format!(
            "Tapp i18n locale must contain a JSON object: {filename}"
        ));
    }
    Ok(())
}

// ── Direct install assets payload ───────────────────────────────────────────

/// Validate that a write-install assets map is allowed by `manifest.assets`.
///
/// - Non-empty payload requires at least one declared asset path.
/// - Every provided key must be listed in `declared` (exact string match).
/// - Path shape / size limits are checked separately per entry.
pub fn validate_write_assets_declaration(
    declared: Option<&[String]>,
    provided_keys: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<(), String> {
    let declared: std::collections::HashSet<&str> = declared
        .unwrap_or(&[])
        .iter()
        .map(String::as_str)
        .collect();
    let provided: Vec<String> = provided_keys
        .into_iter()
        .map(|key| key.as_ref().to_string())
        .collect();
    if declared.is_empty() && !provided.is_empty() {
        return Err("assets payload requires manifest.assets declarations".to_string());
    }
    for relative in &provided {
        if !declared.contains(relative.as_str()) {
            return Err(format!(
                "Asset path is not declared in manifest.assets: {relative}"
            ));
        }
    }
    Ok(())
}

// ── Archive entry pure rules ────────────────────────────────────────────────

/// Reject oversized archive file counts before iterating entries.
pub fn validate_archive_entry_count(count: usize) -> Result<(), String> {
    if count > MAX_TAPP_ARCHIVE_FILES {
        return Err(format!(
            "Tapp archive contains too many entries (max {MAX_TAPP_ARCHIVE_FILES})"
        ));
    }
    Ok(())
}

/// Validate one archive entry name/size and update running totals.
///
/// `paths` tracks duplicates; `total_size` is uncompressed bytes seen so far.
/// Directories contribute neither size nor must exist as files.
pub fn validate_archive_entry(
    name: &str,
    is_dir: bool,
    size: u64,
    paths: &mut HashSet<String>,
    total_size: u64,
) -> Result<u64, String> {
    crate::services::tapp_validation::validate_resource_path(name)?;
    if !paths.insert(name.to_string()) {
        return Err(format!("Duplicate Tapp archive entry: {name}"));
    }
    if is_dir {
        return Ok(total_size);
    }
    if size > MAX_TAPP_RESOURCE_BYTES {
        return Err(format!(
            "Tapp archive entry is too large: {name} (max {MAX_TAPP_RESOURCE_BYTES} bytes)"
        ));
    }
    let total = total_size
        .checked_add(size)
        .ok_or_else(|| "Tapp archive size overflow".to_string())?;
    if total > MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES {
        return Err(format!(
            "Tapp archive expands beyond {MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES} bytes"
        ));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_manifest() -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.resources",
            "name": "Resources",
            "version": "1.0.0",
            "main": "src/main.js",
            "category": "utility",
            "permissions": [],
            "styles": "css/shared.css",
            "pageModules": ["index.js"],
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "templates": { "2x2": "templates/summary.html" }
            }],
            "assets": ["assets/pixel.png"],
            "agent": {
                "protocolVersion": 2,
                "interactions": [{
                    "type": "report.compose",
                    "inputSchema": "schemas/input.json"
                }]
            }
        }))
        .unwrap()
    }

    #[test]
    fn collect_declared_paths_covers_manifest_surfaces() {
        let resources = collect_declared_install_resources(&sample_manifest());
        let paths: HashSet<_> = resources.iter().map(|r| r.relative.as_str()).collect();
        assert!(paths.contains("src/main.js"));
        assert!(paths.contains("css/shared.css"));
        assert!(paths.contains("page/index.js"));
        assert!(paths.contains("templates/summary.html"));
        assert!(paths.contains("assets/pixel.png"));
        assert!(paths.contains("schemas/input.json"));

        let agent = resources
            .iter()
            .find(|r| r.relative == "schemas/input.json")
            .unwrap();
        assert_eq!(agent.kind, DeclaredResourceKind::AgentSchema);
        let asset = resources
            .iter()
            .find(|r| r.relative == "assets/pixel.png")
            .unwrap();
        assert_eq!(asset.kind, DeclaredResourceKind::Asset);
        let main = resources
            .iter()
            .find(|r| r.relative == "src/main.js")
            .unwrap();
        assert_eq!(main.kind, DeclaredResourceKind::Text);
        let page = resources
            .iter()
            .find(|r| r.relative == "page/index.js")
            .unwrap();
        assert_eq!(page.kind, DeclaredResourceKind::PageModule);
    }

    #[test]
    fn text_resource_rejects_non_utf8() {
        assert!(validate_text_resource_bytes("main.js", b"ok").is_ok());
        assert!(validate_text_resource_bytes("main.js", &[0xff, 0xfe])
            .unwrap_err()
            .contains("UTF-8"));
    }

    #[test]
    fn agent_schema_rejects_ref_and_invalid_json() {
        assert!(validate_agent_schema_bytes(
            "schemas/input.json",
            br#"{"type":"object"}"#
        )
        .is_ok());
        assert!(validate_agent_schema_bytes("schemas/input.json", b"not-json")
            .unwrap_err()
            .contains("not valid JSON"));
        assert!(validate_agent_schema_bytes(
            "schemas/input.json",
            br#"{"$ref":"remote.json"}"#
        )
        .unwrap_err()
        .contains("does not support $ref"));
        let huge = vec![b'a'; MAX_AGENT_SCHEMA_RESOURCE_BYTES + 1];
        assert!(validate_agent_schema_bytes("schemas/input.json", &huge)
            .unwrap_err()
            .contains("exceeds"));
    }

    #[test]
    fn asset_totals_and_per_file_limits() {
        assert_eq!(
            validate_asset_resource_bytes("assets/a.png", 10, 0).unwrap(),
            10
        );
        assert!(validate_asset_resource_bytes(
            "assets/a.png",
            MAX_TAPP_ASSET_BYTES + 1,
            0
        )
        .unwrap_err()
        .contains("exceeds"));
        assert!(validate_asset_resource_bytes(
            "assets/a.png",
            1,
            MAX_TAPP_ASSETS_TOTAL_BYTES
        )
        .unwrap_err()
        .contains("total size exceeds"));
        assert!(validate_asset_resource_bytes("not-under-assets.png", 1, 0).is_err());
    }

    #[test]
    fn i18n_filename_and_body_rules() {
        assert_eq!(validate_i18n_filename("en-US.json").unwrap(), "en-US");
        assert!(validate_i18n_filename("en-US.txt")
            .unwrap_err()
            .contains("JSON file"));
        assert!(validate_i18n_filename("../x.json").is_err());

        assert!(validate_i18n_file_bytes("en-US.json", br#"{"title":"T"}"#).is_ok());
        assert!(validate_i18n_file_bytes("en-US.json", br#"["not","object"]"#)
            .unwrap_err()
            .contains("JSON object"));
        assert!(validate_i18n_file_count(MAX_TAPP_I18N_FILES).is_ok());
        assert!(validate_i18n_file_count(MAX_TAPP_I18N_FILES + 1).is_err());
    }

    #[test]
    fn archive_entry_rules_track_duplicates_and_size() {
        validate_archive_entry_count(1).unwrap();
        assert!(validate_archive_entry_count(MAX_TAPP_ARCHIVE_FILES + 1).is_err());

        let mut paths = HashSet::new();
        let total = validate_archive_entry("src/main.js", false, 10, &mut paths, 0).unwrap();
        assert_eq!(total, 10);
        assert!(validate_archive_entry("src/main.js", false, 1, &mut paths, total)
            .unwrap_err()
            .contains("Duplicate"));
        assert_eq!(
            validate_archive_entry("empty/", true, 0, &mut paths, total).unwrap(),
            total
        );
        assert!(validate_archive_entry(
            "big.bin",
            false,
            MAX_TAPP_RESOURCE_BYTES + 1,
            &mut HashSet::new(),
            0
        )
        .unwrap_err()
        .contains("too large"));
    }

    #[test]
    fn missing_resource_messages_mention_page_css_hint() {
        let msg = missing_after_install("page.css");
        assert!(msg.contains("page.css"));
        assert!(msg.contains("pageStyles") || msg.contains("pageCss"));
    }

    #[test]
    fn write_assets_declaration_requires_manifest_list() {
        assert!(validate_write_assets_declaration(None, ["assets/a.png"]).is_err());
        assert!(validate_write_assets_declaration(Some(&[]), ["assets/a.png"]).is_err());
        let declared = vec!["assets/a.png".to_string(), "assets/b.bin".to_string()];
        assert!(validate_write_assets_declaration(Some(&declared), ["assets/a.png"]).is_ok());
        assert!(validate_write_assets_declaration(
            Some(&declared),
            ["assets/missing.png"]
        )
        .unwrap_err()
        .contains("not declared"));
        // Empty payload always ok.
        assert!(validate_write_assets_declaration(None, std::iter::empty::<&str>()).is_ok());
    }
}
