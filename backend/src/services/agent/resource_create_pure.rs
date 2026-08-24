//! Pure helpers for agent resource_create handlers.
//!
//! Handlers keep DB/FS/AI. Domain owns:
//! - HTML escape
//! - generated Tapp JSON parse + manifest normalization
//! - report content rendering
//! - note/bookmark/reminder param projection
//! - storage id prefixes and auto titles

use crate::services::tapp_validation::{validate_resource_extension, validate_resource_path};
use serde_json::{json, Value};
use std::collections::HashMap;

// ── HTML ────────────────────────────────────────────────────────────────────

/// Minimal HTML entity escape for report HTML mode.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Generated Tapp JSON ─────────────────────────────────────────────────────

/// Parse AI-generated Tapp JSON (raw object or fenced / embedded braces).
pub fn parse_generated_tapp_json(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok().or_else(|| {
        let start = raw.find('{')?;
        let end = raw.rfind('}')?;
        serde_json::from_str(&raw[start..=end]).ok()
    })
}

/// Fallback structure when AI output is not valid JSON (code = whole response).
pub fn generated_tapp_fallback(raw: &str) -> Value {
    json!({
        "code": raw,
        "manifest": {
            "name": "Generated Tapp",
            "version": "1.0.0",
            "permissions": []
        }
    })
}

/// Obsolete top-level keys the layer contract no longer accepts.
const RETIRED_MANIFEST_FIELDS: &[&str] = &[
    "main",
    "hasPage",
    "cssMode",
    "styles",
    "pageTemplate",
    "pageStyles",
    "widgetStyles",
    "pageModules",
];

/// Normalize agent install/generate manifest fields before persist.
///
/// Sets id/name, default version / category / permissions, layer entries,
/// optional description/author. Drops retired top-level keys so the row
/// matches the current package contract.
pub fn normalize_agent_tapp_manifest(
    mut manifest: Value,
    tapp_id: &str,
    name: &str,
    description: Option<&str>,
    author: &Value,
) -> Result<Value, String> {
    let manifest_object = manifest
        .as_object_mut()
        .ok_or_else(|| "Tapp manifest must be an object".to_string())?;
    for field in RETIRED_MANIFEST_FIELDS {
        manifest_object.remove(*field);
    }
    manifest_object.insert("id".to_string(), json!(tapp_id));
    manifest_object.insert("name".to_string(), json!(name));
    manifest_object
        .entry("version".to_string())
        .or_insert_with(|| json!("1.0.0"));
    manifest_object
        .entry("category".to_string())
        .or_insert_with(|| json!("utility"));
    ensure_layer_entry(manifest_object, "core", "core.js")?;
    ensure_layer_entry(manifest_object, "page", "page/index.js")?;
    manifest_object
        .entry("permissions".to_string())
        .or_insert_with(|| json!([]));
    if let Some(description) = description {
        manifest_object.insert("description".to_string(), json!(description));
    }
    manifest_object
        .entry("author".to_string())
        .or_insert_with(|| author.clone());
    Ok(manifest)
}

fn ensure_layer_entry(
    manifest: &mut serde_json::Map<String, Value>,
    layer: &str,
    default_entry: &str,
) -> Result<String, String> {
    if !manifest.contains_key(layer) {
        manifest.insert(layer.to_string(), json!({ "entry": default_entry }));
    }
    let object = manifest
        .get_mut(layer)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{layer} must be an object"))?;
    let path = object
        .entry("entry".to_string())
        .or_insert_with(|| json!(default_entry))
        .as_str()
        .ok_or_else(|| format!("{layer}.entry must be a string"))?
        .to_string();
    validate_resource_path(&path)?;
    validate_resource_extension(&path, ".js", &format!("{layer}.entry"))?;
    Ok(path)
}

