//! Tapp 存储实体定义

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Tapp 键值存储
#[derive(Clone, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "tapp_storage")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,

    /// 所属 Tapp ID
    #[sea_orm(column_type = "String(StringLen::N(255))")]
    pub tapp_id: String,

    /// 所属用户 ID
    pub user_id: i32,

    /// 键名
    #[sea_orm(column_type = "String(StringLen::N(255))")]
    pub key: String,

    /// 值 (JSON)
    #[sea_orm(column_type = "Json")]
    pub value: serde_json::Value,

    /// Host-only encrypted payload for reserved storage records.
    #[serde(skip)]
    #[sea_orm(column_type = "Text", nullable)]
    pub encrypted_value: Option<String>,

    /// Authorization fingerprint for host-only credential bindings.
    #[serde(skip)]
    #[sea_orm(column_type = "String(StringLen::N(64))", nullable)]
    pub binding_fingerprint: Option<String>,

    pub created_at: DateTimeWithTimeZone,

    pub updated_at: DateTimeWithTimeZone,
}

// Keep host-only material out of diagnostics as well as JSON. SeaORM models
// are frequently logged with `?row` while investigating database problems, so
// a derived `Debug` implementation would turn an otherwise harmless debug log
// into a ciphertext disclosure path.
impl std::fmt::Debug for Model {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Model")
            .field("id", &self.id)
            .field("tapp_id", &self.tapp_id)
            .field("user_id", &self.user_id)
            .field("key", &self.key)
            .field("value", &self.value)
            .field(
                "encrypted_value",
                &self.encrypted_value.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "binding_fingerprint",
                &self.binding_fingerprint.as_ref().map(|_| "[REDACTED]"),
            )
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

// Ownership is `(user_id, tapp_id)`. Do not expose a misleading tapp_id-only
// SeaORM relation; callers must filter by both columns explicitly.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::Model;

    #[test]
    fn serialized_or_debugged_storage_model_never_contains_host_credential_fields() {
        let now = crate::clock::now().fixed_offset();
        let model = Model {
            id: 1,
            tapp_id: "com.example.test".into(),
            user_id: 1,
            key: "_credentials.api".into(),
            value: serde_json::json!({"kind": "credential", "version": 1}),
            encrypted_value: Some("ciphertext-must-not-serialize".into()),
            binding_fingerprint: Some("f".repeat(64)),
            created_at: now,
            updated_at: now,
        };

        let debugged = format!("{model:?}");
        assert!(!debugged.contains("ciphertext-must-not-serialize"));
        assert!(!debugged.contains(&"f".repeat(64)));

        let serialized = serde_json::to_value(model).expect("model must serialize");
        assert!(serialized.get("encrypted_value").is_none());
        assert!(serialized.get("binding_fingerprint").is_none());
        assert!(
            !serialized
                .to_string()
                .contains("ciphertext-must-not-serialize")
        );
    }
}
