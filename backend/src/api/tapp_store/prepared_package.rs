//! Validated in-memory Tapp package and shared staging writer.
//!
//! Domain validation / resource overrides live in
//! [`crate::services::tapp_prepared_package`]. This module maps domain errors to
//! Axum responses and performs filesystem staging (resources write + archive extract).

use super::{
    api_error, archive_entry_path, log_install_failure, validate_installed_resources,
    validate_tapp_archive, widget_template_path, write_install_assets, write_install_generation,
    write_tapp_resource, ApiResponse,
};
use axum::{http::StatusCode, Json};
use chrono::{DateTime, FixedOffset};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

use crate::services::tapp_prepared_package::{
    check_manifest_byte_size, nonempty_content, parse_manifest_json, resolved_style_content,
    PackageLoadError, PackageValidateError,
};

// Path-stable re-exports for installation / store_package / tests.
pub(super) use crate::services::tapp_prepared_package::{
    PreparedTappPackage, PreparedTappResources,
};

type PackageError = (StatusCode, Json<ApiResponse<()>>);

#[derive(Debug, Clone, Copy)]
pub(super) struct PackageStageContext {
    pub user_id: i32,
    pub installation_owner_id: i32,
}

fn map_validate_error(err: PackageValidateError) -> PackageError {
    (
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::BAD_REQUEST),
        api_error(err.message()),
    )
}

fn map_load_error(err: PackageLoadError) -> PackageError {
    (
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::BAD_REQUEST),
        api_error(err.message()),
    )
}

/// HTTP adapter: load a .tapp archive into a validated prepared package.
pub(super) fn package_from_archive(
    file_data: Vec<u8>,
) -> Result<PreparedTappPackage, PackageError> {
    let cursor = std::io::Cursor::new(&file_data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|_| map_load_error(PackageLoadError::InvalidArchive))?;
    validate_tapp_archive(&mut archive)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;

    let manifest_content = {
        let mut manifest_file = archive
            .by_name("manifest.json")
            .map_err(|_| map_load_error(PackageLoadError::ManifestNotFound))?;
        check_manifest_byte_size(manifest_file.size()).map_err(map_load_error)?;
        let mut content = String::new();
        std::io::Read::read_to_string(&mut manifest_file, &mut content)
            .map_err(|_| map_load_error(PackageLoadError::ManifestUnreadable))?;
        content
    };

    let manifest = parse_manifest_json(&manifest_content).map_err(map_load_error)?;
    PreparedTappPackage::from_archive_parts(manifest, file_data).map_err(map_validate_error)
}

/// Extension methods that stay HTTP-bound (StatusCode mapping + staging IO).
pub(super) trait PreparedTappPackageHttp {
    fn validate_http(&self, expected_tapp_id: Option<&str>) -> Result<(), PackageError>;
    fn stage_into_http(
        &self,
        tapp_dir: &Path,
        generation: DateTime<FixedOffset>,
        context: PackageStageContext,
    ) -> impl std::future::Future<Output = Result<(), PackageError>> + Send;
}

impl PreparedTappPackageHttp for PreparedTappPackage {
    fn validate_http(&self, expected_tapp_id: Option<&str>) -> Result<(), PackageError> {
        self.validate(expected_tapp_id).map_err(map_validate_error)
    }

    async fn stage_into_http(
        &self,
        tapp_dir: &Path,
        generation: DateTime<FixedOffset>,
        context: PackageStageContext,
    ) -> Result<(), PackageError> {
        match self.resources() {
            Some(resources) => {
                write_resources(self, tapp_dir, resources, context).await?;
                let manifest_json =
                    serde_json::to_string_pretty(&self.manifest).unwrap_or_default();
                let manifest_path = tapp_dir.join("manifest.json");
                fs::write(&manifest_path, manifest_json)
                    .await
                    .map_err(|error| {
                        log_write_failure(self, "write_manifest", context, &manifest_path, &error);
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            api_error(format!("Failed to save manifest: {error}")),
                        )
                    })?;
            }
            None => {
                // MYR-025: share Arc into extract; do not clone the full zip.
                let file_data = self.archive_arc().expect("archive package has bytes");
                extract_archive(self, tapp_dir, file_data, context).await?;
            }
        }

        write_install_generation(tapp_dir, generation).map_err(|error| {
            log_write_failure(self, "write_install_generation", context, tapp_dir, &error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Failed to save install state: {error}")),
            )
        })?;
        validate_installed_resources(&self.manifest, tapp_dir)
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))
    }
}

// Convenience wrappers preserving call-site method names used by installation.
impl PreparedTappPackage {
    /// Path-stable: archive load with HTTP error mapping.
    pub(super) fn from_archive(file_data: Vec<u8>) -> Result<Self, PackageError> {
        package_from_archive(file_data)
    }

    /// Path-stable: domain validate mapped to HTTP errors.
    pub(super) fn validate_for_http(
        &self,
        expected_tapp_id: Option<&str>,
    ) -> Result<(), PackageError> {
        self.validate_http(expected_tapp_id)
    }

    /// Path-stable: stage to disk with HTTP errors.
    pub(super) async fn stage_into(
        &self,
        tapp_dir: &Path,
        generation: DateTime<FixedOffset>,
        context: PackageStageContext,
    ) -> Result<(), PackageError> {
        self.stage_into_http(tapp_dir, generation, context).await
    }
}

