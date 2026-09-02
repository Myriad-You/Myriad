//! Declared install-resource checks for staged Tapp packages.
//!
//! Pure path enumeration and content rules live here so post-stage validation
//! does not own contract messages only in the API layer. The API still resolves
//! sandbox paths (canonicalize / symlink rejection) and performs file IO.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use myriad_tapp_contract::manifest::TappManifest;

use crate::services::tapp_validation::{
    is_safe_path_component, validate_asset_path, validate_inline_data_schema,
    MAX_AGENT_SCHEMA_RESOURCE_BYTES, MAX_TAPP_ARCHIVE_BYTES, MAX_TAPP_ARCHIVE_FILES,
    MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES, MAX_TAPP_ASSETS_TOTAL_BYTES, MAX_TAPP_ASSET_BYTES,
    MAX_TAPP_GAME_ARCHIVE_BYTES, MAX_TAPP_GAME_ARCHIVE_FILES,
    MAX_TAPP_GAME_ARCHIVE_UNCOMPRESSED_BYTES, MAX_TAPP_GAME_ASSETS_TOTAL_BYTES,
    MAX_TAPP_GAME_ASSET_BYTES, MAX_TAPP_GAME_RESOURCE_BYTES, MAX_TAPP_I18N_FILES,
    MAX_TAPP_I18N_RESOURCE_BYTES, MAX_TAPP_RESOURCE_BYTES,
};

/// Kind of declared install resource used when reading/validating bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredResourceKind {
    /// UTF-8 text entrypoint / CSS / HTML / widget templates.
    Text,
    /// Page module under `page/` (same UTF-8 rule; distinct missing-file copy).
    ///
    /// 目前无构造点：安装期把 page/ 也按 Text 处理。校验分支保留，
    /// 需要区分 page 模块的缺失文案时直接可用。
    #[allow(dead_code)]
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
/// Order is stable for diagnostics: layer entries and layer styles, widget
/// templates, agent schemas, then assets. Files a layer entry pulls in via
/// `require` are scanned from the package rather than declared, so they are
/// not listed here. i18n is directory-scanned separately.
pub fn collect_declared_install_resources(manifest: &TappManifest) -> Vec<DeclaredInstallResource> {
    let mut resources = Vec::new();

    for entry in manifest.layer_entries() {
        resources.push(DeclaredInstallResource {
            relative: entry.to_string(),
            kind: DeclaredResourceKind::Text,
        });
    }
    let layer_styles = [
        manifest
            .core
            .as_ref()
            .and_then(|core| core.styles.as_deref()),
        manifest
            .page
            .as_ref()
            .and_then(|page| page.styles.as_deref()),
        manifest
            .page
            .as_ref()
            .and_then(|page| page.template.as_deref()),
    ];
    for optional in layer_styles.into_iter().flatten() {
        resources.push(DeclaredInstallResource {
            relative: optional.to_string(),
            kind: DeclaredResourceKind::Text,
        });
    }
    if let Some(widgets) = &manifest.widgets {
        for widget in widgets {
            if let Some(styles) = &widget.styles {
                resources.push(DeclaredInstallResource {
                    relative: styles.clone(),
                    kind: DeclaredResourceKind::Text,
                });
            }
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

/// 解析包内 `require` 目标：只接受字符串字面量的相对路径。
///
/// 这不是模块系统的第二份实现——运行时的装载与隔离仍只在宿主一侧。这里只做安装期
/// 的存在性检查，让「引用了不存在的文件」在装包时就失败，而不是等到打开应用。
pub fn resolve_require_target(from_module: &str, request: &str) -> Option<String> {
    let base = match from_module.rsplit_once('/') {
        Some((dir, _)) if !request.starts_with('/') => dir,
        _ => "",
    };
    let joined = if request.starts_with('/') {
        request.trim_start_matches('/').to_string()
    } else if base.is_empty() {
        request.to_string()
    } else {
        format!("{base}/{request}")
    };

    let mut resolved: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                // 逃出包根不折叠回根内，见运行时解析器里的同一条注释。
                resolved.pop()?;
            }
            other => resolved.push(other),
        }
    }
    if resolved.is_empty() {
        return None;
    }
    Some(resolved.join("/"))
}

