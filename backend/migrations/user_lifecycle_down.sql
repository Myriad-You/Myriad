-- Reverse of user_lifecycle.sql (migration 006 `down`): removes every
-- constraint, trigger and function it installs; idempotent.
DO $$
DECLARE
    fk record;
    tbl text;
BEGIN
    FOR fk IN
        SELECT * FROM (VALUES
            ('fk_agent_sessions_user', 'agent_sessions'),
            ('fk_agent_messages_session', 'agent_messages'),
            ('fk_agent_notifications_user', 'agent_notifications'),
            ('fk_agent_task_presets_user', 'agent_task_presets'),
            ('fk_activity_events_user', 'activity_events'),
            ('fk_metadata_history_user', 'metadata_history'),
            ('fk_platform_metadata_user', 'platform_metadata'),
            ('fk_platform_reports_user', 'platform_reports'),
            ('fk_phantasi_comments_user', 'phantasi_comments'),
            ('fk_phantasi_categories_user', 'phantasi_categories'),
            ('fk_rsshub_instances_user', 'rsshub_instances'),
            ('fk_fed_keys_user', 'federation_keys'),
            ('fk_fed_follows_user', 'federation_follows'),
            ('fk_fed_activities_user', 'federation_activities'),
            ('fk_fed_channels_user', 'federation_channels'),
            ('fk_fed_channel_messages_channel', 'federation_channel_messages'),
            ('fk_fed_published_user', 'federation_published_content'),
            ('fk_fed_timeline_user', 'federation_timeline'),
            ('fk_fed_room_members_local_user', 'federation_room_members')
        ) AS t(name, child)
    LOOP
        CONTINUE WHEN to_regclass(fk.child) IS NULL;
        EXECUTE format('ALTER TABLE %I DROP CONSTRAINT IF EXISTS %I', fk.child, fk.name);
    END LOOP;

    FOREACH tbl IN ARRAY ARRAY[
        'agent_tasks', 'tapp_task_executions', 'tapp_scheduled_tasks', 'tapp_user_activities',
        'tapp_quota_usage', 'tapp_storage', 'tapp_widgets', 'tapps', 'tapp_runtime_registry',
        'tapp_ai_cost_ledger', 'phantasi_user_states', 'phantasi_sources'
    ] LOOP
        CONTINUE WHEN to_regclass(tbl) IS NULL;
        EXECUTE format('DROP TRIGGER IF EXISTS %I ON %I', 'trg_' || tbl || '_subject_user', tbl);
    END LOOP;

    IF to_regclass('users') IS NOT NULL THEN
        DROP TRIGGER IF EXISTS trg_users_after_delete_subject_rows ON users;
        DROP TRIGGER IF EXISTS trg_users_delete_subject_rows ON users;
    END IF;
END $$;

DROP FUNCTION IF EXISTS delete_user_subject_rows();
DROP FUNCTION IF EXISTS guard_subject_user();
