//! Playground project validation helpers (production).

use super::*;
use serde_json::json;
use std::collections::HashSet;

pub(super) fn validate_playground_project(project: &PlaygroundProject) -> Result<(), String> {
    validate_tapp_manifest(&project.manifest)?;
    let manifest = &project.manifest;
    let code = &project.code;

    if manifest.version != "1.0.0" {
        return Err("Playground project version must remain 1.0.0".to_string());
    }
    if manifest.main != "main.js"
        || manifest.styles.as_deref() != Some("styles.css")
        || manifest.css_mode.as_deref() != Some("unified")
    {
        return Err("Playground requires main.js, styles.css, and unified CSS".to_string());
    }

    let manifest_widgets = manifest.widgets.as_deref().unwrap_or_default();
    let has_widgets = !manifest_widgets.is_empty();

    // Dual mode: Page and/or Widget-only. Reject empty projects (neither).
    if !manifest.has_page && !has_widgets {
        return Err(
            "Playground project requires a Page (hasPage) and/or non-empty Widgets".to_string(),
        );
    }

    if manifest.has_page {
        if manifest.page_template.as_deref() != Some("page.html") {
            return Err("Playground Page mode requires pageTemplate: page.html".to_string());
        }
        if code.page.trim().is_empty() || code.page_html.trim().is_empty() {
            return Err(
                "Playground project requires non-empty page code and HTML when hasPage is true"
                    .to_string(),
            );
        }
    } else {
        // Widget-only: pageTemplate optional/absent; page fields may be empty.
        if let Some(template) = manifest.page_template.as_deref() {
            if template != "page.html" {
                return Err("Playground pageTemplate must be page.html when declared".to_string());
            }
        }
        if code.widget.as_deref().is_none_or(str::is_empty)
            || code.widget_html.as_deref().is_none_or(str::is_empty)
        {
            return Err(
                "Widget-only Playground projects require non-empty code.widget and code.widgetHtml"
                    .to_string(),
            );
        }
    }

    let mut code_fields = vec![
        ("core", code.core.as_str()),
        ("page", code.page.as_str()),
        ("styles", code.styles.as_str()),
        ("pageHtml", code.page_html.as_str()),
    ];
    for (name, value) in [
        ("widget", code.widget.as_deref()),
        ("widgetHtml", code.widget_html.as_deref()),
        ("widgetCSS", code.widget_css.as_deref()),
        ("pageCSS", code.page_css.as_deref()),
    ] {
        if let Some(value) = value {
            code_fields.push((name, value));
        }
    }
    for (path, source) in &code.page_modules {
        code_fields.push((path.as_str(), source.as_str()));
    }
    for (name, value) in &code_fields {
        if value.len() > MAX_CODE_FIELD_BYTES {
            return Err(format!("{name} exceeds {MAX_CODE_FIELD_BYTES} bytes"));
        }
    }

    // Validate page HTML only when present (widget-only may leave it empty).
    if !code.page_html.trim().is_empty() {
        validate_template_html("pageHtml", &code.page_html)?;
    }
    if let Some(widget_html) = &code.widget_html {
        if !widget_html.trim().is_empty() {
            validate_template_html("widgetHtml", widget_html)?;
        }
    }
    validate_generated_source(&code_fields)?;
    validate_sdk_namespaces(&code_fields)?;
    validate_permission_usage(manifest, &code_fields)?;

    if has_widgets
        && (code.widget.as_deref().is_none_or(str::is_empty)
            || code.widget_html.as_deref().is_none_or(str::is_empty))
    {
        return Err(
            "Manifest Widgets require non-empty code.widget and code.widgetHtml".to_string(),
        );
    }

    let manifest_modules: HashSet<&str> = manifest
        .page_modules
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();
    let code_modules: HashSet<&str> = code.page_modules.keys().map(String::as_str).collect();
    if manifest_modules != code_modules {
        return Err(
            "manifest.pageModules and code.pageModules must contain the same paths".to_string(),
        );
    }
    if code.page_modules.len() > 64 {
        return Err("code.pageModules accepts at most 64 entries".to_string());
    }
    for (path, source) in &code.page_modules {
        if source.len() > MAX_CODE_FIELD_BYTES {
            return Err(format!(
                "Page module {path} exceeds {MAX_CODE_FIELD_BYTES} bytes"
            ));
        }
    }
    if !code.page_module_order.is_empty() {
        let order: HashSet<&str> = code.page_module_order.iter().map(String::as_str).collect();
        if order != manifest_modules || order.len() != code.page_module_order.len() {
            return Err(
                "code.pageModuleOrder must contain every manifest Page module exactly once"
                    .to_string(),
            );
        }
    }

    let manifest_assets: HashSet<&str> = manifest
        .assets
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();
    let code_assets: HashSet<&str> = code.assets.keys().map(String::as_str).collect();
    if manifest_assets != code_assets {
        return Err("manifest.assets and code.assets must contain the same paths".to_string());
    }
    if code.assets.len() > 64 {
        return Err("code.assets accepts at most 64 entries".to_string());
    }
    if code
        .assets
        .values()
        .any(|value| value.len() > MAX_CODE_FIELD_BYTES)
    {
        return Err(format!(
            "Each encoded asset must not exceed {MAX_CODE_FIELD_BYTES} bytes"
        ));
    }

    if code.i18n.len() > 16 || code.i18n.values().any(|value| !value.is_object()) {
        return Err("i18n must contain at most 16 locale objects".to_string());
    }
    let bytes =
        serde_json::to_vec(project).map_err(|_| "project cannot be serialized".to_string())?;
    if bytes.len() > MAX_PROJECT_BYTES {
        return Err(format!("project exceeds {MAX_PROJECT_BYTES} bytes"));
    }
    Ok(())
}

