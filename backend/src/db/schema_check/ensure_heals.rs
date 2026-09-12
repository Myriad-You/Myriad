//! Runtime schema heal helpers (CREATE IF NOT EXISTS / structural ALTER).
//!
//! **Support floor: product ≥ 0.3.10.** Thin ADD COLUMN one-shots are not kept;
//! missing columns go through `get_expected_schema` + generic DDL.
//! Heals here: CREATE IF NOT EXISTS, unique-index cleanup, analytics `target` PK,
//! federation FK report/apply, triggers, credential CHECK, inbox_scope rebuild,
//! retired-report DELETE.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};

/// `brew_items.topic` 的部分索引（`migrations/003` 已 CREATE）。
///
/// 不进 `get_expected_indexes`：通用索引路径生成不出 `WHERE` 子句，注册成普通
/// 索引会让新库（部分索引）与修复出来的旧库（普通索引）形状不一致。
/// 与 003 的 DDL 必须一字不差。缺列时通用 ADD COLUMN 先补，这里只管索引。
pub(crate) async fn ensure_brew_item_topic_index(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        "CREATE INDEX IF NOT EXISTS idx_brew_items_topic ON brew_items (topic) WHERE topic IS NOT NULL",
    )
    .await?;
    Ok(())
}

/// 阅读状态版本触发器。列走通用 ADD；触发器不进 TableDef。
pub(crate) async fn ensure_brew_state_revision(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
            CREATE OR REPLACE FUNCTION brew_advance_state_revision() RETURNS trigger AS $$
            BEGIN
                NEW.revision := OLD.revision + 1;
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql
        "#,
    )
    .await?;
    db.execute_unprepared("DROP TRIGGER IF EXISTS brew_state_revision ON brew_user_states")
        .await?;
    db.execute_unprepared("CREATE TRIGGER brew_state_revision BEFORE UPDATE ON brew_user_states FOR EACH ROW EXECUTE FUNCTION brew_advance_state_revision()")
        .await?;
    Ok(())
}

/// 正文版本触发器。列走通用 ADD；触发器不进 TableDef。
pub(crate) async fn ensure_brew_content_revision(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
            CREATE OR REPLACE FUNCTION brew_advance_content_revision() RETURNS trigger AS $$
            BEGIN
                IF NEW.content IS DISTINCT FROM OLD.content
                    OR NEW.content_md IS DISTINCT FROM OLD.content_md THEN
                    NEW.content_revision := OLD.content_revision + 1;
                ELSE
                    NEW.content_revision := OLD.content_revision;
                END IF;
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql
        "#,
    )
    .await?;
    db.execute_unprepared("DROP TRIGGER IF EXISTS brew_content_revision ON brew_items")
        .await?;
    db.execute_unprepared("CREATE TRIGGER brew_content_revision BEFORE UPDATE ON brew_items FOR EACH ROW EXECUTE FUNCTION brew_advance_content_revision()")
        .await?;
    Ok(())
}

/// 近月功能表兜底（`migrations/005` 已 CREATE）。
pub(crate) async fn ensure_federation_content_filters_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS federation_content_filters (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    filter_type VARCHAR NOT NULL,
    value TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
"#,
    )
    .await?;
    Ok(())
}

/// 近月功能表兜底（`migrations/005` 已 CREATE）。缺列由 `get_expected_schema` 通用 ADD。
/// 字段级对齐不再为 <0.3.10 单独维护。
pub(crate) async fn ensure_federation_policy_settings_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS federation_policy_settings (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    min_trust_level SMALLINT NOT NULL DEFAULT 0,
    allowed_domains JSONB NOT NULL DEFAULT '[]'::jsonb,
    auto_discover BOOLEAN NOT NULL DEFAULT true,
    rate_max_requests BIGINT NOT NULL DEFAULT 100,
    rate_window_seconds BIGINT NOT NULL DEFAULT 60,
    rate_trusted_multiplier BIGINT NOT NULL DEFAULT 5,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO federation_policy_settings (id) VALUES (1)
ON CONFLICT (id) DO NOTHING;
"#,
    )
    .await?;
    Ok(())
}

