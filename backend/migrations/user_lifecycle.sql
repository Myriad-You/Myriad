-- User lifecycle is owned by the schema: `DELETE FROM users WHERE id = $1`
-- removes (or detaches) everything a local account owns. Shared by migration
-- 006 (greenfield) and `schema_check::ensure_user_lifecycle` (existing DBs);
-- idempotent.
--
-- 1. Foreign keys for columns that only ever hold real `users.id` values.
--    A missing (or differently-acting) constraint is healed under a table lock:
--    orphans of owned rows are deleted, orphans of detachable references are
--    set NULL — exactly what deleting their user would have done — and the
--    constraint is added. A new user-owned table only needs its own FK here or
--    in its CREATE TABLE.
DO $$
DECLARE
    fk record;
BEGIN
    FOR fk IN
        SELECT * FROM (VALUES
            ('fk_agent_sessions_user', 'agent_sessions', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_agent_messages_session', 'agent_messages', 'session_id', 'agent_sessions', 'id', 'CASCADE'),
            ('fk_agent_notifications_user', 'agent_notifications', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_agent_task_presets_user', 'agent_task_presets', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_activity_events_user', 'activity_events', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_metadata_history_user', 'metadata_history', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_platform_metadata_user', 'platform_metadata', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_platform_reports_user', 'platform_reports', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_phantasi_comments_user', 'phantasi_comments', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_phantasi_categories_user', 'phantasi_categories', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_rsshub_instances_user', 'rsshub_instances', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_fed_keys_user', 'federation_keys', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_fed_follows_user', 'federation_follows', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_fed_activities_user', 'federation_activities', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_fed_channels_user', 'federation_channels', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_fed_channel_messages_channel', 'federation_channel_messages', 'channel_id', 'federation_channels', 'channel_id', 'CASCADE'),
            ('fk_fed_published_user', 'federation_published_content', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_fed_timeline_user', 'federation_timeline', 'user_id', 'users', 'id', 'CASCADE'),
            ('fk_fed_room_members_local_user', 'federation_room_members', 'local_user_id', 'users', 'id', 'SET NULL')
        ) AS t(name, child, col, parent, parent_col, action)
    LOOP
        CONTINUE WHEN EXISTS (
            SELECT 1 FROM pg_constraint
            WHERE conname = fk.name
              AND conrelid = to_regclass(fk.child)
              AND contype = 'f'
              AND confdeltype = CASE fk.action WHEN 'SET NULL' THEN 'n' ELSE 'c' END
        );
        EXECUTE format('LOCK TABLE %I IN SHARE ROW EXCLUSIVE MODE', fk.child);
        EXECUTE format('ALTER TABLE %I DROP CONSTRAINT IF EXISTS %I', fk.child, fk.name);
        IF fk.action = 'SET NULL' THEN
            EXECUTE format(
                'UPDATE %1$I AS c SET %2$I = NULL WHERE c.%2$I IS NOT NULL '
                'AND NOT EXISTS (SELECT 1 FROM %3$I AS p WHERE p.%4$I = c.%2$I)',
                fk.child, fk.col, fk.parent, fk.parent_col);
        ELSE
            EXECUTE format(
                'DELETE FROM %1$I AS c WHERE c.%2$I IS NOT NULL '
                'AND NOT EXISTS (SELECT 1 FROM %3$I AS p WHERE p.%4$I = c.%2$I)',
                fk.child, fk.col, fk.parent, fk.parent_col);
        END IF;
        EXECUTE format(
            'ALTER TABLE %I ADD CONSTRAINT %I FOREIGN KEY (%I) REFERENCES %I (%I) ON DELETE %s',
            fk.child, fk.name, fk.col, fk.parent, fk.parent_col, fk.action);
    END LOOP;
END $$;

-- 2. Subject-keyed tables. Their id columns also carry non-account subjects
--    (0 = system heartbeat / anonymous / unattributed, negative = browser
--    guests), so no FK can express them. The same DELETE removes a real
--    user's rows here, before the row itself goes and the FKs above cascade.
CREATE OR REPLACE FUNCTION delete_user_subject_rows() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM agent_tasks WHERE user_id = OLD.id;
    DELETE FROM tapp_task_executions
    WHERE user_id = OLD.id
       OR scheduled_task_id IN (SELECT id FROM tapp_scheduled_tasks WHERE user_id = OLD.id);
    DELETE FROM tapp_scheduled_tasks WHERE user_id = OLD.id;
    DELETE FROM tapp_user_activities WHERE user_id = OLD.id;
    DELETE FROM tapp_quota_usage WHERE user_id = OLD.id;
    DELETE FROM tapp_storage WHERE user_id = OLD.id;
    DELETE FROM tapp_widgets WHERE user_id = OLD.id;
    DELETE FROM tapps WHERE user_id = OLD.id;
    DELETE FROM tapp_runtime_registry WHERE subject_id = OLD.id OR owner_id = OLD.id;
    DELETE FROM tapp_ai_cost_ledger WHERE subject_id = OLD.id OR owner_id = OLD.id;
    DELETE FROM phantasi_user_states WHERE user_id = OLD.id;
    -- Items (and their states/comments) cascade from their source.
    DELETE FROM phantasi_sources WHERE user_id = OLD.id;
    RETURN OLD;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_users_delete_subject_rows' AND tgrelid = to_regclass('users')
    ) THEN
        CREATE TRIGGER trg_users_delete_subject_rows
            BEFORE DELETE ON users
            FOR EACH ROW EXECUTE FUNCTION delete_user_subject_rows();
    END IF;
END $$;
