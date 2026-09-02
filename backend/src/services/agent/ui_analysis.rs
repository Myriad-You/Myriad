//! Pure UI analysis and frontend-action planning for agent ui_control handlers.
//!
//! Handlers keep DB access, AI calls, and timestamps. This module owns:
//! - tappId path-safety checks
//! - HTML/JS structure parsing and action inference
//! - router path validation / full-path build
//! - page interact action allow-list
//! - window close/focus target selection
//! - page type / breadcrumb / route context
//! - music control / playlist id pure mapping

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;

// HTML element parsing regexes (compiled once)
static RE_BUTTON: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<button[^>]*(?:id=[\"']([^\"']*)[\"'])?[^>]*(?:class=[\"']([^\"']*)[\"'])?[^>]*(?:title=[\"']([^\"']*)[\"'])?[^>]*>([^<]*)"#).unwrap()
});
static RE_INPUT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<(?:input|textarea)[^>]*(?:id=[\"']([^\"']*)[\"'])?[^>]*(?:type=[\"']([^\"']*)[\"'])?[^>]*(?:placeholder=[\"']([^\"']*)[\"'])?[^>]*"#).unwrap()
});
static RE_FORM: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<form[^>]*(?:id=[\"']([^\"']*)[\"'])?[^>]*(?:action=[\"']([^\"']*)[\"'])?[^>]*"#)
        .unwrap()
});
static RE_LINK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"<a[^>]*href=[\"']([^\"']*)[\"'][^>]*>([^<]*)"#).unwrap());
static RE_ONCLICK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"<(\w+)[^>]*onclick=[\"']([^\"']*)[\"'][^>]*(?:id=[\"']([^\"']*)[\"'])?"#).unwrap()
});

