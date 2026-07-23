#![allow(dead_code)]
#![recursion_limit = "512"]

use serde_json::{json, Value};
use std::collections::BTreeMap;

#[path = "../../../backend/src/api/tapp_store/contract_rules.rs"]
mod contract_rules;
#[path = "../../../backend/src/api/tapp_store/manifest.rs"]
mod manifest;

fn string_map<'a>(values: &'a [(&'a str, &'a str)]) -> BTreeMap<&'a str, &'a str> {
    values.iter().copied().collect()
}

fn list_map<'a>(values: &'a [(&'a str, &'a [&'a str])]) -> BTreeMap<&'a str, &'a [&'a str]> {
    values.iter().copied().collect()
}

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

fn main() {
    let schema = schemars::schema_for!(manifest::TappManifest);
    let output: Value = json!({
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
            "pageModules": contract_rules::MAX_PAGE_MODULES,
            "backgroundRequirements": contract_rules::MAX_BACKGROUND_REQUIREMENTS,
            "tappSettings": contract_rules::MAX_TAPP_SETTINGS,
            "settingLabelLength": contract_rules::MAX_SETTING_LABEL_LEN,
            "settingOptions": contract_rules::MAX_SETTING_OPTIONS,
            "settingOptionValueLength": contract_rules::MAX_SETTING_OPTION_VALUE_LEN,
            "widgetSizes": contract_rules::MAX_WIDGET_SIZES,
            "tappApis": contract_rules::MAX_TAPP_APIS,
            "apiMethodLength": contract_rules::MAX_API_METHOD_LEN,
            "apiCacheTtlSeconds": contract_rules::MAX_API_CACHE_TTL_SECONDS,
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
            "widgetRefreshIntervalMinSeconds": contract_rules::MIN_WIDGET_REFRESH_INTERVAL_SECONDS,
            "widgetRefreshIntervalMaxSeconds": contract_rules::MAX_WIDGET_REFRESH_INTERVAL_SECONDS
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
            "cssModes": contract_rules::CSS_MODES,
            "httpUrlSchemes": contract_rules::HTTP_URL_SCHEMES,
            "resourceExtensions": string_map(contract_rules::RESOURCE_EXTENSIONS),
            "assetForbiddenExtensions": contract_rules::ASSET_FORBIDDEN_EXTENSIONS,
            "packageResourceDirectories": contract_rules::PACKAGE_RESOURCE_DIRECTORIES,
            "packageResourceExtensions": string_map(contract_rules::PACKAGE_RESOURCE_EXTENSIONS),
            "packageJsonObjectDirectories": contract_rules::PACKAGE_JSON_OBJECT_DIRECTORIES,
            "packageResourceFileLimits": string_map(contract_rules::PACKAGE_RESOURCE_FILE_LIMITS),
            "packageResourceByteLimits": string_map(contract_rules::PACKAGE_RESOURCE_BYTE_LIMITS),
            "assetDirectory": contract_rules::ASSET_DIRECTORY,
            "pageModuleDirectory": contract_rules::PAGE_MODULE_DIRECTORY,
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
            "themeColor": contract_rules::THEME_COLOR_PATTERN,
            "httpMethod": contract_rules::HTTP_METHOD_PATTERN
        }
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("contract must serialize")
    );
}
