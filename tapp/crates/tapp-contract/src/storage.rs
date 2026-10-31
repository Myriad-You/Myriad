//! Storage key rules shared by install validation, sandbox writes, and CLI.

use crate::contract_rules::MAX_STORAGE_KEY_LEN;

/// A kind of record the host keeps in a Tapp's storage. Each owns a key
/// prefix the sandbox cannot write; every reader, writer and SQL filter takes
/// its prefix from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostNamespace {
    Settings,
    Credentials,
    Shared,
    Private,
    Component,
    Shortcut,
    Report,
}

impl HostNamespace {
    pub const ALL: [Self; 7] = [
        Self::Settings,
        Self::Credentials,
        Self::Shared,
        Self::Private,
        Self::Component,
        Self::Shortcut,
        Self::Report,
    ];

    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Settings => "_settings.",
            Self::Credentials => "_credentials.",
            Self::Shared => "_shared.",
            Self::Private => "_private.",
            Self::Component => "_component:",
            Self::Shortcut => "_shortcut:",
            Self::Report => "_report:",
        }
    }

    /// The namespace's own name as a bare key, reserved alongside its prefix.
    const fn bare_key(self) -> Option<&'static str> {
        match self {
            Self::Settings => Some("_settings"),
            Self::Private => Some("_private"),
            _ => None,
        }
    }

    pub fn key(self, name: &str) -> String {
        format!("{}{name}", self.prefix())
    }

    pub fn strip(self, key: &str) -> Option<&str> {
        key.strip_prefix(self.prefix())
    }
}

/// SQL predicate (over a `key` column) that admits only sandbox keys, so
/// queries using it never load host-managed rows.
pub fn sandbox_key_predicate_sql() -> String {
    HostNamespace::ALL
        .iter()
        .filter_map(|namespace| namespace.bare_key())
        .map(|bare| format!("key <> '{bare}'"))
        .chain(
            HostNamespace::ALL
                .iter()
                .map(|namespace| format!("NOT starts_with(key, '{}')", namespace.prefix())),
        )
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Path segments that collide with fixed `/storage/{segment}` routes.
/// Sandbox keys must not equal these exact strings.
pub const RESERVED_STORAGE_ROUTE_KEYS: &[&str] = &["entries", "usage"];

pub fn validate_storage_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("Key cannot be empty");
    }
    if key.len() > MAX_STORAGE_KEY_LEN {
        return Err("Key too long (max 256 characters)");
    }
    if key.starts_with('.') || key.ends_with('.') {
        return Err("Key cannot start or end with a dot");
    }
    if key.contains("..") {
        return Err("Key cannot contain consecutive dots");
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return Err(
            "Key contains invalid characters (only alphanumeric, underscore, hyphen, dot, colon allowed)",
        );
    }
    Ok(())
}

pub fn is_host_storage_key(key: &str) -> bool {
    HostNamespace::ALL
        .iter()
        .any(|namespace| namespace.bare_key() == Some(key) || key.starts_with(namespace.prefix()))
}

pub fn is_reserved_storage_route_key(key: &str) -> bool {
    RESERVED_STORAGE_ROUTE_KEYS.contains(&key)
}

pub fn validate_sandbox_storage_key(key: &str) -> Result<(), &'static str> {
    validate_storage_key(key)?;
    if is_host_storage_key(key) {
        return Err("Key prefix is reserved for host-managed Tapp data");
    }
    if is_reserved_storage_route_key(key) {
        return Err("Key is reserved for storage API routes (entries, usage); choose another name");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_rules::STORAGE_KEY_PATTERN;

    #[test]
    fn storage_key_rejects_dot_affixes_and_dotdot() {
        for key in [".foo", "foo.", "..", "foo..bar", ".hidden", "trail."] {
            let err = validate_storage_key(key).expect_err(key);
            assert!(
                err.contains("dot"),
                "key={key} should mention dot, got {err}"
            );
            assert!(validate_sandbox_storage_key(key).is_err(), "{key}");
        }
        assert!(validate_storage_key("user.preferences").is_ok());
        assert!(validate_storage_key("a:b_c-d.e").is_ok());
        assert!(validate_sandbox_storage_key("user.preferences").is_ok());
    }

    #[test]
    fn storage_key_pattern_forbids_dot_affixes() {
        assert!(
            STORAGE_KEY_PATTERN.contains("(?:[A-Za-z0-9_:-]+\\.)*"),
            "exported pattern must reject leading/trailing '.' and '..': {STORAGE_KEY_PATTERN}"
        );
        assert!(
            !STORAGE_KEY_PATTERN.starts_with("^[A-Za-z0-9_.:-]+$"),
            "charset-only pattern would still allow '.' affixes"
        );
    }

    #[test]
    fn sandbox_keys_reject_host_prefixes() {
        for key in [
            "_settings",
            "_settings.theme",
            "_credentials.wegame",
            "_shared.posts",
            "_private",
            "_private.token",
            "_component:x",
            "_shortcut:y",
            "_report:z",
        ] {
            assert!(validate_sandbox_storage_key(key).is_err(), "{key}");
            assert!(is_host_storage_key(key), "{key}");
            assert!(validate_storage_key(key).is_ok(), "{key}");
        }
        assert!(!is_host_storage_key("user.preferences"));
    }

    #[test]
    fn sandbox_keys_reject_route_reserved_names() {
        assert!(is_reserved_storage_route_key("entries"));
        assert!(is_reserved_storage_route_key("usage"));
        assert!(!is_reserved_storage_route_key("entries.v1"));
        assert!(!is_reserved_storage_route_key("my-usage"));
        for key in ["entries", "usage"] {
            let err = validate_sandbox_storage_key(key).expect_err(key);
            assert!(
                err.to_ascii_lowercase().contains("reserved"),
                "key={key} err={err}"
            );
        }
        assert!(validate_sandbox_storage_key("entries.v1").is_ok());
        assert!(validate_sandbox_storage_key("usage_stats").is_ok());
    }

    #[test]
    fn storage_key_rejects_empty_long_and_invalid_chars() {
        assert_eq!(validate_storage_key("").unwrap_err(), "Key cannot be empty");
        let too_long = "a".repeat(MAX_STORAGE_KEY_LEN + 1);
        assert!(
            validate_storage_key(&too_long)
                .unwrap_err()
                .contains("too long")
        );
        assert!(validate_storage_key("space key").is_err());
        assert!(validate_storage_key("slash/key").is_err());
    }
}