/// 近月功能表兜底（`migrations/004` 已 CREATE）。Agent 人设四表。
pub(crate) async fn ensure_agent_merope_tables(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS agent_persona (
    id VARCHAR(16) PRIMARY KEY,
    name TEXT NOT NULL DEFAULT '',
    personality TEXT NOT NULL DEFAULT '',
    persona_json JSONB,
    visual_profile JSONB,
    portrait_asset_id TEXT,
    portrait_generation JSONB,
    avatar_asset_id TEXT,
    avatar_generation JSONB,
    updated_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_addressee_state (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    mood DOUBLE PRECISION NOT NULL DEFAULT 70,
    arousal DOUBLE PRECISION NOT NULL DEFAULT 50,
    emotion DOUBLE PRECISION NOT NULL DEFAULT 50,
    emotion_arousal DOUBLE PRECISION NOT NULL DEFAULT 50,
    activity VARCHAR(16) NOT NULL DEFAULT 'idle',
    activity_updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    do_not_disturb BOOLEAN NOT NULL DEFAULT false,
    dnd_start_minute INTEGER,
    dnd_end_minute INTEGER,
    last_user_message_at TIMESTAMPTZ,
    last_proactive_at TIMESTAMPTZ,
    music_mood_credited_at TIMESTAMPTZ,
    mood_settled_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    emotion_settled_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_diary (
    id VARCHAR(64) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    source VARCHAR(16) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_diary_user_created
    ON agent_diary (user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS agent_proactive_messages (
    id BIGSERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(16) NOT NULL,
    content TEXT NOT NULL,
    event_key VARCHAR(64),
    notified BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_proactive_user_created
    ON agent_proactive_messages (user_id, created_at DESC);
"#,
    )
    .await?;
    Ok(())
}

/// 近月功能表兜底（`migrations/004` 已 CREATE）。
pub(crate) async fn ensure_heartbeat_claims_table(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS heartbeat_claims (
    task_id VARCHAR(128) NOT NULL,
    minute_bucket BIGINT NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'running',
    claimed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    PRIMARY KEY (task_id, minute_bucket)
);
CREATE INDEX IF NOT EXISTS idx_heartbeat_claims_claimed_at
    ON heartbeat_claims (claimed_at);
"#,
    )
    .await?;
    Ok(())
}

/// 近期 Agent 意图账本兜底（`migrations/004` 已 CREATE）。
pub(crate) async fn ensure_agent_intentions_table(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS agent_intentions (
    id VARCHAR(64) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_event_id VARCHAR(128) NOT NULL,
    summary TEXT NOT NULL,
    reason_code VARCHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'proposed',
    proposal JSONB NOT NULL,
    work_session_id VARCHAR(64),
    work_run_id VARCHAR(64),
    result_summary TEXT,
    expires_at TIMESTAMPTZ,
    accept_source VARCHAR(16) NOT NULL DEFAULT 'user',
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_intentions_user_status
    ON agent_intentions (user_id, status);
CREATE INDEX IF NOT EXISTS idx_agent_intentions_user_updated
    ON agent_intentions (user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_intentions_source_event
    ON agent_intentions (source_event_id);
ALTER TABLE agent_intentions
    ADD COLUMN IF NOT EXISTS accept_source VARCHAR(16) NOT NULL DEFAULT 'user';
"#,
    )
    .await?;
    // Unique (user_id, source_event_id) is a separate statement so a pre-heal
    // duplicate row cannot roll back accept_source. Keep the oldest row.
    let removed = db
        .execute_unprepared(
            r#"
DELETE FROM agent_intentions a
USING agent_intentions b
WHERE a.user_id = b.user_id
  AND a.source_event_id = b.source_event_id
  AND (
    a.created_at > b.created_at
    OR (a.created_at = b.created_at AND a.id > b.id)
  );
"#,
        )
        .await?;
    if removed.rows_affected() > 0 {
        tracing::info!(
            "🧹 Removed {} duplicate agent_intentions row(s) before unique source-event index",
            removed.rows_affected()
        );
    }
    db.execute_unprepared(
        r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_intentions_user_source_event
    ON agent_intentions (user_id, source_event_id);
"#,
    )
    .await?;
    Ok(())
}

/// 近期 Agent 个人自主授权账本兜底（`migrations/004` 已 CREATE）。
pub(crate) async fn ensure_agent_autonomy_grants_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS agent_autonomy_grants (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    allowed_permissions JSONB NOT NULL DEFAULT '[]'::jsonb,
    revoked BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
"#,
    )
    .await?;
    Ok(())
}

/// First-party site analytics tables.
///
/// **权威建表**：`migrations/001_initial_schema.rs` §8（新库 Migrator）。
/// 与 001 §8 同结构（列 / PK / 索引），作表尚不存在时的幂等 CREATE 兜底。
/// 普通缺列（engagement / ordinal 等）走 `get_expected_schema` 通用 ADD，
/// 不再为 <0.3.10 或中间过渡形态维护逐列 ALTER。
///
/// 仅保留 **event `target` 维度** 的列补齐 + PK 重建（通用 ADD 无法改主键）。
pub(crate) async fn ensure_analytics_tables(db: &DatabaseConnection) -> Result<(), DbErr> {
    // 须与 migrations/001_initial_schema.rs §8 SITE ANALYTICS 保持同步
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS analytics_page_daily (
    day DATE NOT NULL,
    path TEXT NOT NULL,
    views BIGINT NOT NULL DEFAULT 0,
    unique_visitors BIGINT NOT NULL DEFAULT 0,
    engagement_ms BIGINT NOT NULL DEFAULT 0,
    engaged_views BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, path)
);
CREATE INDEX IF NOT EXISTS idx_analytics_page_daily_day
    ON analytics_page_daily (day);

CREATE TABLE IF NOT EXISTS analytics_visitor_seen (
    day DATE NOT NULL,
    path TEXT NOT NULL,
    visitor_hash VARCHAR(64) NOT NULL,
    ordinal BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, path, visitor_hash)
);
CREATE INDEX IF NOT EXISTS idx_analytics_visitor_seen_day
    ON analytics_visitor_seen (day);

CREATE TABLE IF NOT EXISTS analytics_event_daily (
    day DATE NOT NULL,
    event_name TEXT NOT NULL,
    path TEXT NOT NULL DEFAULT '',
    target TEXT NOT NULL DEFAULT '',
    count BIGINT NOT NULL DEFAULT 0,
    unique_visitors BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, event_name, path, target)
);
CREATE INDEX IF NOT EXISTS idx_analytics_event_daily_day
    ON analytics_event_daily (day);

CREATE TABLE IF NOT EXISTS analytics_event_visitor (
    day DATE NOT NULL,
    event_name TEXT NOT NULL,
    path TEXT NOT NULL DEFAULT '',
    target TEXT NOT NULL DEFAULT '',
    visitor_hash VARCHAR(64) NOT NULL,
    PRIMARY KEY (day, event_name, path, target, visitor_hash)
);
CREATE INDEX IF NOT EXISTS idx_analytics_event_visitor_day
    ON analytics_event_visitor (day);

CREATE TABLE IF NOT EXISTS analytics_referrer_daily (
    day DATE NOT NULL,
    host TEXT NOT NULL,
    count BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, host)
);
CREATE INDEX IF NOT EXISTS idx_analytics_referrer_daily_day
    ON analytics_referrer_daily (day);

