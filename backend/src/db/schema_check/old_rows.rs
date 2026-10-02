//! Rows stored in shapes older than the current code, rewritten once into
//! the current shape at startup so nothing has to read the old ones. Every
//! statement is idempotent and touches nothing already current.
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
        "intentions without a known accept source",
        r#"
UPDATE agent_intentions SET accept_source = 'user'
WHERE accept_source IS DISTINCT FROM 'user' AND accept_source IS DISTINCT FROM 'autonomy'
"#,
    ),
];

/// Rewrite every old-shape row; logs what it changed.
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
}