/// Resolve one request against the package module table.
///
/// The CommonJS subset permits omitting `.js`, but never directory indexes or
/// JSON modules. Installation validation and resource projection both call
/// this helper so the accepted graph cannot drift from the graph sent to the
/// runtime.
pub fn resolve_require_against_modules(
    from_module: &str,
    request: &str,
    known: &HashSet<String>,
) -> Option<String> {
    let resolved = resolve_require_target(from_module, request)?;
    if known.contains(&resolved) {
        return Some(resolved);
    }
    let with_extension = format!("{resolved}.js");
    known.contains(&with_extension).then_some(with_extension)
}

/// Exact module closure and request-resolution table for a set of layer
/// entries. Paths are package-relative and independent of directory names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TappModuleGraph {
    pub included: Vec<String>,
    pub resolutions: BTreeMap<String, BTreeMap<String, String>>,
}

pub fn collect_tapp_module_graph(
    sources: &HashMap<String, String>,
    entries: &[String],
) -> Result<TappModuleGraph, String> {
    let known: HashSet<String> = sources.keys().cloned().collect();
    let mut queue: VecDeque<String> = entries.iter().cloned().collect();
    let mut seen = HashSet::new();
    let mut included = Vec::new();
    let mut resolutions = BTreeMap::new();

    for entry in entries {
        if !known.contains(entry) {
            return Err(format!("Declared Tapp layer entry is missing: {entry}"));
        }
    }

    while let Some(current) = queue.pop_front() {
        if !seen.insert(current.clone()) {
            continue;
        }
        let source = sources
            .get(&current)
            .ok_or_else(|| format!("Tapp module is missing: {current}"))?;
        included.push(current.clone());

        let mut module_resolutions = BTreeMap::new();
        for request in extract_require_requests(source) {
            let target = resolve_require_against_modules(&current, &request, &known)
                .ok_or_else(|| require_target_missing(&current, &request))?;
            module_resolutions.insert(request, target.clone());
            if !seen.contains(&target) {
                queue.push_back(target);
            }
        }
        if !module_resolutions.is_empty() {
            resolutions.insert(current, module_resolutions);
        }
    }

    included.sort();
    Ok(TappModuleGraph {
        included,
        resolutions,
    })
}

/// 提取一个模块直接 `require` 的字面量目标（原样，未解析）。
///
/// 先跳过注释与字符串字面量再扫，否则 `var s = "require('./x.js')"` 会被当成真的
/// 依赖，把一个能跑的包判成安装失败。
pub fn extract_require_requests(source: &str) -> Vec<String> {
    let mut requests = Vec::new();
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut index = 0usize;
    let mut previous_code_byte = 0u8;

    while index < len {
        let byte = bytes[index];

        // 注释
        if byte == b'/' && index + 1 < len {
            match bytes[index + 1] {
                b'/' => {
                    index = source[index..]
                        .find('\n')
                        .map_or(len, |offset| index + offset);
                    continue;
                }
                b'*' => {
                    index = source[index + 2..]
                        .find("*/")
                        .map_or(len, |offset| index + 2 + offset + 2);
                    continue;
                }
                _ => {}
            }
        }

        // 字符串 / 模板字面量：整体跳过，内部内容不参与扫描
        if matches!(byte, b'\'' | b'"' | b'`') {
            let quote = byte;
            index += 1;
            while index < len {
                match bytes[index] {
                    b'\\' => index += 2,
                    b if b == quote => {
                        index += 1;
                        break;
                    }
                    _ => index += 1,
                }
            }
            previous_code_byte = quote;
            continue;
        }

        // `require` 调用：标识符边界 + 括号 + 字符串字面量参数
        if byte == b'r' && source[index..].starts_with("require") {
            let boundary_ok =
                !(previous_code_byte == b'_' || previous_code_byte.is_ascii_alphanumeric());
            let rest = source[index + "require".len()..].trim_start();
            if boundary_ok {
                if let Some(inner) = rest.strip_prefix('(').map(str::trim_start) {
                    if let Some(quote) = inner.chars().next().filter(|c| *c == '\'' || *c == '"') {
                        let body = &inner[quote.len_utf8()..];
                        if let Some(end) = body.find(quote) {
                            if end > 0 {
                                requests.push(body[..end].to_string());
                            }
                        }
                    }
                }
            }
            index += "require".len();
            previous_code_byte = b'e';
            continue;
        }

        if !byte.is_ascii_whitespace() {
            previous_code_byte = byte;
        }
        index += 1;
    }

    requests
}

pub fn require_target_missing(from_module: &str, request: &str) -> String {
    format!(
        "Tapp module {from_module} requires {request}, which is not a file in the package. \
Use a relative path with the .js extension; dynamic require and node_modules are not supported."
    )
}