CREATE TABLE IF NOT EXISTS analytics_country_daily (
    day DATE NOT NULL,
    country_code VARCHAR(8) NOT NULL,
    country_name TEXT NOT NULL DEFAULT '',
    views BIGINT NOT NULL DEFAULT 0,
    unique_visitors BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (day, country_code)
);
CREATE INDEX IF NOT EXISTS idx_analytics_country_daily_day
    ON analytics_country_daily (day);

CREATE TABLE IF NOT EXISTS analytics_country_visitor (
    day DATE NOT NULL,
    country_code VARCHAR(8) NOT NULL,
    visitor_hash VARCHAR(64) NOT NULL,
    PRIMARY KEY (day, country_code, visitor_hash)
);
CREATE INDEX IF NOT EXISTS idx_analytics_country_visitor_day
    ON analytics_country_visitor (day);
"#,
    )
    .await?;
    // target 列（post-0.3.10）：缺列时通用 ADD 也会补，但 PK 扩维必须显式 heal。
    for stmt in [
        "ALTER TABLE analytics_event_daily ADD COLUMN IF NOT EXISTS target TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE analytics_event_visitor ADD COLUMN IF NOT EXISTS target TEXT NOT NULL DEFAULT ''",
    ] {
        db.execute_unprepared(stmt).await?;
    }
    // 旧 PK (day, event_name, path) → 含 target。新建表已是新 PK；失败则忽略。
    for stmt in [
        r#"
DO $heal$
BEGIN
  IF EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'analytics_event_daily_pkey'
      AND conrelid = 'analytics_event_daily'::regclass
  ) THEN
    ALTER TABLE analytics_event_daily DROP CONSTRAINT analytics_event_daily_pkey;
  END IF;
  ALTER TABLE analytics_event_daily
    ADD CONSTRAINT analytics_event_daily_pkey
    PRIMARY KEY (day, event_name, path, target);
