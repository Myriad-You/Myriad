//! Installed-package resource plans and asset declaration helpers.

/// Relative text paths to attempt when serving `GET …/resources`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTextResourcePlan {
    /// 作者共享样式（`core.styles`）。
    pub core_styles: Option<String>,
    /// 作者 Page 层样式（`page.styles`）。
    pub page_styles: Option<String>,
    /// 作者 Widget 层样式，按 widget id。
    pub widget_styles: Vec<(String, String)>,
    /// Page HTML 模板（`page.template`）。
    pub page_template: Option<String>,
}

fn layer_string(manifest: &serde_json::Value, layer: &str, key: &str) -> Option<String> {
    manifest
        .get(layer)?
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(String::from)
}

/// Build the text-resource read plan from a stored manifest JSON object.
pub fn installed_text_resource_plan(manifest: &serde_json::Value) -> InstalledTextResourcePlan {
    InstalledTextResourcePlan {
        core_styles: layer_string(manifest, "core", "styles"),
        page_styles: layer_string(manifest, "page", "styles"),
        widget_styles: installed_widget_layer_paths(manifest, "styles"),
        page_template: layer_string(manifest, "page", "template"),
    }
}

/// `core.entry` from a stored manifest JSON.
pub fn installed_core_entry(manifest: &serde_json::Value) -> Option<String> {
    layer_string(manifest, "core", "entry")
}

/// `page.entry` from a stored manifest JSON.
pub fn installed_page_entry(manifest: &serde_json::Value) -> Option<String> {
    layer_string(manifest, "page", "entry")
}

/// Widget ids declared on a stored manifest, in declaration order.
pub fn installed_widget_ids(manifest: &serde_json::Value) -> Vec<String> {
    let Some(widgets) = manifest
        .get("widgets")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    widgets
        .iter()
        .filter_map(|widget| {
            widget
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

/// A `widget_id` query that does not name a declared widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownWidgetId(pub String);

/// Accept a requested widget id only when the manifest actually declares it.
///
/// Omitted id keeps the all-widget projection. A typo must not silently shrink
/// the graph to core-only — that looks like a successful empty widget.
pub fn require_known_widget_id<'a>(
    manifest: &serde_json::Value,
    selected: Option<&'a str>,
) -> Result<Option<&'a str>, UnknownWidgetId> {
    let Some(id) = selected else {
        return Ok(None);
    };
    if installed_widget_ids(manifest)
        .iter()
        .any(|declared| declared == id)
    {
        return Ok(Some(id));
    }
    Err(UnknownWidgetId(id.to_string()))
}

pub fn filter_widget_paths(
    paths: Vec<(String, String)>,
    selected: Option<&str>,
) -> Vec<(String, String)> {
    match selected {
        None => paths,
        Some(id) => paths
            .into_iter()
            .filter(|(widget_id, _)| widget_id == id)
            .collect(),
    }
}

/// `(widget id, path)` pairs for a per-widget layer key such as `entry` / `styles`.
pub fn installed_widget_layer_paths(
    manifest: &serde_json::Value,
    key: &str,
) -> Vec<(String, String)> {
    let Some(widgets) = manifest
        .get("widgets")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    widgets
        .iter()
        .filter_map(|widget| {
            let id = widget.get("id").and_then(serde_json::Value::as_str)?;
            let path = widget.get(key).and_then(serde_json::Value::as_str)?;
            Some((id.to_string(), path.to_string()))
        })
        .collect()
}

/// Every layer entry declared by a stored manifest, in a stable order.
#[allow(dead_code)] // 仅测试调用：本仓无生产调用点（编译器已核）。
pub fn installed_layer_entries(manifest: &serde_json::Value) -> Vec<String> {
    let mut entries = Vec::new();
    if let Some(entry) = installed_core_entry(manifest) {
        entries.push(entry);
    }
    if let Some(entry) = installed_page_entry(manifest) {
        entries.push(entry);
    }
    for (_, entry) in installed_widget_layer_paths(manifest, "entry") {
        entries.push(entry);
    }
    entries
}

/// One widget template file declared under `manifest.widgets[].templates`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledWidgetTemplatePath {
    pub widget_id: String,
    pub size: String,
    pub path: String,
}

