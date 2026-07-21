//! Validated in-memory Tapp package and shared staging writer.

use super::{
    api_error, archive_entry_path, log_install_failure, validate_installed_resources,
    validate_named_resource_keys, validate_tapp_archive, validate_tapp_manifest,
    validate_widget_template_contents, widget_template_path, write_install_assets,
    write_install_generation, write_tapp_resource, ApiResponse, TappManifest,
    WidgetTemplateContents, MAX_TAPP_MANIFEST_BYTES,
};
use axum::{http::StatusCode, Json};
use chrono::{DateTime, FixedOffset};
use std::{collections::HashMap, path::Path};
use tokio::fs;

type PackageError = (StatusCode, Json<ApiResponse<()>>);

#[derive(Debug)]
pub(super) struct PreparedTappPackage {
    pub(super) manifest: TappManifest,
    payload: PreparedTappPayload,
}

#[derive(Debug)]
enum PreparedTappPayload {
    Resources(Box<PreparedTappResources>),
    Archive(Vec<u8>),
}

#[derive(Debug, Default)]
pub(super) struct PreparedTappResources {
    pub code: String,
    pub styles: Option<String>,
    pub widget_styles: Option<String>,
    pub page_styles: Option<String>,
    pub page_template: Option<String>,
    pub widget_templates: Option<WidgetTemplateContents>,
    pub generated_widget_css: Option<String>,
    pub generated_page_css: Option<String>,
    pub i18n: Option<HashMap<String, serde_json::Value>>,
    pub page_modules: Option<HashMap<String, String>>,
    pub assets: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PackageStageContext {
    pub user_id: i32,
    pub installation_owner_id: i32,
}

impl PreparedTappPackage {
    pub(super) fn from_resources(manifest: TappManifest, resources: PreparedTappResources) -> Self {
        Self {
            manifest,
            payload: PreparedTappPayload::Resources(Box::new(resources)),
        }
    }