// JS analysis regexes
static RE_JS_FUNC: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?:async\s+)?function\s+(\w+)\s*\([^)]*\)"#).unwrap());
static RE_TAPP_API: Lazy<Regex> = Lazy::new(|| Regex::new(r#"Tapp\.(\w+)\.(\w+)"#).unwrap());
static RE_ADDEVENT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"\.addEventListener\(['\"](\w+)['\"]"#).unwrap());
static RE_ON_PROP: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\.on(\w+)\s*="#).unwrap());
static RE_I18N_KEY: Lazy<Regex> = Lazy::new(|| Regex::new(r#"t\(['\"]([^'\"]+)['\"]\)"#).unwrap());
static RE_FUNC_NAME: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(\w+)\s*\("#).unwrap());

/// Allowed SPA route prefixes for `router.navigate`.
///
/// Keep in lockstep with `frontend/src/App.tsx` `<Route path>`. Dead prefixes
/// (`/home`, `/platform`, `/report`, `/settings`) used to pass validation and
/// then 404 to `/`, or reject the live `/library` `/reports` `/config` paths
/// the planner is told to emit.
pub const VALID_ROUTER_PREFIXES: &[&str] = &[
    "/", "/library", "/brew", "/reports", "/config", "/tapp", "/setup",
];

/// Allowed `page.interact` action names.
pub const VALID_PAGE_INTERACT_ACTIONS: &[&str] = &[
    "click", "hover", "focus", "scroll", "select", "toggle", "expand", "collapse", "type", "input",
];

/// Reject path-traversal / separator tricks in agent-supplied tappId params.
pub fn is_safe_agent_tapp_id(tapp_id: &str) -> bool {
    !tapp_id.is_empty()
        && !tapp_id.contains("..")
        && !tapp_id.contains('/')
        && !tapp_id.contains('\\')
        && !tapp_id.contains('\0')
}

/// Whether a navigate path is within the host SPA allow-list.
///
/// Prefixes must match as full path segments (`/tapp` or `/tapp/...`).
/// Bare `"/"` only allows the home root — it does **not** open every path.
pub fn is_valid_router_path(path: &str) -> bool {
    VALID_ROUTER_PREFIXES.iter().any(|prefix| {
        if *prefix == "/" {
            path == "/"
        } else {
            path == *prefix || path.starts_with(&format!("{prefix}/"))
        }
    })
}

/// Build `path?k=v` from a path and optional query object.
pub fn build_navigate_full_path(path: &str, query_params: &Value) -> String {
    if let Some(query_obj) = query_params.as_object() {
        if !query_obj.is_empty() {
            let query_string: Vec<String> = query_obj
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or(&v.to_string())))
                .collect();
            return format!("{}?{}", path, query_string.join("&"));
        }
    }
    path.to_string()
}

/// Whether a page.interact action is allowed.
pub fn is_valid_page_interact_action(action: &str) -> bool {
    VALID_PAGE_INTERACT_ACTIONS.contains(&action)
}

/// Turn `page.understand` plan.actions into executable frontendActions.
///
/// `autoExecute` default is false; when true, click/input/scroll become
/// `page_interact` and navigate becomes `navigate`.
pub fn page_understand_frontend_actions(
    plan: &Value,
    auto_execute: bool,
    allow_interact: bool,
) -> Vec<Value> {
    if !auto_execute {
        return Vec::new();
    }
    let Some(actions) = plan
        .get("actions")
        .or_else(|| plan.get("steps"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let timestamp = chrono::Utc::now().timestamp_millis();
    actions
        .iter()
        .filter_map(|step| {
            let kind = step
                .get("type")
                .or_else(|| step.get("action"))
                .and_then(Value::as_str)
                .unwrap_or("click");
            if kind == "navigate" {
                let path = step
                    .get("path")
                    .or_else(|| step.get("value"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())?;
                if !is_valid_router_path(path) {
                    return None;
                }
                return Some(json!({
                    "type": "navigate",
                    "path": path,
                    "timestamp": timestamp
                }));
            }
            if !allow_interact || !is_valid_page_interact_action(kind) {
                return None;
            }
            let target = match step.get("target") {
                Some(Value::Object(_)) => step.get("target").cloned().unwrap(),
                Some(Value::String(text)) if !text.trim().is_empty() => json!({ "text": text }),
                _ => return None,
            };
            let mut action = json!({
                "type": "page_interact",
                "action": kind,
                "target": target,
                "timestamp": timestamp
            });
            if let Some(value) = step.get("value") {
                action["value"] = value.clone();
            }
            Some(action)
        })
        .collect()
}

/// Planner schema for `page.understand` says `userIntent` / `pageSnapshot`;
/// the handler historically read `query` / `context`. Accept both so compact
/// index `p` and injected page snapshots both land.
pub fn page_understand_query(params: &HashMap<String, Value>) -> String {
    ["userIntent", "query"]
        .iter()
        .find_map(|key| {
            params
                .get(*key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

pub fn page_understand_context(params: &HashMap<String, Value>) -> Value {
    params
        .get("context")
        .or_else(|| params.get("pageSnapshot"))
        .cloned()
        .unwrap_or(json!({}))
}

/// Resolve close-window target (windowId → tappId → position).
pub fn resolve_window_close_target(
    window_id: Option<&str>,
    tapp_id: Option<&str>,
    position: Option<&str>,
) -> Result<Value, String> {
    if let Some(wid) = window_id {
        return Ok(json!({ "windowId": wid }));
    }
    if let Some(tid) = tapp_id {
        return Ok(json!({ "tappId": tid }));
    }
    if let Some(pos) = position {
        return Ok(json!({ "position": pos }));
    }
    Err("Must specify windowId, tappId, or position".to_string())
}

/// Resolve focus-window target (windowId → tappId → tappName → position).
pub fn resolve_window_focus_target(
    window_id: Option<&str>,
    tapp_id: Option<&str>,
    tapp_name: Option<&str>,
    position: Option<&str>,
) -> Result<Value, String> {
    if let Some(wid) = window_id {
        return Ok(json!({ "windowId": wid }));
    }
    if let Some(tid) = tapp_id {
        return Ok(json!({ "tappId": tid }));
    }
    if let Some(name) = tapp_name {
        return Ok(json!({ "tappName": name }));
    }
    if let Some(pos) = position {
        return Ok(json!({ "position": pos }));
    }
    Err("Must specify windowId, tappId, tappName, or position".to_string())
}

/// Parse HTML structure overview for tapp.ui analysis.
pub fn parse_html_structure(html: &str) -> Value {
    let has_background =
        html.contains("id=\"tapp-background\"") || html.contains("id='tapp-background'");
    let has_content = html.contains("id=\"tapp-content\"") || html.contains("id='tapp-content'");

    let mut sections = vec![];
    for tag in [
        "header", "main", "footer", "nav", "aside", "article", "section", "form",
    ] {
        if html.contains(&format!("<{}", tag)) {
            sections.push(tag);
        }
    }

    json!({
        "hasBackground": has_background,
        "hasContent": has_content,
        "sections": sections,
        "estimatedComplexity": if html.len() > 5000 {
            "complex"
        } else if html.len() > 1000 {
            "moderate"
        } else {
            "simple"
        }
    })
}

/// Parse interactive HTML elements with optional filter.
pub fn parse_html_elements(html: &str, filter: &str) -> Value {
    let mut buttons = vec![];
    let mut inputs = vec![];
    let mut forms = vec![];
    let mut links = vec![];
    let mut interactive = vec![];

    for cap in RE_BUTTON.captures_iter(html) {
        let id = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let class = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let title = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let text = cap.get(4).map(|m| m.as_str()).unwrap_or("").trim();

        buttons.push(json!({
            "type": "button",
            "id": id,
            "class": class,
            "title": if !title.is_empty() { title } else { text },
            "text": text,
            "action": infer_button_action(id, class, title, text)
        }));
    }

    for cap in RE_INPUT.captures_iter(html) {
        let id = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let input_type = cap.get(2).map(|m| m.as_str()).unwrap_or("text");
        let placeholder = cap.get(3).map(|m| m.as_str()).unwrap_or("");

        inputs.push(json!({
            "type": "input",
            "inputType": input_type,
            "id": id,
            "placeholder": placeholder,
            "purpose": infer_input_purpose(id, input_type, placeholder)
        }));
    }

    for cap in RE_FORM.captures_iter(html) {
        let id = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let action = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        forms.push(json!({ "type": "form", "id": id, "action": action }));
    }

    for cap in RE_LINK.captures_iter(html) {
        let href = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let text = cap.get(2).map(|m| m.as_str()).unwrap_or("").trim();
        links.push(json!({ "type": "link", "href": href, "text": text }));
    }

    for cap in RE_ONCLICK.captures_iter(html) {
        let tag = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let onclick = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let id = cap.get(3).map(|m| m.as_str()).unwrap_or("");

        if tag != "button" && tag != "a" {
            interactive.push(json!({
                "type": tag,
                "id": id,
                "onclick": onclick,
                "action": extract_function_name(onclick)
            }));
        }
    }

    match filter {
        "buttons" => json!({ "buttons": buttons }),
        "inputs" => json!({ "inputs": inputs }),
        "forms" => json!({ "forms": forms }),
        "interactive" => {
            json!({ "buttons": buttons, "inputs": inputs, "interactive": interactive })
        }
        _ => json!({
            "buttons": buttons,
            "inputs": inputs,
            "forms": forms,
            "links": links,
            "interactive": interactive,
            "summary": {
                "totalButtons": buttons.len(),
                "totalInputs": inputs.len(),
                "totalForms": forms.len(),
                "totalLinks": links.len()
            }
        }),
    }
}

/// Infer button action intent from attributes/text.
pub fn infer_button_action(id: &str, class: &str, title: &str, text: &str) -> String {
    let combined = format!("{} {} {} {}", id, class, title, text).to_lowercase();

    if combined.contains("send") || combined.contains("submit") || combined.contains("发送") {
        "submit".to_string()
    } else if combined.contains("add") || combined.contains("新增") || combined.contains("添加")
    {
        "add".to_string()
    } else if combined.contains("delete")
        || combined.contains("remove")
        || combined.contains("删除")
    {
        "delete".to_string()
    } else if combined.contains("search") || combined.contains("搜索") {
        "search".to_string()
    } else if combined.contains("edit") || combined.contains("编辑") {
        "edit".to_string()
    } else if combined.contains("save") || combined.contains("保存") {
        "save".to_string()
    } else if combined.contains("cancel") || combined.contains("取消") {
        "cancel".to_string()
    } else if combined.contains("close") || combined.contains("关闭") {
        "close".to_string()
    } else {
        "click".to_string()
    }
}

/// Infer input purpose from attributes.
pub fn infer_input_purpose(id: &str, input_type: &str, placeholder: &str) -> String {
    let combined = format!("{} {} {}", id, input_type, placeholder).to_lowercase();

    if combined.contains("search") || combined.contains("搜索") {
        "search".to_string()
    } else if combined.contains("password") || combined.contains("密码") {
        "password".to_string()
    } else if combined.contains("email") || combined.contains("邮箱") {
        "email".to_string()
    } else if combined.contains("name") || combined.contains("姓名") {
        "name".to_string()
    } else if combined.contains("note") || combined.contains("笔记") {
        "note".to_string()
    } else {
        "text".to_string()
    }
}

/// Parse JS functions and Tapp API usages.
pub fn parse_js_functions(js: &str) -> Vec<Value> {
    let mut functions = vec![];

    for cap in RE_JS_FUNC.captures_iter(js) {
        let name = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if !name.is_empty() && !name.starts_with('_') {
            functions.push(json!({
                "name": name,
                "type": "function",
                "purpose": infer_function_purpose(name)
            }));
        }
    }

    let mut tapp_apis: std::collections::HashSet<String> = std::collections::HashSet::new();
    for cap in RE_TAPP_API.captures_iter(js) {
        let module = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let method = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        tapp_apis.insert(format!("Tapp.{}.{}", module, method));
    }

    for api in tapp_apis {
        functions.push(json!({
            "name": api.clone(),
            "type": "tapp_api",
            "purpose": infer_tapp_api_purpose(&api)
        }));
    }

    functions
}

/// Parse JS event bindings.
pub fn parse_js_events(js: &str) -> Vec<Value> {
    let mut events = vec![];

    for cap in RE_ADDEVENT.captures_iter(js) {
        let event_type = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        events.push(json!({ "type": event_type, "binding": "addEventListener" }));
    }

    for cap in RE_ON_PROP.captures_iter(js) {
        let event_type = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        events.push(json!({ "type": event_type, "binding": "property" }));
    }

    events
}

/// Detect i18n languages and sample keys from JS source.
pub fn parse_i18n(js: &str) -> Value {
    let mut languages = vec![];
    let mut sample_keys = vec![];

    if js.contains("'zh-CN'") || js.contains("\"zh-CN\"") {
        languages.push("zh-CN");
    }
    if js.contains("'en-US'") || js.contains("\"en-US\"") {
        languages.push("en-US");
    }
    if js.contains("'ja-JP'") || js.contains("\"ja-JP\"") {
        languages.push("ja-JP");
    }

    for (i, cap) in RE_I18N_KEY.captures_iter(js).enumerate() {
        if i >= 10 {
            break;
        }
        let key = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        if !key.is_empty() {
            sample_keys.push(key.to_string());
        }
    }

    json!({
        "supported": !languages.is_empty(),
        "languages": languages,
        "sampleKeys": sample_keys
    })
}

/// Infer JS function purpose from name heuristics.
pub fn infer_function_purpose(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("init") {
        "initialization".to_string()
    } else if lower.contains("render") {
        "rendering".to_string()
    } else if lower.contains("update") {
        "update".to_string()
    } else if lower.contains("add") {
        "add_item".to_string()
    } else if lower.contains("delete") {
        "delete_item".to_string()
    } else if lower.contains("save") {
        "save_data".to_string()
    } else if lower.contains("load") {
        "load_data".to_string()
    } else {
        "utility".to_string()
    }
}

/// Infer Tapp API purpose from API path string.
pub fn infer_tapp_api_purpose(api: &str) -> String {
    if api.contains("storage") {
        "data_persistence".to_string()
    } else if api.contains("ui") {
        "user_interface".to_string()
    } else if api.contains("lifecycle") {
        "lifecycle_management".to_string()
    } else {
        "api_call".to_string()
    }
}

/// Extract function name from an onclick handler string.
pub fn extract_function_name(onclick: &str) -> String {
    RE_FUNC_NAME
        .captures(onclick)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "inline".to_string())
}

/// Generate suggested click/input actions from parsed elements.
pub fn generate_suggested_actions(elements: &Value, _functions: &[Value]) -> Vec<Value> {
    let mut actions = vec![];

    if let Some(buttons) = elements.get("buttons").and_then(|b| b.as_array()) {
        for btn in buttons {
            let id = btn.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let action = btn
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("click");
            let title = btn.get("title").and_then(|v| v.as_str()).unwrap_or("");

            if !id.is_empty() {
                actions.push(json!({
                    "action": format!("click_{}", action),
                    "target": id,
                    "description": format!(
                        "点击 {} 按钮",
                        if !title.is_empty() { title } else { id }
                    ),
                    "command": format!("document.getElementById('{}').click()", id)
                }));
            }
        }
    }

    if let Some(inputs) = elements.get("inputs").and_then(|i| i.as_array()) {
        for input in inputs {
            let id = input.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let purpose = input
                .get("purpose")
                .and_then(|v| v.as_str())
                .unwrap_or("text");

            if !id.is_empty() {
                actions.push(json!({
                    "action": format!("input_{}", purpose),
                    "target": id,
                    "description": format!("在 {} 中输入内容", id),
                    "command": format!("document.getElementById('{}').value = '{{text}}'", id)
                }));
            }
        }
    }

    actions
}

/// Best-effort extract a JSON object string from an AI response.
pub fn extract_json_from_response(response: &str) -> Option<String> {
    if let Some(start) = response.find("```json") {
        if let Some(end) = response[start..]
            .find("```\n")
            .or_else(|| response[start..].rfind("```"))
        {
            let json_start = start + 7;
            let json_content = &response[json_start..start + end];
            return Some(json_content.trim().to_string());
        }
    }

    if response.trim().starts_with('{') {
        return Some(response.trim().to_string());
    }

    if let (Some(start), Some(end)) = (response.find('{'), response.rfind('}')) {
        if end > start {
            return Some(response[start..=end].to_string());
        }
    }

    None
}

/// Detect SPA page type from a path.
pub fn detect_page_type(path: &str) -> &'static str {
    let path_lower = path.to_lowercase();

    if path_lower == "/" || path_lower == "/home" {
        "home"
    } else if path_lower.starts_with("/library") {
        "library"
    } else if path_lower.starts_with("/platform")
        || path_lower.starts_with("/bilibili")
        || path_lower.starts_with("/steam")
        || path_lower.starts_with("/github")
        || path_lower.starts_with("/netease")
    {
        "platform"
    } else if path_lower.starts_with("/brew") {
        "brew"
    } else if path_lower.starts_with("/tapp") {
        "tapp"
    } else if path_lower.starts_with("/report") {
        "report"
    } else if path_lower.starts_with("/settings") || path_lower.starts_with("/config") {
        "settings"
    } else if path_lower.starts_with("/profile") || path_lower.starts_with("/user") {
        "profile"
    } else {
        "other"
    }
}

/// Localized page name for a path + page type.
pub fn get_page_name(path: &str, page_type: &str) -> String {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    match page_type {
        "home" => "首页".to_string(),
        "library" => "资料库".to_string(),
        "platform" => {
            if let Some(platform) = segments.get(1).or(segments.first()) {
                match platform.to_lowercase().as_str() {
                    "bilibili" | "bili" => "哔哩哔哩".to_string(),
                    "steam" => "Steam 游戏".to_string(),
                    "github" => "GitHub 活动".to_string(),
                    "netease" => "网易云音乐".to_string(),
                    _ => format!("{} 数据", platform),
                }
            } else {
                "平台数据".to_string()
            }
        }
        "brew" => {
            if segments.len() > 1 {
                "订阅详情".to_string()
            } else {
                "信息聚合".to_string()
            }
        }
        "tapp" => {
            if segments.len() > 1 {
                "Tapp 详情".to_string()
            } else {
                "Tapp 工坊".to_string()
            }
        }
        "report" => "数据报告".to_string(),
        "settings" => "系统设置".to_string(),
        "profile" => "个人中心".to_string(),
        _ => "页面".to_string(),
    }
}

/// Extract platform/item context from a route path + params.
pub fn extract_route_context(path: &str, params: &HashMap<String, Value>) -> Value {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let page_type = detect_page_type(path);

    let mut context = json!({
        "platform": null,
        "itemId": null,
        "viewMode": "list",
        "filters": {}
    });

    if page_type == "platform" {
        if let Some(platform) = segments.get(1).or(segments.first()) {
            let platform_lower = platform.to_lowercase();
            if [
                "bilibili",
                "bangumi",
                "steam",
                "github",
                "netease",
                "mal",
                "x",
                "discord",
                "xbox",
                "psn",
                "playstation",
            ]
            .contains(&platform_lower.as_str())
            {
                context["platform"] = json!(platform_lower);
            }
        }

        if let Some(item_id) = segments.get(2) {
            context["itemId"] = json!(item_id);
            context["viewMode"] = json!("detail");
        }
    }

    if let Some(view_mode) = params.get("viewMode").and_then(|v| v.as_str()) {
        context["viewMode"] = json!(view_mode);
    }

    if let Some(filters) = params.get("filters") {
        context["filters"] = filters.clone();
    }

    context
}

/// Build breadcrumb labels for a path.
pub fn build_breadcrumb(path: &str) -> Vec<String> {
    let mut breadcrumb = vec!["首页".to_string()];
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    for (i, segment) in segments.iter().enumerate() {
        let name = match segment.to_lowercase().as_str() {
            "platform" | "platforms" => "平台数据".to_string(),
            "bilibili" | "bili" => "哔哩哔哩".to_string(),
            "steam" => "Steam".to_string(),
            "github" => "GitHub".to_string(),
            "netease" => "网易云音乐".to_string(),
            "brew" => "信息聚合".to_string(),
            "tapp" | "tapps" => "Tapp 工坊".to_string(),
            "report" | "reports" => "数据报告".to_string(),
            "settings" => "设置".to_string(),
            "profile" => "个人中心".to_string(),
            "detail" | "details" => "详情".to_string(),
            _ => {
                if i == segments.len() - 1 && segment.len() > 8 {
                    "详情".to_string()
                } else {
                    segment.to_string()
                }
            }
        };

        if name != "首页" {
            breadcrumb.push(name);
        }
    }

    breadcrumb
}

/// Normalized music frontend action payload (without timestamp).
#[derive(Debug, Clone, PartialEq)]
pub struct MusicFrontendAction {
    pub action: String,
    pub value: Option<Value>,
    pub message: &'static str,
}

/// Map music control action + params into a frontend action body.
pub fn normalize_music_control(
    action: &str,
    volume: Option<f64>,
    position: Option<f64>,
) -> Result<MusicFrontendAction, String> {
    match action {
        "play" | "pause" | "toggle" => Ok(MusicFrontendAction {
            action: if action == "toggle" {
                "toggle-play-pause".to_string()
            } else {
                action.to_string()
            },
            value: None,
            message: match action {
                "play" => "正在播放音乐",
                "pause" => "已暂停播放",
                _ => "切换播放状态",
            },
        }),
        "next" => Ok(MusicFrontendAction {
            action: "next".into(),
            value: None,
            message: "切换到下一首",
        }),
        "previous" | "prev" => Ok(MusicFrontendAction {
            action: "previous".into(),
            value: None,
            message: "切换到上一首",
        }),
        "volume" => Ok(MusicFrontendAction {
            action: "volume".into(),
            value: Some(json!(volume.unwrap_or(50.0) / 100.0)),
            message: "已调节音量",
        }),
        "mute" => Ok(MusicFrontendAction {
            action: "mute".into(),
            value: Some(json!(true)),
            message: "已静音",
        }),
        "unmute" => Ok(MusicFrontendAction {
            action: "mute".into(),
            value: Some(json!(false)),
            message: "已取消静音",
        }),
        "seek" => Ok(MusicFrontendAction {
            action: "seek".into(),
            value: Some(json!(position.unwrap_or(0.0))),
            message: "已跳转播放位置",
        }),
        _ => Err(format!("Unknown music control action: {}", action)),
    }
}

/// Parse playlistId from string or number JSON values.
pub fn parse_playlist_id_param(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        if !s.is_empty() {
            return Some(s.to_string());
        }
        return None;
    }
    if let Some(n) = value.as_i64() {
        return Some(n.to_string());
    }
    value.as_u64().map(|n| n.to_string())
}

/// Whether router can go back from this path.
pub fn router_can_go_back(current_path: &str) -> bool {
    current_path != "/" && current_path != "/home"
}

/// Concatenate declared layer sources for UI analysis. Empty parts are dropped.
pub fn join_layer_analysis_sources(sources: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    sources
        .into_iter()
        .map(|source| source.as_ref().to_string())
        .filter(|source| !source.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_nonempty_layer_sources() {
        assert_eq!(
            join_layer_analysis_sources(["function core() {}", "", "function page() {}"]),
            "function core() {}\nfunction page() {}"
        );
        assert_eq!(join_layer_analysis_sources(["", "  "]), "");
    }

    #[test]
    fn safe_tapp_id_rejects_traversal() {
        assert!(is_safe_agent_tapp_id("com.example.app"));
        assert!(!is_safe_agent_tapp_id("../etc"));
        assert!(!is_safe_agent_tapp_id("a/b"));
        assert!(!is_safe_agent_tapp_id("a\\b"));
        assert!(!is_safe_agent_tapp_id(""));
    }

    #[test]
    fn router_path_and_full_path() {
        assert!(is_valid_router_path("/"));
        assert!(is_valid_router_path("/tapp"));
        assert!(is_valid_router_path("/tapp/run/com.example"));
        assert!(is_valid_router_path("/library"));
        assert!(is_valid_router_path("/brew/item/1"));
        assert!(is_valid_router_path("/reports"));
        assert!(is_valid_router_path("/config"));
        assert!(!is_valid_router_path("/home"));
        assert!(!is_valid_router_path("/platform/steam"));
        assert!(!is_valid_router_path("/report"));
        assert!(!is_valid_router_path("/settings"));
        assert!(!is_valid_router_path("/admin/secret"));
        let full = build_navigate_full_path("/tapp", &json!({"id": "x", "tab": "1"}));
        assert!(full.starts_with("/tapp?"));
        assert!(full.contains("id=x"));
        assert_eq!(build_navigate_full_path("/library", &json!({})), "/library");
    }

    #[test]
    fn page_interact_and_window_targets() {
        assert!(is_valid_page_interact_action("click"));
        assert!(!is_valid_page_interact_action("explode"));

        assert_eq!(
            resolve_window_close_target(Some("w1"), None, None).unwrap()["windowId"],
            "w1"
        );
        assert_eq!(
            resolve_window_close_target(None, Some("com.a"), None).unwrap()["tappId"],
            "com.a"
        );
        assert!(resolve_window_close_target(None, None, None).is_err());

        assert_eq!(
            resolve_window_focus_target(None, None, Some("App"), None).unwrap()["tappName"],
            "App"
        );
        assert!(resolve_window_focus_target(None, None, None, None).is_err());
    }

    #[test]
    fn html_structure_and_button_inference() {
        let html = r#"
            <div id="tapp-background"></div>
            <main id="tapp-content">
              <button id="send-btn" class="primary" title="发送">发送</button>
              <input id="search-box" type="text" placeholder="搜索" />
            </main>
        "#;
        let structure = parse_html_structure(html);
        assert_eq!(structure["hasBackground"], true);
        assert_eq!(structure["hasContent"], true);
        assert!(structure["sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "main"));

        let elements = parse_html_elements(html, "all");
        let buttons = elements["buttons"].as_array().unwrap();
        assert!(!buttons.is_empty());
        assert_eq!(buttons[0]["action"], "submit");

        assert_eq!(infer_button_action("add-item", "", "", "新增"), "add");
        assert_eq!(infer_input_purpose("user-email", "text", "邮箱"), "email");
    }

    #[test]
    fn js_parse_and_i18n() {
        let js = r#"
            function initApp() {}
            function renderList() {}
            Tapp.storage.get('k');
            el.addEventListener('click', fn);
            el.onclick = fn;
            t('hello.world');
            const locale = 'zh-CN';
        "#;
        let funcs = parse_js_functions(js);
        assert!(funcs.iter().any(|f| f["name"] == "initApp"));
        assert!(funcs.iter().any(|f| f["type"] == "tapp_api"));
        assert_eq!(infer_function_purpose("initApp"), "initialization");
        assert_eq!(
            infer_tapp_api_purpose("Tapp.storage.get"),
            "data_persistence"
        );

        let events = parse_js_events(js);
        assert!(events.iter().any(|e| e["type"] == "click"));

        let i18n = parse_i18n(js);
        assert_eq!(i18n["supported"], true);
        assert!(i18n["languages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l == "zh-CN"));
    }

    #[test]
    fn page_type_breadcrumb_and_music() {
        assert_eq!(detect_page_type("/platform/steam/123"), "platform");
        assert_eq!(detect_page_type("/library"), "library");
        assert_eq!(detect_page_type("/home"), "home");
        let prev = normalize_music_control("prev", None, None).unwrap();
        assert_eq!(prev.action, "previous");
        assert_eq!(get_page_name("/platform/steam", "platform"), "Steam 游戏");
        let crumbs = build_breadcrumb("/platform/steam");
        assert!(crumbs.contains(&"首页".to_string()));
        assert!(crumbs.contains(&"Steam".to_string()));

        let ctx = extract_route_context(
            "/platform/steam/abc",
            &HashMap::from([("viewMode".into(), json!("grid"))]),
        );
        assert_eq!(ctx["platform"], "steam");
        assert_eq!(ctx["itemId"], "abc");
        assert_eq!(ctx["viewMode"], "grid");

        assert!(router_can_go_back("/tapp"));
        assert!(!router_can_go_back("/home"));

        let play = normalize_music_control("play", None, None).unwrap();
        assert_eq!(play.action, "play");
        let mut understand = HashMap::new();
        understand.insert("userIntent".into(), json!("  打开设置  "));
        understand.insert("pageSnapshot".into(), json!({ "title": "首页" }));
        assert_eq!(page_understand_query(&understand), "打开设置");
        assert_eq!(page_understand_context(&understand)["title"], "首页");
        let mut query_alias = HashMap::new();
        query_alias.insert("query".into(), json!("summarize"));
        query_alias.insert("context".into(), json!({ "html": "<p>x</p>" }));
        assert_eq!(page_understand_query(&query_alias), "summarize");
        assert_eq!(page_understand_context(&query_alias)["html"], "<p>x</p>");
        let vol = normalize_music_control("volume", Some(80.0), None).unwrap();
        assert_eq!(vol.value, Some(json!(0.8)));
        assert!(normalize_music_control("explode", None, None).is_err());
        assert!(is_valid_page_interact_action("input"));
        assert!(is_valid_page_interact_action("type"));
        assert!(!is_valid_page_interact_action("explode"));
        assert!(page_understand_frontend_actions(&json!({"actions":[]}), false, true).is_empty());
        let auto = page_understand_frontend_actions(
            &json!({
                "actions": [
                    {"type": "click", "target": "保存"},
                    {"type": "navigate", "path": "/library"},
                    {"type": "input", "target": {"selector": "#q"}, "value": "hi"}
                ]
            }),
            true,
            true,
        );
        assert_eq!(auto.len(), 3);
        assert_eq!(auto[0]["type"], "page_interact");
        assert_eq!(auto[1]["type"], "navigate");
        assert_eq!(auto[2]["action"], "input");
        let navigate_only = page_understand_frontend_actions(
            &json!({
                "actions": [
                    {"type": "click", "target": "保存"},
                    {"type": "navigate", "path": "/library"}
                ]
            }),
            true,
            false,
        );
        assert_eq!(navigate_only.len(), 1);
        assert_eq!(navigate_only[0]["type"], "navigate");

        assert_eq!(
            parse_playlist_id_param(&json!("12345")).as_deref(),
            Some("12345")
        );
        assert_eq!(parse_playlist_id_param(&json!(99)).as_deref(), Some("99"));
        assert!(parse_playlist_id_param(&json!("")).is_none());
    }

    #[test]
    fn extract_json_and_suggested_actions() {
        let extracted = extract_json_from_response("here ```json\n{\"a\":1}\n``` done");
        assert!(extracted.unwrap().contains("\"a\""));
        assert_eq!(
            extract_json_from_response("{\"x\":true}").unwrap(),
            "{\"x\":true}"
        );

        let elements = json!({
            "buttons": [{"id": "ok", "action": "submit", "title": "OK"}],
            "inputs": [{"id": "q", "purpose": "search"}]
        });
        let actions = generate_suggested_actions(&elements, &[]);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0]["target"], "ok");
        assert_eq!(actions[1]["target"], "q");
    }
}
