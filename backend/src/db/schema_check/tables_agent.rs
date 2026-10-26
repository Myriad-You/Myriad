//! Expected Agent-related DB table definitions (tasks, sessions, messages, notifications, persona).
use super::types::{ColumnDef, TableDef};

pub(crate) fn tables() -> Vec<TableDef> {
    vec![
        TableDef {
            name: "merope_stickers".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("picture", "text").not_null(),
                ColumnDef::new("meaning", "text").not_null(),
                ColumnDef::new("venue", "character varying"),
                ColumnDef::new("asset_id", "character varying").not_null(),
                ColumnDef::new("identity", "character varying").not_null(),
                ColumnDef::new("sent", "integer")
                    .not_null()
                    .default_value("0"),
                ColumnDef::new("last_sent_at", "timestamp with time zone"),
                ColumnDef::new("created_at", "timestamp with time zone")
                    .not_null()
                    .default_value("now()"),
                ColumnDef::new("kept_from", "character varying"),
            ],
        },
        TableDef {
            name: "agent_tasks".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "recipe_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'pending'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "current_step".into(),
                    data_type: "integer".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "total_steps".into(),
                    data_type: "integer".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "step_results".into(),
                    data_type: "json".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "execution_context".into(),
                    data_type: "json".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "recipe".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "pending_question".into(),
                    data_type: "json".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "progress".into(),
                    data_type: "smallint".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "error".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "original_request".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "session_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "lane_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "started_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "completed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "heartbeat_claims".to_string(),
            columns: vec![
                ColumnDef {
                    name: "task_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "minute_bucket".into(),
                    data_type: "bigint".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'running'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "claimed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "completed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_sessions".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "context".into(),
                    data_type: "json".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "message_count".into(),
                    data_type: "integer".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "archived".into(),
                    data_type: "boolean".into(),
                    default_value: Some("false".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "last_active_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_messages".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "session_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "task_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "role".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "content".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "metadata".into(),
                    data_type: "json".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_notifications".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "notification_type".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "priority".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'normal'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "title".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "body".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "metadata".into(),
                    data_type: "json".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "read".into(),
                    data_type: "boolean".into(),
                    default_value: Some("false".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_task_presets".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "input".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "preset_type".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'history'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "parsed_steps".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "intent_summary".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                // 对话标题
                ColumnDef {
                    name: "title".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                // 完整对话记录 (JSON) - 支持「继续对话」模式
                ColumnDef {
                    name: "conversation_data".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "last_used_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "use_count".into(),
                    data_type: "integer".into(),
                    default_value: Some("1".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_persona".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "text".into(),
                    default_value: Some("''::text".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "personality".into(),
                    data_type: "text".into(),
                    default_value: Some("''::text".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "persona_json".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "visual_profile".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "portrait_asset_id".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "portrait_generation".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "avatar_asset_id".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "avatar_generation".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "updated_by".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                // Her zone, an IANA name; empty is the site's own (`TZ`).
                ColumnDef::new("zone", "character varying")
                    .not_null()
                    .default_value("''"),
                // Each field's last edit: `{field: {rev, origin}}`.
                ColumnDef::new("field_revs", "jsonb")
                    .not_null()
                    .default_value("'{}'::jsonb"),
            ],
        },
        TableDef {
            name: "agent_addressee_state".to_string(),
            columns: vec![
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "mood".into(),
                    data_type: "double precision".into(),
                    default_value: Some("70".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "arousal".into(),
                    data_type: "double precision".into(),
                    default_value: Some("50".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "emotion".into(),
                    data_type: "double precision".into(),
                    default_value: Some("50".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "emotion_arousal".into(),
                    data_type: "double precision".into(),
                    default_value: Some("50".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "activity".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'idle'::character varying".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "activity_updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "do_not_disturb".into(),
                    data_type: "boolean".into(),
                    default_value: Some("false".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "dnd_start_minute".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "dnd_end_minute".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "last_user_message_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "last_proactive_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "music_mood_credited_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "mood_settled_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "emotion_settled_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: Some("CURRENT_TIMESTAMP".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_diary".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "content".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "source".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_proactive_messages".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "role".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "content".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "event_key".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "notified".into(),
                    data_type: "boolean".into(),
                    default_value: Some("false".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                // The `say` it went out as on a native child.
                ColumnDef::new("say_id", "character varying"),
                // `cutOff` / `heardChars` when it was cut off aloud.
                ColumnDef::new("metadata", "jsonb"),
            ],
        },
        TableDef {
            name: "agent_intentions".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "source_event_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "summary".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "reason_code".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "status".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'proposed'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "proposal".into(),
                    data_type: "jsonb".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "work_session_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "work_run_id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "result_summary".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "expires_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "accept_source".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'user'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_autonomy_grants".to_string(),
            columns: vec![
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "allowed_permissions".into(),
                    data_type: "jsonb".into(),
                    default_value: Some("'[]'::jsonb".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "revoked".into(),
                    data_type: "boolean".into(),
                    default_value: Some("false".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
            ],
        },
        TableDef {
            name: "agent_memories".to_string(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "user_id".into(),
                    data_type: "integer".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "kind".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "content".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "evidence".into(),
                    data_type: "text".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "speaker".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'user'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "source".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "venue".into(),
                    data_type: "character varying".into(),
                    default_value: Some("'private'".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "audience".into(),
                    data_type: "jsonb".into(),
                    default_value: Some("'[]'::jsonb".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "concepts".into(),
                    data_type: "jsonb".into(),
                    default_value: Some("'[]'::jsonb".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "importance".into(),
                    data_type: "double precision".into(),
                    default_value: Some("0.5".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "access_count".into(),
                    data_type: "integer".into(),
                    default_value: Some("0".into()),
                    not_null: false,
                },
                ColumnDef {
                    name: "last_accessed_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "valid_from".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "invalid_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "invalid_reason".into(),
                    data_type: "character varying".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "created_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                ColumnDef {
                    name: "updated_at".into(),
                    data_type: "timestamp with time zone".into(),
                    default_value: None,
                    not_null: false,
                },
                // The row that took this one's place (on the old row).
                ColumnDef::new("superseded_by", "character varying"),
                // Which side wrote it: `site` or a device.
                ColumnDef::new("origin", "character varying")
                    .not_null()
                    .default_value("'site'"),
            ],
        },
        TableDef {
            name: "agent_nodes".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("user_id", "integer").not_null(),
                ColumnDef::new("form", "character varying").not_null(),
                ColumnDef::new("node_key", "text").not_null(),
                ColumnDef::new("title", "text").not_null(),
                ColumnDef::new("origin", "character varying").not_null(),
                ColumnDef::new("notes", "jsonb")
                    .not_null()
                    .default_value("'[]'::jsonb"),
                ColumnDef::new("refs", "jsonb")
                    .not_null()
                    .default_value("'[]'::jsonb"),
                ColumnDef::new("logged", "bigint")
                    .not_null()
                    .default_value("0"),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
                ColumnDef::new("updated_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_node_events".to_string(),
            columns: vec![
                ColumnDef::new("node_id", "character varying").not_null(),
                ColumnDef::new("seq", "bigint").not_null(),
                ColumnDef::new("change", "character varying").not_null(),
                ColumnDef::new("fields", "jsonb").not_null(),
                ColumnDef::new("at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_memory_cues".to_string(),
            columns: vec![
                ColumnDef::new("memory_id", "character varying").not_null(),
                ColumnDef::new("cues", "text").not_null(),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_memory_embeddings".to_string(),
            columns: vec![
                ColumnDef::new("memory_id", "character varying").not_null(),
                ColumnDef::new("model", "character varying").not_null(),
                ColumnDef::new("digest", "character varying").not_null(),
                ColumnDef::new("vector", "bytea").not_null(),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_sync_devices".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("user_id", "integer").not_null(),
                ColumnDef::new("name", "character varying").not_null(),
                ColumnDef::new("scope", "character varying")
                    .not_null()
                    .default_value("'persona-sync'"),
                ColumnDef::new("token_hash", "character varying").not_null(),
                ColumnDef::new("token_hint", "character varying").not_null(),
                ColumnDef::new("lineage", "character varying"),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
                ColumnDef::new("last_seen_at", "timestamp with time zone"),
                ColumnDef::new("revoked_at", "timestamp with time zone"),
                ColumnDef::new("revoked_reason", "character varying"),
            ],
        },
        TableDef {
            name: "agent_host_lease".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("holder", "character varying").not_null(),
                ColumnDef::new("lineage", "character varying").not_null(),
                ColumnDef::new("epoch", "bigint").not_null(),
                ColumnDef::new("state", "character varying").not_null(),
                ColumnDef::new("move", "jsonb"),
                ColumnDef::new("updated_at", "timestamp with time zone").not_null(),
                ColumnDef::new("done", "jsonb"),
            ],
        },
        TableDef {
            name: "agent_host_backups".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("device_id", "character varying").not_null(),
                ColumnDef::new("day", "character varying").not_null(),
                ColumnDef::new("lineage", "character varying").not_null(),
                ColumnDef::new("epoch", "bigint").not_null(),
                ColumnDef::new("digest", "character varying").not_null(),
                ColumnDef::new("bytes", "bigint").not_null(),
                ColumnDef::new("received", "bigint")
                    .not_null()
                    .default_value("0"),
                ColumnDef::new("complete", "boolean")
                    .not_null()
                    .default_value("false"),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
                ColumnDef::new("updated_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_accounts".to_string(),
            columns: vec![
                ColumnDef::new("user_id", "integer").not_null(),
                ColumnDef::new("region", "character varying"),
                ColumnDef::new("started_at", "timestamp with time zone").not_null(),
                ColumnDef::new("stopped_at", "timestamp with time zone"),
            ],
        },
        TableDef {
            name: "agent_log_rows".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("audience", "character varying").not_null(),
                ColumnDef::new("origin", "character varying").not_null(),
                ColumnDef::new("dseq", "bigint").not_null(),
                ColumnDef::new("hlc", "character varying").not_null(),
                ColumnDef::new("object", "character varying").not_null(),
                ColumnDef::new("subject", "character varying"),
                ColumnDef::new("kind", "character varying").not_null(),
                ColumnDef::new("forgotten", "boolean")
                    .not_null()
                    .default_value("false"),
                ColumnDef::new("envelope", "jsonb").not_null(),
                ColumnDef::new("received_from", "character varying").not_null(),
                ColumnDef::new("received_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_payloads".to_string(),
            columns: vec![
                ColumnDef::new("row_id", "character varying").not_null(),
                ColumnDef::new("sealed", "bytea").not_null(),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_audience_keys".to_string(),
            columns: vec![
                ColumnDef::new("audience", "character varying").not_null(),
                ColumnDef::new("sealed", "text").not_null(),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_object_keys".to_string(),
            columns: vec![
                ColumnDef::new("audience", "character varying").not_null(),
                ColumnDef::new("object", "character varying").not_null(),
                ColumnDef::new("subject", "character varying"),
                ColumnDef::new("sealed", "bytea").not_null(),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_ledger".to_string(),
            columns: vec![
                ColumnDef::new("audience", "character varying").not_null(),
                ColumnDef::new("target", "character varying").not_null(),
                ColumnDef::new("at", "character varying").not_null(),
                ColumnDef::new("recorded_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_counters".to_string(),
            columns: vec![
                ColumnDef::new("audience", "character varying").not_null(),
                ColumnDef::new("dseq", "bigint").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_clock".to_string(),
            columns: vec![
                ColumnDef::new("node", "character varying").not_null(),
                ColumnDef::new("last", "character varying").not_null(),
            ],
        },
        TableDef {
            name: "agent_log_full_links".to_string(),
            columns: vec![
                ColumnDef::new("token_hash", "character varying").not_null(),
                ColumnDef::new("device_id", "character varying").not_null(),
                ColumnDef::new("audience", "character varying").not_null(),
                ColumnDef::new("expires_at", "timestamp with time zone").not_null(),
                ColumnDef::new("used_at", "timestamp with time zone"),
            ],
        },
        TableDef {
            name: "agent_lease_grants".to_string(),
            columns: vec![
                ColumnDef::new("epoch", "bigint").not_null(),
                ColumnDef::new("holder", "character varying").not_null(),
                ColumnDef::new("issued", "character varying").not_null(),
                ColumnDef::new("how", "character varying").not_null(),
                ColumnDef::new("through", "jsonb"),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_lease_ends".to_string(),
            columns: vec![
                ColumnDef::new("device_id", "character varying").not_null(),
                ColumnDef::new("core", "character varying").not_null(),
                ColumnDef::new("heard_at", "bigint"),
                ColumnDef::new("place", "character varying"),
                ColumnDef::new("owner_since", "bigint"),
                ColumnDef::new("owner_seen_at", "bigint"),
                ColumnDef::new("stopped_epoch", "bigint"),
                ColumnDef::new("stopped_at", "bigint"),
            ],
        },
        TableDef {
            name: "agent_kept_for_her".to_string(),
            columns: vec![
                ColumnDef::new("id", "bigint").not_null(),
                ColumnDef::new("user_id", "integer").not_null(),
                ColumnDef::new("event_key", "character varying").not_null(),
                ColumnDef::new("summary", "text").not_null(),
                ColumnDef::new("happened_at", "timestamp with time zone")
                    .not_null()
                    .default_value("now()"),
            ],
        },
        TableDef {
            name: "agent_host_import".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("source", "character varying").not_null(),
                ColumnDef::new("device_id", "character varying"),
                ColumnDef::new("lineage", "character varying"),
                ColumnDef::new("epoch", "bigint"),
                ColumnDef::new("name", "text"),
                ColumnDef::new("manifest", "jsonb"),
                ColumnDef::new("phase", "character varying").not_null(),
                ColumnDef::new("received", "bigint")
                    .not_null()
                    .default_value("0"),
                ColumnDef::new("total", "bigint"),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
                ColumnDef::new("confirmed_at", "timestamp with time zone"),
                ColumnDef::new("updated_at", "timestamp with time zone").not_null(),
                ColumnDef::new("kind", "character varying")
                    .not_null()
                    .default_value("'import'"),
                ColumnDef::new("subject", "character varying"),
                ColumnDef::new("preview", "jsonb"),
                ColumnDef::new("retired", "text"),
            ],
        },
        TableDef {
            name: "agent_host_import_staging".to_string(),
            columns: vec![
                ColumnDef::new("import_id", "character varying").not_null(),
                ColumnDef::new("kind", "character varying").not_null(),
                ColumnDef::new("entity_id", "character varying").not_null(),
                ColumnDef::new("canonical", "jsonb").not_null(),
            ],
        },
        TableDef {
            name: "agent_channel_held".to_string(),
            columns: vec![
                ColumnDef::new("id", "bigint").not_null(),
                ColumnDef::new("platform", "character varying").not_null(),
                ColumnDef::new("session_key", "character varying").not_null(),
                ColumnDef::new("inbound_id", "character varying").not_null(),
                ColumnDef::new("user_id", "integer").not_null(),
                ColumnDef::new("text", "text").not_null(),
                ColumnDef::new("images", "jsonb")
                    .not_null()
                    .default_value("'[]'::jsonb"),
                ColumnDef::new("arrived_at", "timestamp with time zone")
                    .not_null()
                    .default_value("now()"),
            ],
        },
        TableDef {
            name: "agent_affect_events".to_string(),
            columns: vec![
                ColumnDef::new("id", "character varying").not_null(),
                ColumnDef::new("user_id", "integer").not_null(),
                ColumnDef::new("at", "timestamp with time zone").not_null(),
                ColumnDef::new("cause", "character varying").not_null(),
                ColumnDef::new("impulse", "jsonb").not_null(),
                ColumnDef::new("baseline_mood", "double precision").not_null(),
                ColumnDef::new("baseline_arousal", "double precision").not_null(),
                ColumnDef::new("before", "jsonb").not_null(),
                ColumnDef::new("after", "jsonb").not_null(),
                ColumnDef::new("late", "boolean")
                    .not_null()
                    .default_value("false"),
                ColumnDef::new("origin", "character varying")
                    .not_null()
                    .default_value("'site'"),
                ColumnDef::new("rev", "character varying"),
                ColumnDef::new("created_at", "timestamp with time zone").not_null(),
            ],
        },
        TableDef {
            name: "agent_persona_field_history".to_string(),
            columns: vec![
                ColumnDef::new("id", "bigint").not_null(),
                ColumnDef::new("field", "character varying").not_null(),
                ColumnDef::new("value", "jsonb"),
                ColumnDef::new("rev", "character varying").not_null(),
                ColumnDef::new("origin", "character varying").not_null(),
                ColumnDef::new("replaced_at", "timestamp with time zone").not_null(),
                ColumnDef::new("replaced_by_rev", "character varying").not_null(),
                ColumnDef::new("reason", "character varying").not_null(),
            ],
        },
        TableDef {
            name: "agent_affect_anchor".to_string(),
            columns: vec![
                ColumnDef::new("user_id", "integer").not_null(),
                ColumnDef::new("mood", "double precision").not_null(),
                ColumnDef::new("arousal", "double precision").not_null(),
                ColumnDef::new("emotion", "double precision").not_null(),
                ColumnDef::new("emotion_arousal", "double precision").not_null(),
                ColumnDef::new("at", "timestamp with time zone").not_null(),
                ColumnDef::new("through_event", "character varying"),
                ColumnDef::new("rev", "character varying").not_null(),
            ],
        },
    ]
}
