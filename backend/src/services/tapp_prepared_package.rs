//! Validated in-memory Tapp package (resources or archive) — pure domain.
//!
//! Install/update handlers stage packages to disk in the API layer. Validation,
//! resource overrides, widget-template binding, and declared-style content
//! presence rules live here so they are free of Axum/`StatusCode`.

pub use myriad_tapp_rules::{
    check_manifest_byte_size, nonempty_content, parse_manifest_json,
    validate_widget_template_contents, widget_template_path, PackageLoadError,
    PackageValidateError, PreparedTappPackage, PreparedTappResources, WidgetTemplateContents,
};

/// Backend package version used by prepared-package manifest checks.
pub fn current_system_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("backend package version must be valid semver")
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_tapp_contract::contract_rules::MAX_TAPP_MANIFEST_BYTES;
    use myriad_tapp_contract::manifest::TappManifest;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn current_system() -> semver::Version {
        super::current_system_version()
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