pub(super) fn validate_template_html(name: &str, html: &str) -> Result<(), String> {
    let lower = html.to_ascii_lowercase();
    let forbidden = [
        "<script",
        "javascript:",
        " onclick=",
        " onload=",
        " onerror=",
        " onsubmit=",
        " oninput=",
        " onchange=",
    ];
    if let Some(pattern) = forbidden.iter().find(|pattern| lower.contains(**pattern)) {
        return Err(format!("{name} contains forbidden HTML pattern: {pattern}"));
    }
    Ok(())
}

pub(super) fn validate_generated_source(fields: &[(&str, &str)]) -> Result<(), String> {
    let forbidden = [
        ("direct fetch", "fetch("),
        ("XMLHttpRequest", "xmlhttprequest"),
        ("WebSocket", "websocket("),
        ("eval", "eval("),
        ("Function constructor", "new function"),
        ("document.write", "document.write"),
        ("parent window access", "window.parent"),
        ("cookie access", "document.cookie"),
        ("localStorage", "localstorage"),
        ("sessionStorage", "sessionstorage"),
        ("dynamic import", "import("),
    ];
    for (name, source) in fields {
        let lower = source.to_ascii_lowercase();
        if let Some((capability, _)) = forbidden
            .iter()
            .find(|(_, pattern)| lower.contains(pattern))
        {
            return Err(format!("{name} uses forbidden capability: {capability}"));
        }
    }
    Ok(())
}

fn calls_sdk_method(source: &str, method: &str) -> bool {
    let mut rest = source;
    while let Some(position) = rest.find(method) {
        let after = &rest[position + method.len()..];
        match after.chars().next() {
            Some(character) if character.is_ascii_alphanumeric() => {
                rest = &rest[position + 1..];
            }
            _ => return true,
        }
    }
    false
}

