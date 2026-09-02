//! Validated in-memory Tapp package and shared staging writer.
//!
//! Domain validation / resource overrides live in
//! [`crate::services::tapp_prepared_package`]. This module maps domain errors to
//! Axum responses and performs filesystem staging (resources write + archive extract).

use super::{
    api_error, archive_entry_path, log_install_failure, validate_installed_resources,
    validate_tapp_archive, validate_tapp_archive_with, widget_template_path, write_install_assets,
    write_install_generation, write_tapp_resource, ApiResponse,
};
use axum::{http::StatusCode, Json};
use chrono::{DateTime, FixedOffset};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

use crate::services::tapp_package_read::{HOST_PAGE_CSS, HOST_WIDGET_CSS};
use crate::services::tapp_prepared_package::{
    check_manifest_byte_size, nonempty_content, parse_manifest_json, PackageLoadError,
    PackageValidateError,
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
    let budget = crate::services::tapp_install_resources::ArchiveBudget::for_manifest(&manifest);
    budget
        .check_compressed(file_data.len())
        .map_err(|error| (StatusCode::PAYLOAD_TOO_LARGE, api_error(error)))?;
    validate_tapp_archive_with(&mut archive, budget)
        .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))?;
    PreparedTappPackage::from_archive_parts(
        manifest,
        file_data,
        &crate::services::tapp_prepared_package::current_system_version(),
    )
    .map_err(map_validate_error)
}

/// Extension methods that stay HTTP-bound (StatusCode mapping + staging IO).
pub(super) trait PreparedTappPackageHttp {
    fn validate_for_http(&self, expected_tapp_id: Option<&str>) -> Result<(), PackageError>;
    fn stage_into(
        &self,
        tapp_dir: &Path,
        generation: DateTime<FixedOffset>,
        context: PackageStageContext,
    ) -> impl std::future::Future<Output = Result<(), PackageError>> + Send;
}

impl PreparedTappPackageHttp for PreparedTappPackage {
    fn validate_for_http(&self, expected_tapp_id: Option<&str>) -> Result<(), PackageError> {
        self.validate(
            expected_tapp_id,
            &crate::services::tapp_prepared_package::current_system_version(),
        )
        .map_err(map_validate_error)
    }

    async fn stage_into(
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
                            api_error("Failed to save manifest"),
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
                api_error("Failed to save install state"),
            )
        })?;
        validate_installed_resources(&self.manifest, tapp_dir)
            .map_err(|error| (StatusCode::BAD_REQUEST, api_error(error)))
    }
}

