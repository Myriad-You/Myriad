#![recursion_limit = "512"]

#[cfg(feature = "tapp-contract-schema")]
use serde_json::{json, Value};
#[cfg(feature = "tapp-contract-schema")]
use std::collections::BTreeMap;

pub mod contract_rules;
pub mod headers;
pub mod manifest;
pub mod paths;
pub mod permission;
pub mod storage;
pub mod urls;
pub mod validate;

#[cfg(feature = "tapp-contract-schema")]
fn string_map<'a>(values: &'a [(&'a str, &'a str)]) -> BTreeMap<&'a str, &'a str> {
    values.iter().copied().collect()
}

#[cfg(feature = "tapp-contract-schema")]
fn list_map<'a>(values: &'a [(&'a str, &'a [&'a str])]) -> BTreeMap<&'a str, &'a [&'a str]> {
    values.iter().copied().collect()
}

#[cfg(feature = "tapp-contract-schema")]
fn ai_operation_permissions() -> BTreeMap<String, &'static str> {
    use manifest::TappAiOperation::{Analyze, Chat, Generate, Image};
    [Generate, Analyze, Chat, Image]
        .into_iter()
        .map(|operation| {
            let name = serde_json::to_value(operation)
                .expect("AI operation must serialize")
                .as_str()
                .expect("AI operation must serialize as a string")
                .to_string();
            (name, operation.permission())
        })
        .collect()
}

#[cfg(feature = "tapp-contract-schema")]
pub fn export_tapp_contract() -> Value {
    let schema = schemars::schema_for!(manifest::TappManifest);
    json!({
        "schema": schema,
        "limits": {
            "tappIdLength": contract_rules::MAX_TAPP_ID_LEN,
            "resourcePathLength": contract_rules::MAX_RESOURCE_PATH_LEN,
            "archiveBytes": contract_rules::MAX_TAPP_ARCHIVE_BYTES,
            "archiveFiles": contract_rules::MAX_TAPP_ARCHIVE_FILES,
            "archiveUncompressedBytes": contract_rules::MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES,
            "resourceBytes": contract_rules::MAX_TAPP_RESOURCE_BYTES,
            "assets": contract_rules::MAX_TAPP_ASSETS,
            "assetBytes": contract_rules::MAX_TAPP_ASSET_BYTES,
            "assetsTotalBytes": contract_rules::MAX_TAPP_ASSETS_TOTAL_BYTES,
            "gameArchiveBytes": contract_rules::MAX_TAPP_GAME_ARCHIVE_BYTES,
            "gameArchiveFiles": contract_rules::MAX_TAPP_GAME_ARCHIVE_FILES,
            "gameArchiveUncompressedBytes": contract_rules::MAX_TAPP_GAME_ARCHIVE_UNCOMPRESSED_BYTES,
            "gameResourceBytes": contract_rules::MAX_TAPP_GAME_RESOURCE_BYTES,
            "gameAssets": contract_rules::MAX_TAPP_GAME_ASSETS,
            "gameAssetBytes": contract_rules::MAX_TAPP_GAME_ASSET_BYTES,
            "gameAssetsTotalBytes": contract_rules::MAX_TAPP_GAME_ASSETS_TOTAL_BYTES,
            "uploadBytes": contract_rules::MAX_TAPP_UPLOAD_BYTES,
            "manifestBytes": contract_rules::MAX_TAPP_MANIFEST_BYTES,
            "widgets": contract_rules::MAX_WIDGETS_PER_TAPP,
            "dataExchangeDeclarations": contract_rules::MAX_DATA_EXCHANGE_DECLARATIONS,
            "dataExchangeIdLength": contract_rules::MAX_DATA_EXCHANGE_ID_LEN,
            "dataExchangeSchemaBytes": contract_rules::MAX_DATA_EXCHANGE_SCHEMA_BYTES,
            "dataExchangeResponseBytes": contract_rules::MAX_DATA_EXCHANGE_RESPONSE_BYTES,
            "agentSchemaBytes": contract_rules::MAX_AGENT_SCHEMA_RESOURCE_BYTES,
            "i18nFiles": contract_rules::MAX_TAPP_I18N_FILES,
            "i18nResourceBytes": contract_rules::MAX_TAPP_I18N_RESOURCE_BYTES,
            "tappNameLength": contract_rules::MAX_TAPP_NAME_LEN,
            "tappDescriptionLength": contract_rules::MAX_TAPP_DESCRIPTION_LEN,
            "tappLocales": contract_rules::MAX_TAPP_LOCALES,
            "localeTagLength": contract_rules::MAX_LOCALE_TAG_LEN,
            "tappIconLength": contract_rules::MAX_TAPP_ICON_LEN,
            "tappIconSvgBytes": contract_rules::MAX_TAPP_ICON_SVG_BYTES,
            "httpUrlLength": contract_rules::MAX_HTTP_URL_LEN,
            "authorEmailLength": contract_rules::MAX_AUTHOR_EMAIL_LEN,
            "storageKeyLength": contract_rules::MAX_STORAGE_KEY_LEN,
            "tappPermissions": contract_rules::MAX_TAPP_PERMISSIONS,
            "backgroundRequirements": contract_rules::MAX_BACKGROUND_REQUIREMENTS,
            "tappSettings": contract_rules::MAX_TAPP_SETTINGS,
            "tappCredentials": contract_rules::MAX_TAPP_CREDENTIALS,
            "credentialKeyLength": contract_rules::MAX_CREDENTIAL_KEY_LEN,
            "credentialValueLength": contract_rules::MAX_CREDENTIAL_VALUE_LEN,
            "credentialHeaderPrefixLength": contract_rules::MAX_CREDENTIAL_HEADER_PREFIX_LEN,
            "credentialFieldLength": contract_rules::MAX_CREDENTIAL_FIELD_LEN,
            "credentialSignOver": contract_rules::MAX_CREDENTIAL_SIGN_OVER,
            "settingLabelLength": contract_rules::MAX_SETTING_LABEL_LEN,
            "settingOptions": contract_rules::MAX_SETTING_OPTIONS,
            "settingOptionValueLength": contract_rules::MAX_SETTING_OPTION_VALUE_LEN,
            "widgetSizes": contract_rules::MAX_WIDGET_SIZES,
            "tappApis": contract_rules::MAX_TAPP_APIS,
            "apiCacheTtlSeconds": contract_rules::MAX_API_CACHE_TTL_SECONDS,
            "tappNonJsonHttpRequestBytes": contract_rules::MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES,
            "apiInjectAliases": contract_rules::MAX_API_INJECT_ALIASES,
            "apiInjectTemplateLength": contract_rules::MAX_API_INJECT_TEMPLATE_LEN,
            "dataExchangeDescriptionLength": contract_rules::MAX_DATA_EXCHANGE_DESCRIPTION_LEN,
            "dataExchangeRecords": contract_rules::MAX_DATA_EXCHANGE_RECORDS,
            "inlineSchemaDepth": contract_rules::MAX_INLINE_SCHEMA_DEPTH,
            "aiOperations": contract_rules::MAX_AI_OPERATIONS,
            "aiContextSources": contract_rules::MAX_AI_CONTEXT_SOURCES,
            "aiOutputFormats": contract_rules::MAX_AI_OUTPUT_FORMATS,
            "eventTopics": contract_rules::MAX_EVENT_TOPICS,
            "agentInteractions": contract_rules::MAX_AGENT_INTERACTIONS,
            "agentIntents": contract_rules::MAX_AGENT_INTENTS,
            "openUrls": contract_rules::MAX_OPEN_URLS,
            "openUrlIdLength": contract_rules::MAX_OPEN_URL_ID_LEN,
            "openUrlQueryKeys": contract_rules::MAX_OPEN_URL_QUERY_KEYS,
            "openUrlQueryValueLength": contract_rules::MAX_OPEN_URL_QUERY_VALUE_LEN,
            "widgetRefreshIntervalMinSeconds": contract_rules::MIN_WIDGET_REFRESH_INTERVAL_SECONDS,
            "widgetRefreshIntervalMaxSeconds": contract_rules::MAX_WIDGET_REFRESH_INTERVAL_SECONDS,
            "routeVerifyPrefixLength": contract_rules::ROUTE_MAX_PREFIX_LEN,
            "routeMinMaxSkewSecs": contract_rules::ROUTE_MIN_MAX_SKEW_SECS,
            "routeMaxMaxSkewSecs": contract_rules::ROUTE_MAX_MAX_SKEW_SECS,
            "routeMaxBodyBytes": contract_rules::ROUTE_MAX_BODY_BYTES
        },
        "rules": {
            "widgetSizes": contract_rules::WIDGET_SIZES,
            "backgroundRequirements": contract_rules::BACKGROUND_REQUIREMENTS,
            "settingTypes": contract_rules::SETTING_TYPES,
            "agentIntents": contract_rules::AGENT_INTENTS,
            "apiBuiltins": contract_rules::API_BUILTINS,
            "apiTypes": contract_rules::API_TYPES,
            "httpApiType": contract_rules::HTTP_API_TYPE,
            "builtinApiType": contract_rules::BUILTIN_API_TYPE,
            "defaultApiType": contract_rules::DEFAULT_API_TYPE,
            "defaultHttpMethod": contract_rules::DEFAULT_HTTP_METHOD,
            "defaultHttpBodyMode": contract_rules::DEFAULT_HTTP_BODY_MODE,
            "httpMethods": contract_rules::HTTP_METHODS,
            "httpBodyMethods": contract_rules::HTTP_BODY_METHODS,
            "forbiddenOutboundHeaders": contract_rules::FORBIDDEN_OUTBOUND_HEADERS,
            "httpUrlSchemes": contract_rules::HTTP_URL_SCHEMES,
            "resourceExtensions": string_map(contract_rules::RESOURCE_EXTENSIONS),
            "assetForbiddenExtensions": contract_rules::ASSET_FORBIDDEN_EXTENSIONS,
            "packageResourceDirectories": contract_rules::PACKAGE_RESOURCE_DIRECTORIES,
            "packageResourceExtensions": string_map(contract_rules::PACKAGE_RESOURCE_EXTENSIONS),
            "packageJsonObjectDirectories": contract_rules::PACKAGE_JSON_OBJECT_DIRECTORIES,
            "packageResourceFileLimits": string_map(contract_rules::PACKAGE_RESOURCE_FILE_LIMITS),
            "packageResourceByteLimits": string_map(contract_rules::PACKAGE_RESOURCE_BYTE_LIMITS),
            "assetDirectory": contract_rules::ASSET_DIRECTORY,
            "pageLayerDirectory": contract_rules::PAGE_LAYER_DIRECTORY,
            "widgetLayerDirectory": contract_rules::WIDGET_LAYER_DIRECTORY,
            "hostWidgetCss": contract_rules::HOST_WIDGET_CSS,
            "hostPageCss": contract_rules::HOST_PAGE_CSS,
            "manifestResourceFields": string_map(contract_rules::MANIFEST_RESOURCE_FIELDS),
            "agentSchemaFields": contract_rules::AGENT_SCHEMA_FIELDS,
            "urlFields": contract_rules::URL_FIELDS,
            "dataExchangeDirections": string_map(contract_rules::DATA_EXCHANGE_DIRECTIONS),
            "eventTopicPrefixes": list_map(contract_rules::EVENT_TOPIC_PREFIXES),
            "tappCategoryAliases": contract_rules::TAPP_CATEGORY_ALIASES,
            "widgetCategoryAliases": contract_rules::WIDGET_CATEGORY_ALIASES,
            "aiOperationPermissions": ai_operation_permissions(),
            "widgetManifestPermission": contract_rules::WIDGET_MANIFEST_PERMISSION,
            "httpApiPermission": contract_rules::HTTP_API_PERMISSION,
            "openUrlPermission": contract_rules::OPEN_URL_PERMISSION,
            "openUrlMatchModes": contract_rules::OPEN_URL_MATCH_MODES,
            "eventPermissionRules": string_map(contract_rules::EVENT_PERMISSION_RULES),
            "aiContextPermissionRules": string_map(contract_rules::AI_CONTEXT_PERMISSION_RULES),
            "aiBuiltinOutputFormat": contract_rules::AI_BUILTIN_OUTPUT_FORMAT,
            "requiredManifestFields": contract_rules::REQUIRED_MANIFEST_FIELDS,
            "inlineSchemaRootKeys": contract_rules::INLINE_SCHEMA_ROOT_KEYS,
            "aiOperationOutputRules": string_map(contract_rules::AI_OPERATION_OUTPUT_RULES),
            "apiBuiltinAiOperations": string_map(contract_rules::API_BUILTIN_AI_OPERATIONS),
            "apiBuiltinPermissions": string_map(contract_rules::API_BUILTIN_PERMISSIONS),
            "httpOnlyApiFields": contract_rules::HTTP_ONLY_API_FIELDS,
            "apiInjectReservedPrefixes": contract_rules::API_INJECT_RESERVED_PREFIXES,
            "credentialInValues": contract_rules::CREDENTIAL_IN_VALUES,
            "credentialEncodings": contract_rules::CREDENTIAL_ENCODINGS,
            "credentialSignAlgs": contract_rules::CREDENTIAL_SIGN_ALGS,
            "credentialSignAlgsImplemented": contract_rules::CREDENTIAL_SIGN_ALGS_IMPLEMENTED,
            "routeMethods": contract_rules::ROUTE_METHODS,
            "routeVerifyAlgs": contract_rules::ROUTE_VERIFY_ALGS,
            "routeVerifyOver": contract_rules::ROUTE_VERIFY_OVER,
            "routeVerifyEncodings": contract_rules::ROUTE_VERIFY_ENCODINGS,
            "routeReservedHeaders": contract_rules::ROUTE_RESERVED_HEADERS,
            "eventSubscribePrefixes": contract_rules::EVENT_SUBSCRIBE_PREFIXES,
            "assetLiteralMethods": contract_rules::ASSET_LITERAL_METHODS,
            "sourceCodeExtensions": contract_rules::SOURCE_CODE_EXTENSIONS,
            "sourceScanSkipDirectories": contract_rules::SOURCE_SCAN_SKIP_DIRECTORIES,
            "protocolVersion": contract_rules::TAPP_PROTOCOL_VERSION,
            "settingFieldTypes": string_map(contract_rules::SETTING_FIELD_TYPES),
            "settingDefaultKinds": string_map(contract_rules::SETTING_DEFAULT_KINDS),
            "widgetRefreshModes": string_map(contract_rules::WIDGET_REFRESH_MODES),
            "semverPrefixes": contract_rules::SEMVER_PREFIXES
        },
        "patterns": {
            "safeComponent": contract_rules::SAFE_COMPONENT_PATTERN,
            "localeTag": contract_rules::LOCALE_TAG_PATTERN,
            "semver": contract_rules::SEMVER_PATTERN,
            "namedValue": contract_rules::NAMED_VALUE_PATTERN,
            "storageKey": contract_rules::STORAGE_KEY_PATTERN,
            "themeColor": contract_rules::THEME_COLOR_PATTERN
        },
        "permissionLevels": permission::permission_levels(),
        "replacementHints": permission::replacement_hints(),
        "requiresAuthenticatedSubject": permission::requires_authenticated_subject_names()
    })
}

#[cfg(all(test, feature = "tapp-contract-schema"))]
mod export_catalog_tests {
    use super::export_tapp_contract;
    use crate::permission::{
        permission_levels, replacement_hints, requires_authenticated_subject_names,
    };

    #[test]
    fn export_tapp_contract_includes_catalog_facts() {
        let exported = export_tapp_contract();
        let levels = exported["permissionLevels"]
            .as_object()
            .expect("permissionLevels");
        for (name, level) in permission_levels() {
            assert_eq!(
                levels
                    .get(name)
                    .and_then(|value| value.as_str())
                    .expect(name),
                level
            );
        }
        assert_eq!(levels.len(), permission_levels().len());

        let hints = exported["replacementHints"]
            .as_object()
            .expect("replacementHints");
        for (name, hint) in replacement_hints() {
            assert_eq!(
                hints
                    .get(name)
                    .and_then(|value| value.as_str())
                    .expect(name),
                hint
            );
        }

        let authenticated = exported["requiresAuthenticatedSubject"]
            .as_array()
            .expect("requiresAuthenticatedSubject")
            .iter()
            .map(|value| value.as_str().expect("name"))
            .collect::<Vec<_>>();
        assert_eq!(authenticated, requires_authenticated_subject_names());

        assert_eq!(
            exported["rules"]["hostWidgetCss"].as_str(),
            Some(crate::contract_rules::HOST_WIDGET_CSS)
        );
        assert_eq!(
            exported["rules"]["hostPageCss"].as_str(),
            Some(crate::contract_rules::HOST_PAGE_CSS)
        );
    }
}
