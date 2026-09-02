//! Validation and bounded-resource rules for Tapp package manifests.
//!
//! Pure domain checks (ids, paths, settings, full manifest) live here so store
//! install/update paths and agent tooling do not depend on `crate::api::tapp_store`.
//! Limits re-export [`myriad_tapp_contract::contract_rules`] (shared with the
//! offline CLI) so install and `check` cannot drift.

use myriad_tapp_contract::manifest::TappManifest;

// Single source of truth shared with the offline CLI contract exporter.
pub use myriad_tapp_contract::contract_rules::{
    FORBIDDEN_OUTBOUND_HEADERS, HTTP_BODY_METHODS, HTTP_METHODS, MAX_AGENT_SCHEMA_RESOURCE_BYTES,
    MAX_CREDENTIAL_HEADER_PREFIX_LEN, MAX_CREDENTIAL_KEY_LEN, MAX_DATA_EXCHANGE_DECLARATIONS,
    MAX_DATA_EXCHANGE_ID_LEN, MAX_DATA_EXCHANGE_RESPONSE_BYTES, MAX_DATA_EXCHANGE_SCHEMA_BYTES,
    MAX_RESOURCE_PATH_LEN, MAX_TAPP_ARCHIVE_BYTES, MAX_TAPP_ARCHIVE_FILES,
    MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES, MAX_TAPP_ASSETS, MAX_TAPP_ASSETS_TOTAL_BYTES,
    MAX_TAPP_ASSET_BYTES, MAX_TAPP_CREDENTIALS, MAX_TAPP_GAME_ARCHIVE_BYTES,
    MAX_TAPP_GAME_ARCHIVE_FILES, MAX_TAPP_GAME_ARCHIVE_UNCOMPRESSED_BYTES, MAX_TAPP_GAME_ASSETS,
    MAX_TAPP_GAME_ASSETS_TOTAL_BYTES, MAX_TAPP_GAME_ASSET_BYTES, MAX_TAPP_GAME_MESSAGE_BYTES,
    MAX_TAPP_GAME_PLAYERS, MAX_TAPP_GAME_PROTOCOL_LEN, MAX_TAPP_GAME_RESOURCE_BYTES,
    MAX_TAPP_I18N_FILES, MAX_TAPP_I18N_RESOURCE_BYTES, MAX_TAPP_ID_LEN, MAX_TAPP_MANIFEST_BYTES,
    MAX_TAPP_RESOURCE_BYTES, MAX_TAPP_RUNTIME_MODULES, MAX_TAPP_UPLOAD_BYTES, MAX_WIDGETS_PER_TAPP,
    MIN_TAPP_GAME_PLAYERS, TAPP_RUNTIME_MODULES,
};
pub use myriad_tapp_contract::paths::{
    is_safe_path_component, is_valid_widget_size, parse_system_version,
    tapp_setting_value_is_valid, valid_data_exchange_id, validate_asset_path,
    validate_inline_data_schema, validate_resource_extension, validate_resource_path,
    validate_tapp_id, validate_tapp_settings, validate_widget_refresh_policy,
};
pub use myriad_tapp_contract::urls::{validate_http_url, validate_open_url_target};
pub use myriad_tapp_contract::validate::{valid_locale_tag, validate_named_resource_keys};

pub fn validate_tapp_manifest(manifest: &TappManifest) -> Result<(), String> {
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("backend package version must be valid semver");
    myriad_tapp_contract::validate::validate_tapp_manifest(manifest, &current)
}

pub fn guess_asset_mime_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".wav") {
        "audio/wav"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".wasm") {
        "application/wasm"
    } else if lower.ends_with(".json") {
        "application/json"
    } else if lower.ends_with(".txt") || lower.ends_with(".csv") {
        "text/plain"
    } else if lower.ends_with(".bin") {
        "application/octet-stream"
    } else if lower.ends_with(".glb") {
        "model/gltf-binary"
    } else if lower.ends_with(".gltf") {
        "model/gltf+json"
    } else {
        "application/octet-stream"
    }
}