pub(super) fn validate_permission_usage(
    manifest: &TappManifest,
    fields: &[(&str, &str)],
) -> Result<(), String> {
    let source = fields
        .iter()
        .map(|(_, value)| *value)
        .collect::<Vec<_>>()
        .join("\n");
    let required = [
        (
            &[
                "Tapp.storage.get",
                "Tapp.storage.keys",
                "Tapp.storage.getAll",
                "Tapp.storage.usage",
                "Tapp.settings.get",
                "Tapp.settings.getAll",
                "Tapp.file.download",
            ][..],
            "storage:read",
        ),
        (
            &[
                "Tapp.storage.set",
                "Tapp.storage.remove",
                "Tapp.storage.clear",
                "Tapp.settings.set",
            ][..],
            "storage:write",
        ),
        (&["Tapp.ui.confirm"][..], "ui:confirm"),
        (&["Tapp.ui.requestFullscreen"][..], "ui:fullscreen"),
        (&["Tapp.widget.register"][..], "widget:register"),
    ];
    for (needles, permission) in required {
        if let Some(needle) = needles
            .iter()
            .copied()
            .find(|needle| calls_sdk_method(&source, needle))
        {
            if !manifest
                .permissions
                .iter()
                .any(|declared| declared == permission)
            {
                return Err(format!(
                    "Code calls {needle} but manifest.permissions is missing {permission}"
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_sdk_namespaces(fields: &[(&str, &str)]) -> Result<(), String> {
    const KNOWN_NAMESPACES: &[&str] = &[
        "id",
        "version",
        "name",
        "permissions",
        "lifecycle",
        "i18n",
        "widget",
        "widgets",
        "pages",
        "tappList",
        "brewList",
        "platform",
        "ai",
        "report",
        "storage",
        "dataExchange",
        "settings",
        "ui",
        "data",
        "api",
        "context",
        "media",
        "component",
        "shortcut",
        "event",
        "dom",
        "file",
        "assets",
        "user",
        "background",
        "scheduler",
        "dynamicContent",
        "animation",
        "speech",
        "federation",
        "on",
    ];

    for (field, source) in fields {
        let mut remaining = *source;
        while let Some(position) = remaining.find("Tapp.") {
            let after_prefix = &remaining[position + "Tapp.".len()..];
            let namespace = after_prefix
                .chars()
                .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                .collect::<String>();
            if !namespace.is_empty() && !KNOWN_NAMESPACES.contains(&namespace.as_str()) {
                return Err(format!(
                    "{field} uses unknown Tapp SDK namespace: Tapp.{namespace}"
                ));
            }
            remaining = after_prefix.get(namespace.len()..).unwrap_or_default();
        }
    }
    Ok(())
}

/// Host capabilities available in temporary Playground preview only (MYR-024).
///
/// Manifest `permissions` are install-time declarations. Preview must never
/// treat the full list as granted host capabilities — only this allowlist may
/// be exercised, and only when also declared. Keep in sync with frontend
/// `PREVIEW_PERMISSIONS` in `frontend/src/tapp/utils/previewGrants.ts`.
pub(super) const PREVIEW_PERMISSIONS: &[&str] = &[
    "storage:read",
    "storage:write",
    "ui:theme",
    "ui:confirm",
    "ui:fullscreen",
    "ui:openUrl",
];

/// Intersect manifest declarations with temporary preview grants.
///
/// Deny-by-default: undeclared allowlist entries are not auto-granted.
pub(super) fn select_preview_granted_permissions(declared: &[String]) -> Vec<String> {
    declared
        .iter()
        .filter(|permission| PREVIEW_PERMISSIONS.contains(&permission.as_str()))
        .cloned()
        .collect()
}

pub(super) fn preview_warnings(permissions: &[String]) -> Vec<String> {
    let unavailable: Vec<&str> = permissions
        .iter()
        .map(String::as_str)
        .filter(|permission| !PREVIEW_PERMISSIONS.contains(permission))
        .collect();
    if unavailable.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "These permissions are disabled in temporary preview and become available only after installation approval: {}",
            unavailable.join(", ")
        )]
    }
}

pub(super) fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

// api_error lives in types_generate (sibling submodule)

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project(name: &str) -> PlaygroundProject {
        let raw = json!({
            "manifest": {
                "id": format!("com.myriad.playground.{}", name.to_ascii_lowercase()),
                "name": name,
                "version": "1.0.0",
                "description": format!("A {name}"),
                "author": { "name": "Myriad Playground" },
                "main": "main.js",
                "styles": "styles.css",
                "pageTemplate": "page.html",
                "cssMode": "unified",
                "permissions": ["storage:read"],
                "icon": "🧪",
                "themeColor": "#7C3AED",
                "hasPage": true,
                "category": "developer"
            },
            "code": {
                "core": "",
                "page": "Tapp.lifecycle.onReady(function () {});",
                "styles": ".app { color: var(--color-primary); }",
                "pageHtml": format!("<main class=\"app\">{name}</main>"),
                "i18n": { "zh-CN": {}, "en-US": {}, "ja-JP": {} }
            }
        });
        serde_json::from_value(raw).expect("sample project")
    }

    fn project_json() -> String {
        json!({
            "project": {
                "manifest": {
                    "id": "com.myriad.playground.counter",
                    "name": "Counter",
                    "version": "1.0.0",
                    "description": "A counter",
                    "author": { "name": "Myriad Playground" },
                    "main": "main.js",
                    "styles": "styles.css",
                    "pageTemplate": "page.html",
                    "cssMode": "unified",
                    "permissions": ["storage:read"],
                    "icon": "🧪",
                    "themeColor": "#7C3AED",
                    "hasPage": true,
                    "category": "developer"
                },
                "code": {
                    "core": "",
                    "page": "Tapp.lifecycle.onReady(function () {});",
                    "styles": ".app { color: var(--color-primary); }",
                    "pageHtml": "<main class=\"app\">Counter</main>",
                    "i18n": { "zh-CN": {}, "en-US": {}, "ja-JP": {} }
                }
            },
            "explanation": "Created a counter."
        })
        .to_string()
    }

    #[test]
    fn codegen_messages_empty_history_is_create_only() {
        let messages = build_codegen_messages(&[], "FINAL USER CONTENT").expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "FINAL USER CONTENT");
    }

    #[test]
    fn codegen_messages_two_turn_modify_order() {
        let p1 = sample_project("One");
        let p2 = sample_project("Two");
        let history = vec![
            PlaygroundHistoryTurn {
                instruction: "Create a counter".into(),
                explanation: "Created counter v1".into(),
                origin: Some("user".into()),
                created_at: 1,
                warnings: vec![],
                validation: None,
                project: Some(p1.clone()),
                failed: false,
                error: None,
            },
            PlaygroundHistoryTurn {
                instruction: "Make it blue".into(),
                explanation: "Theme is now blue".into(),
                origin: Some("user".into()),
                created_at: 2,
                warnings: vec![],
                validation: None,
                project: Some(p2.clone()),
                failed: false,
                error: None,
            },
        ];
        let messages =
            build_codegen_messages(&history, "FINAL: add reset button").expect("messages");
        // 2 successful turns => 4 messages + final user
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.contains("Create a counter"));
        assert!(messages[0]
            .content
            .contains("<previous_project>null</previous_project>"));
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content.contains("Created counter v1"));
        assert!(messages[1].content.contains(&p1.manifest.id));
        assert_eq!(messages[2].role, "user");
        assert!(messages[2].content.contains("Make it blue"));
        // Second user turn should carry previous project (turn 1 output)
        assert!(messages[2].content.contains(&p1.manifest.id));
        assert_eq!(messages[3].role, "assistant");
        assert!(messages[3].content.contains("Theme is now blue"));
        assert!(messages[3].content.contains(&p2.manifest.id));
        assert_eq!(messages[4].role, "user");
        assert_eq!(messages[4].content, "FINAL: add reset button");
    }

    #[test]
    fn codegen_messages_include_failed_attempt_tail() {
        let p1 = sample_project("Base");
        let history = vec![
            PlaygroundHistoryTurn {
                instruction: "Create app".into(),
                explanation: "Done".into(),
                origin: Some("user".into()),
                created_at: 1,
                warnings: vec![],
                validation: None,
                project: Some(p1.clone()),
                failed: false,
                error: None,
            },
            PlaygroundHistoryTurn {
                instruction: "Add broken feature".into(),
                explanation: String::new(),
                origin: Some("user".into()),
                created_at: 2,
                warnings: vec![],
                validation: None,
                project: Some(p1.clone()),
                failed: true,
                error: Some("validation failed: bad field".into()),
            },
        ];
        let messages = build_codegen_messages(&history, "FINAL retry").expect("messages");
        // success pair (2) + failed user (1) + final (1)
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[2].role, "user");
        assert!(messages[2].content.contains("previous attempt"));
        assert!(messages[2].content.contains("Add broken feature"));
        assert!(messages[2].content.contains("validation failed: bad field"));
        assert!(messages[2].content.contains(&p1.manifest.id));
        // Failed tail uses compact summary, not a full source dump.
        assert!(messages[2].content.contains("<project_summary>"));
        assert!(!messages[2].content.contains("Tapp.lifecycle.onReady"));
        assert_eq!(messages[3].content, "FINAL retry");
    }

    #[test]
    fn codegen_messages_older_turns_use_compact_summary_last_k_full() {
        // 3 successful turns with K=2 → turn 0 compact; turns 1-2 full.
        let p1 = sample_project("One");
        let p2 = sample_project("Two");
        let p3 = sample_project("Three");
        // Distinct source markers so we can assert omission.
        let mut p1 = p1;
        p1.code.page = "/* SOURCE_TURN_1_UNIQUE */ Tapp.lifecycle.onReady(function () {});".into();
        let mut p2 = p2;
        p2.code.page = "/* SOURCE_TURN_2_UNIQUE */ Tapp.lifecycle.onReady(function () {});".into();
        let mut p3 = p3;
        p3.code.page = "/* SOURCE_TURN_3_UNIQUE */ Tapp.lifecycle.onReady(function () {});".into();

        let history = vec![
            PlaygroundHistoryTurn {
                instruction: "Create one".into(),
                explanation: "Made one".into(),
                origin: Some("user".into()),
                created_at: 1,
                warnings: vec![],
                validation: None,
                project: Some(p1.clone()),
                failed: false,
                error: None,
            },
            PlaygroundHistoryTurn {
                instruction: "Create two".into(),
                explanation: "Made two".into(),
                origin: Some("user".into()),
                created_at: 2,
                warnings: vec![],
                validation: None,
                project: Some(p2.clone()),
                failed: false,
                error: None,
            },
            PlaygroundHistoryTurn {
                instruction: "Create three".into(),
                explanation: "Made three".into(),
                origin: Some("manual".into()),
                created_at: 3,
                warnings: vec![],
                validation: None,
                project: Some(p3.clone()),
                failed: false,
                error: None,
            },
        ];
        let messages = build_codegen_messages(&history, "FINAL: keep going").expect("messages");
        // 3 pairs + final
        assert_eq!(messages.len(), 7);

        // Oldest successful turn: compact, no full source.
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.contains("Create one"));
        assert!(messages[0].content.contains("compact memory"));
        assert!(messages[0].content.contains("<previous_project_summary>"));
        assert!(!messages[0].content.contains("SOURCE_TURN_"));
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[1].content.contains("Made one"));
        assert!(messages[1].content.contains("<project_summary>"));
        assert!(messages[1].content.contains(&p1.manifest.id));
        assert!(messages[1].content.contains("page="));
        assert!(!messages[1].content.contains("SOURCE_TURN_1_UNIQUE"));
        assert!(!messages[1].content.contains("PROJECT JSON:"));

        // Last K=2 turns keep full project JSON.
        assert_eq!(messages[2].role, "user");
        assert!(messages[2].content.contains("Create two"));
        assert!(!messages[2].content.contains("compact memory"));
        assert_eq!(messages[3].role, "assistant");
        assert!(messages[3].content.contains("PROJECT JSON:"));
        assert!(messages[3].content.contains("SOURCE_TURN_2_UNIQUE"));
        assert!(messages[3].content.contains(&p2.manifest.id));

        assert_eq!(messages[4].role, "user");
        assert!(messages[4].content.contains("Create three"));
        // Previous project for last-K turn should be full JSON of turn 2.
        assert!(messages[4].content.contains("<previous_project>"));
        assert!(messages[4].content.contains("SOURCE_TURN_2_UNIQUE"));
        assert_eq!(messages[5].role, "assistant");
        assert!(messages[5].content.contains("SOURCE_TURN_3_UNIQUE"));
        assert!(messages[5].content.contains(&p3.manifest.id));

        // Order preserved; final user content last.
        assert_eq!(messages[6].role, "user");
        assert_eq!(messages[6].content, "FINAL: keep going");
    }

    #[test]
    fn compact_project_summary_omits_source_body() {
        let mut project = sample_project("Summary");
        project.code.page = "SECRET_SOURCE_BODY_SHOULD_NOT_APPEAR".into();
        let summary = compact_project_summary(&project);
        assert!(summary.contains(&project.manifest.id));
        assert!(summary.contains("Summary"));
        assert!(summary.contains("1.0.0"));
        assert!(summary.contains("permissions="));
        assert!(summary.contains("page="));
        assert!(!summary.contains("SECRET_SOURCE_BODY_SHOULD_NOT_APPEAR"));
    }

    #[test]
    fn validation_repair_appends_without_dropping_history() {
        let p1 = sample_project("Base");
        let history = vec![PlaygroundHistoryTurn {
            instruction: "Create app".into(),
            explanation: "Done".into(),
            origin: Some("user".into()),
            created_at: 1,
            warnings: vec![],
            validation: None,
            project: Some(p1),
            failed: false,
            error: None,
        }];
        let mut messages = build_codegen_messages(&history, "FINAL modify").expect("base");
        let base_len = messages.len();
        assert_eq!(base_len, 3); // user+assistant+final
        messages.push(ChatMessage::user(
            "VALIDATION TOOL RESULT:\n<validation_error>missing field</validation_error>"
                .to_string(),
        ));
        assert_eq!(messages.len(), base_len + 1);
        // History pair still present at the front
        assert_eq!(messages[0].role, "user");
        assert!(messages[0].content.contains("Create app"));
        assert_eq!(messages[1].role, "assistant");
        assert!(messages[base_len].content.contains("missing field"));
    }

    #[test]
    fn history_rejects_too_many_turns() {
        let project = sample_project("Many");
        let history: Vec<PlaygroundHistoryTurn> = (0..MAX_HISTORY_TURNS + 1)
            .map(|i| PlaygroundHistoryTurn {
                instruction: format!("turn {i}"),
                explanation: "ok".into(),
                origin: Some("user".into()),
                created_at: i as i64,
                warnings: vec![],
                validation: None,
                project: Some(project.clone()),
                failed: false,
                error: None,
            })
            .collect();
        let err = validate_history(&history).unwrap_err();
        assert_eq!(err.0.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn prior_instructions_list_is_ordered_full_text() {
        let history = vec![
            PlaygroundHistoryTurn {
                instruction: "First full instruction text".into(),
                explanation: "a".into(),
                origin: Some("user".into()),
                created_at: 1,
                warnings: vec![],
                validation: None,
                project: Some(sample_project("A")),
                failed: false,
                error: None,
            },
            PlaygroundHistoryTurn {
                instruction: "Second full instruction text".into(),
                explanation: "b".into(),
                origin: Some("runtime-repair".into()),
                created_at: 2,
                warnings: vec![],
                validation: None,
                project: Some(sample_project("B")),
                failed: false,
                error: None,
            },
            PlaygroundHistoryTurn {
                instruction: "failed not listed".into(),
                explanation: String::new(),
                origin: Some("user".into()),
                created_at: 3,
                warnings: vec![],
                validation: None,
                project: None,
                failed: true,
                error: Some("boom".into()),
            },
        ];
        let text = format_prior_instructions(&history);
        assert!(text.contains("1. [user] First full instruction text"));
        assert!(text.contains("2. [runtime-repair] Second full instruction text"));
        assert!(!text.contains("failed not listed"));
    }

    #[test]
    fn accepts_valid_project_with_surrounding_model_text() {
        let raw = format!("```json\n{}\n```", project_json());
        let (output, normalized_aliases) =
            parse_and_validate_model_output(&raw).expect("valid project");
        assert_eq!(normalized_aliases, 0);
        assert_eq!(output.project.manifest.version, "1.0.0");
        assert!(output.project.manifest.has_page);
    }

    #[test]
    fn canonicalizes_generated_setting_default_alias() {
        let mut value: Value = serde_json::from_str(&project_json()).unwrap();
        value["project"]["manifest"]["settings"] = json!([{
            "key": "compact",
            "label": "Compact mode",
            "type": "toggle",
            "default": true
        }]);
        let (output, normalized_aliases) =
            parse_and_validate_model_output(&value.to_string()).expect("normalized project");
        assert_eq!(normalized_aliases, 1);
        assert_eq!(
            output.project.manifest.settings.unwrap()[0].default_value,
            Some(json!(true))
        );
    }

    #[test]
    fn rejects_script_in_page_html() {
        let mut value: Value = serde_json::from_str(&project_json()).unwrap();
        value["project"]["code"]["pageHtml"] = json!("<script>alert(1)</script>");
        let error = parse_and_validate_model_output(&value.to_string()).unwrap_err();
        assert!(error.contains("pageHtml contains forbidden HTML pattern"));
    }

    #[test]
    fn rejects_direct_network_access_in_generated_code() {
        let mut value: Value = serde_json::from_str(&project_json()).unwrap();
        value["project"]["code"]["page"] = json!("fetch('https://example.com')");
        let error = parse_and_validate_model_output(&value.to_string()).unwrap_err();
        assert!(error.contains("direct fetch"));
    }

    #[test]
    fn rejects_invented_sdk_namespaces() {
        let mut value: Value = serde_json::from_str(&project_json()).unwrap();
        value["project"]["code"]["page"] = json!("Tapp.translation.magic('title');");
        let error = parse_and_validate_model_output(&value.to_string()).unwrap_err();
        assert!(error.contains("unknown Tapp SDK namespace"));
    }

    #[test]
    fn planner_contract_is_bounded() {
        let plan = parse_agent_plan(
            r#"{"queries":["Tapp widget sizes"],"capabilities":["widget"],"acceptanceCriteria":["renders"]}"#,
        )
        .expect("valid plan");
        assert_eq!(plan.queries.len(), 1);

        let invalid = r#"{"queries":[],"capabilities":[],"acceptanceCriteria":[]}"#;
        assert!(parse_agent_plan(invalid).is_err());
    }

    #[test]
    fn runtime_feedback_is_bounded() {
        assert!(validate_runtime_feedback(&["ReferenceError: missingValue".into()]).is_ok());
        assert!(validate_runtime_feedback(&vec!["error".into(); 9]).is_err());
    }

    #[test]
    fn reports_permissions_unavailable_in_preview() {
        let warnings = preview_warnings(&[
            "storage:read".into(),
            "storage:write".into(),
            "network:fetch".into(),
        ]);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("network:fetch"));
        assert!(!warnings[0].contains("storage,"));
        assert!(!warnings[0].contains("storage:read"));
        assert!(!warnings[0].contains("storage:write"));
    }

    #[test]
    fn storage_permission_usage_requires_split_tokens() {
        let mut project = sample_project("Notes");
        project.manifest.permissions = vec!["storage:read".into()];
        assert!(validate_permission_usage(
            &project.manifest,
            &[("page", "Tapp.storage.get('key')")]
        )
        .is_ok());
        assert!(validate_permission_usage(
            &project.manifest,
            &[("page", "Tapp.storage.getAll()")]
        )
        .is_ok());

        let write_error = validate_permission_usage(
            &project.manifest,
            &[("page", "Tapp.storage.set('key', 1)")],
        )
        .expect_err("writes require storage:write");
        assert!(write_error.contains("storage:write"), "{write_error}");
        assert!(
            !write_error.contains("missing storage\""),
            "{write_error}"
        );

        project.manifest.permissions = vec!["storage".into()];
        let retired = validate_permission_usage(
            &project.manifest,
            &[("page", "Tapp.storage.get('key'); Tapp.storage.set('key', 1)")],
        )
        .expect_err("retired storage does not satisfy split tokens");
        assert!(retired.contains("storage:read") || retired.contains("storage:write"), "{retired}");
    }

    #[test]
    fn preview_grants_are_allowlist_intersection_not_full_manifest() {
        // MYR-024: declared ≠ granted for real host capabilities in preview.
        let declared = vec![
            "storage:read".into(),
            "storage:write".into(),
            "storage".into(),
            "network:fetch".into(),
            "ai:generate".into(),
            "ui:theme".into(),
            "platform:read".into(),
        ];
        assert_eq!(
            select_preview_granted_permissions(&declared),
            vec![
                "storage:read".to_string(),
                "storage:write".to_string(),
                "ui:theme".to_string()
            ]
        );
        assert!(select_preview_granted_permissions(&[]).is_empty());
        // Deny-by-default: allowlist entries not declared stay ungranted.
        assert_eq!(
            select_preview_granted_permissions(&["ui:confirm".into()]),
            vec!["ui:confirm".to_string()]
        );
        assert!(select_preview_granted_permissions(&["media:read".into()]).is_empty());
    }

    #[test]
    fn strips_widget_template_paths_from_assets_before_validate() {
        let mut value: Value = serde_json::from_str(&project_json()).unwrap();
        value["project"]["manifest"]["permissions"] = json!(["widget:register"]);
        value["project"]["manifest"]["widgets"] = json!([{
            "id": "summary",
            "name": "Summary",
            "defaultSize": "2x2",
            "sizes": ["2x2"],
            "category": "utility",
            "templates": { "2x2": "templates/widget-2x2.html" }
        }]);
        value["project"]["manifest"]["assets"] = json!(["templates/widget-2x2.html"]);
        value["project"]["code"]["widget"] = json!("Tapp.lifecycle.onReady(function () {});");
        value["project"]["code"]["widgetHtml"] = json!("<div class=\"widget\">Hi</div>");
        value["project"]["code"]["assets"] = json!({
            "templates/widget-2x2.html": "<div>should not be an asset</div>"
        });

        let (output, normalized) =
            parse_and_validate_model_output(&value.to_string()).expect("normalized project");
        assert!(
            normalized >= 2,
            "expected at least two asset path removals, got {normalized}"
        );
        assert!(
            output
                .project
                .manifest
                .assets
                .as_ref()
                .is_none_or(|assets| assets.is_empty()),
            "manifest.assets should be empty after stripping template path"
        );
        assert!(
            output.project.code.assets.is_empty(),
            "code.assets should be empty after stripping template path"
        );
        assert_eq!(
            output.project.code.widget_html.as_deref(),
            Some("<div class=\"widget\">Hi</div>")
        );
    }

    #[test]
    fn keeps_valid_binary_assets_during_normalize() {
        let mut value: Value = serde_json::from_str(&project_json()).unwrap();
        // Minimal 1x1 PNG
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        value["project"]["manifest"]["assets"] = json!(["assets/icon.png"]);
        value["project"]["code"]["assets"] = json!({
            "assets/icon.png": png_b64
        });

        let (output, normalized) =
            parse_and_validate_model_output(&value.to_string()).expect("valid binary asset");
        assert_eq!(normalized, 0);
        assert_eq!(
            output.project.manifest.assets.as_deref(),
            Some(vec!["assets/icon.png".to_string()].as_slice())
        );
        assert_eq!(
            output
                .project
                .code
                .assets
                .get("assets/icon.png")
                .map(String::as_str),
            Some(png_b64)
        );
    }

    #[test]
    fn strips_entrypoint_asset_paths_but_keeps_real_assets() {
        let mut value: Value = serde_json::from_str(&project_json()).unwrap();
        let png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
        value["project"]["manifest"]["assets"] =
            json!(["assets/icon.png", "main.js", "styles.css", "assets/hack.js"]);
        value["project"]["code"]["assets"] = json!({
            "assets/icon.png": png_b64,
            "main.js": "console.log(1)",
            "styles.css": ".x{}",
            "assets/hack.js": "evil"
        });

        let (output, normalized) =
            parse_and_validate_model_output(&value.to_string()).expect("partial strip");
        assert_eq!(normalized, 6); // 3 invalid on each side
        assert_eq!(
            output.project.manifest.assets.as_deref(),
            Some(vec!["assets/icon.png".to_string()].as_slice())
        );
        assert_eq!(output.project.code.assets.len(), 1);
        assert!(output.project.code.assets.contains_key("assets/icon.png"));
    }

    #[test]
    fn stream_events_serialize_with_type_tag() {
        let step = PlaygroundStreamEvent::Step {
            tool: "plan_context".into(),
            status: "success".into(),
            summary: "Planned 2 queries".into(),
        };
        let step_json = serde_json::to_value(&step).expect("step json");
        assert_eq!(step_json["type"], "step");
        assert_eq!(step_json["tool"], "plan_context");
        assert_eq!(step_json["status"], "success");
        assert_eq!(step_json["summary"], "Planned 2 queries");

        let err = PlaygroundStreamEvent::Error {
            message: "Generation cancelled".into(),
        };
        let err_json = serde_json::to_value(&err).expect("error json");
        assert_eq!(err_json["type"], "error");
        assert_eq!(err_json["message"], "Generation cancelled");
    }

    #[test]
    fn cancelled_from_watch_reads_flag() {
        let (tx, rx) = watch::channel(false);
        assert!(!cancelled_from_watch(&rx));
        tx.send(true).expect("send cancel");
        assert!(cancelled_from_watch(&rx));
    }

    fn sample_widget_only_project() -> PlaygroundProject {
        let raw = json!({
            "manifest": {
                "id": "com.myriad.playground.widgetonly",
                "name": "Widget Only",
                "version": "1.0.0",
                "description": "A widget-only playground project",
                "author": { "name": "Myriad Playground" },
                "main": "main.js",
                "styles": "styles.css",
                "cssMode": "unified",
                "permissions": ["widget:register"],
                "icon": "🧩",
                "themeColor": "#7C3AED",
                "hasPage": false,
                "category": "utility",
                "widgets": [{
                    "id": "card",
                    "name": "Card",
                    "defaultSize": "2x2",
                    "sizes": ["2x2"],
                    "category": "utility"
                }]
            },
            "code": {
                "core": "Tapp.lifecycle.onReady(function () {});",
                "page": "",
                "styles": ".widget { color: var(--color-primary); }",
                "pageHtml": "",
                "widget": "Tapp.widget.register('card', { render: function () {} });",
                "widgetHtml": "<div class=\"widget\">Hi</div>",
                "i18n": { "zh-CN": {}, "en-US": {}, "ja-JP": {} }
            }
        });
        serde_json::from_value(raw).expect("widget-only sample project")
    }

    #[test]
    fn accepts_widget_only_project() {
        let project = sample_widget_only_project();
        validate_playground_project(&project).expect("widget-only should validate");
        assert!(!project.manifest.has_page);
        assert!(project.code.page_html.trim().is_empty());
        assert!(!project.manifest.widgets.as_ref().unwrap().is_empty());
    }

    #[test]
    fn accepts_page_only_project() {
        let project = sample_project("PageOnly");
        validate_playground_project(&project).expect("page-only should validate");
        assert!(project.manifest.has_page);
    }

    #[test]
    fn rejects_project_with_neither_page_nor_widgets() {
        let mut project = sample_project("Empty");
        project.manifest.has_page = false;
        project.manifest.page_template = None;
        project.code.page = String::new();
        project.code.page_html = String::new();
        project.manifest.widgets = None;
        let err = validate_playground_project(&project).unwrap_err();
        assert!(
            err.contains("Page") || err.contains("Widgets") || err.contains("hasPage"),
            "expected empty-project error, got: {err}"
        );
    }

    #[test]
    fn rejects_has_page_true_without_page_content() {
        let mut project = sample_project("MissingPage");
        project.code.page = String::new();
        project.code.page_html = String::new();
        let err = validate_playground_project(&project).unwrap_err();
        assert!(
            err.contains("page code and HTML") || err.contains("hasPage"),
            "expected missing page content error, got: {err}"
        );
    }

    #[test]
    fn rejects_widget_only_without_widget_code() {
        let mut project = sample_widget_only_project();
        project.code.widget = Some(String::new());
        project.code.widget_html = Some(String::new());
        let err = validate_playground_project(&project).unwrap_err();
        assert!(
            err.contains("widget"),
            "expected widget code error, got: {err}"
        );
    }
}
