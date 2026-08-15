//! Validation and bounded-resource rules for Tapp package manifests.
//!
//! Domain implementation lives in [`crate::services::tapp_validation`]. This
//! module is a path-stable re-export surface for store handlers and
//! `manifest_tests`.

// Re-export barrel: symbols are consumed via `super::` / `pub(crate) use validation::*`.
#![allow(unused_imports)]

pub(crate) use crate::services::tapp_validation::{
    decode_asset_base64, guess_asset_mime_type, is_safe_path_component, is_valid_widget_size,
    parse_system_version, tapp_setting_value_is_valid, valid_data_exchange_id, validate_asset_path,
    validate_http_url, validate_inline_data_schema, validate_named_resource_keys,
    validate_resource_extension, validate_resource_path, validate_tapp_id, validate_tapp_manifest,
    validate_tapp_settings, validate_widget_refresh_policy, HTTP_METHODS,
    MAX_AGENT_SCHEMA_RESOURCE_BYTES, MAX_DATA_EXCHANGE_DECLARATIONS, MAX_DATA_EXCHANGE_ID_LEN,
    MAX_DATA_EXCHANGE_RESPONSE_BYTES, MAX_DATA_EXCHANGE_SCHEMA_BYTES, MAX_RESOURCE_PATH_LEN,
    MAX_TAPP_ARCHIVE_BYTES, MAX_TAPP_ARCHIVE_FILES, MAX_TAPP_ARCHIVE_UNCOMPRESSED_BYTES,
    MAX_TAPP_ASSETS, MAX_TAPP_ASSETS_TOTAL_BYTES, MAX_TAPP_ASSET_BYTES, MAX_TAPP_I18N_FILES,
    MAX_TAPP_I18N_RESOURCE_BYTES, MAX_TAPP_ID_LEN, MAX_TAPP_MANIFEST_BYTES, MAX_TAPP_RESOURCE_BYTES,
    MAX_WIDGETS_PER_TAPP,
};
