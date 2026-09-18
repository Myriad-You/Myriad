//! Pure Tapp evaluators that depend on tapp-contract but not on backend I/O.
//!
//! HMAC and transform evaluation live here, not in the contract crate.
//! Backend services re-export moved symbols so existing imports compile.

pub mod feed;
pub mod hmac;
pub mod outbound;
pub mod package;
pub mod package_fs;
pub mod prepared;
pub mod transform;

pub use feed::{
    FEDERATION_FEED_LIMIT, FederationFeedRowView, dedupe_federation_feed,
    federation_feed_includes_personal, federation_feed_item, merge_federation_feed,
    merge_federation_feed_with_limit,
};
pub use hmac::{encode_hmac, hmac_matches, hmac_sha256};
pub use outbound::{
    OutboundHttpErrorKind, classify_outbound_http_error, outbound_endpoint_identity,
    strip_urls_from_error,
};
pub use package::{
    InstalledTextResourcePlan, InstalledWidgetTemplatePath, UnknownWidgetId,
    asset_bytes_within_limit, filter_widget_paths, installed_core_entry, installed_layer_entries,
    installed_manifest_declares_asset, installed_page_entry, installed_text_resource_plan,
    installed_widget_ids, installed_widget_layer_paths, installed_widget_template_paths,
    manifest_declares_asset, manifest_declares_core, manifest_declares_page,
    manifest_declares_widgets, require_known_widget_id,
};
pub use package_fs::{
    LIFECYCLE_ARTIFACT_KINDS, MANIFEST_JSON, RecoveryPlan, TAPP_INSTALL_STATE_FILE,
    TappDirEntryClass, archive_entry_relative_path, classify_tapp_directory_entry,
    filesystem_error_message, filesystem_error_status_hint, has_reinstall_orphan_state,
    install_generation_matches_micros, install_generation_payload, is_lifecycle_artifact_filename,
    is_staging_artifact_filename, is_storage_unwritable_error, lifecycle_artifact_dir_name,
    lifecycle_artifact_prefixes, lifecycle_artifact_tapp_id,
    looks_like_tapp_installation_from_markers, orphan_tapp_key_if_unowned,
    parse_tapp_owner_dir_name, plan_tapp_directory_recovery, recovery_artifact_sort_key,
    recovery_artifacts_to_remove_after_promote, recovery_discard_artifact_name,
    recovery_plan_mutates_live, resource_relative_path, sandbox_path_matches_relative,
    should_log_filesystem_permission_context, should_preserve_orphan_path,
    sort_recovery_artifact_paths, tapp_id_from_dir_entry_class, tapp_installation_marker_names,
};
pub use prepared::{
    PackageLoadError, PackageValidateError, PreparedTappPackage, PreparedTappResources,
    WidgetTemplateContents, check_manifest_byte_size, nonempty_content, parse_manifest_json,
    validate_widget_template_contents, widget_template_path,
};
pub use transform::{
    DataTransformError, MAX_MAP_OPERATIONS, MAX_PIPELINE_STEPS, MapOp, ProcessStep, apply_map_op,
    apply_pipeline, apply_process_step, items_from_agent_input, items_from_value,
    parse_pipeline_steps, parse_pipeline_steps_lenient,
};