EXCEPTION
  WHEN duplicate_object OR invalid_table_definition OR unique_violation THEN
    NULL;
END
$heal$;
"#,
        r#"
DO $heal$
BEGIN
  IF EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'analytics_event_visitor_pkey'
      AND conrelid = 'analytics_event_visitor'::regclass
  ) THEN
    ALTER TABLE analytics_event_visitor DROP CONSTRAINT analytics_event_visitor_pkey;
  END IF;
  ALTER TABLE analytics_event_visitor
    ADD CONSTRAINT analytics_event_visitor_pkey
    PRIMARY KEY (day, event_name, path, target, visitor_hash);
EXCEPTION
  WHEN duplicate_object OR invalid_table_definition OR unique_violation THEN
    NULL;
END
$heal$;
"#,
    ] {
        if let Err(e) = db.execute_unprepared(stmt).await {
            tracing::warn!("analytics event target PK heal: {}", e);
        }
    }
    Ok(())
}

/// 投递队列去重：`(activity_id, target_inbox)` 唯一索引。
/// Only a missing unique index needs data cleanup. DELETE and CREATE are atomic;
/// an existing valid unique index skips the table scan entirely.
pub(crate) async fn ensure_delivery_queue_unique(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_delivery_queue_activity_target')
          AND indrelid = 'federation_delivery_queue'::regclass
          AND indisunique AND indisvalid
    ) THEN
        DELETE FROM federation_delivery_queue a
        USING federation_delivery_queue b
        WHERE a.activity_id = b.activity_id AND a.target_inbox = b.target_inbox AND a.id > b.id;
        CREATE UNIQUE INDEX idx_delivery_queue_activity_target
            ON federation_delivery_queue (activity_id, target_inbox);
    END IF;
END $$;
"#,
    )
    .await?;
    Ok(())
}

