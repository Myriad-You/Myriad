//! Rows stored in shapes older than the current code, rewritten once into
//! the current shape at startup so nothing has to read the old ones. Every
//! statement is idempotent and touches nothing already current.
//!
//! Columns whose old rows are settled here are then held to the shape new
//! databases get, so the database itself keeps them current.
//!
//! The read-compat these replaced is gone in the same release; this file
//! goes when the support floor moves past it.

use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};

/// Each: what it settles, and the statement that settles it.
const OLD_ROWS: &[(&str, &str)] = &[
    (
        // A dev-build name for the toast switch.
        "notification toast switch under its old name",
        r#"
UPDATE users SET notification_preferences = jsonb_set(notification_preferences, '{delivery}',
    CASE WHEN notification_preferences->'delivery' ? 'toast'
         THEN (notification_preferences->'delivery') - 'high_priority_toast'
         ELSE ((notification_preferences->'delivery') - 'high_priority_toast')
              || jsonb_build_object('toast', notification_preferences->'delivery'->'high_priority_toast')
    END)
WHERE jsonb_typeof(notification_preferences->'delivery') = 'object'
  AND notification_preferences->'delivery' ? 'high_priority_toast'
"#,
    ),
    (
        // Notes published before 0.4.10 have no cloud document; the site
        // owner is credited with the one written here.
        "published notes without a document",
        r#"
WITH owner AS (
    SELECT id FROM users WHERE is_owner OR is_admin
     ORDER BY CASE WHEN is_owner THEN 0 ELSE 1 END, id LIMIT 1
), created AS (
    INSERT INTO phantasi_note_docs
        (user_id, item_id, title, content_md, topic, image, status, published_at, revision, created_at, updated_at)
    SELECT (SELECT id FROM owner), i.id, i.title, COALESCE(i.content_md, ''),
           NULLIF(btrim(COALESCE(i.topic, '')), ''), NULLIF(btrim(COALESCE(i.image, '')), ''),
           'published', date_trunc('milliseconds', i.published_at), 1, NOW(), NOW()
      FROM phantasi_items i JOIN phantasi_sources s ON s.id = i.source_id
     WHERE s.source_type = 'note' AND EXISTS (SELECT 1 FROM owner)
       AND NOT EXISTS (SELECT 1 FROM phantasi_note_docs d WHERE d.item_id = i.id)
    RETURNING id, user_id
)
INSERT INTO phantasi_note_authors (doc_id, user_id, role)
SELECT id, user_id, 'owner' FROM created
ON CONFLICT (doc_id, user_id) DO NOTHING
"#,
    ),
    (
        // Platform auto-refresh tasks named in Chinese before the English names.
        "auto-refresh task names in Chinese",
        r#"
UPDATE tapp_scheduled_tasks SET name = regexp_replace(name, '^自动刷新 (.+) 数据$', 'Auto-refresh \1 data')
WHERE name ~ '^自动刷新 .+ 数据$'
"#,
    ),
    (
        "scheduled-task notice titles in Chinese",
        r#"
UPDATE agent_notifications SET title = regexp_replace(title, '^定时任务:\s*', 'Scheduled task: ')
WHERE title ~ '^定时任务:'
"#,
    ),
    (
        "failed scheduled-task notice titles in Chinese",
        r#"UPDATE agent_notifications SET title = 'Scheduled task failed' WHERE title = '定时任务失败'"#,
    ),
    (
        "MCP tools-loaded notices in Chinese",
        r#"
UPDATE agent_notifications SET body = regexp_replace(body, '^已加载 ([0-9]+) 个工具$', 'Loaded \1 tools')
WHERE body ~ '^已加载 [0-9]+ 个工具$'
"#,
    ),
    (
        // Platform sync failures in Chinese only; no current notice matches.
        "platform sync failure notices in Chinese",
        r#"DELETE FROM agent_notifications WHERE title LIKE '%自动刷新失败%' OR body LIKE '%自动刷新失败%'"#,
    ),
    (
        // Agent tasks carrying step text from before 0.4.10: none of them can
        // run on today's engine, and finished ones without a completion time
        // were never swept.
        "agent tasks with Chinese step text",
        r#"
DELETE FROM agent_tasks
WHERE COALESCE(error, '') || ' ' || COALESCE(step_results::text, '') || ' ' || COALESCE(pending_question::text, '')
      ~ '执行超时|尝试了 .* 个源都无法订阅|需要人工确认|未经确认的高风险|不应被直接调用|未对普通用户开放|API Key 未配置|阅读列表|网络搜索|此操作将|将调用外部 MCP'
"#,
    ),
    (
        // Rows from before the lane column carry only the session.
        "agent tasks without their lane",
        r#"
UPDATE agent_tasks SET lane_id = 'user:' || user_id || ':session:' || btrim(session_id)
WHERE lane_id IS NULL AND session_id IS NOT NULL AND btrim(session_id) <> ''
"#,
    ),
    (
        // Work checkpoints from before 0.4.13 had no budget; one untouched
        // that long has expired (7 days, refreshed on every save).
        "work checkpoints without a budget",
        r#"
DELETE FROM runtime_registry
WHERE namespace = 'agent_work_checkpoint'
  AND (payload->'budget' IS NULL OR jsonb_typeof(payload->'budget') = 'null')
"#,
    ),
    (
        // Activities written without an arrival time; the column default
        // covers every writer from here on.
        "federation activities without an arrival time",
        r#"UPDATE federation_activities SET received_at = COALESCE(published_at, NOW()) WHERE received_at IS NULL"#,
    ),
    (
        "room members without a membership status",
        r#"UPDATE federation_room_members SET membership_status = 'active' WHERE membership_status IS NULL"#,
    ),
    (
        // Manifest widgets written before rows named their source. Both
        // columns are `json`: the merge goes through jsonb and back.
        "manifest widgets without a source",
        r#"
UPDATE tapp_widgets w
   SET config = (CASE WHEN json_typeof(w.config) = 'object' THEN w.config::jsonb ELSE '{}'::jsonb END
                 || jsonb_build_object('source', 'manifest', 'installationOwnerId', w.user_id))::json
  FROM tapps t
 WHERE t.tapp_id = w.tapp_id AND t.user_id = w.user_id
   AND (w.config IS NULL OR json_typeof(w.config) <> 'object' OR w.config ->> 'source' IS NULL)
   AND EXISTS (
       SELECT 1 FROM json_array_elements(
           CASE WHEN json_typeof(t.manifest -> 'widgets') = 'array'
                THEN t.manifest -> 'widgets' ELSE '[]'::json END) d
        WHERE 'tapp.' || w.tapp_id || '.' || (d ->> 'id') = w.widget_id)
"#,
    ),
    (
        // A runtime widget without an installation owner belongs to the
        // installation of the user it was registered for.
        "runtime widgets without an installation owner",
        r#"
UPDATE tapp_widgets
   SET config = (config::jsonb || jsonb_build_object('installationOwnerId', user_id))::json
 WHERE json_typeof(config) = 'object' AND config ->> 'source' = 'runtime'
   AND json_typeof(config -> 'installationOwnerId') IS DISTINCT FROM 'number'
"#,
    ),
    (
        "intentions without a known accept source",
        r#"
UPDATE agent_intentions SET accept_source = 'user'
WHERE accept_source IS DISTINCT FROM 'user' AND accept_source IS DISTINCT FROM 'autonomy'
"#,
    ),
];

