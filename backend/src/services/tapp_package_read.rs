//! Pure resource-resolution plans for *installed* Tapp packages.
//!
//! HTTP handlers keep visibility/auth and filesystem IO. This module owns how
//! a stored manifest JSON maps to relative paths the reader should attempt.

use myriad_tapp_contract::manifest::TappManifest;

/// CSS packaging mode from `manifest.cssMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledCssMode {
    /// Default / legacy: shared styles + optional widget.css / page.css files.
    Combined,
    /// Separated: widgetStyles / pageStyles are first-class paths.
    Separated,
}

impl InstalledCssMode {
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Combined => None,
            Self::Separated => Some("separated"),
        }
    }
}

/// Parse cssMode from a raw manifest JSON value (DB-stored).
pub fn installed_css_mode(manifest: &serde_json::Value) -> InstalledCssMode {
    match manifest.get("cssMode").and_then(serde_json::Value::as_str) {
        Some("separated") => InstalledCssMode::Separated,
        _ => InstalledCssMode::Combined,
    }
}

/// Relative text paths to attempt when serving `GET …/resources`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledTextResourcePlan {
    /// Wire `cssMode` field (only set for separated).
    pub css_mode: Option<&'static str>,
    /// Shared styles path when present (manifest.styles or default styles.css).
    pub styles: Option<String>,
    /// Separated-mode widget styles path.
    pub widget_styles: Option<String>,
    /// Separated-mode page styles path.
    pub page_styles: Option<String>,
    /// Combined-mode legacy widget.css (None when separated).
    pub widget_css: Option<String>,
    /// Combined-mode legacy page.css (None when separated).
    pub page_css: Option<String>,
    /// Page HTML template path (default `page.html`).
    pub page_template: String,
}

/// Build the text-resource read plan from a stored manifest JSON object.
pub fn installed_text_resource_plan(manifest: &serde_json::Value) -> InstalledTextResourcePlan {
    let mode = installed_css_mode(manifest);
    let is_separated = mode == InstalledCssMode::Separated;

    let styles = if let Some(path) = manifest.get("styles").and_then(serde_json::Value::as_str) {
        Some(path.to_string())
    } else if !is_separated {
        Some("styles.css".to_string())
    } else {
        None
    };

    let widget_styles = if is_separated {
        Some(
            manifest
                .get("widgetStyles")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("widget.css")
                .to_string(),
        )
    } else {
        None
    };

    let page_styles = if is_separated {
        Some(
            manifest
                .get("pageStyles")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("page.css")
                .to_string(),
        )
    } else {
        None
    };

    let page_template = manifest
        .get("pageTemplate")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("page.html")
        .to_string();

    InstalledTextResourcePlan {
        css_mode: mode.as_str(),
        styles,
        widget_styles,
        page_styles,
        widget_css: if is_separated {
            None
        } else {
            Some("widget.css".to_string())
        },
        page_css: if is_separated {
            None
        } else {
            Some("page.css".to_string())
        },
        page_template,
    }
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

/// Ordered page module file names from `manifest.pageModules` (array of strings).
pub fn installed_page_module_names(manifest: &serde_json::Value) -> Option<Vec<String>> {
    manifest
        .get("pageModules")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .filter(|names| !names.is_empty())
}

/// Relative path under the install dir for a page module file name.
pub fn installed_page_module_relative_path(name: &str) -> String {
    format!("page/{name}")
}

/// Whether `path` is declared in a typed manifest's `assets` list.
pub fn manifest_declares_asset(manifest: &TappManifest, path: &str) -> bool {
    manifest
        .assets
        .as_ref()
        .is_some_and(|declared| declared.iter().any(|entry| entry == path))
}

/// Whether an asset byte length is within the single-file install limit.
pub fn asset_bytes_within_limit(size: u64, max_bytes: u64) -> bool {
    size <= max_bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn combined_mode_defaults_styles_and_legacy_css() {
        let plan = installed_text_resource_plan(&json!({}));
        assert_eq!(plan.css_mode, None);
        assert_eq!(plan.styles.as_deref(), Some("styles.css"));
        assert_eq!(plan.widget_styles, None);
        assert_eq!(plan.page_styles, None);
        assert_eq!(plan.widget_css.as_deref(), Some("widget.css"));
        assert_eq!(plan.page_css.as_deref(), Some("page.css"));
        assert_eq!(plan.page_template, "page.html");
    }

    #[test]
    fn separated_mode_uses_declared_widget_and_page_styles() {
        let plan = installed_text_resource_plan(&json!({
            "cssMode": "separated",
            "widgetStyles": "w.css",
            "pageStyles": "p.css",
            "pageTemplate": "shell.html",
            "styles": "shared.css"
        }));
        assert_eq!(plan.css_mode, Some("separated"));
        assert_eq!(plan.styles.as_deref(), Some("shared.css"));
        assert_eq!(plan.widget_styles.as_deref(), Some("w.css"));
        assert_eq!(plan.page_styles.as_deref(), Some("p.css"));
        assert!(plan.widget_css.is_none());
        assert!(plan.page_css.is_none());
        assert_eq!(plan.page_template, "shell.html");
    }

    #[test]
    fn separated_mode_defaults_when_paths_omitted() {
        let plan = installed_text_resource_plan(&json!({ "cssMode": "separated" }));
        assert!(plan.styles.is_none());
        assert_eq!(plan.widget_styles.as_deref(), Some("widget.css"));
        assert_eq!(plan.page_styles.as_deref(), Some("page.css"));
    }

    #[test]
    fn widget_template_and_page_module_plans() {
        let manifest = json!({
            "widgets": [{
                "id": "card",
                "templates": {
                    "2x2": "templates/card-2x2.html",
                    "4x2": "templates/card-4x2.html"
                }
            }],
            "pageModules": ["extra.js", "boot.js"]
        });
        let templates = installed_widget_template_paths(&manifest);
        assert_eq!(templates.len(), 2);
        assert!(templates
            .iter()
            .any(|t| t.widget_id == "card" && t.size == "2x2"));

        let names = installed_page_module_names(&manifest).unwrap();
        assert_eq!(names, vec!["extra.js", "boot.js"]);
        assert_eq!(
            installed_page_module_relative_path("extra.js"),
            "page/extra.js"
        );
        assert!(installed_page_module_names(&json!({})).is_none());
        assert!(installed_widget_template_paths(&json!({})).is_empty());
    }

    #[test]
    fn asset_declaration_and_size_gate() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.app",
            "name": "App",
            "version": "1.0.0",
            "main": "main.js",
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
                "main": "main.js",
                "category": "utility",
                "permissions": []
            }))
            .unwrap(),
            "assets/icon.png"
        ));
        assert!(asset_bytes_within_limit(10, 100));
        assert!(!asset_bytes_within_limit(101, 100));
    }
}