/// Federation foreign keys — **conservative by default**.
///
/// # Policy (strict, no data mutation)
///
/// 005 SeaORM 主表没有 `ForeignKey::create`（本函数的候选 FK）。
/// 扩展 SQL 里 `federation_object_interactions.user_id` 已 `REFERENCES users`。
/// 孤儿行清理（DELETE / SET NULL）从不自动执行。
///
/// **Default (`MYRIAD_FEDERATION_APPLY_FKS` unset/false): report-only.**
/// For each candidate FK, count orphans and log whether the constraint is
/// missing. No `ALTER TABLE`. Safe for every production boot.
///
/// **Opt-in apply (`MYRIAD_FEDERATION_APPLY_FKS=1` or `true`):**
/// 1. Skip if the constraint already exists.
/// 2. Count orphans (SELECT only).
/// 3. If orphans > 0 → warn and **skip** (still no DELETE / SET NULL).
/// 4. If orphans = 0 → `ALTER TABLE … ADD CONSTRAINT`.
///
/// Operators: run `scripts/extra/federation-fk-orphan-report.sql` on a replica
/// first. If orphans > 0, decide manually — preferred conservative remediations:
/// - nullable columns → `SET NULL` (keeps the row)
/// - dead rows with no business value → `DELETE` only after explicit review
/// - unsure → leave unconstrained
///
/// # Not constrained (by design)
///
/// - `federation_timeline.activity_id` → activities (inbound feed often has no local activity row)
/// - `federation_remote_actors.domain` → instances (soft discovery cache)
/// - `federation_file_transfers.channel_id` → channels (room transfers use `''` channel_id)
pub(crate) async fn ensure_federation_foreign_keys(db: &DatabaseConnection) -> Result<(), DbErr> {
    /// One candidate FK. `orphan_sql` must return a single bigint column `orphans`.
    struct FedFk {
        name: &'static str,
        orphan_sql: &'static str,
        add_sql: &'static str,
    }

    let apply = matches!(
        std::env::var("MYRIAD_FEDERATION_APPLY_FKS")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    );
    if !apply {
        tracing::info!(
            "Federation FK heal is report-only \
             (set MYRIAD_FEDERATION_APPLY_FKS=1 to add constraints when orphan-free). \
             See scripts/extra/federation-fk-orphan-report.sql"
        );
    }

    // ON DELETE:
    // - CASCADE for ownership / membership / messages when parent is gone
    // - SET NULL for optional references (nullable columns)
    const FKS: &[FedFk] = &[
        FedFk {
            name: "fk_fed_keys_user",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_keys k
WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = k.user_id)"#,
            add_sql: r#"
ALTER TABLE federation_keys
  ADD CONSTRAINT fk_fed_keys_user
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_follows_user",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_follows f
WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = f.user_id)"#,
            add_sql: r#"
ALTER TABLE federation_follows
  ADD CONSTRAINT fk_fed_follows_user
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_follows_remote_actor",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_follows f
WHERE NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = f.remote_actor_id)"#,
            add_sql: r#"
ALTER TABLE federation_follows
  ADD CONSTRAINT fk_fed_follows_remote_actor
  FOREIGN KEY (remote_actor_id) REFERENCES federation_remote_actors(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_activities_user",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_activities a
WHERE a.user_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = a.user_id)"#,
            add_sql: r#"
ALTER TABLE federation_activities
  ADD CONSTRAINT fk_fed_activities_user
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL"#,
        },
        FedFk {
            name: "fk_fed_activities_remote_actor",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_activities a
WHERE a.remote_actor_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = a.remote_actor_id)"#,
            add_sql: r#"
ALTER TABLE federation_activities
  ADD CONSTRAINT fk_fed_activities_remote_actor
  FOREIGN KEY (remote_actor_id) REFERENCES federation_remote_actors(id) ON DELETE SET NULL"#,
        },
        FedFk {
            name: "fk_fed_delivery_activity",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_delivery_queue d
WHERE NOT EXISTS (SELECT 1 FROM federation_activities a WHERE a.id = d.activity_id)"#,
            add_sql: r#"
ALTER TABLE federation_delivery_queue
  ADD CONSTRAINT fk_fed_delivery_activity
  FOREIGN KEY (activity_id) REFERENCES federation_activities(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_channels_user",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_channels c
WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = c.user_id)"#,
            add_sql: r#"
ALTER TABLE federation_channels
  ADD CONSTRAINT fk_fed_channels_user
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_channels_remote_actor",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_channels c
WHERE NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = c.remote_actor_id)"#,
            add_sql: r#"
