//! Host-managed Tapp component registry (`_component:{type}:{id}` in tapp_storage).
//!
//! Domain validates config, keys, and performs install-owner scoped upsert/list/delete.
//! API handlers map [`ComponentRegistryError`] to Axum and own grant/permission checks.

use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::models::entities::tapp_storage;

const COMPONENT_KEY_PREFIX: &str = "_component:";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    Theme,
    Agent,
}

impl ComponentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Agent => "agent",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "theme" => Some(Self::Theme),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}

/// Domain errors for component registry operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentRegistryError {
    InvalidConfig { message: String },
    InvalidType,
    Database,
    NotFound,
    RegisterFailed,
    UpdateFailed,
    UnregisterFailed,
}

impl ComponentRegistryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig { .. } => "INVALID_COMPONENT_CONFIG",
            Self::InvalidType => "INVALID_COMPONENT_TYPE",
            Self::Database => "COMPONENT_DATABASE_ERROR",
            Self::NotFound => "COMPONENT_NOT_FOUND",
            Self::RegisterFailed => "COMPONENT_REGISTER_FAILED",
            Self::UpdateFailed => "COMPONENT_UPDATE_FAILED",
            Self::UnregisterFailed => "COMPONENT_UNREGISTER_FAILED",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidConfig { message } => message.clone(),
            Self::InvalidType => "Invalid component type".to_string(),
            Self::Database => "Failed to load components".to_string(),
            Self::NotFound => "Component not found".to_string(),
            Self::RegisterFailed => "Failed to register component".to_string(),
            Self::UpdateFailed => "Failed to update component".to_string(),
            Self::UnregisterFailed => "Failed to unregister component".to_string(),
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::InvalidConfig { .. } | Self::InvalidType => 400,
            Self::NotFound => 404,
            Self::Database | Self::RegisterFailed | Self::UpdateFailed | Self::UnregisterFailed => {
                500
            }
        }
    }
}

impl std::fmt::Display for ComponentRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ComponentRegistryError {}

fn invalid_config(message: impl Into<String>) -> ComponentRegistryError {
    ComponentRegistryError::InvalidConfig {
        message: message.into(),
    }
}

/// Validate theme/agent config and return the component id.
pub fn validate_component_config(
    component_type: ComponentType,
    config: &Value,
) -> Result<&str, ComponentRegistryError> {
    let object = config
        .as_object()
        .ok_or_else(|| invalid_config("Component config must be an object"))?;
    let id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 64
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        .ok_or_else(|| invalid_config("Component id is invalid"))?;
    object
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty() && name.len() <= 100)
        .ok_or_else(|| {
            invalid_config("Component name is required and must not exceed 100 bytes")
        })?;

    match component_type {
        ComponentType::Theme => {
            if object
                .keys()
                .any(|key| !matches!(key.as_str(), "id" | "name" | "surface" | "glow"))
            {
                return Err(invalid_config(
                    "Theme config only supports id, name, surface and glow",
                ));
            }
            if object.get("surface").is_some_and(|value| {
                !matches!(value.as_str(), Some("glass" | "solid" | "flat" | "outline"))
            }) {
                return Err(invalid_config("Theme surface is invalid"));
            }
            if object.get("glow").is_some_and(|value| {
                !matches!(value.as_str(), Some("identity" | "primary" | "none"))
            }) {
                return Err(invalid_config("Theme glow is invalid"));
            }
        }
        ComponentType::Agent => {
            if object
                .keys()
                .any(|key| !matches!(key.as_str(), "id" | "name" | "description" | "capabilities"))
            {
                return Err(invalid_config(
                    "Agent config only supports id, name, description and capabilities",
                ));
            }
            if object
                .get("description")
                .is_some_and(|value| value.as_str().is_none_or(|text| text.len() > 500))
            {
                return Err(invalid_config("Agent description is invalid"));
            }
            let capabilities = object
                .get("capabilities")
                .and_then(Value::as_array)
                .filter(|items| !items.is_empty() && items.len() <= 64)
                .ok_or_else(|| invalid_config("Agent capabilities must contain 1-64 items"))?;
            if capabilities.iter().any(|value| {
                value
                    .as_str()
                    .is_none_or(|capability| capability.is_empty() || capability.len() > 64)
            }) {
                return Err(invalid_config("Agent capability is invalid"));
            }
        }
    }
    Ok(id)
}

pub fn component_storage_key(component_type: &str, component_id: &str) -> String {
    format!("{COMPONENT_KEY_PREFIX}{component_type}:{component_id}")
}

/// Result of a successful register/upsert.
#[derive(Debug, Clone)]
pub struct RegisteredComponent {
    pub id: String,
    pub component_type: String,
    pub tapp_id: String,
    pub registered_at: String,
}