    pub(super) fn from_archive(file_data: Vec<u8>) -> Result<Self, PackageError> {
        let cursor = std::io::Cursor::new(&file_data);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                api_error("Invalid .tapp file format"),
            )
        })?;
        validate_tapp_archive(&mut archive)
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

        let manifest_content = {
            let mut manifest_file = archive.by_name("manifest.json").map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("manifest.json not found in .tapp file"),
                )
            })?;
            if manifest_file.size() > MAX_TAPP_MANIFEST_BYTES {
                return Err((
                    StatusCode::BAD_REQUEST,
                    api_error(format!(
                        "manifest.json exceeds {MAX_TAPP_MANIFEST_BYTES} bytes"
                    )),
                ));
            }
            let mut content = String::new();
            std::io::Read::read_to_string(&mut manifest_file, &mut content).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    api_error("Failed to read manifest.json"),
                )
            })?;
            content
        };

        let manifest = serde_json::from_str(&manifest_content).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                api_error(format!("Invalid manifest.json: {error}")),
            )
        })?;
        let package = Self {
            manifest,
            payload: PreparedTappPayload::Archive(file_data),
        };
        package.validate(None)?;
        Ok(package)
    }

    pub(super) fn apply_resource_overrides(
        &mut self,
        i18n: Option<HashMap<String, serde_json::Value>>,
        page_modules: Option<HashMap<String, String>>,
        assets: Option<HashMap<String, String>>,
    ) {
        if let PreparedTappPayload::Resources(resources) = &mut self.payload {
            resources.i18n = i18n.or(resources.i18n.take());
            resources.page_modules = page_modules.or(resources.page_modules.take());
            resources.assets = assets.or(resources.assets.take());
        }
    }

    pub(super) fn validate(&self, expected_tapp_id: Option<&str>) -> Result<(), PackageError> {
        if expected_tapp_id.is_some_and(|expected| expected != self.manifest.id) {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error("manifest id does not match target tapp id"),
            ));
        }
        validate_tapp_manifest(&self.manifest)
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

        if let PreparedTappPayload::Resources(resources) = &self.payload {
            if let Some(templates) = &resources.widget_templates {
                validate_widget_template_contents(&self.manifest, templates)
                    .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
            }
            // Declared pageStyles/widgetStyles must include content to write.
            // Without this, validate_installed_resources fails with a misleading
            // "not a regular file: page.css" after stage.
            if let Some(declared) = self.manifest.page_styles.as_deref() {
                let has_page = resources
                    .page_styles
                    .as_ref()
                    .is_some_and(|s| !s.is_empty())
                    || resources
                        .generated_page_css
                        .as_ref()
                        .is_some_and(|s| !s.is_empty());
                if !has_page {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        api_error(format!(
                            "Install package is missing pageStyles/pageCss content required by \
manifest.pageStyles={declared} (frontend must send pageCss; store fetch must download download.page_styles)"
                        )),
                    ));
                }
            }
            if self.manifest.page_template.is_some() {
                let has_tpl = resources
                    .page_template
                    .as_ref()
                    .is_some_and(|s| !s.is_empty());
                if !has_tpl {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        api_error(
                            "Install package is missing pageTemplate content required by manifest.pageTemplate",
                        ),
                    ));
                }
            }
            if let Some(declared) = self.manifest.widget_styles.as_deref() {
                let has_widget = resources
                    .widget_styles
                    .as_ref()
                    .is_some_and(|s| !s.is_empty())
                    || resources
                        .generated_widget_css
                        .as_ref()
                        .is_some_and(|s| !s.is_empty());
                if !has_widget {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        api_error(format!(
                            "Install package is missing widgetStyles/widgetCss content required by \
manifest.widgetStyles={declared}"
                        )),
                    ));
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
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
            validate_named_resource_keys(
                resources
                    .page_modules
                    .as_ref()
                    .into_iter()
                    .flat_map(|modules| modules.keys()),
                "page module filename",
            )
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
        }
        Ok(())
    }

    pub(super) async fn stage_into(
        &self,
        tapp_dir: &Path,
        generation: DateTime<FixedOffset>,
        context: PackageStageContext,
    ) -> Result<(), PackageError> {
        match &self.payload {
            PreparedTappPayload::Resources(resources) => {
                self.write_resources(tapp_dir, resources, context).await?;
                let manifest_json =
                    serde_json::to_string_pretty(&self.manifest).unwrap_or_default();
                let manifest_path = tapp_dir.join("manifest.json");
                fs::write(&manifest_path, manifest_json)
                    .await
                    .map_err(|error| {
                        self.log_write_failure("write_manifest", context, &manifest_path, &error);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            api_error(format!("Failed to save manifest: {error}")),
                        )
                    })?;
            }
            PreparedTappPayload::Archive(file_data) => {
                self.extract_archive(tapp_dir, file_data, context).await?;
            }
        }

        write_install_generation(tapp_dir, generation).map_err(|error| {
            self.log_write_failure("write_install_generation", context, tapp_dir, &error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Failed to save install state: {error}")),
            )
        })?;
        validate_installed_resources(&self.manifest, tapp_dir)
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))
    }

    async fn write_resources(
        &self,
        tapp_dir: &Path,
        resources: &PreparedTappResources,
        context: PackageStageContext,
    ) -> Result<(), PackageError> {
        self.write_text(
            tapp_dir,
            &self.manifest.main,
            &resources.code,
            "main",
            context,
        )
        .await?;

        if let Some(content) = &resources.styles {
            let path = self.manifest.styles.as_deref().unwrap_or("styles.css");
            self.write_text(tapp_dir, path, content, "styles", context)
                .await?;
        }
        // Widget CSS: declared widgetStyles path first, else generated widget.css.
        // Accept content from either channel so mis-routed payload still installs.
        {
            let content = resources
                .widget_styles
                .as_ref()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    resources
                        .generated_widget_css
                        .as_ref()
                        .filter(|s| !s.is_empty())
                });
            if let Some(declared) = self.manifest.widget_styles.as_deref() {
                if let Some(content) = content {
                    self.write_text(tapp_dir, declared, content, "widget_styles", context)
                        .await?;
                } else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        api_error(format!(
                            "Missing widgetStyles content for declared path {declared}"
                        )),
                    ));
                }
            } else if let Some(content) = resources.generated_widget_css.as_ref() {
                self.write_text(tapp_dir, "widget.css", content, "widget_css", context)
                    .await?;
            }
        }
        // Page CSS: MUST write declared pageStyles (e.g. page.css) or install validation fails.
        // Prefer page_styles content; fall back to generated_page_css if mis-routed by cssMode.
        {
            let content = resources
                .page_styles
                .as_ref()
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    resources
                        .generated_page_css
                        .as_ref()
                        .filter(|s| !s.is_empty())
                });
            if let Some(declared) = self.manifest.page_styles.as_deref() {
                if let Some(content) = content {
                    self.write_text(tapp_dir, declared, content, "page_styles", context)
                        .await?;
                } else {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        api_error(format!(
                            "Missing pageStyles content for declared path {declared} \
(frontend must send pageCss; store fetch must download download.page_styles)"
                        )),
                    ));
                }
            } else if let Some(content) = resources.generated_page_css.as_ref() {
                self.write_text(tapp_dir, "page.css", content, "page_css", context)
                    .await?;
            }
        }
        if let Some(content) = &resources.page_template {
            let path = self
                .manifest
                .page_template
                .as_deref()
                .unwrap_or("page.html");
            self.write_text(tapp_dir, path, content, "page_template", context)
                .await?;
        }
        if let Some(widgets) = &resources.widget_templates {
            for (widget_id, templates) in widgets {
                for (size, content) in templates {
                    let path = widget_template_path(&self.manifest, widget_id, size)
                        .expect("validated Widget template path");
                    self.write_text(tapp_dir, path, content, "widget_template", context)
                        .await?;
                }
            }
        }
        if let Some(i18n) = &resources.i18n {
            for (language, data) in i18n {
                let json = serde_json::to_string_pretty(data).map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        api_error("Failed to serialize i18n resource"),
                    )
                })?;
                self.write_text(
                    tapp_dir,
                    &format!("i18n/{language}.json"),
                    &json,
                    "i18n",
                    context,
                )
                .await?;
            }
        }
        if let Some(page_modules) = &resources.page_modules {
            for (filename, content) in page_modules {
                self.write_text(
                    tapp_dir,
                    &format!("page/{filename}"),
                    content,
                    "page_module",
                    context,
                )
                .await?;
            }
        }
        if let Some(assets) = &resources.assets {
            write_install_assets(tapp_dir, &self.manifest, assets)
                .await
                .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
        }
        Ok(())
    }

    async fn write_text(
        &self,
        tapp_dir: &Path,
        path: &str,
        content: &str,
        label: &'static str,
        context: PackageStageContext,
    ) -> Result<(), PackageError> {
        write_tapp_resource(tapp_dir, path, content)
            .await
            .map(|_| ())
            .map_err(|error| {
                self.log_write_failure(
                    &format!("write_tapp_resource({label})"),
                    context,
                    tapp_dir,
                    &error,
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error(format!("Failed to save {label}: {error}")),
                )
            })
    }

    async fn extract_archive(
        &self,
        tapp_dir: &Path,
        file_data: &[u8],
        context: PackageStageContext,
    ) -> Result<(), PackageError> {
        let tapp_dir = tapp_dir.to_path_buf();
        let extraction_dir = tapp_dir.clone();
        let file_data = file_data.to_vec();
        let result = tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
            use std::io::Read;

            let cursor = std::io::Cursor::new(file_data);
            let mut archive = zip::ZipArchive::new(cursor)?;
            for index in 0..archive.len() {
                let mut file = archive.by_index(index)?;
                let out_path = archive_entry_path(&extraction_dir, file.name())
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                if file.is_dir() {
                    std::fs::create_dir_all(&out_path)?;
                    continue;
                }
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut content = Vec::new();
                file.read_to_end(&mut content)?;
                std::fs::write(out_path, content)?;
            }
            Ok(())
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.log_write_failure("extract_write", context, tapp_dir.as_path(), &error);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error(format!("Failed to save files: {error}")),
                ))
            }
            Err(error) => {
                self.log_write_failure("extract_join", context, tapp_dir.as_path(), &error);
                Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error(format!("Failed to extract files: {error}")),
                ))
            }
        }
    }

    fn log_write_failure(
        &self,
        operation: &str,
        context: PackageStageContext,
        path: &Path,
        error: &dyn std::fmt::Display,
    ) {
        log_install_failure(
            operation,
            &self.manifest.id,
            context.user_id,
            context.installation_owner_id,
            Some(path),
            error,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use std::io::Write;

    fn manifest() -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.prepared",
            "name": "Prepared package",
            "version": "1.0.0",
            "main": "main.js",
            "category": "media",
            "permissions": []
        }))
        .unwrap()
    }

    #[test]
    fn validation_rejects_target_id_mismatch() {
        let package = PreparedTappPackage::from_resources(
            manifest(),
            PreparedTappResources {
                code: "export {};".to_string(),
                ..PreparedTappResources::default()
            },
        );

        assert!(package.validate(Some("com.example.other")).is_err());
        assert!(package.validate(Some("com.example.prepared")).is_ok());
    }

    #[test]
    fn explicit_resource_overrides_replace_store_resources() {
        let mut original_i18n = HashMap::new();
        original_i18n.insert("en-US".to_string(), json!({ "title": "Store" }));
        let mut package = PreparedTappPackage::from_resources(
            manifest(),
            PreparedTappResources {
                code: "export {};".to_string(),
                i18n: Some(original_i18n),
                ..PreparedTappResources::default()
            },
        );
        let mut override_i18n = HashMap::new();
        override_i18n.insert("en-US".to_string(), json!({ "title": "Override" }));

        package.apply_resource_overrides(Some(override_i18n), None, None);

        let PreparedTappPayload::Resources(resources) = &package.payload else {
            panic!("expected structured resources");
        };
        assert_eq!(
            resources.i18n.as_ref().unwrap()["en-US"]["title"],
            "Override"
        );
    }

    #[tokio::test]
    async fn structured_package_stages_every_declared_resource() {
        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-structured-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.structured",
            "name": "Structured package",
            "version": "1.0.0",
            "main": "src/main.js",
            "styles": "styles/app.css",
            "pageModules": ["extra.js"],
            "assets": ["assets/pixel.png"],
            "category": "media",
            "permissions": []
        }))
        .unwrap();
        let mut i18n = HashMap::new();
        i18n.insert("en-US".to_string(), json!({ "title": "Prepared" }));
        let mut page_modules = HashMap::new();
        page_modules.insert(
            "extra.js".to_string(),
            "export const extra = true;".to_string(),
        );
        let mut assets = HashMap::new();
        assets.insert("assets/pixel.png".to_string(), "iVBORw0KGgo=".to_string());
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                code: "export const ready = true;".to_string(),
                styles: Some("body { color: red; }".to_string()),
                i18n: Some(i18n),
                page_modules: Some(page_modules),
                assets: Some(assets),
                ..PreparedTappResources::default()
            },
        );

        package.validate(None).unwrap();
        package
            .stage_into(
                &root,
                Utc::now().fixed_offset(),
                PackageStageContext {
                    user_id: 1,
                    installation_owner_id: 1,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("src/main.js")).unwrap(),
            "export const ready = true;"
        );
        assert!(root.join("manifest.json").is_file());
        assert!(root.join("styles/app.css").is_file());
        assert!(root.join("i18n/en-US.json").is_file());
        assert!(root.join("page/extra.js").is_file());
        assert!(root.join("assets/pixel.png").is_file());
        assert!(root.join(super::super::TAPP_INSTALL_STATE_FILE).is_file());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn archive_package_stages_validated_nested_entrypoint() {
        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-archive-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest = json!({
            "id": "com.example.archive",
            "name": "Archive package",
            "version": "1.0.0",
            "main": "src/main.js",
            "category": "media",
            "permissions": []
        });
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("manifest.json", options).unwrap();
        writer
            .write_all(serde_json::to_string(&manifest).unwrap().as_bytes())
            .unwrap();
        writer.start_file("src/main.js", options).unwrap();
        writer.write_all(b"export const archive = true;").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let package = PreparedTappPackage::from_archive(bytes).unwrap();

        package
            .stage_into(
                &root,
                Utc::now().fixed_offset(),
                PackageStageContext {
                    user_id: 2,
                    installation_owner_id: 2,
                },
            )
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("src/main.js")).unwrap(),
            "export const archive = true;"
        );
        assert!(root.join("manifest.json").is_file());
        assert!(root.join(super::super::TAPP_INSTALL_STATE_FILE).is_file());

        std::fs::remove_dir_all(root).unwrap();
    }

    /// doudizhu-like: cssMode=separated + pageStyles=page.css + non-empty pageCss
    /// must write page.css before validate_installed_resources.
    #[tokio::test]
    async fn separated_page_styles_writes_declared_page_css() {
        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-page-css-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.myriad.doudizhu",
            "name": "Dou Dizhu",
            "version": "1.0.0",
            "main": "main.js",
            "cssMode": "separated",
            "pageStyles": "page.css",
            "hasPage": true,
            "category": "game",
            "permissions": []
        }))
        .unwrap();
        let page_css = "/* doudizhu page styles */ .table { color: gold; }";
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                code: "export {};".to_string(),
                page_styles: Some(page_css.to_string()),
                ..PreparedTappResources::default()
            },
        );

        package.validate(None).unwrap();
        package
            .stage_into(
                &root,
                Utc::now().fixed_offset(),
                PackageStageContext {
                    user_id: 1,
                    installation_owner_id: 1,
                },
            )
            .await
            .unwrap();

        assert!(root.join("page.css").is_file());
        assert_eq!(std::fs::read_to_string(root.join("page.css")).unwrap(), page_css);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Content mis-routed into generated_page_css (e.g. cssMode mapping quirk) still
    /// writes the declared pageStyles path.
    #[tokio::test]
    async fn declared_page_styles_accepts_generated_page_css_fallback() {
        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-page-css-fallback-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.separated-fallback",
            "name": "Separated fallback",
            "version": "1.0.0",
            "main": "main.js",
            "cssMode": "separated",
            "pageStyles": "page.css",
            "category": "game",
            "permissions": []
        }))
        .unwrap();
        let css = ".fallback { display: block; }";
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                code: "export {};".to_string(),
                // Simulates pageCss landing in generated_page_css instead of page_styles
                generated_page_css: Some(css.to_string()),
                ..PreparedTappResources::default()
            },
        );

        package.validate(None).unwrap();
        package
            .stage_into(
                &root,
                Utc::now().fixed_offset(),
                PackageStageContext {
                    user_id: 1,
                    installation_owner_id: 1,
                },
            )
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(root.join("page.css")).unwrap(), css);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validate_rejects_missing_page_styles_content_with_clear_error() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.missing-page-css",
            "name": "Missing page css",
            "version": "1.0.0",
            "main": "main.js",
            "cssMode": "separated",
            "pageStyles": "page.css",
            "category": "game",
            "permissions": []
        }))
        .unwrap();
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                code: "export {};".to_string(),
                ..PreparedTappResources::default()
            },
        );

        let err = package.validate(None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let body = serde_json::to_string(&err.1 .0).unwrap_or_default();
        assert!(
            body.contains("pageStyles") || body.contains("pageCss"),
            "error should mention pageStyles/pageCss, got: {body}"
        );
        assert!(
            body.contains("page.css"),
            "error should mention declared path page.css, got: {body}"
        );
    }

    #[test]
    fn validate_rejects_empty_page_styles_content() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.empty-page-css",
            "name": "Empty page css",
            "version": "1.0.0",
            "main": "main.js",
            "pageStyles": "styles/page.css",
            "category": "utility",
            "permissions": []
        }))
        .unwrap();
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                code: "export {};".to_string(),
                page_styles: Some(String::new()),
                generated_page_css: Some(String::new()),
                ..PreparedTappResources::default()
            },
        );

        let err = package.validate(None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let body = serde_json::to_string(&err.1 .0).unwrap_or_default();
        assert!(
            body.contains("pageStyles") || body.contains("pageCss"),
            "error should mention pageStyles/pageCss, got: {body}"
        );
        assert!(
            body.contains("styles/page.css"),
            "error should mention declared path, got: {body}"
        );
    }

    #[test]
    fn validate_rejects_missing_widget_styles_content() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.missing-widget-css",
            "name": "Missing widget css",
            "version": "1.0.0",
            "main": "main.js",
            "widgetStyles": "widget.css",
            "category": "utility",
            "permissions": []
        }))
        .unwrap();
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                code: "export {};".to_string(),
                ..PreparedTappResources::default()
            },
        );

        let err = package.validate(None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let body = serde_json::to_string(&err.1 .0).unwrap_or_default();
        assert!(
            body.contains("widgetStyles") || body.contains("widgetCss"),
            "error should mention widgetStyles/widgetCss, got: {body}"
        );
    }
}