ALTER TABLE federation_channels
  ADD CONSTRAINT fk_fed_channels_remote_actor
  FOREIGN KEY (remote_actor_id) REFERENCES federation_remote_actors(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_channel_messages_channel",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_channel_messages m
WHERE NOT EXISTS (SELECT 1 FROM federation_channels c WHERE c.channel_id = m.channel_id)"#,
            add_sql: r#"
ALTER TABLE federation_channel_messages
  ADD CONSTRAINT fk_fed_channel_messages_channel
  FOREIGN KEY (channel_id) REFERENCES federation_channels(channel_id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_room_members_room",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_room_members m
WHERE NOT EXISTS (SELECT 1 FROM federation_rooms r WHERE r.room_id = m.room_id)"#,
            add_sql: r#"
ALTER TABLE federation_room_members
  ADD CONSTRAINT fk_fed_room_members_room
  FOREIGN KEY (room_id) REFERENCES federation_rooms(room_id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_room_members_local_user",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_room_members m
WHERE m.local_user_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = m.local_user_id)"#,
            add_sql: r#"
ALTER TABLE federation_room_members
  ADD CONSTRAINT fk_fed_room_members_local_user
  FOREIGN KEY (local_user_id) REFERENCES users(id) ON DELETE SET NULL"#,
        },
        FedFk {
            name: "fk_fed_room_messages_room",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_room_messages m
WHERE NOT EXISTS (SELECT 1 FROM federation_rooms r WHERE r.room_id = m.room_id)"#,
            add_sql: r#"
ALTER TABLE federation_room_messages
  ADD CONSTRAINT fk_fed_room_messages_room
  FOREIGN KEY (room_id) REFERENCES federation_rooms(room_id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_published_user",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_published_content p
WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = p.user_id)"#,
            add_sql: r#"
ALTER TABLE federation_published_content
  ADD CONSTRAINT fk_fed_published_user
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_published_activity",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_published_content p
WHERE NOT EXISTS (SELECT 1 FROM federation_activities a WHERE a.activity_id = p.activity_id)"#,
            add_sql: r#"
ALTER TABLE federation_published_content
  ADD CONSTRAINT fk_fed_published_activity
  FOREIGN KEY (activity_id) REFERENCES federation_activities(activity_id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_timeline_user",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_timeline t
WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = t.user_id)"#,
            add_sql: r#"
ALTER TABLE federation_timeline
  ADD CONSTRAINT fk_fed_timeline_user
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE"#,
        },
        FedFk {
            name: "fk_fed_timeline_remote_actor",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_timeline t
WHERE t.remote_actor_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = t.remote_actor_id)"#,
            add_sql: r#"
ALTER TABLE federation_timeline
  ADD CONSTRAINT fk_fed_timeline_remote_actor
  FOREIGN KEY (remote_actor_id) REFERENCES federation_remote_actors(id) ON DELETE SET NULL"#,
        },
        FedFk {
            name: "fk_fed_file_transfers_owner",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_file_transfers f
WHERE f.owner_user_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = f.owner_user_id)"#,
            add_sql: r#"
ALTER TABLE federation_file_transfers
  ADD CONSTRAINT fk_fed_file_transfers_owner
  FOREIGN KEY (owner_user_id) REFERENCES users(id) ON DELETE SET NULL"#,
        },
        FedFk {
            name: "fk_fed_file_transfers_room",
            orphan_sql: r#"
SELECT COUNT(*)::bigint AS orphans FROM federation_file_transfers f
WHERE f.room_id IS NOT NULL AND btrim(f.room_id) <> ''
  AND NOT EXISTS (SELECT 1 FROM federation_rooms r WHERE r.room_id = f.room_id)"#,
            add_sql: r#"
ALTER TABLE federation_file_transfers
  ADD CONSTRAINT fk_fed_file_transfers_room
  FOREIGN KEY (room_id) REFERENCES federation_rooms(room_id) ON DELETE SET NULL"#,
        },
    ];

    let mut missing_clean: u32 = 0;
    let mut missing_orphans: u32 = 0;
    let mut present: u32 = 0;

    for fk in FKS {
        let exists = db
            .query_one_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                r#"
SELECT 1 AS ok
FROM information_schema.table_constraints
WHERE table_schema = 'public'
  AND constraint_type = 'FOREIGN KEY'
  AND constraint_name = $1
LIMIT 1
"#,
                vec![fk.name.into()],
            ))
            .await?;
        if exists.is_some() {
            present += 1;
            continue;
        }

        let orphan_row = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                fk.orphan_sql.to_string(),
            ))
            .await?;
        let orphans: i64 = orphan_row
            .as_ref()
            .and_then(|r| r.try_get::<i64>("", "orphans").ok())
            .unwrap_or(0);

        if orphans > 0 {
            missing_orphans += 1;
            tracing::warn!(
                constraint = fk.name,
                orphans,
                "Federation FK missing and has orphan rows — not applying \
                 (heal never deletes). Run scripts/extra/federation-fk-orphan-report.sql; \
                 prefer SET NULL on nullable columns over DELETE"
            );
            continue;
        }

        missing_clean += 1;
        if !apply {
            tracing::info!(
                constraint = fk.name,
                "Federation FK missing, orphan-free — ready to add when \
                 MYRIAD_FEDERATION_APPLY_FKS=1"
            );
            continue;
        }

        match db.execute_unprepared(fk.add_sql).await {
            Ok(_) => {
                tracing::info!(constraint = fk.name, "✅ Added federation foreign key");
            }
            Err(e) => {
                // Race with another replica, or unexpected schema: do not fail boot.
                tracing::warn!(
                    constraint = fk.name,
                    error = %e,
                    "Could not add federation FK (will retry next boot if APPLY still set)"
                );
            }
        }
    }

    tracing::info!(
        present,
        missing_clean,
        missing_orphans,
        apply,
        "Federation FK heal summary (conservative: no orphan cleanup)"
    );

    Ok(())
}