/// Constraints new databases are created with, applied to columns that a
/// heal added without them once their old rows are settled above.
const SETTLED_COLUMNS: &[&str] = &[
    "ALTER TABLE federation_activities ALTER COLUMN received_at SET DEFAULT NOW(), ALTER COLUMN received_at SET NOT NULL",
    "ALTER TABLE federation_room_members ALTER COLUMN membership_status SET DEFAULT 'active', ALTER COLUMN membership_status SET NOT NULL",
];

/// Rewrite every old-shape row, then hold the settled columns to their
/// current shape; logs what it changed.
pub(crate) async fn rewrite_old_rows(db: &DatabaseConnection) -> Result<u64, DbErr> {
    let mut total = 0;
    for (what, sql) in OLD_ROWS {
        let changed = db
            .execute_unprepared(sql)
            .await
            .map_err(|error| DbErr::Custom(format!("rewrite old rows ({what}): {error}")))?
            .rows_affected();
        if changed > 0 {
            tracing::info!(changed, what, "Rewrote rows stored in an old shape");
        }
        total += changed;
    }
    for sql in SETTLED_COLUMNS {
        db.execute_unprepared(sql)
            .await
            .map_err(|error| DbErr::Custom(format!("settle column ({sql}): {error}")))?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::OLD_ROWS;

    #[test]
    fn every_rewrite_is_named_and_bounded_to_old_rows() {
        for (what, sql) in OLD_ROWS {
            assert!(!what.is_empty());
            let sql = sql.to_ascii_uppercase();
            assert!(
                sql.contains("WHERE"),
                "{what}: a rewrite must be limited to old rows"
            );
        }
    }

    /// Every statement runs on the migrated schema, and old widget rows come
    /// out in the shape the code reads.
    #[tokio::test]
    async fn old_widget_rows_are_rewritten_on_a_migrated_schema() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            eprintln!("skipping: set MYRIAD_MEDIA_TEST_DATABASE_URL to run the old row rewrites");
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "old_rows_test").await;
        let db = isolated.db.clone();
        db.execute_unprepared(
            r#"INSERT INTO users (id, username) VALUES (1, 'owner'), (2, 'member');
               INSERT INTO tapps (tapp_id, user_id, name, version, manifest, file_path, code_path)
               VALUES ('com.ex', 1, 'Ex', '1.0.0', '{"widgets":[{"id":"card"}]}', 'f', 'c');
               INSERT INTO tapp_widgets (widget_id, tapp_id, user_id, name, config) VALUES
                 ('tapp.com.ex.card', 'com.ex', 1, 'declared', '{"settings":{"a":1}}'),
                 ('tapp.com.ex.gone', 'com.ex', 1, 'undeclared', '{}'),
                 ('tapp.com.ex.mine', 'com.ex', 2, 'runtime', '{"source":"runtime"}'),
                 ('tapp.com.ex.bound', 'com.ex', 2, 'bound', '{"source":"runtime","installationOwnerId":1}')"#,
        )
        .await
        .unwrap();
        super::rewrite_old_rows(&db).await.unwrap();
        let rows = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT name, config::jsonb AS config FROM tapp_widgets ORDER BY name",
            ))
            .await
            .unwrap();
        let config = |name: &str| {
            rows.iter()
                .find(|row| row.try_get::<String>("", "name").unwrap() == name)
                .map(|row| row.try_get::<serde_json::Value>("", "config").unwrap())
                .unwrap()
        };
        assert_eq!(
            config("declared"),
            serde_json::json!({"settings":{"a":1},"source":"manifest","installationOwnerId":1})
        );
        assert_eq!(config("undeclared"), serde_json::json!({}));
        assert_eq!(
            config("runtime"),
            serde_json::json!({"source":"runtime","installationOwnerId":2})
        );
        assert_eq!(
            config("bound"),
            serde_json::json!({"source":"runtime","installationOwnerId":1})
        );
        // A second start finds nothing left to change.
        assert_eq!(super::rewrite_old_rows(&db).await.unwrap(), 0);
        isolated.drop().await;
    }
}
