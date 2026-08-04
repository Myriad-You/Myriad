//! Expected table definitions (tables_core).
use super::types::{ColumnDef, TableDef};

pub(crate) fn tables() -> Vec<TableDef> {
    vec![
        TableDef {
            name: "platforms".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "display_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "icon".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "api_endpoint".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "auth_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "enabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: true,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        TableDef {
            name: "users".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "github_id".into(),
                    data_type: "bigint".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "username".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "display_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "email".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "avatar_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "github_profile_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "bio".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "location".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "company".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "is_admin".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "last_login_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                // 本地认证字段
                ColumnDef {
                    name: "password_hash".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "auth_provider".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'github'".into()),
                },
                ColumnDef {
                    name: "linked_github_id".into(),
                    data_type: "bigint".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "local_login_disabled".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "notification_preferences".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'{}'::jsonb".into()),
                },
                ColumnDef {
                    name: "tapp_list_card_sizes".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'{}'::jsonb".into()),
                },
                // 在线状态跟踪（base 001）
                ColumnDef {
                    name: "last_seen_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "online_seconds".into(),
                    data_type: "bigint".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                // 站点 owner（base 001）
                ColumnDef {
                    name: "is_owner".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                // 画像源选择（base 001）：NULL = auto，沿用隐式优先级，故存量库补列即可，无需回填
                ColumnDef {
                    name: "avatar_source_kind".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "avatar_source_ref".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "avatar_resolved_url".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "avatar_updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                // 名称/简介文案来源（与 avatar_source_* 独立；NULL = auto）
                ColumnDef {
                    name: "profile_text_source_kind".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "profile_text_source_ref".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                // JWT session epoch (MYR-005): compare with claim `tv` on every auth check.
                // DEFAULT 0 so existing users keep working until first revoke bump.
                ColumnDef {
                    name: "token_version".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
            ],
        },
        TableDef {
            name: "configurations".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "key".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "value".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "description".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "category".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: Some("'general'".into()),
                },
                ColumnDef {
                    name: "is_encrypted".into(),
                    data_type: "boolean".into(),
                    is_nullable: true,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "is_public".into(),
                    data_type: "boolean".into(),
                    is_nullable: true,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        TableDef {
            name: "platform_metadata".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "raw_data".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "fetched_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: true,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        TableDef {
            name: "metadata_history".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "changed_fields".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "old_data".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "new_data".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "change_date".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        TableDef {
            name: "activity_events".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata_history_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata_id".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform_name".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "event_type".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "changes".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: Some("'[]'::jsonb".into()),
                },
                ColumnDef {
                    name: "change_count".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "importance".into(),
                    data_type: "smallint".into(),
                    is_nullable: false,
                    default_value: Some("0".into()),
                },
                ColumnDef {
                    name: "occurred_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
            ],
        },
        TableDef {
            name: "platform_reports".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "platform".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "metadata".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "report".into(),
                    data_type: "jsonb".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "report_title".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                },
                ColumnDef {
                    name: "expires_at".into(),
                    data_type: "timestamp without time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        }
    ]
}