async fn write_resources(
    package: &PreparedTappPackage,
    tapp_dir: &Path,
    resources: &PreparedTappResources,
    context: PackageStageContext,
) -> Result<(), PackageError> {
    write_text(
        package,
        tapp_dir,
        &package.manifest.main,
        &resources.code,
        "main",
        context,
    )
    .await?;

    if let Some(content) = &resources.styles {
        let path = package.manifest.styles.as_deref().unwrap_or("styles.css");
        write_text(package, tapp_dir, path, content, "styles", context).await?;
    }

    // Widget CSS: declared widgetStyles path first, else generated widget.css.
    {
        let content = resolved_style_content(
            resources.widget_styles.as_ref(),
            resources.generated_widget_css.as_ref(),
        );
        if let Some(declared) = package.manifest.widget_styles.as_deref() {
            if let Some(content) = content {
                write_text(
                    package,
                    tapp_dir,
                    declared,
                    content,
                    "widget_styles",
                    context,
                )
                .await?;
            } else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    api_error(format!(
                        "Missing widgetStyles content for declared path {declared}"
                    )),
                ));
            }
        } else if let Some(content) = nonempty_content(resources.generated_widget_css.as_ref()) {
            write_text(
                package,
                tapp_dir,
                "widget.css",
                content,
                "widget_css",
                context,
            )
            .await?;
        }
    }

    // Page CSS: MUST write declared pageStyles (e.g. page.css) or install validation fails.
    {
        let content = resolved_style_content(
            resources.page_styles.as_ref(),
            resources.generated_page_css.as_ref(),
        );
        if let Some(declared) = package.manifest.page_styles.as_deref() {
            if let Some(content) = content {
                write_text(package, tapp_dir, declared, content, "page_styles", context).await?;
            } else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    api_error(format!(
                        "Missing pageStyles content for declared path {declared} \
(frontend must send pageCss; store fetch must download download.page_styles)"
                    )),
                ));
            }
        } else if let Some(content) = nonempty_content(resources.generated_page_css.as_ref()) {
            write_text(package, tapp_dir, "page.css", content, "page_css", context).await?;
        }
    }

    if let Some(content) = nonempty_content(resources.page_template.as_ref()) {
        let path = package
            .manifest
            .page_template
            .as_deref()
            .unwrap_or("page.html");
        write_text(package, tapp_dir, path, content, "page_template", context).await?;
    }
    if let Some(widgets) = &resources.widget_templates {
        for (widget_id, templates) in widgets {
            for (size, content) in templates {
                let path = widget_template_path(&package.manifest, widget_id, size)
                    .expect("validated Widget template path");
                write_text(package, tapp_dir, path, content, "widget_template", context).await?;
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
            write_text(
                package,
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
            write_text(
                package,
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
        write_install_assets(tapp_dir, &package.manifest, assets)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    }
    Ok(())
}

async fn write_text(
    package: &PreparedTappPackage,
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
            log_write_failure(
                package,
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
    package: &PreparedTappPackage,
    tapp_dir: &Path,
    file_data: Arc<Vec<u8>>,
    context: PackageStageContext,
) -> Result<(), PackageError> {
    let tapp_dir = tapp_dir.to_path_buf();
    let extraction_dir = tapp_dir.clone();
    // MYR-025: move Arc into blocking task — refcount share, not full zip clone.
    let result = tokio::task::spawn_blocking(move || -> Result<(), std::io::Error> {
        use std::io::Read;

        let cursor = std::io::Cursor::new(file_data.as_slice());
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
            log_write_failure(
                package,
                "extract_write",
                context,
                tapp_dir.as_path(),
                &error,
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Failed to save files: {error}")),
            ))
        }
        Err(error) => {
            log_write_failure(package, "extract_join", context, tapp_dir.as_path(), &error);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error(format!("Failed to extract files: {error}")),
            ))
        }
    }
}

fn log_write_failure(
    package: &PreparedTappPackage,
    operation: &str,
    context: PackageStageContext,
    path: &Path,
    error: &dyn std::fmt::Display,
) {
    log_install_failure(
        operation,
        &package.manifest.id,
        context.user_id,
        context.installation_owner_id,
        Some(path),
        error,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use std::collections::HashMap;
    use std::io::Write;

    use myriad_tapp_contract::manifest::TappManifest;

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

        assert!(package
            .validate_for_http(Some("com.example.other"))
            .is_err());
        assert!(package
            .validate_for_http(Some("com.example.prepared"))
            .is_ok());
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

        assert_eq!(
            package.resources().unwrap().i18n.as_ref().unwrap()["en-US"]["title"],
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

        package.validate_for_http(None).unwrap();
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

        package.validate_for_http(None).unwrap();
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
        assert_eq!(
            std::fs::read_to_string(root.join("page.css")).unwrap(),
            page_css
        );
        std::fs::remove_dir_all(root).unwrap();
    }

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
                generated_page_css: Some(css.to_string()),
                ..PreparedTappResources::default()
            },
        );

        package.validate_for_http(None).unwrap();
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

        let err = package.validate_for_http(None).unwrap_err();
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

        let err = package.validate_for_http(None).unwrap_err();
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

        let err = package.validate_for_http(None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let body = serde_json::to_string(&err.1 .0).unwrap_or_default();
        assert!(
            body.contains("widgetStyles") || body.contains("widgetCss"),
            "error should mention widgetStyles/widgetCss, got: {body}"
        );
    }
}
