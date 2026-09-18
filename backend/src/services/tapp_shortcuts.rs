//! Host-managed Tapp shortcut registry (`_shortcut:{id}` in tapp_storage).
//!
//! Domain validates key chords, detects conflicts, and performs install-owner
//! scoped upsert/list/delete. API maps [`ShortcutRegistryError`] to Axum.

use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, Set,
};
use serde_json::{Value, json};

use crate::models::entities::tapp_storage;

const SHORTCUT_KEY_PREFIX: &str = "_shortcut:";

/// Domain errors for shortcut registry operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutRegistryError {
    InvalidKeys,
    Conflict { conflicting_shortcut: String },
    Database,
    NotFound,
    RegisterFailed,
    UpdateFailed,
    UnregisterFailed,
}

impl ShortcutRegistryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidKeys => "INVALID_SHORTCUT_KEYS",
            Self::Conflict { .. } => "SHORTCUT_KEY_CONFLICT",
            Self::Database => "SHORTCUT_DATABASE_ERROR",
            Self::NotFound => "SHORTCUT_NOT_FOUND",
            Self::RegisterFailed => "SHORTCUT_REGISTER_FAILED",
            Self::UpdateFailed => "SHORTCUT_UPDATE_FAILED",
            Self::UnregisterFailed => "SHORTCUT_UNREGISTER_FAILED",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidKeys => "Invalid shortcut key format".to_string(),
            Self::Conflict {
                conflicting_shortcut,
            } => {
                format!("Shortcut key conflict with {conflicting_shortcut}")
            }
            Self::Database => "Failed to load shortcuts".to_string(),
            Self::NotFound => "Shortcut not found".to_string(),
            Self::RegisterFailed => "Failed to register shortcut".to_string(),
            Self::UpdateFailed => "Failed to update shortcut".to_string(),
            Self::UnregisterFailed => "Failed to unregister shortcut".to_string(),
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::InvalidKeys => 400,
            Self::Conflict { .. } => 409,
            Self::NotFound => 404,
            Self::Database | Self::RegisterFailed | Self::UpdateFailed | Self::UnregisterFailed => {
                500
            }
        }
    }
}

impl std::fmt::Display for ShortcutRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ShortcutRegistryError {}

/// Chord: 1–4 `+`-parts, ≤50 bytes; last part is the key, earlier parts must be modifiers.
pub fn validate_shortcut_keys(keys: &str) -> bool {
    if keys.is_empty() || keys.len() > 50 {
        return false;
    }

    let parts: Vec<&str> = keys.split('+').collect();
    if parts.is_empty() || parts.len() > 4 {
        return false;
    }

    let valid_modifiers = ["ctrl", "alt", "shift", "meta", "cmd"];
    let mut has_key = false;

    for (i, part) in parts.iter().enumerate() {
        let lower = part.to_lowercase();
        if i == parts.len() - 1 {
            if lower.len() == 1
                || lower.starts_with('f') && lower.len() <= 3
                || [
                    "enter",
                    "escape",
                    "space",
                    "tab",
                    "backspace",
                    "delete",
                    "up",
                    "down",
                    "left",
                    "right",
                    "home",
                    "end",
                    "pageup",
                    "pagedown",
                ]
                .contains(&lower.as_str())
            {
                has_key = true;
            }
        } else if !valid_modifiers.contains(&lower.as_str()) {
            return false;
        }
    }

    has_key
}

pub fn shortcut_storage_key(shortcut_id: &str) -> String {
    format!("{SHORTCUT_KEY_PREFIX}{shortcut_id}")
}

/// Register or update a shortcut under the installation owner namespace.
///
/// Detects key conflicts across all shortcuts owned by the same install owner.
pub async fn register_shortcut(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    shortcut_id: &str,
    keys: &str,
    description: &str,
    action: &str,
    scope: Option<String>,
) -> Result<Value, ShortcutRegistryError> {
    if !validate_shortcut_keys(keys) {
        return Err(ShortcutRegistryError::InvalidKeys);
    }

    let now = chrono::Utc::now().fixed_offset();
    let storage_key = shortcut_storage_key(shortcut_id);
    let shortcut_data = json!({
        "id": shortcut_id,
        "tappId": tapp_id,
        "keys": keys,
        "description": description,
        "action": action,
        "scope": scope.unwrap_or_else(|| "tapp".to_string()),
        "registeredAt": now.to_rfc3339(),
        "enabled": true
    });

    // 检查快捷键冲突（同一安装 owner 命名空间内）
    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::Key.starts_with(SHORTCUT_KEY_PREFIX))
        .all(db)
        .await
        .map_err(|_| ShortcutRegistryError::Database)?;

    for item in existing {
        if let Some(existing_keys) = item.value.get("keys").and_then(|v| v.as_str()) {
            if existing_keys == keys {
                if let Some(id) = item.value.get("id").and_then(|v| v.as_str()) {
                    if id != shortcut_id {
                        return Err(ShortcutRegistryError::Conflict {
                            conflicting_shortcut: id.to_string(),
                        });
                    }
                }
            }
        }
    }

    let existing_item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(db)
        .await
        .map_err(|_| ShortcutRegistryError::Database)?;

    if let Some(existing_item) = existing_item {
        let mut active: tapp_storage::ActiveModel = existing_item.into();
        active.value = Set(shortcut_data.clone());
        active.updated_at = Set(now);
        active.update(db).await.map_err(|error| {
            map_shortcut_write_error(error, ShortcutRegistryError::UpdateFailed)
        })?;
    } else {
        let storage = tapp_storage::ActiveModel {
            id: NotSet,
            tapp_id: Set(tapp_id.to_string()),
            user_id: Set(owner_id),
            key: Set(storage_key),
            value: Set(shortcut_data.clone()),
            encrypted_value: NotSet,
            binding_fingerprint: NotSet,
            created_at: Set(now),
            updated_at: Set(now),
        };
        storage.insert(db).await.map_err(|error| {
            map_shortcut_write_error(error, ShortcutRegistryError::RegisterFailed)
        })?;
    }

    Ok(shortcut_data)
}