/// Register or update a component under the installation owner namespace.
pub async fn register_component(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    component_type: ComponentType,
    config: Value,
) -> Result<RegisteredComponent, ComponentRegistryError> {
    let type_str = component_type.as_str();
    let component_id = validate_component_config(component_type, &config)?.to_string();
    let now = chrono::Utc::now().fixed_offset();
    let storage_key = component_storage_key(type_str, &component_id);
    let registered_at = now.to_rfc3339();

    let component_data = json!({
        "id": component_id,
        "type": type_str,
        "tappId": tapp_id,
        "config": config,
        "registeredAt": registered_at,
        "enabled": true
    });

    let existing = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(db)
        .await
        .map_err(|_| ComponentRegistryError::Database)?;

    if let Some(existing) = existing {
        let mut active: tapp_storage::ActiveModel = existing.into();
        active.value = Set(component_data);
        active.updated_at = Set(now);
        active
            .update(db)
            .await
            .map_err(|_| ComponentRegistryError::UpdateFailed)?;
    } else {
        let storage = tapp_storage::ActiveModel {
            id: NotSet,
            tapp_id: Set(tapp_id.to_string()),
            user_id: Set(owner_id),
            key: Set(storage_key),
            value: Set(component_data),
            encrypted_value: NotSet,
            binding_fingerprint: NotSet,
            created_at: Set(now),
            updated_at: Set(now),
        };
        storage
            .insert(db)
            .await
            .map_err(|_| ComponentRegistryError::RegisterFailed)?;
    }

    Ok(RegisteredComponent {
        id: component_id,
        component_type: type_str.to_string(),
        tapp_id: tapp_id.to_string(),
        registered_at,
    })
}

/// Unregister a component by type + id under the installation owner namespace.
pub async fn unregister_component(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    component_type: &str,
    component_id: &str,
) -> Result<(), ComponentRegistryError> {
    if ComponentType::from_str(component_type).is_none() {
        return Err(ComponentRegistryError::InvalidType);
    }
    let storage_key = component_storage_key(component_type, component_id);
    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(db)
        .await
        .map_err(|_| ComponentRegistryError::UnregisterFailed)?;
    if result.rows_affected == 0 {
        return Err(ComponentRegistryError::NotFound);
    }
    Ok(())
}

/// List components for one install owner + tapp, optional type filter.
pub async fn list_components_for_tapp(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    type_filter: Option<&str>,
) -> Result<Vec<Value>, ComponentRegistryError> {
    if let Some(t) = type_filter {
        if ComponentType::from_str(t).is_none() {
            return Err(ComponentRegistryError::InvalidType);
        }
    }
    let key_prefix = if let Some(t) = type_filter {
        format!("{COMPONENT_KEY_PREFIX}{t}:")
    } else {
        COMPONENT_KEY_PREFIX.to_string()
    };

    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.starts_with(&key_prefix))
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|_| ComponentRegistryError::Database)?;

    Ok(items.into_iter().map(|item| item.value).collect())
}

/// List all components of a type for a subject (cross-tapp, subject namespace).
pub async fn list_components_by_type_for_subject(
    db: &DatabaseConnection,
    subject_id: i32,
    component_type: &str,
) -> Result<Vec<Value>, ComponentRegistryError> {
    if ComponentType::from_str(component_type).is_none() {
        return Err(ComponentRegistryError::InvalidType);
    }
    let key_prefix = format!("{COMPONENT_KEY_PREFIX}{component_type}:");
    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(subject_id))
        .filter(tapp_storage::Column::Key.starts_with(&key_prefix))
        .order_by_asc(tapp_storage::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|_| ComponentRegistryError::Database)?;
    Ok(items.into_iter().map(|item| item.value).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        component_storage_key, validate_component_config, ComponentRegistryError, ComponentType,
    };
    use serde_json::json;

    #[test]
    fn theme_config_accepts_only_effective_fields() {
        assert!(validate_component_config(
            ComponentType::Theme,
            &json!({
                "id": "glass.primary",
                "name": "Glass Primary",
                "surface": "glass",
                "glow": "primary"
            })
        )
        .is_ok());
        assert!(validate_component_config(
            ComponentType::Theme,
            &json!({ "id": "legacy", "name": "Legacy", "styles": "*{}" })
        )
        .is_err());
    }

    #[test]
    fn agent_config_requires_declared_capabilities() {
        assert!(validate_component_config(
            ComponentType::Agent,
            &json!({ "id": "helper", "name": "Helper", "capabilities": ["chat"] })
        )
        .is_ok());
        assert!(validate_component_config(
            ComponentType::Agent,
            &json!({ "id": "helper", "name": "Helper", "capabilities": [] })
        )
        .is_err());
    }

    #[test]
    fn storage_key_and_type_helpers() {
        assert_eq!(
            component_storage_key("theme", "glass.primary"),
            "_component:theme:glass.primary"
        );
        assert_eq!(ComponentType::from_str("theme"), Some(ComponentType::Theme));
        assert_eq!(ComponentType::from_str("widget"), None);
        assert_eq!(ComponentType::Agent.as_str(), "agent");
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            ComponentRegistryError::InvalidConfig {
                message: "x".into()
            }
            .code(),
            "INVALID_COMPONENT_CONFIG"
        );
        assert_eq!(
            ComponentRegistryError::NotFound.message(),
            "Component not found"
        );
        assert_eq!(ComponentRegistryError::NotFound.status_hint(), 404);
        assert_eq!(ComponentRegistryError::InvalidType.status_hint(), 400);
        assert_eq!(
            ComponentRegistryError::RegisterFailed.message(),
            "Failed to register component"
        );
    }
}