/// Error when a declared path fails basic existence / sandbox file checks.
pub fn invalid_declared_path(relative: &str) -> String {
    format!("Declared Tapp resource has invalid path: {relative}")
}

pub fn missing_after_install(relative: &str) -> String {
    format!(
        "Declared Tapp resource is missing after install (expected regular file): {relative}. \
Every layer entry and layer resource declared in the manifest must ship in the package."
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
    validate_inline_data_schema(&schema).map_err(|error| {
        tracing::error!(%error, relative, "invalid agent schema");
        format!("Invalid Agent schema {relative}: {error}")
    })
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
        if manifest.uses_game_package_limits() {
            Self {
                max_each: MAX_TAPP_GAME_ASSET_BYTES,
                max_total: MAX_TAPP_GAME_ASSETS_TOTAL_BYTES,
            }
        } else {
            Self::standard()
        }
    }
}

/// ZIP / `.tapp` size budget. Game packages use the raised ceiling.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveBudget {
    pub max_bytes: usize,
    pub max_files: usize,
    pub max_entry_bytes: u64,
    pub max_uncompressed_bytes: u64,
}

impl ArchiveBudget {
    pub fn standard() -> Self {
        Self {
            max_bytes: MAX_TAPP_ARCHIVE_BYTES,
            max_files: MAX_TAPP_ARCHIVE_FILES,
            max_entry_bytes: MAX_TAPP_RESOURCE_BYTES,
            max_uncompressed_bytes: MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES,
        }
    }

    pub fn game() -> Self {
        Self {
            max_bytes: MAX_TAPP_GAME_ARCHIVE_BYTES,
            max_files: MAX_TAPP_GAME_ARCHIVE_FILES,
            max_entry_bytes: MAX_TAPP_GAME_RESOURCE_BYTES,
            max_uncompressed_bytes: MAX_TAPP_GAME_ARCHIVE_UNCOMPRESSED_BYTES,
        }
    }

    pub fn ceiling() -> Self {
        Self::game()
    }

    pub fn for_manifest(manifest: &TappManifest) -> Self {
        if manifest.uses_game_package_limits() {
            Self::game()
        } else {
            Self::standard()
        }
    }

    pub fn check_compressed(&self, size: usize) -> Result<(), String> {
        if size > self.max_bytes {
            return Err(format!(".tapp file exceeds {} bytes", self.max_bytes));
        }
        Ok(())
    }
}

/// Validate one package asset size and running total.
///
/// Returns the updated total after adding this asset.
#[allow(dead_code)] // 仅测试调用：生产走同名 *_with(budget) 变体，这是默认预算的便捷包装。
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
    let declared: std::collections::HashSet<&str> =
        declared.unwrap_or(&[]).iter().map(String::as_str).collect();
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
#[allow(dead_code)] // 仅测试调用：生产走同名 *_with(budget) 变体，这是默认预算的便捷包装。
pub fn validate_archive_entry_count(count: usize) -> Result<(), String> {
    validate_archive_entry_count_with(count, ArchiveBudget::standard())
}

pub fn validate_archive_entry_count_with(
    count: usize,
    budget: ArchiveBudget,
) -> Result<(), String> {
    if count > budget.max_files {
        return Err(format!(
            "Tapp archive contains too many entries (max {})",
            budget.max_files
        ));
    }
    Ok(())
}

/// Validate one archive entry name/size and update running totals.
///
/// `paths` tracks duplicates; `total_size` is uncompressed bytes seen so far.
/// Directories contribute neither size nor must exist as files.
#[allow(dead_code)] // 仅测试调用：生产走同名 *_with(budget) 变体，这是默认预算的便捷包装。
pub fn validate_archive_entry(
    name: &str,
    is_dir: bool,
    size: u64,
    paths: &mut HashSet<String>,
    total_size: u64,
) -> Result<u64, String> {
    validate_archive_entry_with(
        name,
        is_dir,
        size,
        paths,
        total_size,
        ArchiveBudget::standard(),
    )
}