/// Flatten widget template path declarations from stored manifest JSON.
pub fn installed_widget_template_paths(
    manifest: &serde_json::Value,
) -> Vec<InstalledWidgetTemplatePath> {
    let Some(widgets) = manifest
        .get("widgets")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for widget in widgets {
        let Some(widget_id) = widget.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some(declared) = widget
            .get("templates")
            .and_then(serde_json::Value::as_object)
        else {
            continue;
        };
        for (size, path) in declared {
            if let Some(path) = path.as_str() {
                out.push(InstalledWidgetTemplatePath {
                    widget_id: widget_id.to_string(),
                    size: size.clone(),
                    path: path.to_string(),
                });
            }
        }
    }
    out
}

/// Page exists if the `page` object is declared. Replaces the old `hasPage` flag.
pub fn manifest_declares_page(manifest: &serde_json::Value) -> bool {
    manifest.get("page").is_some()
}

/// Shared layer is present when `core` is declared.
pub fn manifest_declares_core(manifest: &serde_json::Value) -> bool {
    manifest.get("core").is_some()
}

/// At least one widget is declared.
pub fn manifest_declares_widgets(manifest: &serde_json::Value) -> bool {
    manifest
        .get("widgets")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|widgets| !widgets.is_empty())
}

/// Whether `path` is declared in a typed manifest's `assets` list.
///
/// For payloads already validated against the current contract. Serving
/// installed packages must use [`installed_manifest_declares_asset`], which
/// reads by key and cannot fail on a manifest from another contract version.
#[allow(dead_code)] // 仅测试调用：本仓无生产调用点（编译器已核）。
pub fn manifest_declares_asset(
    manifest: &myriad_tapp_contract::manifest::TappManifest,
    path: &str,
) -> bool {
    manifest
        .assets
        .as_ref()
        .is_some_and(|declared| declared.iter().any(|entry| entry == path))
}

/// Whether `path` is declared in a stored manifest JSON's `assets` list.
///
/// Reads by key like the rest of the serve path, so a manifest shaped for a
/// different contract version degrades to "not declared" instead of failing
/// deserialization and surfacing as a 500.
pub fn installed_manifest_declares_asset(manifest: &serde_json::Value, path: &str) -> bool {
    manifest
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|declared| {
            declared
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(|entry| entry == path)
        })
}