async fn write_resources(
    package: &PreparedTappPackage,
    tapp_dir: &Path,
    resources: &PreparedTappResources,
    context: PackageStageContext,
) -> Result<(), PackageError> {
    // 层入口与层内被 require 的文件按各自的包内相对路径落盘。
    for (relative, content) in &resources.modules {
        write_text(package, tapp_dir, relative, content, "module", context).await?;
    }

    if let Some(declared) = package
        .manifest
        .core
        .as_ref()
        .and_then(|core| core.styles.as_deref())
    {
        let Some(content) = nonempty_content(resources.core_styles.as_ref()) else {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error(format!(
                    "Missing content for declared core.styles={declared}"
                )),
            ));
        };
        write_text(package, tapp_dir, declared, content, "core_styles", context).await?;
    }

    if let Some(declared) = package
        .manifest
        .page
        .as_ref()
        .and_then(|page| page.styles.as_deref())
    {
        let Some(content) = nonempty_content(resources.page_styles.as_ref()) else {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error(format!(
                    "Missing content for declared page.styles={declared}"
                )),
            ));
        };
        write_text(package, tapp_dir, declared, content, "page_styles", context).await?;
    }

    if let Some(widgets) = &package.manifest.widgets {
        for widget in widgets {
            let Some(declared) = widget.styles.as_deref() else {
                continue;
            };
            let content = resources
                .widget_styles
                .as_ref()
                .and_then(|styles| styles.get(&widget.id));
            let Some(content) = nonempty_content(content) else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    api_error(format!(
                        "Missing content for declared widgets[{}].styles={declared}",
                        widget.id
                    )),
                ));
            };
            write_text(
                package,
                tapp_dir,
                declared,
                content,
                "widget_styles",
                context,
            )
            .await?;
        }
    }

    // 宿主预编译 Tailwind 走固定路径，不参与 manifest 声明的资源校验。
    if let Some(content) = nonempty_content(resources.generated_widget_css.as_ref()) {
        write_text(
            package,
            tapp_dir,
            HOST_WIDGET_CSS,
            content,
            "widget_css",
            context,
        )
        .await?;
    }
    if let Some(content) = nonempty_content(resources.generated_page_css.as_ref()) {
        write_text(
            package,
            tapp_dir,
            HOST_PAGE_CSS,
            content,
            "page_css",
            context,
        )
        .await?;
    }

    if let Some(declared) = package
        .manifest
        .page
        .as_ref()
        .and_then(|page| page.template.as_deref())
    {
        let Some(content) = nonempty_content(resources.page_template.as_ref()) else {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error(format!(
                    "Missing content for declared page.template={declared}"
                )),
            ));
        };
        write_text(
            package,
            tapp_dir,
            declared,
            content,
            "page_template",
            context,
        )
        .await?;
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
                api_error("Failed to save package files"),
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
                api_error("Failed to save files"),
            ))
        }
        Err(error) => {
            log_write_failure(package, "extract_join", context, tapp_dir.as_path(), &error);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                api_error("Failed to extract files"),
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
            "core": { "entry": "core.js" },
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
                modules: HashMap::from([("core.js".to_string(), "export {};".to_string())]),
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
            "core": { "entry": "src/core.js", "styles": "styles/app.css" },
            "page": { "entry": "page/index.js" },
            "assets": ["assets/pixel.png"],
            "category": "media",
            "permissions": []
        }))
        .unwrap();
        let mut i18n = HashMap::new();
        i18n.insert("en-US".to_string(), json!({ "title": "Prepared" }));
        let mut assets = HashMap::new();
        assets.insert("assets/pixel.png".to_string(), "iVBORw0KGgo=".to_string());
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([
                    (
                        "src/core.js".to_string(),
                        "export const ready = true;".to_string(),
                    ),
                    (
                        "page/index.js".to_string(),
                        "export const extra = true;".to_string(),
                    ),
                ]),
                core_styles: Some("body { color: red; }".to_string()),
                i18n: Some(i18n),
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
            std::fs::read_to_string(root.join("src/core.js")).unwrap(),
            "export const ready = true;"
        );
        assert!(root.join("manifest.json").is_file());
        assert!(root.join("styles/app.css").is_file());
        assert!(root.join("i18n/en-US.json").is_file());
        assert!(root.join("page/index.js").is_file());
        assert!(root.join("assets/pixel.png").is_file());
        assert!(root.join(super::super::TAPP_INSTALL_STATE_FILE).is_file());

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 安装 → 落盘 → 扫描登记 → 按入口闭包分发，一条链路走完。
    ///
    /// 文件故意不放进 `page/` / `widget/`：层归属必须来自 manifest 入口和
    /// require 图，不能来自目录名。
    #[tokio::test]
    async fn staged_layers_are_distributed_by_entry_graph() {
        use crate::services::tapp_install_resources::collect_tapp_module_graph;

        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-layers-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.layers",
            "name": "Layered package",
            "version": "1.0.0",
            "core": { "entry": "src/core.js" },
            "page": { "entry": "screens/page.js" },
            "widgets": [{
                "id": "card",
                "name": "Card",
                "defaultSize": "2x2",
                "sizes": ["2x2"],
                "category": "utility",
                "entry": "components/card.js"
            }],
            "category": "utility",
            "permissions": ["widget:register"]
        }))
        .unwrap();

        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([
                    (
                        "src/core.js".to_string(),
                        "var shared = require('../lib/shared.js');".to_string(),
                    ),
                    (
                        "lib/shared.js".to_string(),
                        "module.exports = 1;".to_string(),
                    ),
                    (
                        "screens/page.js".to_string(),
                        "require('../src/core.js'); require('../shared/page-state.js');"
                            .to_string(),
                    ),
                    (
                        "shared/page-state.js".to_string(),
                        "module.exports = {};".to_string(),
                    ),
                    (
                        "components/card.js".to_string(),
                        "require('../src/core.js');".to_string(),
                    ),
                    (
                        "components/other-widget.js".to_string(),
                        "globalThis.otherWidget = true;".to_string(),
                    ),
                ]),
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

        // 扫描登记：落盘后每个 require 目标都必须存在。
        crate::api::tapp_store::package_files::validate_installed_package_modules(&root).unwrap();

        let scanned = crate::api::tapp_store::package_files::collect_package_module_paths(&root);
        let sources: HashMap<String, String> = scanned
            .iter()
            .map(|relative| {
                (
                    relative.clone(),
                    std::fs::read_to_string(root.join(relative)).unwrap(),
                )
            })
            .collect();
        let for_entries = |entries: &[&str]| {
            collect_tapp_module_graph(
                &sources,
                &entries
                    .iter()
                    .map(|entry| (*entry).to_string())
                    .collect::<Vec<_>>(),
            )
            .unwrap()
            .included
        };

        assert_eq!(
            for_entries(&["src/core.js", "components/card.js"]),
            vec!["components/card.js", "lib/shared.js", "src/core.js"]
        );
        assert_eq!(
            for_entries(&["src/core.js", "screens/page.js"]),
            vec![
                "lib/shared.js",
                "screens/page.js",
                "shared/page-state.js",
                "src/core.js"
            ]
        );
        assert_eq!(
            for_entries(&["src/core.js"]),
            vec!["lib/shared.js", "src/core.js"]
        );
        assert!(!for_entries(&["src/core.js", "components/card.js"])
            .iter()
            .any(|path| path == "components/other-widget.js"));

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 层内文件引用了不存在的模块，装包时就要失败，而不是等打开应用才白屏。
    #[tokio::test]
    async fn staging_rejects_a_require_target_that_does_not_exist() {
        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-dangling-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.dangling",
            "name": "Dangling require",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "category": "utility",
            "permissions": []
        }))
        .unwrap();

        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([(
                    "core.js".to_string(),
                    "require('./missing.js');".to_string(),
                )]),
                ..PreparedTappResources::default()
            },
        );

        package.validate_for_http(None).unwrap();
        let error = package
            .stage_into(
                &root,
                Utc::now().fixed_offset(),
                PackageStageContext {
                    user_id: 1,
                    installation_owner_id: 1,
                },
            )
            .await
            .expect_err("dangling require must fail staging");
        assert_eq!(error.0, StatusCode::BAD_REQUEST);

        let _ = std::fs::remove_dir_all(root);
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
            "core": { "entry": "src/main.js" },
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
        let package = package_from_archive(bytes).unwrap();

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
            "core": { "entry": "core.js" },
            "page": { "entry": "page/index.js", "styles": "page.css" },
            "category": "game",
            "permissions": []
        }))
        .unwrap();
        let page_css = "/* doudizhu page styles */ .table { color: gold; }";
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([
                    ("core.js".to_string(), "export {};".to_string()),
                    ("page/index.js".to_string(), "export {};".to_string()),
                ]),
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

    /// 层内多文件必须真的能装上：入口 require 进来的文件不在 Manifest 里声明，
    /// 但要落盘、要受检，且 require 目标缺失时安装必须失败。
    #[tokio::test]
    async fn stages_layer_internal_modules_and_checks_require_targets() {
        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-layer-modules-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.layer-modules",
            "name": "Layer modules",
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
                modules: HashMap::from([
                    (
                        "core.js".to_string(),
                        "module.exports = { shared: 1 };".to_string(),
                    ),
                    (
                        "page/index.js".to_string(),
                        "var core = require('../core.js'); var s = require('./state.js');"
                            .to_string(),
                    ),
                    (
                        "page/state.js".to_string(),
                        "module.exports = {};".to_string(),
                    ),
                ]),
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

        // 未声明但被 require 的文件同样落盘
        assert!(root.join("page/state.js").is_file());
        super::super::validate_installed_resources(&package.manifest, &root).unwrap();

        // 指向不存在文件的 require 必须在安装期就失败
        std::fs::write(root.join("page/index.js"), "require('./ghost.js');").unwrap();
        let error = super::super::validate_installed_resources(&package.manifest, &root)
            .expect_err("missing require target must fail install validation");
        assert!(error.contains("ghost.js"), "got: {error}");

        std::fs::remove_dir_all(root).unwrap();
    }

    /// 作者层样式与宿主预编译 Tailwind 是两条通道：即使作者把 `page.styles`
    /// 命名成 `page.css`，宿主产物也落在自己的目录里，互不覆盖。
    #[tokio::test]
    async fn host_compiled_css_does_not_collide_with_author_styles() {
        let root = std::env::temp_dir().join(format!(
            "myriad-prepared-page-css-channels-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.two-channels",
            "name": "Two channels",
            "version": "1.0.0",
            "core": { "entry": "core.js" },
            "page": { "entry": "page/index.js", "styles": "page.css" },
            "category": "game",
            "permissions": []
        }))
        .unwrap();
        let author_css = ".author { display: block; }";
        let host_css = ".host-generated { display: flex; }";
        let package = PreparedTappPackage::from_resources(
            manifest,
            PreparedTappResources {
                modules: HashMap::from([
                    ("core.js".to_string(), "export {};".to_string()),
                    ("page/index.js".to_string(), "export {};".to_string()),
                ]),
                page_styles: Some(author_css.to_string()),
                generated_page_css: Some(host_css.to_string()),
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
            std::fs::read_to_string(root.join("page.css")).unwrap(),
            author_css
        );
        assert_eq!(
            std::fs::read_to_string(root.join("host/page.css")).unwrap(),
            host_css
        );
        std::fs::remove_dir_all(root).unwrap();
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

        let err = package.validate_for_http(None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let body = serde_json::to_string(&err.1 .0).unwrap_or_default();
        assert!(
            body.contains("page.styles"),
            "error should name the declaring layer field, got: {body}"
        );
        assert!(
            body.contains("page.css"),
            "error should mention declared path page.css, got: {body}"
        );
    }

    /// 宿主预编译产物不能顶替作者声明的层样式——它们落在不同路径上。
    #[test]
    fn validate_rejects_empty_page_styles_even_with_host_css() {
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
                generated_page_css: Some(".host {}".to_string()),
                ..PreparedTappResources::default()
            },
        );

        let err = package.validate_for_http(None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let body = serde_json::to_string(&err.1 .0).unwrap_or_default();
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

        let err = package.validate_for_http(None).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
        let body = serde_json::to_string(&err.1 .0).unwrap_or_default();
        assert!(
            body.contains("widgets[].styles"),
            "error should name the declaring layer field, got: {body}"
        );
    }
}