fn unique_violation(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505")
        || lower.contains("duplicate key")
        || lower.contains("unique")
        || lower.contains("idx_tapp_shortcuts_owner_chord")
}

fn map_shortcut_write_error(
    error: sea_orm::DbErr,
    fallback: ShortcutRegistryError,
) -> ShortcutRegistryError {
    if unique_violation(&error) {
        ShortcutRegistryError::Conflict {
            conflicting_shortcut: "existing".to_string(),
        }
    } else {
        tracing::error!(%error, "shortcut write failed");
        fallback
    }
}

/// Unregister a shortcut by id under the installation owner namespace.
pub async fn unregister_shortcut(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    shortcut_id: &str,
) -> Result<(), ShortcutRegistryError> {
    let storage_key = shortcut_storage_key(shortcut_id);
    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(db)
        .await
        .map_err(|_| ShortcutRegistryError::UnregisterFailed)?;
    if result.rows_affected == 0 {
        return Err(ShortcutRegistryError::NotFound);
    }
    Ok(())
}

/// List shortcuts for one install owner, filtered to a single tapp_id.
pub async fn list_shortcuts(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
) -> Result<Vec<Value>, ShortcutRegistryError> {
    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::Key.starts_with(SHORTCUT_KEY_PREFIX))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|_| ShortcutRegistryError::Database)?;
    Ok(items.into_iter().map(|item| item.value).collect())
}

#[cfg(test)]
mod tests {
    use super::{ShortcutRegistryError, shortcut_storage_key, validate_shortcut_keys};

    #[test]
    fn accepts_common_chords() {
        assert!(validate_shortcut_keys("ctrl+k"));
        assert!(validate_shortcut_keys("Ctrl+Shift+P"));
        assert!(validate_shortcut_keys("meta+enter"));
        assert!(validate_shortcut_keys("alt+f1"));
        assert!(validate_shortcut_keys("shift+space"));
    }

    #[test]
    fn rejects_invalid_chords() {
        assert!(!validate_shortcut_keys(""));
        assert!(!validate_shortcut_keys("ctrl+"));
        assert!(!validate_shortcut_keys("ctrl+alt+shift+meta+k")); // >4 parts
        assert!(!validate_shortcut_keys("super+k"));
        assert!(!validate_shortcut_keys(&"a".repeat(51)));
    }

    #[test]
    fn storage_key_contract() {
        assert_eq!(shortcut_storage_key("open"), "_shortcut:open");
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            ShortcutRegistryError::InvalidKeys.message(),
            "Invalid shortcut key format"
        );
        assert_eq!(ShortcutRegistryError::InvalidKeys.status_hint(), 400);
        assert_eq!(
            ShortcutRegistryError::Conflict {
                conflicting_shortcut: "x".into()
            }
            .status_hint(),
            409
        );
        assert_eq!(
            ShortcutRegistryError::Conflict {
                conflicting_shortcut: "x".into()
            }
            .code(),
            "SHORTCUT_KEY_CONFLICT"
        );
        assert_eq!(
            ShortcutRegistryError::NotFound.message(),
            "Shortcut not found"
        );
        assert_eq!(ShortcutRegistryError::NotFound.status_hint(), 404);
    }

    #[test]
    fn register_maps_chord_unique_to_conflict() {
        let src = include_str!("tapp_shortcuts.rs");
        let register = src
            .split("pub async fn register_shortcut")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn unregister_shortcut").next())
            .expect("register_shortcut");
        assert!(register.contains("map_shortcut_write_error"));
        assert_eq!(
            super::map_shortcut_write_error(
                sea_orm::DbErr::Custom(
                    "23505 duplicate key value violates unique constraint \"idx_tapp_shortcuts_owner_chord\""
                        .into()
                ),
                ShortcutRegistryError::RegisterFailed
            ),
            ShortcutRegistryError::Conflict {
                conflicting_shortcut: "existing".into()
            }
        );
    }

    #[tokio::test]
    async fn concurrent_chords_are_rejected_by_unique_index() {
        use crate::models::entities::tapp_storage;
        use sea_orm::{
            ConnectOptions, ConnectionTrait, Database, DatabaseBackend, EntityTrait, Schema,
        };

        let Ok(url) = std::env::var("PHANTASI_TEST_DATABASE_URL") else {
            return;
        };
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        let schema = Schema::new(DatabaseBackend::Postgres);
        let sql = schema
            .create_table_from_entity(tapp_storage::Entity)
            .to_string(sea_orm::sea_query::PostgresQueryBuilder)
            .replacen("CREATE TABLE", "CREATE TEMP TABLE", 1);
        db.execute_unprepared(&sql).await.unwrap();
        db.execute_unprepared(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_tapp_shortcuts_owner_chord \
             ON tapp_storage (user_id, (value->>'keys')) \
             WHERE starts_with(key, '_shortcut:')",
        )
        .await
        .unwrap();
        super::register_shortcut(&db, 1, "tapp.a", "open", "ctrl+k", "Open", "open", None)
            .await
            .unwrap();
        let conflict =
            super::register_shortcut(&db, 1, "tapp.b", "other", "ctrl+k", "Other", "other", None)
                .await;
        assert!(matches!(
            conflict,
            Err(ShortcutRegistryError::Conflict { .. })
        ));
        let rows = tapp_storage::Entity::find().all(&db).await.unwrap();
        assert_eq!(rows.len(), 1);
    }
}