/// `require` 字面量：从 `from_module` 所在目录指向 `to_module`。
///
/// 与安装期 `resolve_require_target` 同构，只生成相对路径，不猜测默认文件名。
pub fn relative_require_request(from_module: &str, to_module: &str) -> Result<String, String> {
    validate_resource_path(from_module)?;
    validate_resource_path(to_module)?;
    if from_module == to_module {
        return Err("page.entry and core.entry must be different files".to_string());
    }
    let from_dir: Vec<&str> = match from_module.rsplit_once('/') {
        Some((dir, _)) => dir.split('/').filter(|part| !part.is_empty()).collect(),
        None => Vec::new(),
    };
    let to_parts: Vec<&str> = to_module
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let to_file = *to_parts
        .last()
        .ok_or_else(|| format!("Invalid Tapp resource path: {to_module}"))?;
    let to_dir = &to_parts[..to_parts.len() - 1];
    let mut common = 0;
    while common < from_dir.len() && common < to_dir.len() && from_dir[common] == to_dir[common] {
        common += 1;
    }
    let ups = from_dir.len() - common;
    let mut segments: Vec<&str> = vec![".."; ups];
    segments.extend_from_slice(&to_dir[common..]);
    segments.push(to_file);
    if ups == 0 {
        Ok(format!("./{}", segments.join("/")))
    } else {
        Ok(segments.join("/"))
    }
}

/// Page 入口里那一行：把共享层拉进来。写入路径必须等于声明入口。
pub fn agent_page_require_core_source(
    page_entry: &str,
    core_entry: &str,
) -> Result<String, String> {
    let request = relative_require_request(page_entry, core_entry)?;
    Ok(format!("require('{request}');\n"))
}

