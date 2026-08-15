//! Tapp store source administration pure policy and projection.
//!
//! HTTP handlers keep Claims/DB. Domain owns response DTO shape, official-source
//! immutability rules, default enabled flag, and URL validation for admin CRUD.

use serde::Serialize;

use crate::services::tapp_store_package::{
    is_allowed_store_url_scheme, is_disallowed_store_host, normalize_store_catalog_url,
};

/// Public list/detail view for a configured store catalog source.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoreSourceView {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub enabled: bool,
    pub official: bool,
    pub icon: Option<String>,
}

impl StoreSourceView {
    pub fn new(
        id: i32,
        name: String,
        description: Option<String>,
        url: String,
        enabled: bool,
        official: bool,
        icon: Option<String>,
    ) -> Self {
        Self {
            id,
            name,
            description,
            url,
            enabled,
            official,
            icon,
        }
    }
}

/// Policy failures for store-source admin mutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreSourcePolicyError {
    /// Official catalog URL cannot be rewritten (seeded source).
    OfficialUrlImmutable,
    /// Official catalog cannot be deleted.
    OfficialNotDeletable,
    /// URL failed scheme/host allow rules.
    InvalidUrl { message: String },
}

impl StoreSourcePolicyError {
    pub fn message(&self) -> String {
        match self {
            Self::OfficialUrlImmutable => {
                "Official store source URL cannot be changed".to_string()
            }
            Self::OfficialNotDeletable => "Official store source cannot be deleted".to_string(),
            Self::InvalidUrl { message } => message.clone(),
        }
    }

    /// HTTP status hint: 403 for official locks, 400 for bad URL.
    pub fn status_hint(&self) -> u16 {
        match self {
            Self::OfficialUrlImmutable | Self::OfficialNotDeletable => 403,
            Self::InvalidUrl { .. } => 400,
        }
    }
}

impl std::fmt::Display for StoreSourcePolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for StoreSourcePolicyError {}

/// Default `enabled` when the admin omits the field on create.
pub fn default_store_source_enabled(enabled: Option<bool>) -> bool {
    enabled.unwrap_or(true)
}

/// Official sources may not change their catalog URL.
pub fn may_change_store_source_url(
    is_official: bool,
    new_url: Option<&str>,
) -> Result<(), StoreSourcePolicyError> {
    if is_official && new_url.is_some() {
        return Err(StoreSourcePolicyError::OfficialUrlImmutable);
    }
    Ok(())
}

/// Official sources may not be deleted.
pub fn may_delete_store_source(is_official: bool) -> Result<(), StoreSourcePolicyError> {
    if is_official {
        return Err(StoreSourcePolicyError::OfficialNotDeletable);
    }
    Ok(())
}

/// Validate a catalog URL for admin create/update (scheme + non-internal host).
///
/// Empty / unparsable URLs fail closed as invalid.
pub fn validate_store_source_url(url: &str) -> Result<(), StoreSourcePolicyError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(StoreSourcePolicyError::InvalidUrl {
            message: "Store source URL cannot be empty".to_string(),
        });
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| StoreSourcePolicyError::InvalidUrl {
        message: "Store source URL is not a valid absolute URL".to_string(),
    })?;
    if !is_allowed_store_url_scheme(parsed.scheme()) {
        return Err(StoreSourcePolicyError::InvalidUrl {
            message: "Only HTTP(S) store source URLs are allowed".to_string(),
        });
    }
    let host = parsed.host_str().ok_or_else(|| StoreSourcePolicyError::InvalidUrl {
        message: "Store source URL must include a host".to_string(),
    })?;
    if is_disallowed_store_host(host) {
        return Err(StoreSourcePolicyError::InvalidUrl {
            message: "Internal network store source URLs are not allowed".to_string(),
        });
    }
    // Normalize is intentional no-op for validity; ensures trailing index.json forms are accepted.
    let _ = normalize_store_catalog_url(trimmed);
    Ok(())
}

/// Whether two store-source URL strings collide after catalog normalization.
pub fn store_source_urls_conflict(existing_url: &str, candidate_url: &str) -> bool {
    normalize_store_catalog_url(existing_url) == normalize_store_catalog_url(candidate_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_enabled_is_true_when_omitted() {
        assert!(default_store_source_enabled(None));
        assert!(default_store_source_enabled(Some(true)));
        assert!(!default_store_source_enabled(Some(false)));
    }

    #[test]
    fn official_source_url_and_delete_are_locked() {
        assert!(may_change_store_source_url(true, Some("https://x")).is_err());
        assert_eq!(
            may_change_store_source_url(true, Some("https://x")).unwrap_err(),
            StoreSourcePolicyError::OfficialUrlImmutable
        );
        assert!(may_change_store_source_url(true, None).is_ok());
        assert!(may_change_store_source_url(false, Some("https://x")).is_ok());

        assert_eq!(
            may_delete_store_source(true).unwrap_err(),
            StoreSourcePolicyError::OfficialNotDeletable
        );
        assert!(may_delete_store_source(false).is_ok());
        assert_eq!(
            may_delete_store_source(true).unwrap_err().status_hint(),
            403
        );
    }

    #[test]
    fn validate_store_source_url_rejects_internal_and_empty() {
        assert!(validate_store_source_url("https://raw.githubusercontent.com/org/repo/main").is_ok());
        assert!(validate_store_source_url("https://example.com/store/index.json").is_ok());
        assert!(validate_store_source_url("").is_err());
        assert!(validate_store_source_url("not-a-url").is_err());
        assert!(validate_store_source_url("ftp://example.com").is_err());
        assert!(validate_store_source_url("http://localhost/store").is_err());
        assert!(validate_store_source_url("http://192.168.0.1/store").is_err());
        assert_eq!(
            validate_store_source_url("http://127.0.0.1/")
                .unwrap_err()
                .status_hint(),
            400
        );
    }

    #[test]
    fn store_source_urls_conflict_uses_normalized_catalog_base() {
        assert!(store_source_urls_conflict(
            "https://ex.com/store/index.json",
            "https://ex.com/store/"
        ));
        assert!(!store_source_urls_conflict(
            "https://ex.com/store/",
            "https://other.com/store/"
        ));
    }

    #[test]
    fn store_source_view_roundtrip_fields() {
        let view = StoreSourceView::new(
            1,
            "Official".into(),
            Some("desc".into()),
            "https://ex.com/store".into(),
            true,
            true,
            Some("icon.svg".into()),
        );
        assert_eq!(view.id, 1);
        assert!(view.official);
        assert_eq!(view.icon.as_deref(), Some("icon.svg"));
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["name"], "Official");
        assert_eq!(json["enabled"], true);
    }
}
