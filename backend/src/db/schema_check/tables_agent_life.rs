//! Expected table definitions for Agent Life.
use super::types::{ColumnDef, TableDef};

pub(crate) fn tables() -> Vec<TableDef> {
    vec![
        TableDef {
            name: "agent_persona".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: Some("''::text".into()),
                },
                ColumnDef {
                    name: "personality".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: Some("''::text".into()),
                },
                ColumnDef {
                    name: "persona_json".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "visual_profile".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "portrait_asset_id".into(),
                    data_type: "text".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "portrait_generation".into(),
                    data_type: "jsonb".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_by".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        TableDef {
            name: "agent_addressee_state".to_string(),
            columns: vec![
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "mood".into(),
                    data_type: "double precision".into(),
                    is_nullable: false,
                    default_value: Some("70".into()),
                },
                ColumnDef {
                    name: "activity".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: Some("'idle'::character varying".into()),
                },
                ColumnDef {
                    name: "do_not_disturb".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "dnd_start_minute".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "dnd_end_minute".into(),
                    data_type: "integer".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_user_message_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_proactive_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "last_departure_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        TableDef {
            name: "agent_diary".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
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
                    name: "content".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "source".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
        TableDef {
            name: "agent_proactive_messages".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "bigint".into(),
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
                    name: "role".into(),
                    data_type: "character varying".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "content".into(),
                    data_type: "text".into(),
                    is_nullable: false,
                    default_value: None,
                },
                ColumnDef {
                    name: "event_key".into(),
                    data_type: "character varying".into(),
                    is_nullable: true,
                    default_value: None,
                },
                ColumnDef {
                    name: "notified".into(),
                    data_type: "boolean".into(),
                    is_nullable: false,
                    default_value: Some("false".into()),
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    is_nullable: false,
                    default_value: None,
                },
            ],
        },
    ]
}