pub fn decode_asset_base64(value: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let trimmed = value.trim();
    // Allow optional data-URL prefix: data:<mime>;base64,<payload>
    let payload = trimmed
        .split_once("base64,")
        .map(|(_, data)| data)
        .unwrap_or(trimmed);
    STANDARD
        .decode(payload.trim())
        .map_err(|_| "Invalid asset base64 encoding".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::permission_service::TappPermission;
    use myriad_tapp_contract::manifest::{
        TappHttpBodyMode, TappManifest, TappSettingDef, TappSettingOption,
    };
    use serde_json::json;

    #[test]
    fn tapp_id_accepts_safe_dotted_names_and_rejects_path_tricks() {
        assert!(validate_tapp_id("com.myriad.safe-app_2").is_ok());
        assert!(validate_tapp_id("").is_err());
        assert!(validate_tapp_id(".hidden").is_err());
        assert!(validate_tapp_id("a/b").is_err());
        assert!(validate_tapp_id("../x").is_err());
        assert!(validate_tapp_id(&"a".repeat(MAX_TAPP_ID_LEN + 1)).is_err());
    }

    #[test]
    fn removed_brew_permissions_are_rejected_with_replacement_hints() {
        let manifest = |permissions: Vec<&str>| {
            serde_json::from_value::<TappManifest>(json!({
                "id": "com.example.brew-removed",
                "name": "Brew removed",
                "version": "1.0.0",
                "core": { "entry": "main.js" },
                "category": "utility",
                "permissions": permissions,
            }))
            .unwrap()
        };

        // brew:comment → brew:read + brew:commentWrite
        let error = validate_tapp_manifest(&manifest(vec!["brew:comment"])).unwrap_err();
        assert!(error.contains("brew:comment"), "{error}");
        assert!(error.contains("brew:read"), "{error}");
        assert!(error.contains("brew:commentWrite"), "{error}");

        // 普通未知名不带替代提示
        let error = validate_tapp_manifest(&manifest(vec!["legacy:unknown"])).unwrap_err();
        assert!(
            error.contains("Unknown Tapp permission 'legacy:unknown'"),
            "{error}"
        );
        assert!(!error.contains("instead"), "{error}");

        // 重复名保持 duplicate 语义，不附加替代提示
        let error = validate_tapp_manifest(&manifest(vec!["brew:read", "brew:read"])).unwrap_err();
        assert!(
            error.contains("Duplicate Tapp permission: brew:read"),
            "{error}"
        );
    }

    fn credential_manifest(endpoint: &str) -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.credential",
            "name": "Credential test",
            "version": "1.0.0",
            "core": { "entry": "main.js" },
            "category": "utility",
            "permissions": ["network:fetch"],
            "credentials": [{ "key": "wegame", "label": "WeGame API Key" }],
            "apis": {
                "games": {
                    "type": "http",
                    "access": "public",
                    "endpoint": endpoint,
                    "credential": {
                        "key": "wegame",
                        "header": "Authorization",
                        "prefix": "Bearer "
                    }
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn credential_binding_accepts_fixed_https_origin() {
        assert!(validate_tapp_manifest(&credential_manifest(
            "https://api.example.com/games/{{params.id}}"
        ))
        .is_ok());
    }

    #[test]
    fn credential_binding_rejects_templated_destination_host() {
        let error = validate_tapp_manifest(&credential_manifest("https://{{params.host}}/games"))
            .unwrap_err();
        assert!(error.contains("fixed absolute HTTPS"));
    }

    #[test]
    fn credential_key_cannot_overlap_public_setting_key() {
        let mut manifest = credential_manifest("https://api.example.com/games");
        manifest.settings = Some(
            serde_json::from_value(json!([{
                "key": "wegame",
                "label": "Legacy public key",
                "type": "input"
            }]))
            .unwrap(),
        );

        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("conflicts with public setting key"));
    }

    #[test]
    fn signed_credential_requires_object_body_and_declared_over_fields() {
        let mut manifest = credential_manifest("https://afdian.com/api/open/query-sponsor");
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .credential = Some(
            serde_json::from_value(json!({
                "key": "wegame",
                "in": "sign",
                "field": "sign",
                "sign": {
                    "alg": "md5-sorted-kv",
                    "over": ["params", "ts", "user_id"],
                    "timestampField": "ts"
                }
            }))
            .unwrap(),
        );
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .method = "POST".into();
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .body = Some(json!({
            "user_id": "{{settings.userId}}",
            "params": "{\"page\":1}"
        }));
        assert!(validate_tapp_manifest(&manifest).is_ok());

        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .body_mode = TappHttpBodyMode::Raw;
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .body = Some(json!("{{params.body}}"));
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("bodyMode raw"));
    }

    fn inbound_route_json() -> serde_json::Value {
        json!({
            "path": "/sponsors",
            "methods": ["GET"],
            "verify": {
                "key": "inbound",
                "alg": "hmac-sha256-raw",
                "header": "X-Signature",
                "over": "canonical-query",
                "timestampHeader": "X-Timestamp",
                "nonceHeader": "X-Nonce"
            }
        })
    }

    #[test]
    fn inbound_route_accepts_public_hmac_and_inbound_only_credential() {
        let mut manifest = credential_manifest("https://api.example.com/games");
        manifest.credentials.as_mut().unwrap().push(
            serde_json::from_value(json!({ "key": "inbound", "label": "Inbound HMAC" })).unwrap(),
        );
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .route = Some(serde_json::from_value(inbound_route_json()).unwrap());
        assert!(validate_tapp_manifest(&manifest).is_ok());

        let inbound_only = serde_json::from_value(json!({
            "id": "com.example.inbound",
            "name": "Inbound only",
            "version": "1.0.0",
            "core": { "entry": "main.js" },
            "category": "utility",
            "permissions": ["network:fetch"],
            "credentials": [{ "key": "inbound", "label": "Inbound HMAC" }],
            "apis": {
                "games": {
                    "type": "http",
                    "access": "public",
                    "endpoint": "https://api.example.com/games",
                    "route": inbound_route_json()
                }
            }
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&inbound_only).is_ok());
    }

    #[test]
    fn inbound_route_rejects_protected_missing_verify_and_ai() {
        let mut manifest = credential_manifest("https://api.example.com/games");
        manifest.credentials.as_mut().unwrap().push(
            serde_json::from_value(json!({ "key": "inbound", "label": "Inbound HMAC" })).unwrap(),
        );
        let mut route: myriad_tapp_contract::manifest::TappApiRoute =
            serde_json::from_value(inbound_route_json()).unwrap();
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .access = myriad_tapp_contract::manifest::TappApiAccess::Protected;
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .route = Some(route.clone());
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("access public"));

        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .access = myriad_tapp_contract::manifest::TappApiAccess::Public;
        route.path = "/nope/nested".into();
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .route = Some(route);
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("Invalid inbound path"));
    }

    #[test]
    fn inbound_route_rejects_proxy_verify_headers() {
        let mut manifest = credential_manifest("https://api.example.com/games");
        manifest.credentials.as_mut().unwrap().push(
            serde_json::from_value(json!({ "key": "inbound", "label": "Inbound HMAC" })).unwrap(),
        );
        let mut route: myriad_tapp_contract::manifest::TappApiRoute =
            serde_json::from_value(inbound_route_json()).unwrap();
        route.verify.header = "X-Forwarded-For".into();
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .route = Some(route);
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("Invalid inbound verify header"));
    }

    #[test]
    fn signed_credential_rejects_default_get_and_object_over_fields() {
        let mut manifest = credential_manifest("https://afdian.com/api/open/ping");
        let api = manifest.apis.as_mut().unwrap().get_mut("games").unwrap();
        api.credential = Some(
            serde_json::from_value(json!({
                "key": "wegame",
                "in": "sign",
                "field": "sign",
                "sign": {
                    "alg": "md5-sorted-kv",
                    "over": ["params", "ts", "user_id"],
                    "timestampField": "ts"
                }
            }))
            .unwrap(),
        );
        api.body = Some(json!({
            "user_id": "{{settings.userId}}",
            "params": "{\"page\":1}"
        }));
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("signed credentials require one of:"));

        let api = manifest.apis.as_mut().unwrap().get_mut("games").unwrap();
        api.method = "POST".into();
        api.body = Some(json!({
            "user_id": "{{settings.userId}}",
            "params": { "page": 1 }
        }));
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("must be a scalar"));
    }

    #[test]
    fn form_credential_rejects_default_get_and_missing_object_body() {
        let mut manifest = credential_manifest("https://api.example.com/submit");
        let api = manifest.apis.as_mut().unwrap().get_mut("games").unwrap();
        api.body_mode = TappHttpBodyMode::Form;
        api.credential = Some(
            serde_json::from_value(json!({
                "key": "wegame",
                "in": "form",
                "field": "token"
            }))
            .unwrap(),
        );
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("form credentials require one of:"));

        let api = manifest.apis.as_mut().unwrap().get_mut("games").unwrap();
        api.method = "POST".into();
        api.body = None;
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("form credentials require a form object body"));
    }

    #[test]
    fn query_credential_rejects_duplicate_query_name() {
        let mut manifest = credential_manifest("https://api.example.com/weather?appid=placeholder");
        manifest
            .apis
            .as_mut()
            .unwrap()
            .get_mut("games")
            .unwrap()
            .credential = Some(
            serde_json::from_value(json!({
                "key": "wegame",
                "in": "query",
                "field": "appid"
            }))
            .unwrap(),
        );
        let error = validate_tapp_manifest(&manifest).unwrap_err();
        assert!(error.contains("query field twice"));
    }

    #[test]
    fn generated_contract_forbidden_headers_match_outbound_guard() {
        for value in FORBIDDEN_OUTBOUND_HEADERS {
            let header = myriad_tapp_contract::headers::parse_http_header_name(value)
                .expect("contract header name must be valid");
            assert!(
                crate::services::outbound_security::validate_outbound_header(&header).is_err(),
                "generated CLI contract allows backend-forbidden header: {value}"
            );
        }
    }

    #[test]
    fn resource_path_rejects_absolute_and_escape_components() {
        assert!(validate_resource_path("page/state.js").is_ok());
        assert!(validate_resource_path("/abs.js").is_err());
        assert!(validate_resource_path("../escape.js").is_err());
        assert!(validate_resource_path("").is_err());
        assert!(validate_resource_path("a\\b.js").is_err());
    }

    #[test]
    fn asset_path_must_live_under_assets_and_not_be_entrypoint() {
        assert!(validate_asset_path("assets/icon.png").is_ok());
        assert!(validate_asset_path("assets/").is_err());
        assert!(validate_asset_path("icon.png").is_err());
        assert!(validate_asset_path("assets/main.js").is_err());
        assert!(validate_asset_path("assets/page.html").is_err());
    }

    #[test]
    fn widget_sizes_match_contract_allow_list() {
        assert!(is_valid_widget_size("2x2"));
        assert!(is_valid_widget_size("4x4"));
        assert!(!is_valid_widget_size("5x5"));
        assert!(!is_valid_widget_size("2x2x2"));
    }

    #[test]
    fn guess_asset_mime_type_covers_common_extensions() {
        assert_eq!(guess_asset_mime_type("a.PNG"), "image/png");
        assert_eq!(guess_asset_mime_type("a.jpg"), "image/jpeg");
        assert_eq!(guess_asset_mime_type("a.webp"), "image/webp");
        assert_eq!(guess_asset_mime_type("a.wav"), "audio/wav");
        assert_eq!(guess_asset_mime_type("a.wasm"), "application/wasm");
        assert_eq!(
            guess_asset_mime_type("a.unknown"),
            "application/octet-stream"
        );
    }

    #[test]
    fn decode_asset_base64_accepts_raw_and_data_url() {
        let raw = decode_asset_base64("aGVsbG8=").expect("raw");
        assert_eq!(raw, b"hello");
        let data_url = decode_asset_base64("data:image/png;base64,aGVsbG8=").expect("data url");
        assert_eq!(data_url, b"hello");
        assert!(decode_asset_base64("!!!").is_err());
    }

    #[test]
    fn data_exchange_id_and_inline_schema_rules() {
        assert!(valid_data_exchange_id("share.profile"));
        assert!(!valid_data_exchange_id(""));
        assert!(!valid_data_exchange_id("has space"));

        assert!(validate_inline_data_schema(&json!({"type": "object"})).is_ok());
        assert!(validate_inline_data_schema(&json!({"$ref": "#/x"})).is_err());
        assert!(validate_inline_data_schema(&json!({})).is_err());
        assert!(validate_inline_data_schema(&json!("string")).is_err());
    }

    #[test]
    fn locale_tag_and_http_url_helpers() {
        assert!(valid_locale_tag("zh-CN"));
        assert!(valid_locale_tag("en"));
        assert!(!valid_locale_tag(""));
        assert!(!valid_locale_tag("toolonglangtag-xxxxxxxx"));
        assert!(!valid_locale_tag("1n"));

        assert!(validate_http_url("https://example.com/x", "homepage").is_ok());
        assert!(validate_http_url("ftp://example.com", "homepage").is_err());
        assert!(
            validate_http_url(&format!("https://x/{}", "a".repeat(3_000)), "homepage").is_err()
        );

        assert!(validate_open_url_target("https://docs.example.com/a", "openUrls[0].url").is_ok());
        assert!(validate_open_url_target("http://localhost:3000/x", "openUrls[0].url").is_ok());
        assert!(validate_open_url_target("http://example.com/x", "openUrls[0].url").is_err());
        assert!(
            validate_open_url_target("https://user:pass@example.com/", "openUrls[0].url").is_err()
        );
    }

    #[test]
    fn open_urls_require_permission_and_allowlist_pair() {
        use myriad_tapp_contract::manifest::{TappCategory, TappOpenUrlDef, TappOpenUrlMatch};

        let mut manifest = TappManifest {
            id: "com.example.open".into(),
            name: "Open".into(),
            version: "1.0.0".into(),
            description: None,
            locales: None,
            author: None,
            core: Some(myriad_tapp_contract::manifest::TappCoreLayer {
                entry: "core.js".into(),
                styles: None,
            }),
            page: None,
            permissions: vec!["ui:openUrl".into()],
            icon: None,
            icon_svg: None,
            theme_color: None,
            homepage: None,
            repository: None,
            min_system_version: None,
            widgets: None,
            background_requirements: None,
            settings: None,
            credentials: None,
            category: Some(TappCategory::Utility),
            runtime_modules: None,
            game: None,
            apis: None,
            data_exchange: None,
            ai: None,
            events: None,
            agent: None,
            assets: None,
            open_urls: None,
        };

        // Permission without openUrls → fail
        assert!(validate_tapp_manifest(&manifest).is_err());

        manifest.open_urls = Some(vec![TappOpenUrlDef {
            id: "docs".into(),
            url: "https://docs.example.com/guide/".into(),
            match_mode: TappOpenUrlMatch::Prefix,
        }]);
        assert!(validate_tapp_manifest(&manifest).is_ok());

        // openUrls without permission → fail
        manifest.permissions.clear();
        assert!(validate_tapp_manifest(&manifest).is_err());

        // Public http host → fail
        manifest.permissions = vec!["ui:openUrl".into()];
        manifest.open_urls = Some(vec![TappOpenUrlDef {
            id: "insecure".into(),
            url: "http://example.com/".into(),
            match_mode: TappOpenUrlMatch::Exact,
        }]);
        assert!(validate_tapp_manifest(&manifest).is_err());
    }

    #[test]
    fn setting_value_respects_type_and_number_bounds() {
        let number = TappSettingDef {
            key: "volume".into(),
            label: "Volume".into(),
            setting_type: "number".into(),
            description: None,
            default_value: None,
            options: None,
            min: Some(0.0),
            max: Some(100.0),
            step: Some(1.0),
            placeholder: None,
        };
        assert!(tapp_setting_value_is_valid(&number, &json!(75)));
        assert!(!tapp_setting_value_is_valid(&number, &json!(101)));
        assert!(!tapp_setting_value_is_valid(&number, &json!("75")));

        let select = TappSettingDef {
            key: "theme".into(),
            label: "Theme".into(),
            setting_type: "select".into(),
            description: None,
            default_value: None,
            options: Some(vec![TappSettingOption {
                value: "dark".into(),
                label: "Dark".into(),
            }]),
            min: None,
            max: None,
            step: None,
            placeholder: None,
        };
        assert!(tapp_setting_value_is_valid(&select, &json!("dark")));
        assert!(!tapp_setting_value_is_valid(&select, &json!("system")));
    }

    #[test]
    fn parse_system_version_strips_optional_v_prefix() {
        assert_eq!(
            parse_system_version("v1.2.3").unwrap(),
            semver::Version::new(1, 2, 3)
        );
        assert_eq!(
            parse_system_version("0.3.18").unwrap(),
            semver::Version::new(0, 3, 18)
        );
        assert!(parse_system_version("not-a-version").is_err());
    }

    #[test]
    fn named_resource_keys_require_safe_components() {
        assert!(
            validate_named_resource_keys([&"page".to_string(), &"schemas".to_string()], "dir")
                .is_ok()
        );
        assert!(validate_named_resource_keys([&"../x".to_string()], "dir").is_err());
    }

    fn permission_manifest(permissions: &[&str]) -> TappManifest {
        serde_json::from_value(json!({
            "id": "com.example.permissions",
            "name": "Permissions test",
            "version": "1.0.0",
            "core": { "entry": "main.js" },
            "category": "utility",
            "permissions": permissions,
        }))
        .unwrap()
    }

    #[test]
    fn unknown_storage_permission_error_recommends_split_permissions_fail_closed() {
        // 拆分后的 storage:read / storage:write 是独立权限，`storage` 不再可解析。
        assert!(TappPermission::from_str("storage").is_none());
        assert!(TappPermission::from_str("storage:read").is_some());
        assert!(TappPermission::from_str("storage:write").is_some());

        // 安装校验对 `storage` 显式失败，并复用共享 replacement hint。
        let error = validate_tapp_manifest(&permission_manifest(&["storage"])).unwrap_err();
        assert!(error.contains("'storage'"), "{error}");
        assert!(error.contains("storage:read"), "{error}");
        assert!(error.contains("storage:write"), "{error}");
        assert!(error.contains("Manifest"), "{error}");
        assert!(error.contains("reinstall"), "{error}");
        assert!(!error.contains("Duplicate"), "{error}");
    }

    #[test]
    fn retired_federation_write_manifest_lists_all_replacements() {
        let error =
            validate_tapp_manifest(&permission_manifest(&["federation:write"])).unwrap_err();
        assert!(error.contains("'federation:write'"), "{error}");
        for replacement in [
            "federation:post",
            "federation:interact",
            "federation:channel",
            "federation:room",
            "federation:ring",
        ] {
            assert!(error.contains(replacement), "{error}");
        }
        assert!(error.contains("Manifest"), "{error}");
        assert!(error.contains("reinstall"), "{error}");
    }

    #[test]
    fn generic_unknown_permission_keeps_unknown_error() {
        let error = validate_tapp_manifest(&permission_manifest(&["legacy:unknown"])).unwrap_err();
        assert_eq!(error, "Unknown Tapp permission 'legacy:unknown'");
    }

    #[test]
    fn duplicate_permission_reports_duplicate_not_unknown() {
        let error = validate_tapp_manifest(&permission_manifest(&["storage:read", "storage:read"]))
            .unwrap_err();
        assert_eq!(error, "Duplicate Tapp permission: storage:read");
        assert!(!error.contains("Unknown"), "{error}");
    }

    #[test]
    fn game_declaration_requires_permission_and_safe_protocol() {
        let mut manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.chess",
            "name": "Chess",
            "version": "1.0.0",
            "core": { "entry": "main.js" },
            "category": "game",
            "permissions": [
                "game:session",
                "federation:read",
                "federation:room",
                "federation:message"
            ],
            "game": { "protocol": "v1", "maxPlayers": 2 }
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&manifest).is_ok());
        manifest.permissions.clear();
        assert!(validate_tapp_manifest(&manifest)
            .unwrap_err()
            .contains("game:session"));
        manifest.permissions = vec![
            "game:session".into(),
            "federation:read".into(),
            "federation:room".into(),
        ];
        assert!(validate_tapp_manifest(&manifest)
            .unwrap_err()
            .contains("federation:message"));
        manifest.permissions = vec![
            "game:session".into(),
            "federation:read".into(),
            "federation:room".into(),
            "federation:message".into(),
        ];
        manifest.game.as_mut().unwrap().protocol = "V1".into();
        assert!(validate_tapp_manifest(&manifest)
            .unwrap_err()
            .contains("lowercase"));
    }

    #[test]
    fn runtime_modules_only_on_game_or_developer() {
        let mut manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.lab",
            "name": "Lab",
            "version": "1.0.0",
            "core": { "entry": "main.js" },
            "category": "developer",
            "permissions": [],
            "runtimeModules": ["three"]
        }))
        .unwrap();
        assert!(validate_tapp_manifest(&manifest).is_ok());
        assert!(manifest.uses_game_asset_limits());
        manifest.category = Some(myriad_tapp_contract::manifest::TappCategory::Utility);
        assert!(validate_tapp_manifest(&manifest)
            .unwrap_err()
            .contains("runtimeModules"));
    }
}
