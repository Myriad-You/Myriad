//! Expected index definitions.
use super::types::IndexDef;

pub(crate) fn get_expected_indexes() -> Vec<IndexDef> {
    vec![
        IndexDef {
            name: "idx_users_github_id".into(),
            table: "users".into(),
            columns: vec!["github_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_users_username".into(),
            table: "users".into(),
            columns: vec!["username".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_users_is_admin".into(),
            table: "users".into(),
            columns: vec!["is_admin".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_linked_github_id".into(),
            table: "users".into(),
            columns: vec!["linked_github_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_users_auth_provider".into(),
            table: "users".into(),
            columns: vec!["auth_provider".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_metadata_user".into(),
            table: "platform_metadata".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_metadata_platform".into(),
            table: "platform_metadata".into(),
            columns: vec!["platform_name".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_metadata_history_user".into(),
            table: "metadata_history".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_metadata_history_metadata".into(),
            table: "metadata_history".into(),
            columns: vec!["metadata_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "activity_events_metadata_history_id_key".into(),
            table: "activity_events".into(),
            columns: vec!["metadata_history_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_activity_events_user_date".into(),
            table: "activity_events".into(),
            columns: vec!["user_id".into(), "occurred_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_activity_events_platform_date".into(),
            table: "activity_events".into(),
            columns: vec!["platform_name".into(), "occurred_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_reports_user_id".into(),
            table: "platform_reports".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_platform_reports_platform".into(),
            table: "platform_reports".into(),
            columns: vec!["platform".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_configurations_category".into(),
            table: "configurations".into(),
            columns: vec!["category".into()],
            is_unique: false,
        },
        // 002_tapp_system.rs 索引
        // tapps 索引
        IndexDef {
            name: "idx_tapps_user_tapp_id".into(),
            table: "tapps".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapps_user_id".into(),
            table: "tapps".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapps_status".into(),
            table: "tapps".into(),
            columns: vec!["status".into()],
            is_unique: false,
        },
        // tapp_widgets 索引
        IndexDef {
            name: "idx_tapp_widgets_unique".into(),
            table: "tapp_widgets".into(),
            columns: vec!["user_id".into(), "tapp_id".into(), "widget_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_widgets_user_id".into(),
            table: "tapp_widgets".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        // tapp_storage 索引
        IndexDef {
            name: "idx_tapp_storage_unique".into(),
            table: "tapp_storage".into(),
            columns: vec!["user_id".into(), "tapp_id".into(), "key".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_storage_user_tapp".into(),
            table: "tapp_storage".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: false,
        },
        // shared runtime state 索引
        IndexDef {
            name: "idx_tapp_runtime_registry_subject".into(),
            table: "tapp_runtime_registry".into(),
            columns: vec!["namespace".into(), "subject_id".into(), "expires_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_registry_tapp".into(),
            table: "tapp_runtime_registry".into(),
            columns: vec!["namespace".into(), "tapp_id".into(), "expires_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_registry_runtime".into(),
            table: "tapp_runtime_registry".into(),
            columns: vec!["namespace".into(), "runtime_id".into(), "expires_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_mailbox_recipient".into(),
            table: "tapp_runtime_mailbox".into(),
            columns: vec!["channel".into(), "runtime_id".into(), "message_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_runtime_mailbox_expiry".into(),
            table: "tapp_runtime_mailbox".into(),
            columns: vec!["expires_at".into()],
            is_unique: false,
        },
        // tapp_quota_usage 索引
        IndexDef {
            name: "idx_tapp_quota_unique".into(),
            table: "tapp_quota_usage".into(),
            columns: vec![
                "user_id".into(),
                "tapp_id".into(),
                "quota_type".into(),
                "period_start".into(),
            ],
            is_unique: true,
        },
        // tapp_ai_cost_ledger 索引
        IndexDef {
            name: "idx_tapp_ai_cost_subject_time".into(),
            table: "tapp_ai_cost_ledger".into(),
            columns: vec!["subject_id".into(), "occurred_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_ai_cost_tapp_time".into(),
            table: "tapp_ai_cost_ledger".into(),
            columns: vec!["tapp_id".into(), "occurred_at".into()],
            is_unique: false,
        },
        // tapp_store_sources 索引
        IndexDef {
            name: "idx_tapp_store_sources_url".into(),
            table: "tapp_store_sources".into(),
            columns: vec!["url".into()],
            is_unique: true,
        },
        // tapp_scheduled_tasks 索引
        IndexDef {
            name: "idx_tapp_scheduled_tasks_unique".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["user_id".into(), "tapp_id".into(), "task_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_scheduled_tasks_user".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_scheduled_tasks_tapp".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["tapp_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_scheduled_tasks_next_run".into(),
            table: "tapp_scheduled_tasks".into(),
            columns: vec!["enabled".into(), "next_run_at".into()],
            is_unique: false,
        },
        // tapp_task_executions 索引
        IndexDef {
            name: "idx_tapp_task_executions_task".into(),
            table: "tapp_task_executions".into(),
            columns: vec!["scheduled_task_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_task_executions_user_tapp".into(),
            table: "tapp_task_executions".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_tapp_task_executions_executed_at".into(),
            table: "tapp_task_executions".into(),
            columns: vec!["executed_at".into()],
            is_unique: false,
        },
        // tapp_user_activities 索引
        IndexDef {
            name: "idx_tapp_user_activities_unique".into(),
            table: "tapp_user_activities".into(),
            columns: vec!["user_id".into(), "tapp_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_tapp_user_activities_user_last_run".into(),
            table: "tapp_user_activities".into(),
            columns: vec!["user_id".into(), "last_run_at".into()],
            is_unique: false,
        },
        // 003_brew_system.rs 索引
        // brew_sources 索引
        IndexDef {
            name: "idx_brew_sources_user_id".into(),
            table: "brew_sources".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_sources_user_url".into(),
            table: "brew_sources".into(),
            columns: vec!["user_id".into(), "url".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_brew_sources_category".into(),
            table: "brew_sources".into(),
            columns: vec!["user_id".into(), "category".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_sources_schedule".into(),
            table: "brew_sources".into(),
            columns: vec!["enabled".into(), "last_fetched_at".into()],
            is_unique: false,
        },
        // brew_items 索引
        IndexDef {
            name: "idx_brew_items_source_guid".into(),
            table: "brew_items".into(),
            columns: vec!["source_id".into(), "guid".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_brew_items_published".into(),
            table: "brew_items".into(),
            columns: vec!["source_id".into(), "published_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_items_timeline".into(),
            table: "brew_items".into(),
            columns: vec!["published_at".into()],
            is_unique: false,
        },
        // brew_user_states 索引
        IndexDef {
            name: "idx_brew_user_states_unique".into(),
            table: "brew_user_states".into(),
            columns: vec!["user_id".into(), "item_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_brew_user_states_unread".into(),
            table: "brew_user_states".into(),
            columns: vec!["user_id".into(), "is_read".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_user_states_starred".into(),
            table: "brew_user_states".into(),
            columns: vec!["user_id".into(), "is_starred".into()],
            is_unique: false,
        },
        // brew_categories 索引
        IndexDef {
            name: "idx_brew_categories_unique".into(),
            table: "brew_categories".into(),
            columns: vec!["user_id".into(), "name".into()],
            is_unique: true,
        },
        // brew_annotations 索引
        IndexDef {
            name: "idx_brew_annotations_item".into(),
            table: "brew_annotations".into(),
            columns: vec!["item_id".into()],
            is_unique: false,
        },
        // brew_podcasts 索引
        IndexDef {
            name: "idx_brew_podcasts_item".into(),
            table: "brew_podcasts".into(),
            columns: vec!["item_id".into()],
            is_unique: true, // 每篇文章只有一个播客
        },
        // brew_comments 索引
        IndexDef {
            name: "idx_brew_comments_item".into(),
            table: "brew_comments".into(),
            columns: vec!["item_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_comments_user".into(),
            table: "brew_comments".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_brew_comments_item_user".into(),
            table: "brew_comments".into(),
            columns: vec!["item_id".into(), "user_id".into()],
            is_unique: false,
        },
        // rsshub_instances 索引
        IndexDef {
            name: "idx_rsshub_instances_user_url".into(),
            table: "rsshub_instances".into(),
            columns: vec!["user_id".into(), "url".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_rsshub_instances_priority".into(),
            table: "rsshub_instances".into(),
            columns: vec!["user_id".into(), "enabled".into(), "priority".into()],
            is_unique: false,
        },
        // agent_tasks 索引
        IndexDef {
            name: "idx_agent_tasks_user_id".into(),
            table: "agent_tasks".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_tasks_status".into(),
            table: "agent_tasks".into(),
            columns: vec!["status".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_tasks_session_id".into(),
            table: "agent_tasks".into(),
            columns: vec!["session_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_tasks_updated_at".into(),
            table: "agent_tasks".into(),
            columns: vec!["updated_at".into()],
            is_unique: false,
        },
        // agent_sessions 索引
        IndexDef {
            name: "idx_agent_sessions_user_id".into(),
            table: "agent_sessions".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        // agent_messages 索引
        IndexDef {
            name: "idx_agent_messages_session_id".into(),
            table: "agent_messages".into(),
            columns: vec!["session_id".into()],
            is_unique: false,
        },
        // agent_notifications 索引
        IndexDef {
            name: "idx_agent_notifications_created_at".into(),
            table: "agent_notifications".into(),
            columns: vec!["created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_notifications_user_id".into(),
            table: "agent_notifications".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        // agent_task_presets 索引
        IndexDef {
            name: "idx_agent_task_presets_user_type".into(),
            table: "agent_task_presets".into(),
            columns: vec!["user_id".into(), "preset_type".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_task_presets_user_input".into(),
            table: "agent_task_presets".into(),
            columns: vec!["user_id".into(), "input".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_agent_task_presets_last_used".into(),
            table: "agent_task_presets".into(),
            columns: vec!["last_used_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_diary_user_created".into(),
            table: "agent_diary".into(),
            columns: vec!["user_id".into(), "created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_agent_proactive_user_created".into(),
            table: "agent_proactive_messages".into(),
            columns: vec!["user_id".into(), "created_at".into()],
            is_unique: false,
        },
        // federation 索引
        IndexDef {
            name: "idx_remote_actors_domain".into(),
            table: "federation_remote_actors".into(),
            columns: vec!["domain".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_follows_user".into(),
            table: "federation_follows".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_follows_direction_status".into(),
            table: "federation_follows".into(),
            columns: vec!["direction".into(), "status".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_follows_unique".into(),
            table: "federation_follows".into(),
            columns: vec![
                "user_id".into(),
                "remote_actor_id".into(),
                "direction".into(),
            ],
            is_unique: true,
        },
        IndexDef {
            name: "idx_activities_user".into(),
            table: "federation_activities".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_activities_type".into(),
            table: "federation_activities".into(),
            columns: vec!["activity_type".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_activities_published".into(),
            table: "federation_activities".into(),
            columns: vec!["published_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_delivery_pending".into(),
            table: "federation_delivery_queue".into(),
            columns: vec!["status".into(), "next_retry_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_delivery_queue_activity_target".into(),
            table: "federation_delivery_queue".into(),
            columns: vec!["activity_id".into(), "target_inbox".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_delivery_lease_expiry".into(),
            table: "federation_delivery_queue".into(),
            columns: vec!["status".into(), "lease_expires_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_timeline_user_activity".into(),
            table: "federation_timeline".into(),
            columns: vec!["user_id".into(), "activity_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_channels_user".into(),
            table: "federation_channels".into(),
            columns: vec!["user_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_channels_status".into(),
            table: "federation_channels".into(),
            columns: vec!["status".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_channel_msgs_channel".into(),
            table: "federation_channel_messages".into(),
            columns: vec!["channel_id".into(), "created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_room_members_room".into(),
            table: "federation_room_members".into(),
            columns: vec!["room_id".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_room_members_unique".into(),
            table: "federation_room_members".into(),
            columns: vec!["room_id".into(), "actor_url".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_room_msgs_room".into(),
            table: "federation_room_messages".into(),
            columns: vec!["room_id".into(), "created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_room_msgs_thread".into(),
            table: "federation_room_messages".into(),
            columns: vec!["thread_id".into()],
            is_unique: false,
        },
        // Room attachment library / list_room_transfers (room_id may be null for DM transfers)
        IndexDef {
            name: "idx_file_transfers_room".into(),
            table: "federation_file_transfers".into(),
            columns: vec!["room_id".into(), "created_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "federation_inbox_receipts_pkey".into(),
            table: "federation_inbox_receipts".into(),
            columns: vec!["signer".into(), "activity_id".into(), "inbox_scope".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_published_user_type".into(),
            table: "federation_published_content".into(),
            columns: vec!["user_id".into(), "content_type".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_published_content_unique".into(),
            table: "federation_published_content".into(),
            columns: vec!["content_type".into(), "content_id".into()],
            is_unique: true,
        },
        IndexDef {
            name: "idx_timeline_user_received".into(),
            table: "federation_timeline".into(),
            columns: vec!["user_id".into(), "received_at".into()],
            is_unique: false,
        },
        // 近月新功能索引（001 analytics / 004 heartbeat / 005 fed 扩展）
        IndexDef {
            name: "idx_analytics_page_daily_day".into(),
            table: "analytics_page_daily".into(),
            columns: vec!["day".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_analytics_visitor_seen_day".into(),
            table: "analytics_visitor_seen".into(),
            columns: vec!["day".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_analytics_event_daily_day".into(),
            table: "analytics_event_daily".into(),
            columns: vec!["day".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_analytics_event_visitor_day".into(),
            table: "analytics_event_visitor".into(),
            columns: vec!["day".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_analytics_referrer_daily_day".into(),
            table: "analytics_referrer_daily".into(),
            columns: vec!["day".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_analytics_country_daily_day".into(),
            table: "analytics_country_daily".into(),
            columns: vec!["day".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_analytics_country_visitor_day".into(),
            table: "analytics_country_visitor".into(),
            columns: vec!["day".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_heartbeat_claims_claimed_at".into(),
            table: "heartbeat_claims".into(),
            columns: vec!["claimed_at".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_federation_domain_aliases_new".into(),
            table: "federation_domain_aliases".into(),
            columns: vec!["new_base_url".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_fed_interactions_object_kind".into(),
            table: "federation_object_interactions".into(),
            columns: vec!["object_id".into(), "kind".into()],
            is_unique: false,
        },
        IndexDef {
            name: "idx_fed_interactions_user_kind_created".into(),
            table: "federation_object_interactions".into(),
            columns: vec!["user_id".into(), "kind".into(), "created_at".into()],
            is_unique: false,
        },
    ]
}
