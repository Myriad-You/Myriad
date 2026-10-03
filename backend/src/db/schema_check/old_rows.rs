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
        // Platform auto-refresh tasks named in Chinese before the English
        // names. Only the platform's own tasks: a Tapp may name its own so.
        "auto-refresh task names in Chinese",
        r#"
UPDATE tapp_scheduled_tasks SET name = regexp_replace(name, '^自动刷新 (.+) 数据$', 'Auto-refresh \1 data')
WHERE tapp_id = 'myriad.core.platform-sync' AND name ~ '^自动刷新 .+ 数据$'
"#,
    ),
    // Notices below were written in Chinese until 0.4.10 (released
    // 2026-09-15). Text alone cannot tell them apart: Tapps and heartbeat
    // results write free text into the same columns. Each is bounded by the
    // type that carried it and by a creation time before that release.
    (
        "scheduled-task notice titles in Chinese",
        r#"
UPDATE agent_notifications SET title = regexp_replace(title, '^定时任务:\s*', 'Scheduled task: ')
WHERE notification_type = 'heartbeat_result' AND created_at < '2026-09-16'
  AND title ~ '^定时任务:'
"#,
    ),
    (
        "failed scheduled-task notice titles in Chinese",
        r#"
UPDATE agent_notifications SET title = regexp_replace(title, '^定时任务失败', 'Scheduled task failed')
WHERE notification_type = 'tapp_notification' AND created_at < '2026-09-16'
  AND title ~ '^定时任务失败(:|$)'
"#,
    ),
    (
        "MCP tools-loaded notices in Chinese",
        r#"
UPDATE agent_notifications SET body = regexp_replace(body, '^已加载 ([0-9]+) 个工具$', 'Loaded \1 tools')
WHERE notification_type = 'mcp_server_status' AND created_at < '2026-09-16'
  AND body ~ '^已加载 [0-9]+ 个工具$'
"#,
    ),
    (
        // Platform sync failures in Chinese only; no current notice matches.
        "platform sync failure notices in Chinese",
        r#"
DELETE FROM agent_notifications
WHERE notification_type = 'system_info' AND created_at < '2026-09-16'
  AND title LIKE '%自动刷新失败%'
"#,
    ),
    (
        // Agent tasks carrying step text from before 0.4.10: none of them can
        // run on today's engine, and finished ones without a completion time
        // were never swept. Step results hold fetched content, so the text
        // alone could match a current task; only tasks started before that
        // release qualify.
        "agent tasks with Chinese step text",
        r#"
DELETE FROM agent_tasks
WHERE started_at < '2026-09-16'
  AND COALESCE(error, '') || ' ' || COALESCE(step_results::text, '') || ' ' || COALESCE(pending_question::text, '')
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
   AND COALESCE(strpos(w.config::text, '\u0000'), 0) = 0
   AND EXISTS (
       SELECT 1 FROM json_array_elements(
           CASE WHEN json_typeof(t.manifest -> 'widgets') = 'array'
                THEN t.manifest -> 'widgets' ELSE '[]'::json END) d
        WHERE 'tapp.' || w.tapp_id || '.' || (d ->> 'id') = w.widget_id)
"#,
    ),
    (
        // A runtime widget without an installation owner belongs to the
        // installation of the user it was registered for; one stored as a
        // numeric string was read as that owner.
        "runtime widgets without an installation owner",
        r#"
UPDATE tapp_widgets
   SET config = (config::jsonb || jsonb_build_object('installationOwnerId',
       CASE WHEN config ->> 'installationOwnerId' ~ '^[0-9]{1,9}$'
            THEN (config ->> 'installationOwnerId')::int ELSE user_id END))::json
 WHERE json_typeof(config) = 'object' AND config ->> 'source' = 'runtime'
   AND json_typeof(config -> 'installationOwnerId') IS DISTINCT FROM 'number'
   AND strpos(config::text, '\u0000') = 0
"#,
    ),
    (
        // Recipe-level confirmations are gone; a channel prompt parked for
        // one, or stored before prompts carried an id, cannot be answered.
        "channel prompts for retired confirmations",
        r#"
DELETE FROM runtime_registry
WHERE namespace IN ('qq_c2c_pending', 'telegram_dm_pending', 'discord_dm_pending',
                    'feishu_p2p_pending', 'onebot_private_pending')
  AND (jsonb_typeof(payload #> '{prompt,kind}') IS DISTINCT FROM 'object'
       OR payload #> '{prompt,kind}' ? 'Confirm'
       OR COALESCE(btrim(payload #>> '{prompt,id}'), '') = '')
"#,
    ),
    (
        "channel outbox buttons for retired confirmations",
        r#"
UPDATE runtime_registry SET payload = jsonb_set(payload, '{prompt}', 'null'::jsonb)
WHERE namespace IN ('qq_c2c_outbound', 'telegram_dm_outbound', 'discord_dm_outbound',
                    'feishu_p2p_outbound', 'onebot_private_outbound')
  AND jsonb_typeof(payload -> 'prompt') = 'object'
  AND (payload #> '{prompt,kind}' ? 'Confirm'
       OR COALESCE(btrim(payload #>> '{prompt,id}'), '') = '')
"#,
    ),
    (
        // Messages that carried a recipe-level confirmation: its synthetic
        // task id and question point at nothing. The column is `json`, which
        // keeps a `\u0000` that `jsonb` refuses: such rows are left alone
        // rather than failing startup. The filter names only the retired
        // keys, so a current `"questionType":"confirmation"` never gets here.
        "session messages carrying a retired confirmation",
        r#"
UPDATE agent_messages a SET metadata = cleaned.m::json
FROM (
    SELECT id, metadata::jsonb - 'confirmation' AS m0 FROM agent_messages
    WHERE json_typeof(metadata) = 'object'
      AND metadata::text ~ '"confirmation"\s*:|"confirmationId"|"confirmation_id"|"confirmation:'
      AND strpos(metadata::text, '\u0000') = 0
) src
CROSS JOIN LATERAL (
    SELECT CASE WHEN COALESCE(t.m ->> 'task_id', '') LIKE 'confirmation:%'
                THEN t.m - 'task_id' ELSE t.m END AS m1
    FROM (SELECT CASE WHEN COALESCE(src.m0 ->> 'taskId', '') LIKE 'confirmation:%'
                      THEN src.m0 - 'taskId' ELSE src.m0 END AS m) t
) step1
CROSS JOIN LATERAL (
    SELECT CASE WHEN jsonb_typeof(step1.m1 -> 'pendingQuestion') = 'object'
                THEN CASE WHEN (step1.m1 -> 'pendingQuestion') ?| ARRAY['confirmationId', 'confirmation_id']
                          THEN step1.m1 - 'pendingQuestion' ELSE step1.m1 END
                ELSE step1.m1 END AS m2
) step2
CROSS JOIN LATERAL (
    SELECT CASE WHEN jsonb_typeof(step2.m2 #> '{task,pendingQuestion}') = 'object'
                THEN CASE WHEN (step2.m2 #> '{task,pendingQuestion}') ?| ARRAY['confirmationId', 'confirmation_id']
                          THEN step2.m2 #- '{task,pendingQuestion}' ELSE step2.m2 END
                ELSE step2.m2 END AS m
) cleaned
WHERE a.id = src.id AND cleaned.m IS DISTINCT FROM a.metadata::jsonb
"#,
    ),
    (
        // The same synthetic id was also stored in the message's own column.
        "session messages bound to a retired confirmation",
        r#"UPDATE agent_messages SET task_id = NULL WHERE task_id LIKE 'confirmation:%'"#,
    ),
    (
        // Ring filters stored under the name the Brew rename gave them. The
        // column is `json`.
        "ring category filters under their old name",
        r#"
UPDATE federation_ring_memberships
   SET gossip_config = ((gossip_config::jsonb - 'phantasi_category')
       || CASE WHEN gossip_config::jsonb ? 'category' THEN '{}'::jsonb
               ELSE jsonb_build_object('category', gossip_config::jsonb -> 'phantasi_category') END)::json
 WHERE json_typeof(gossip_config) = 'object' AND gossip_config::text LIKE '%"phantasi_category"%'
   AND strpos(gossip_config::text, '\u0000') = 0
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
    total += record_activity_for_old_history(db).await?;
    for sql in SETTLED_COLUMNS {
        db.execute_unprepared(sql)
            .await
            .map_err(|error| DbErr::Custom(format!("settle column ({sql}): {error}")))?;
    }
    Ok(total)
}

/// Platform history recorded before every change got an activity event. The
/// feed reads events only; these get one each, titled like current events.
async fn record_activity_for_old_history(db: &DatabaseConnection) -> Result<u64, DbErr> {
    use sea_orm::{DatabaseBackend, Statement};
    const ORPHANS: &str = "FROM metadata_history h
        WHERE NOT EXISTS (SELECT 1 FROM activity_events e WHERE e.metadata_history_id = h.id)";
    let platforms = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!("SELECT DISTINCT h.platform_name {ORPHANS}"),
        ))
        .await?;
    let mut total = 0;
    for row in platforms {
        let platform: String = row.try_get("", "platform_name")?;
        let title = crate::services::activity_event_service::platform_label(&platform).to_string();
        total += db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!(
                    "INSERT INTO activity_events (metadata_history_id, metadata_id, user_id,
                         platform_name, event_type, title, changes, change_count, importance,
                         occurred_at, created_at)
                     SELECT h.id, h.metadata_id, h.user_id, h.platform_name, 'updated', $2,
                            jsonb_build_array(jsonb_build_object(
                                'kind', 'summary', 'metric', 'data_changes', 'new', c.n)),
                            c.n, 0, h.change_date, NOW()
                     FROM metadata_history h
                     CROSS JOIN LATERAL (
                         SELECT CASE WHEN json_typeof(h.changed_fields) = 'array'
                                     THEN json_array_length(h.changed_fields) ELSE 1 END AS n
                     ) c
                     WHERE h.platform_name = $1 AND h.id IN (SELECT h.id {ORPHANS})"
                ),
                [platform.into(), title.into()],
            ))
            .await?
            .rows_affected();
    }
    if total > 0 {
        tracing::info!(total, "Recorded activity events for older platform history");
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

    /// Every statement runs on the migrated schema, and old rows come out in
    /// the shape the code reads.
    #[tokio::test]
    async fn old_rows_are_rewritten_on_a_migrated_schema() {
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
                 ('tapp.com.ex.bound', 'com.ex', 2, 'bound', '{"source":"runtime","installationOwnerId":1}'),
                 ('tapp.com.ex.text', 'com.ex', 2, 'text owner', '{"source":"runtime","installationOwnerId":"1"}');
               INSERT INTO runtime_registry (namespace, record_id, payload, expires_at) VALUES
                 ('telegram_dm_pending', 'no-id', '{"prompt":{"id":"","kind":{"Clarify":{"original_input":"x"}},"question":"q","options":[],"expires_at_unix":null}}', 9999999999),
                 ('telegram_dm_pending', 'confirm', '{"prompt":{"id":"ab12","kind":{"Confirm":{"confirmation_id":"c"}},"question":"q","options":[],"expires_at_unix":null}}', 9999999999),
                 ('telegram_dm_pending', 'answer', '{"prompt":{"id":"cd34","kind":{"Answer":{"task_id":"t","question_id":"q","question_type":"confirmation"}},"question":"q","options":[],"expires_at_unix":null}}', 9999999999),
                 ('telegram_dm_outbound', 'out', '{"run_id":"r","items":[],"next_index":0,"prompt":{"id":"ef56","kind":{"Confirm":{"confirmation_id":"c"}},"question":"q","options":[],"expires_at_unix":null}}', 9999999999);
               INSERT INTO agent_sessions (id, user_id, created_at, last_active_at)
                 VALUES ('s', 1, NOW(), NOW());
               INSERT INTO agent_messages (session_id, role, content, metadata, task_id, created_at) VALUES
                 ('s', 'assistant', 'old', '{"taskId":"confirmation:c1","pendingQuestion":{"question":"q","confirmationId":"c1"},"message":"x"}', 'confirmation:c1', NOW()),
                 ('s', 'assistant', 'current', '{"taskId":"t1","task":{"pendingQuestion":{"question":"q","questionType":"confirmation","questionId":"q1"}}}', 't1', NOW()),
                 ('s', 'assistant', 'nul', '{"confirmationId":"c2","output":"a\u0000b"}', NULL, NOW());
               INSERT INTO agent_tasks (id, user_id, recipe_id, status, current_step, step_results, progress, started_at, updated_at) VALUES
                 ('old-task', 1, 'r', 'completed', 1, '[{"output":"网络搜索 - x"}]', 100, '2026-09-01', '2026-09-01'),
                 ('new-task', 1, 'r', 'running', 1, '[{"output":"网络搜索 - x"}]', 50, NOW(), NOW());
               INSERT INTO federation_ring_memberships (id, ring_id, ring_type, gossip_config) VALUES
                 (1, 'r1', 'phantasi-recommend', '{"fanout":3,"phantasi_category":"生活"}'),
                 (2, 'r2', 'phantasi-recommend', '{"category":"技术","phantasi_category":"生活"}');
               INSERT INTO agent_notifications (id, notification_type, priority, title, body, user_id, read, created_at) VALUES
                 ('n-old-beat', 'heartbeat_result', 'low', '定时任务: 日报', 'ok', 1, false, '2026-09-01'),
                 ('n-old-fail', 'tapp_notification', 'high', '定时任务失败: 签到', 'e', 1, false, '2026-09-01'),
                 ('n-old-sync', 'system_info', 'high', 'bilibili 自动刷新失败', 'e', 1, false, '2026-09-01'),
                 ('n-tapp', 'tapp_notification', 'normal', '定时任务: 签到', '自动刷新失败', 1, false, NOW()),
                 ('n-new-fail', 'tapp_notification', 'high', '定时任务失败', 'e', 1, false, NOW())"#,
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
        assert_eq!(
            config("text owner"),
            serde_json::json!({"source":"runtime","installationOwnerId":1})
        );
        let registry = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT record_id, payload -> 'prompt' AS prompt FROM runtime_registry ORDER BY 1",
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row.try_get::<String>("", "record_id").unwrap(),
                    row.try_get::<serde_json::Value>("", "prompt").unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(registry.len(), 2, "{registry:?}");
        assert_eq!(registry[0].0, "answer");
        assert_eq!(registry[1], ("out".to_string(), serde_json::Value::Null));
        let messages = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT content, metadata::jsonb AS metadata, task_id FROM agent_messages
                  WHERE content <> 'nul' ORDER BY content",
            ))
            .await
            .unwrap();
        let metadata = |content: &str| {
            messages
                .iter()
                .find(|row| row.try_get::<String>("", "content").unwrap() == content)
                .map(|row| row.try_get::<serde_json::Value>("", "metadata").unwrap())
                .unwrap()
        };
        assert_eq!(metadata("old"), serde_json::json!({"message":"x"}));
        let task_ids = messages
            .iter()
            .map(|row| row.try_get::<Option<String>>("", "task_id").unwrap())
            .collect::<Vec<_>>();
        assert_eq!(task_ids, [Some("t1".to_string()), None], "current, old");
        assert_eq!(
            metadata("current"),
            serde_json::json!({"taskId":"t1","task":{"pendingQuestion":{"question":"q","questionType":"confirmation","questionId":"q1"}}})
        );
        // Rows the current code writes are never touched, whatever their text.
        let tasks = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT id FROM agent_tasks ORDER BY id",
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get::<String>("", "id").unwrap())
            .collect::<Vec<_>>();
        assert_eq!(tasks, ["new-task"]);
        let titles = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT id, title FROM agent_notifications ORDER BY id",
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row.try_get::<String>("", "id").unwrap(),
                    row.try_get::<String>("", "title").unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            titles,
            [
                ("n-new-fail".to_string(), "定时任务失败".to_string()),
                ("n-old-beat".to_string(), "Scheduled task: 日报".to_string()),
                (
                    "n-old-fail".to_string(),
                    "Scheduled task failed: 签到".to_string()
                ),
                ("n-tapp".to_string(), "定时任务: 签到".to_string()),
            ]
        );
        let rings = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT gossip_config::jsonb AS config FROM federation_ring_memberships ORDER BY id",
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get::<serde_json::Value>("", "config").unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            rings,
            [
                serde_json::json!({"fanout":3,"category":"生活"}),
                serde_json::json!({"category":"技术"}),
            ],
            "the current name wins when both are there"
        );
        // A second start finds nothing left to change.
        assert_eq!(super::rewrite_old_rows(&db).await.unwrap(), 0);
        isolated.drop().await;
    }
}