/// 时间线去重：`(user_id, activity_id)` 唯一索引。同一用户同一 activity_id 只留一条。
/// Skip cleanup when the valid unique index already enforces this invariant.
pub(crate) async fn ensure_timeline_unique(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_timeline_user_activity')
          AND indrelid = 'federation_timeline'::regclass
          AND indisunique AND indisvalid
    ) THEN
        DELETE FROM federation_timeline a
        USING federation_timeline b
        WHERE a.user_id = b.user_id AND a.activity_id = b.activity_id AND a.id > b.id;
        CREATE UNIQUE INDEX idx_timeline_user_activity
            ON federation_timeline (user_id, activity_id);
    END IF;
END $$;
"#,
    )
    .await?;
    Ok(())
}

/// 近月功能表兜底（`migrations/005` 已 CREATE）。
pub(crate) async fn ensure_federation_domain_aliases_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS federation_domain_aliases (
    id SERIAL PRIMARY KEY,
    old_base_url TEXT NOT NULL UNIQUE,
    new_base_url TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_federation_domain_aliases_new
    ON federation_domain_aliases (new_base_url);
"#,
    )
    .await?;
    Ok(())
}

/// 退休跨平台综合报告：历史行 `platform = 'all'` 已无生成/读取产品路径。
///
/// API 侧已用 `platform.ne("all")` 过滤，这里做一次幂等物理清理，避免库内残留
/// 被 federation content 导出或手工 SQL 重新暴露。
pub(crate) async fn cleanup_retired_comprehensive_reports(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DELETE FROM platform_reports WHERE platform = 'all';
"#,
    )
    .await?;
    Ok(())
}

/// 近月功能表兜底（`migrations/005` 已 CREATE）。
pub(crate) async fn ensure_federation_object_interactions_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS federation_object_interactions (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    object_id TEXT NOT NULL,
    kind VARCHAR(20) NOT NULL,
    activity_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT federation_object_interactions_kind_check
        CHECK (kind IN ('like', 'bookmark', 'announce')),
    CONSTRAINT federation_object_interactions_unique
        UNIQUE (user_id, object_id, kind)
);
CREATE INDEX IF NOT EXISTS idx_fed_interactions_object_kind
    ON federation_object_interactions (object_id, kind);
CREATE INDEX IF NOT EXISTS idx_fed_interactions_user_kind_created
    ON federation_object_interactions (user_id, kind, created_at DESC);
"#,
    )
    .await?;
    Ok(())
}