/// Extract permission strings from a manifest object.
pub fn manifest_permission_strings(manifest: &Value) -> Vec<String> {
    manifest
        .get("permissions")
        .and_then(Value::as_array)
        .map(|permissions| {
            permissions
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Require non-empty generated/installed code.
pub fn require_nonempty_code(code: &str) -> Result<(), String> {
    if code.trim().is_empty() {
        return Err("Generated Tapp code is empty".to_string());
    }
    Ok(())
}

/// Truncate upstream JSON for tapp.generate prompt context (char budget).
pub fn truncate_json_for_prompt(data: &Value, max_chars: usize) -> String {
    serde_json::to_string(data)
        .unwrap_or_default()
        .chars()
        .take(max_chars)
        .collect()
}

// ── Report ──────────────────────────────────────────────────────────────────

/// Allowed report formats for report.create.
pub fn is_valid_report_format(format: &str) -> bool {
    matches!(format, "markdown" | "html" | "json")
}

/// Render report body for the requested format.
pub fn render_report_content(
    title: &str,
    format: &str,
    analysis: &Value,
    generated_at_display: &str,
    generated_at_rfc3339: &str,
) -> String {
    match format {
        "markdown" => format!(
            "# {}\n\n生成时间：{}\n\n## 分析结果\n\n{}",
            title,
            generated_at_display,
            serde_json::to_string_pretty(analysis).unwrap_or_default()
        ),
        "html" => format!(
            "<h1>{}</h1><p>生成时间：{}</p><pre>{}</pre>",
            escape_html(title),
            generated_at_display,
            escape_html(&serde_json::to_string_pretty(analysis).unwrap_or_default())
        ),
        _ => serde_json::to_string_pretty(&json!({
            "title": title,
            "generatedAt": generated_at_rfc3339,
            "analysis": analysis
        }))
        .unwrap_or_default(),
    }
}

/// Storage key prefix + millis → report id.
pub fn format_report_id(millis: i64) -> String {
    format!("report_{millis}")
}

// ── Note / bookmark / reminder ──────────────────────────────────────────────

/// Extract note content from content / input / data params.
pub fn extract_note_content(params: &HashMap<String, Value>) -> Result<String, String> {
    if let Some(s) = params.get("content").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(input) = params.get("input").or_else(|| params.get("data")) {
        return Ok(match input.as_str() {
            Some(s) => s.to_string(),
            None => serde_json::to_string_pretty(input).unwrap_or_default(),
        });
    }
    Err("Missing content".to_string())
}

/// Auto title from explicit title or first N chars of content.
pub fn note_auto_title(title: Option<&str>, content: &str, max_chars: usize) -> String {
    title
        .map(|s| s.to_string())
        .unwrap_or_else(|| content.chars().take(max_chars).collect::<String>())
}

/// Collect string tags from a params array field.
pub fn extract_string_tags(params: &HashMap<String, Value>, key: &str) -> Vec<String> {
    params
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub fn format_note_id(millis: i64) -> String {
    format!("note_{millis}")
}

pub fn format_bookmark_id(millis: i64) -> String {
    format!("bookmark_{millis}")
}

pub fn format_reminder_id(millis: i64) -> String {
    format!("reminder_{millis}")
}

/// Default reminder repeat when omitted.
pub fn reminder_repeat_or_default(repeat: Option<&str>) -> &str {
    repeat.unwrap_or("none")
}

/// Extract `<title>...</title>` text from an HTML snippet (first occurrence).
pub fn extract_html_title(html: &str) -> Option<String> {
    let start = html.find("<title>")?;
    let end = html[start..].find("</title>")?;
    Some(html[start + 7..start + end].to_string())
}

/// Bookmark final title: explicit → fetched → Untitled.
pub fn resolve_bookmark_title<'a>(explicit: Option<&'a str>, fetched: Option<&'a str>) -> &'a str {
    explicit.or(fetched).unwrap_or("Untitled")
}

/// Cap HTML body size before title scan (chars).
pub const BOOKMARK_TITLE_HTML_CHAR_LIMIT: usize = 100_000;

/// Limit HTML for title extraction.
pub fn limit_html_for_title(html: &str) -> String {
    html.chars().take(BOOKMARK_TITLE_HTML_CHAR_LIMIT).collect()
}

// ── Storage namespace constants (agent tapp_id keys) ─────────────────────────

pub const AGENT_NOTES_TAPP_ID: &str = "agent_notes";
pub const AGENT_BOOKMARKS_TAPP_ID: &str = "agent_bookmarks";
pub const AGENT_REMINDERS_TAPP_ID: &str = "agent_reminders";
pub const AGENT_REPORTS_TAPP_ID: &str = "agent_reports";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_entities() {
        assert_eq!(escape_html(r#"a&b<c>"d""#), "a&amp;b&lt;c&gt;&quot;d&quot;");
    }

    #[test]
    fn parses_json_from_a_markdown_fence() {
        let parsed = parse_generated_tapp_json(
            "```json\n{\"manifest\":{\"name\":\"Demo\"},\"code\":\"console.log(1)\"}\n```",
        )
        .expect("fenced JSON should parse");
        assert_eq!(parsed["manifest"]["name"], "Demo");
        assert_eq!(parsed["code"], "console.log(1)");
    }

    #[test]
    fn parse_raw_object_and_fallback() {
        let v = parse_generated_tapp_json(r#"{"code":"x","manifest":{"name":"N"}}"#).unwrap();
        assert_eq!(v["code"], "x");
        let fb = generated_tapp_fallback("not json at all");
        assert_eq!(fb["code"], "not json at all");
        assert_eq!(fb["manifest"]["name"], "Generated Tapp");
    }

    #[test]
    fn normalize_manifest_and_permissions() {
        require_nonempty_code("console.log(1)").unwrap();
        assert!(require_nonempty_code("  ").is_err());

        let author = json!({"name": "Agent"});
        let m = normalize_agent_tapp_manifest(
            json!({
                "description": "d",
                "main": "main.js",
                "hasPage": true,
                "pageModules": ["index.js"]
            }),
            "agent.x",
            "App",
            Some("desc"),
            &author,
        )
        .unwrap();
        assert_eq!(m["id"], "agent.x");
        assert_eq!(m["name"], "App");
        assert_eq!(m["core"]["entry"], "core.js");
        assert_eq!(m["page"]["entry"], "page/index.js");
        assert_eq!(m["category"], "utility");
        assert!(m.get("main").is_none());
        assert!(m.get("hasPage").is_none());
        assert!(m.get("pageModules").is_none());
        assert_eq!(m["version"], "1.0.0");
        assert!(m["permissions"].is_array());
        assert_eq!(m["description"], "desc");

        let perms = manifest_permission_strings(&json!({
            "permissions": ["storage:read", 1, "network"]
        }));
        assert_eq!(
            perms,
            vec!["storage:read".to_string(), "network".to_string()]
        );
    }

    #[test]
    fn normalize_keeps_declared_layer_entries() {
        let m = normalize_agent_tapp_manifest(
            json!({
                "core": { "entry": "src/core.js", "styles": "styles.css" },
                "page": { "template": "shell.html" }
            }),
            "agent.x",
            "App",
            None,
            &json!({"name": "Agent"}),
        )
        .unwrap();
        assert_eq!(m["core"]["entry"], "src/core.js");
        assert_eq!(m["core"]["styles"], "styles.css");
        assert_eq!(m["page"]["entry"], "page/index.js");
        assert_eq!(m["page"]["template"], "shell.html");
    }

    #[test]
    fn normalize_rejects_unsafe_layer_entry() {
        let err = normalize_agent_tapp_manifest(
            json!({ "core": { "entry": "../core.js" } }),
            "agent.x",
            "App",
            None,
            &json!({"name": "Agent"}),
        )
        .unwrap_err();
        assert!(err.contains("Invalid Tapp resource path"));
    }

    #[test]
    fn page_require_follows_declared_entries() {
        use crate::services::tapp_install_resources::resolve_require_target;

        let source = agent_page_require_core_source("page/index.js", "core.js").unwrap();
        assert_eq!(source, "require('../core.js');\n");
        assert_eq!(
            resolve_require_target("page/index.js", "../core.js").as_deref(),
            Some("core.js")
        );

        let nested = agent_page_require_core_source("page/app.js", "src/core.js").unwrap();
        assert_eq!(nested, "require('../src/core.js');\n");
        assert_eq!(
            resolve_require_target("page/app.js", "../src/core.js").as_deref(),
            Some("src/core.js")
        );

        let siblings = agent_page_require_core_source("src/page.js", "src/core.js").unwrap();
        assert_eq!(siblings, "require('./core.js');\n");
        assert!(relative_require_request("core.js", "core.js").is_err());
        assert!(relative_require_request("../core.js", "page/index.js").is_err());
    }

    #[test]
    fn report_render_and_ids() {
        assert!(is_valid_report_format("markdown"));
        assert!(!is_valid_report_format("pdf"));
        let md = render_report_content(
            "T",
            "markdown",
            &json!({"a": 1}),
            "2026-01-01 00:00:00",
            "2026-01-01T00:00:00Z",
        );
        assert!(md.contains("# T"));
        assert!(md.contains("分析结果"));
        let html = render_report_content("<bad>", "html", &json!({}), "t", "t");
        assert!(html.contains("&lt;bad&gt;"));
        assert_eq!(format_report_id(42), "report_42");
    }

    #[test]
    fn note_bookmark_reminder_projection() {
        let mut params = HashMap::new();
        params.insert("content".into(), json!("hello world"));
        assert_eq!(extract_note_content(&params).unwrap(), "hello world");
        params.clear();
        params.insert("input".into(), json!({"k": 1}));
        assert!(extract_note_content(&params).unwrap().contains("\"k\""));
        assert!(extract_note_content(&HashMap::new()).is_err());

        assert_eq!(note_auto_title(Some("T"), "long", 3), "T");
        assert_eq!(note_auto_title(None, "abcdefghij", 4), "abcd");

        let mut params = HashMap::new();
        params.insert("tags".into(), json!(["a", 1, "b"]));
        assert_eq!(
            extract_string_tags(&params, "tags"),
            vec!["a".to_string(), "b".to_string()]
        );

        assert_eq!(format_note_id(1), "note_1");
        assert_eq!(format_bookmark_id(2), "bookmark_2");
        assert_eq!(format_reminder_id(3), "reminder_3");
        assert_eq!(reminder_repeat_or_default(None), "none");
        assert_eq!(reminder_repeat_or_default(Some("daily")), "daily");

        let title = extract_html_title("<html><title>Hello</title></html>");
        assert_eq!(title.as_deref(), Some("Hello"));
        assert_eq!(resolve_bookmark_title(Some("A"), Some("B")), "A");
        assert_eq!(resolve_bookmark_title(None, Some("B")), "B");
        assert_eq!(resolve_bookmark_title(None, None), "Untitled");
        assert_eq!(limit_html_for_title(&"x".repeat(10)).len(), 10);
    }
}