pub fn validate_archive_entry_with(
    name: &str,
    is_dir: bool,
    size: u64,
    paths: &mut HashSet<String>,
    total_size: u64,
    budget: ArchiveBudget,
) -> Result<u64, String> {
    crate::services::tapp_validation::validate_resource_path(name)?;
    if !paths.insert(name.to_string()) {
        return Err(format!("Duplicate Tapp archive entry: {name}"));
    }
    if is_dir {
        return Ok(total_size);
    }
    if size > budget.max_entry_bytes {
        return Err(format!(
            "Tapp archive entry is too large: {name} (max {} bytes)",
            budget.max_entry_bytes
        ));
    }
    let total = total_size
        .checked_add(size)
        .ok_or_else(|| "Tapp archive size overflow".to_string())?;
    if total > budget.max_uncompressed_bytes {
        return Err(format!(
            "Tapp archive expands beyond {} bytes",
            budget.max_uncompressed_bytes
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
            "core": { "entry": "src/core.js", "styles": "css/shared.css" },
            "page": { "entry": "page/index.js", "template": "page.html" },
            "category": "utility",
            "permissions": [],
            "widgets": [{
                "id": "summary",
                "name": "Summary",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "entry": "widget.js",
                "styles": "css/summary.css",
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
        assert!(paths.contains("src/core.js"));
        assert!(paths.contains("page/index.js"));
        assert!(paths.contains("widget.js"));
        assert!(paths.contains("css/shared.css"));
        assert!(paths.contains("css/summary.css"));
        assert!(paths.contains("page.html"));
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
        let core = resources
            .iter()
            .find(|r| r.relative == "src/core.js")
            .unwrap();
        assert_eq!(core.kind, DeclaredResourceKind::Text);
        let page = resources
            .iter()
            .find(|r| r.relative == "page/index.js")
            .unwrap();
        assert_eq!(page.kind, DeclaredResourceKind::Text);
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
        assert!(validate_agent_schema_bytes("schemas/input.json", br#"{"type":"object"}"#).is_ok());
        assert!(
            validate_agent_schema_bytes("schemas/input.json", b"not-json")
                .unwrap_err()
                .contains("not valid JSON")
        );
        assert!(
            validate_agent_schema_bytes("schemas/input.json", br#"{"$ref":"remote.json"}"#)
                .unwrap_err()
                .contains("does not support $ref")
        );
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
        assert!(
            validate_asset_resource_bytes("assets/a.png", MAX_TAPP_ASSET_BYTES + 1, 0)
                .unwrap_err()
                .contains("exceeds")
        );
        assert!(
            validate_asset_resource_bytes("assets/a.png", 1, MAX_TAPP_ASSETS_TOTAL_BYTES)
                .unwrap_err()
                .contains("total size exceeds")
        );
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
        assert!(
            validate_i18n_file_bytes("en-US.json", br#"["not","object"]"#)
                .unwrap_err()
                .contains("JSON object")
        );
        assert!(validate_i18n_file_count(MAX_TAPP_I18N_FILES).is_ok());
        assert!(validate_i18n_file_count(MAX_TAPP_I18N_FILES + 1).is_err());
    }

    #[test]
    fn archive_budget_game_is_larger_than_standard() {
        let standard = ArchiveBudget::standard();
        let game = ArchiveBudget::game();
        assert!(game.max_bytes > standard.max_bytes);
        assert!(game.max_files > standard.max_files);
        assert!(game.max_entry_bytes > standard.max_entry_bytes);
        assert!(game.max_uncompressed_bytes > standard.max_uncompressed_bytes);
        assert!(validate_archive_entry_count_with(standard.max_files + 1, standard).is_err());
        assert!(validate_archive_entry_count_with(standard.max_files + 1, game).is_ok());
        assert!(standard.check_compressed(standard.max_bytes).is_ok());
        assert!(standard.check_compressed(standard.max_bytes + 1).is_err());
    }

    #[test]
    fn archive_entry_rules_track_duplicates_and_size() {
        validate_archive_entry_count(1).unwrap();
        assert!(validate_archive_entry_count(MAX_TAPP_ARCHIVE_FILES + 1).is_err());

        let mut paths = HashSet::new();
        let total = validate_archive_entry("src/main.js", false, 10, &mut paths, 0).unwrap();
        assert_eq!(total, 10);
        assert!(
            validate_archive_entry("src/main.js", false, 1, &mut paths, total)
                .unwrap_err()
                .contains("Duplicate")
        );
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

    /// 与运行时解析器共用的用例表。
    ///
    /// 安装期这份只做存在性检查，运行时那份负责解析加装载，但两者必须对同一组
    /// 输入给出同一个答案。改这张表时同步改
    /// `frontend/src/tapp/runtime/moduleRuntime.test.ts` 里的同名用例。
    const SHARED_RESOLUTION_CASES: &[(&str, &str, Option<&str>)] = &[
        ("page/index.js", "./state.js", Some("page/state.js")),
        ("page/index.js", "../core.js", Some("core.js")),
        (
            "page/ui/list.js",
            "../state/store.js",
            Some("page/state/store.js"),
        ),
        ("core.js", "./lib/a.js", Some("lib/a.js")),
        // 逃出包根的写法被拒绝，不折叠回根内。
        ("core.js", "../../outside.js", None),
        ("core.js", "../core.js", None),
        ("page/index.js", "../../core.js", None),
    ];

    #[test]
    fn resolves_require_targets_like_the_runtime() {
        for (from, request, expected) in SHARED_RESOLUTION_CASES {
            assert_eq!(
                resolve_require_target(from, request).as_deref(),
                *expected,
                "resolving {request} from {from}"
            );
        }
    }

    #[test]
    fn module_graph_follows_entries_not_directory_names() {
        let sources = HashMap::from([
            (
                "src/core.js".to_string(),
                "exports.name = 'core';".to_string(),
            ),
            (
                "screens/detail.js".to_string(),
                "require('../src/core'); require('../shared/view.js');".to_string(),
            ),
            (
                "widgets/card.js".to_string(),
                "require('../shared/view.js');".to_string(),
            ),
            (
                "widgets/other.js".to_string(),
                "globalThis.other = true;".to_string(),
            ),
            (
                "shared/view.js".to_string(),
                "exports.ok = true;".to_string(),
            ),
        ]);

        let graph =
            collect_tapp_module_graph(&sources, &["src/core.js".into(), "widgets/card.js".into()])
                .unwrap();

        assert_eq!(
            graph.included,
            vec!["shared/view.js", "src/core.js", "widgets/card.js"]
        );
        assert!(!graph.included.contains(&"widgets/other.js".to_string()));
        assert_eq!(
            graph.resolutions["widgets/card.js"]["../shared/view.js"],
            "shared/view.js"
        );
    }

    #[test]
    fn module_graph_handles_cycles_and_rejects_missing_targets() {
        let cyclic = HashMap::from([
            ("a.js".to_string(), "require('./b.js');".to_string()),
            ("b.js".to_string(), "require('./a.js');".to_string()),
        ]);
        let graph = collect_tapp_module_graph(&cyclic, &["a.js".into()]).unwrap();
        assert_eq!(graph.included, vec!["a.js", "b.js"]);

        let broken = HashMap::from([(
            "entry.js".to_string(),
            "require('./missing.js');".to_string(),
        )]);
        assert!(collect_tapp_module_graph(&broken, &["entry.js".into()])
            .unwrap_err()
            .contains("requires ./missing.js"));
    }

    /// 与运行时提取器共用的用例。改这里时同步改
    /// `frontend/src/tapp/runtime/moduleRuntime.test.ts` 的同名用例。
    const SHARED_EXTRACTION_SOURCE: &str = r#"
        var a = require('./a.js')
        var b = require("../b.js")
        var dynamic = require(name)
        var similar = myRequire('./c.js')
        // require('./commented.js')
        /* require('./blocked.js') */
        var text = "require('./in-string.js')"
        var tpl = `require('./in-template.js')`
    "#;

    #[test]
    fn extracts_only_real_string_literal_requires() {
        assert_eq!(
            extract_require_requests(SHARED_EXTRACTION_SOURCE),
            vec!["./a.js".to_string(), "../b.js".to_string()]
        );
    }

    #[test]
    fn missing_resource_message_points_at_the_declaring_layer() {
        let msg = missing_after_install("page/index.js");
        assert!(msg.contains("page/index.js"));
        assert!(msg.contains("layer"));
    }

    #[test]
    fn write_assets_declaration_requires_manifest_list() {
        assert!(validate_write_assets_declaration(None, ["assets/a.png"]).is_err());
        assert!(validate_write_assets_declaration(Some(&[]), ["assets/a.png"]).is_err());
        let declared = vec!["assets/a.png".to_string(), "assets/b.bin".to_string()];
        assert!(validate_write_assets_declaration(Some(&declared), ["assets/a.png"]).is_ok());
        assert!(
            validate_write_assets_declaration(Some(&declared), ["assets/missing.png"])
                .unwrap_err()
                .contains("not declared")
        );
        // Empty payload always ok.
        assert!(validate_write_assets_declaration(None, std::iter::empty::<&str>()).is_ok());
    }
}