/// Inbox receipt table (authoritative CREATE is `migrations/005`).
///
/// Old DBs that already applied 005 get `CREATE IF NOT EXISTS`. Review DBs that
/// recorded the short-lived scope-less 012 shape are rebuilt here — generic ADD
/// cannot introduce `inbox_scope` into that primary key.
pub(crate) async fn ensure_federation_inbox_receipts_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF to_regclass('federation_inbox_receipts') IS NOT NULL
       AND NOT EXISTS (
           SELECT 1
           FROM information_schema.columns
           WHERE table_schema = current_schema()
             AND table_name = 'federation_inbox_receipts'
             AND column_name = 'inbox_scope'
       )
    THEN
        DROP TABLE federation_inbox_receipts;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS federation_inbox_receipts (
    signer TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    inbox_scope TEXT NOT NULL,
    body_digest CHAR(64) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'processing',
    outcome_status SMALLINT,
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT federation_inbox_receipts_status_check
        CHECK (status IN ('processing', 'accepted', 'rejected')),
    CONSTRAINT federation_inbox_receipts_digest_check
        CHECK (body_digest ~ '^[0-9a-f]{64}$'),
    CONSTRAINT federation_inbox_receipts_pkey
        PRIMARY KEY (signer, activity_id, inbox_scope)
);
"#,
    )
    .await?;
    Ok(())
}

pub(crate) async fn ensure_tapp_storage_quota(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE OR REPLACE FUNCTION enforce_tapp_storage_quota()
RETURNS TRIGGER AS $$
DECLARE
    current_bytes BIGINT;
    projected_bytes BIGINT;
BEGIN
    PERFORM pg_advisory_xact_lock(
        hashtextextended('tapp-storage:' || NEW.user_id::text || ':' || NEW.tapp_id, 0)
    );

    IF TG_OP = 'UPDATE' THEN
        SELECT COALESCE(SUM(
                   octet_length(key)
                   + octet_length(value::text)
                   + COALESCE(octet_length(encrypted_value), 0)
               ), 0)::BIGINT
          INTO current_bytes
          FROM tapp_storage
         WHERE user_id = NEW.user_id
           AND tapp_id = NEW.tapp_id
           AND id <> OLD.id;
    ELSE
        SELECT COALESCE(SUM(
                   octet_length(key)
                   + octet_length(value::text)
                   + COALESCE(octet_length(encrypted_value), 0)
               ), 0)::BIGINT
          INTO current_bytes
          FROM tapp_storage
         WHERE user_id = NEW.user_id
           AND tapp_id = NEW.tapp_id;
    END IF;

    projected_bytes := current_bytes
        + octet_length(NEW.key)
        + octet_length(NEW.value::text)
        + COALESCE(octet_length(NEW.encrypted_value), 0);
    IF projected_bytes > 8388608 THEN
        RAISE EXCEPTION 'Tapp storage quota exceeded: % bytes', projected_bytes
            USING ERRCODE = '54000';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM pg_trigger
         WHERE tgname = 'trg_tapp_storage_quota'
           AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER trg_tapp_storage_quota
        BEFORE INSERT OR UPDATE OF key, value, encrypted_value, user_id, tapp_id ON tapp_storage
        FOR EACH ROW EXECUTE FUNCTION enforce_tapp_storage_quota();
    END IF;
END $$;
"#,
    )
    .await?;

    Ok(())
}

/// Keep host-only credential columns structurally bound to the reserved key
/// namespace even though credentials intentionally reuse `tapp_storage`.
pub(crate) async fn ensure_tapp_storage_credential_constraint(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM pg_constraint
         WHERE conname = 'tapp_storage_credential_fields_check'
           AND conrelid = 'tapp_storage'::regclass
    ) THEN
        ALTER TABLE tapp_storage
            ADD CONSTRAINT tapp_storage_credential_fields_check
            CHECK (
                (encrypted_value IS NULL AND binding_fingerprint IS NULL)
                OR (
                    starts_with(key, '_credentials.')
                    AND encrypted_value IS NOT NULL
                    AND binding_fingerprint IS NOT NULL
                )
            ) NOT VALID;
    END IF;
END $$;

ALTER TABLE tapp_storage
    VALIDATE CONSTRAINT tapp_storage_credential_fields_check;
"#,
    )
    .await?;
    Ok(())
}