/// Whether an asset byte length is within the single-file install limit.
pub fn asset_bytes_within_limit(size: u64, max_bytes: u64) -> bool {
    size <= max_bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_tapp_contract::contract_rules::{HOST_PAGE_CSS, HOST_WIDGET_CSS};
    use serde_json::json;

    #[test]
    fn layerless_manifest_declares_no_author_styles() {
        let plan = installed_text_resource_plan(&json!({}));
        assert!(plan.core_styles.is_none());
        assert!(plan.page_styles.is_none());
        assert!(plan.widget_styles.is_empty());
        assert!(plan.page_template.is_none());
    }

    /// 作者样式来自层声明；宿主预编译产物走固定路径，不参与这份计划。
    #[test]
    fn author_styles_come_from_layer_declarations() {
        let plan = installed_text_resource_plan(&json!({
            "core": { "entry": "core.js", "styles": "shared.css" },
            "page": { "entry": "page/index.js", "template": "shell.html", "styles": "p.css" },
            "widgets": [{ "id": "card", "entry": "widget.js", "styles": "w.css" }]
        }));
        assert_eq!(plan.core_styles.as_deref(), Some("shared.css"));
        assert_eq!(plan.page_styles.as_deref(), Some("p.css"));
        assert_eq!(
            plan.widget_styles,
            vec![("card".to_string(), "w.css".to_string())]
        );
        assert_eq!(plan.page_template.as_deref(), Some("shell.html"));
        // 宿主产物在独立目录，作者可以放心把层样式命名成 page.css / widget.css
        assert_eq!(HOST_WIDGET_CSS, "host/widget.css");
        assert_eq!(HOST_PAGE_CSS, "host/page.css");
    }

    #[test]
    fn layer_entries_follow_core_page_widget_order() {
        let manifest = json!({
            "core": { "entry": "core.js" },
            "page": { "entry": "page/index.js" },
            "widgets": [{ "id": "card", "entry": "widget.js" }]
        });
        assert_eq!(installed_core_entry(&manifest).as_deref(), Some("core.js"));
        assert_eq!(
            installed_page_entry(&manifest).as_deref(),
            Some("page/index.js")
        );
        assert_eq!(
            installed_layer_entries(&manifest),
            vec!["core.js", "page/index.js", "widget.js"]
        );
        assert!(installed_layer_entries(&json!({})).is_empty());
    }

    #[test]
    fn requested_widget_id_must_be_declared() {
        let manifest = json!({
            "widgets": [
                { "id": "card", "entry": "components/card.js" },
                { "id": "list", "entry": "components/list.js" }
            ]
        });
        assert_eq!(installed_widget_ids(&manifest), vec!["card", "list"]);
        assert_eq!(require_known_widget_id(&manifest, None).unwrap(), None);
        assert_eq!(
            require_known_widget_id(&manifest, Some("card")).unwrap(),
            Some("card")
        );
        assert_eq!(
            require_known_widget_id(&manifest, Some("ghost")).unwrap_err(),
            UnknownWidgetId("ghost".into())
        );
        assert_eq!(
            filter_widget_paths(
                vec![
                    ("card".into(), "components/card.js".into()),
                    ("list".into(), "components/list.js".into())
                ],
                Some("card")
            ),
            vec![("card".into(), "components/card.js".into())]
        );
    }

    #[test]
    fn widget_template_plans() {
        let manifest = json!({
            "widgets": [{
                "id": "card",
                "templates": {
                    "2x2": "templates/card-2x2.html",
                    "4x2": "templates/card-4x2.html"
                }
            }]
        });
        let templates = installed_widget_template_paths(&manifest);
        assert_eq!(templates.len(), 2);
        assert!(templates
            .iter()
            .any(|t| t.widget_id == "card" && t.size == "2x2"));
        assert!(installed_widget_template_paths(&json!({})).is_empty());
    }

    #[test]
    fn manifest_declares_layers_from_json_keys() {
        let empty = json!({});
        assert!(!manifest_declares_page(&empty));
        assert!(!manifest_declares_core(&empty));
        assert!(!manifest_declares_widgets(&empty));
        let declared = json!({
            "core": { "entry": "core.js" },
            "page": { "entry": "page/index.js" },
            "widgets": [{ "id": "card" }]
        });
        assert!(manifest_declares_page(&declared));
        assert!(manifest_declares_core(&declared));
        assert!(manifest_declares_widgets(&declared));
        assert!(!manifest_declares_widgets(&json!({ "widgets": [] })));
    }

    #[test]
    fn asset_declaration_and_size_gate() {
        use myriad_tapp_contract::manifest::TappManifest;
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.app",
            "name": "App",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "category": "utility",
            "permissions": [],
            "assets": ["assets/icon.png", "assets/felt/table.png"]
        }))
        .unwrap();
        assert!(manifest_declares_asset(&manifest, "assets/icon.png"));
        assert!(!manifest_declares_asset(&manifest, "assets/missing.png"));
        assert!(!manifest_declares_asset(
            &serde_json::from_value(json!({
                "id": "com.example.app",
                "name": "App",
                "version": "1.0.0",
                "core": { "entry": "core.js" },
                "category": "utility",
                "permissions": []
            }))
            .unwrap(),
            "assets/icon.png"
        ));
        assert!(asset_bytes_within_limit(10, 100));
        assert!(!asset_bytes_within_limit(101, 100));
    }

    #[test]
    fn installed_asset_declaration_degrades_instead_of_failing() {
        let manifest = json!({ "assets": ["assets/icon.png"] });
        assert!(installed_manifest_declares_asset(
            &manifest,
            "assets/icon.png"
        ));
        assert!(!installed_manifest_declares_asset(
            &manifest,
            "assets/missing.png"
        ));

        for shape in [
            json!({}),
            json!({ "assets": "not-an-array" }),
            json!({ "assets": [42] }),
            json!({ "core": { "entry": "core.js" } }),
        ] {
            assert!(!installed_manifest_declares_asset(
                &shape,
                "assets/icon.png"
            ));
        }
    }
}
