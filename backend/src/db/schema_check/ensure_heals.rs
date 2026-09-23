//! Runtime schema heal helpers (CREATE IF NOT EXISTS / structural ALTER).
//!
//! **Support floor: product ≥ 0.3.10.** Thin ADD COLUMN one-shots are not kept;
//! missing columns go through `get_expected_schema` + generic DDL.
//! Heals here: recent (~1 month) CREATE IF NOT EXISTS, unique-index cleanup,
//! federation FK report/apply, triggers, credential CHECK, inbox_scope rebuild.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};

/// `phantasi_items.topic` 的部分索引（`migrations/003` 已 CREATE）。
///
/// 不进 `get_expected_indexes`：通用索引路径生成不出 `WHERE` 子句，注册成普通
/// 索引会让新库（部分索引）与修复出来的旧库（普通索引）形状不一致。
/// 与 003 的 DDL 必须一字不差。缺列时通用 ADD COLUMN 先补，这里只管索引。
pub(crate) async fn ensure_phantasi_item_topic_index(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        "CREATE INDEX IF NOT EXISTS idx_phantasi_items_topic ON phantasi_items (topic) WHERE topic IS NOT NULL",
    )
    .await?;
    Ok(())
}

/// 阅读状态版本触发器。列走通用 ADD；触发器不进 TableDef。
pub(crate) async fn ensure_phantasi_state_revision(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
            CREATE OR REPLACE FUNCTION phantasi_advance_state_revision() RETURNS trigger AS $$
            BEGIN
                NEW.revision := OLD.revision + 1;
                RETURN NEW;
            END;
            $$ LANGUAGE plpgsql
        "#,
    )
    .await?;
    db.execute_unprepared("DROP TRIGGER IF EXISTS phantasi_state_revision ON phantasi_user_states")
        .await?;
    db.execute_unprepared("CREATE TRIGGER phantasi_state_revision BEFORE UPDATE ON phantasi_user_states FOR EACH ROW EXECUTE FUNCTION phantasi_advance_state_revision()")
        .await?;
    Ok(())
}

/// 正文版本触发器。列走通用 ADD；触发器不进 TableDef。
pub(crate) async fn ensure_phantasi_content_revision(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
            CREATE OR REPLACE FUNCTION phantasi_advance_content_revision() RETURNS trigger AS $$
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
    db.execute_unprepared("DROP TRIGGER IF EXISTS phantasi_content_revision ON phantasi_items")
        .await?;
    db.execute_unprepared("CREATE TRIGGER phantasi_content_revision BEFORE UPDATE ON phantasi_items FOR EACH ROW EXECUTE FUNCTION phantasi_advance_content_revision()")
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
"#,
    )
    .await?;
    // Deduplicate before the unique index. Keep the oldest row.
    // Missing `accept_source` is generic ADD, already applied above.
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

/// One active channel relationship per (user, remote actor, type).
pub(crate) async fn ensure_channels_active_relationship_unique(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_channels_active_relationship')
          AND indrelid = 'federation_channels'::regclass
          AND indisunique AND indisvalid
    ) THEN
        IF EXISTS (
            SELECT 1
            FROM federation_channels a
            JOIN federation_channels b
              ON a.user_id = b.user_id
             AND a.remote_actor_id = b.remote_actor_id
             AND a.channel_type = b.channel_type
             AND a.channel_id <> b.channel_id
             AND a.status IN ('pending', 'accepted', 'active')
             AND b.status IN ('pending', 'accepted', 'active')
        ) THEN
            RAISE EXCEPTION
                'active channel relationship collision across different channel_id; refusing unique index';
        END IF;
        DELETE FROM federation_channels a
        USING federation_channels b
        WHERE a.user_id = b.user_id
          AND a.remote_actor_id = b.remote_actor_id
          AND a.channel_type = b.channel_type
          AND a.channel_id = b.channel_id
          AND a.status IN ('pending', 'accepted', 'active')
          AND b.status IN ('pending', 'accepted', 'active')
          AND (
                COALESCE(a.last_activity_at, a.created_at)
                < COALESCE(b.last_activity_at, b.created_at)
             OR (
                    COALESCE(a.last_activity_at, a.created_at)
                    IS NOT DISTINCT FROM COALESCE(b.last_activity_at, b.created_at)
                AND a.id < b.id
             )
          );
        CREATE UNIQUE INDEX idx_channels_active_relationship
            ON federation_channels (user_id, remote_actor_id, channel_type)
            WHERE status IN ('pending', 'accepted', 'active');
    END IF;
END $$;
"#,
    )
    .await?;
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

/// One current snapshot per `(user_id, platform_name)`. Collapse duplicates
/// before CREATE UNIQUE so existing rows cannot fail the index.
pub(crate) async fn ensure_platform_metadata_unique(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_platform_metadata_user_platform')
          AND indrelid = 'platform_metadata'::regclass
          AND indisunique AND indisvalid
    ) THEN
        WITH ranked AS (
            SELECT id, user_id, platform_name,
                   ROW_NUMBER() OVER (
                       PARTITION BY user_id, platform_name
                       ORDER BY fetched_at DESC NULLS LAST,
                                updated_at DESC NULLS LAST,
                                id DESC
                   ) AS rn
            FROM platform_metadata
        ),
        loser AS (
            SELECT r.id AS lose_id, w.id AS keep_id
            FROM ranked r
            JOIN ranked w
              ON w.user_id = r.user_id
             AND w.platform_name = r.platform_name
             AND w.rn = 1
            WHERE r.rn > 1
        )
        UPDATE metadata_history h
           SET metadata_id = l.keep_id
          FROM loser l
         WHERE h.metadata_id = l.lose_id;

        DELETE FROM platform_metadata
         WHERE id IN (
            SELECT id FROM (
                SELECT id,
                       ROW_NUMBER() OVER (
                           PARTITION BY user_id, platform_name
                           ORDER BY fetched_at DESC NULLS LAST,
                                    updated_at DESC NULLS LAST,
                                    id DESC
                       ) AS rn
                FROM platform_metadata
            ) ranked
            WHERE rn > 1
         );
        CREATE UNIQUE INDEX idx_platform_metadata_user_platform
            ON platform_metadata (user_id, platform_name);
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
           AND tapp_id = NEW.tapp_id
           AND key <> NEW.key;
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

pub(crate) fn agent_tasks_status_check_sql() -> String {
    let values = myriad_agent_rules::TASK_STATUS_DB_VALUES
        .iter()
        .map(|status| format!("'{status}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"
DO $$
BEGIN
    IF to_regclass('agent_tasks') IS NOT NULL
       AND NOT EXISTS (
           SELECT 1
             FROM pg_constraint
            WHERE conname = 'agent_tasks_status_check'
              AND conrelid = 'agent_tasks'::regclass
       )
    THEN
        ALTER TABLE agent_tasks
            ADD CONSTRAINT agent_tasks_status_check
            CHECK (status IN ({values})) NOT VALID;
    END IF;
END $$;
"#
    )
}

/// Unknown `agent_tasks.status` values must not become executable Pending.
pub(crate) async fn ensure_agent_tasks_status_check(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(&agent_tasks_status_check_sql())
        .await?;
    Ok(())
}

/// 云端笔记文档。草稿 / 定时不进 `phantasi_items`，发布时才落文章。
pub(crate) async fn ensure_phantasi_note_docs_table(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS phantasi_note_docs (
    id SERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL,
    last_edited_by INTEGER,
    item_id INTEGER,
    title TEXT NOT NULL DEFAULT '',
    content_md TEXT NOT NULL DEFAULT '',
    topic TEXT,
    image TEXT,
    status VARCHAR NOT NULL DEFAULT 'draft',
    scheduled_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_docs_user
    ON phantasi_note_docs (user_id);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_docs_item
    ON phantasi_note_docs (item_id);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_docs_schedule
    ON phantasi_note_docs (status, scheduled_at);
-- 文章被级联删掉时文档没有外键，会留下 published + 空 item_id。
UPDATE phantasi_note_docs AS d
SET item_id = NULL,
    status = CASE WHEN d.status = 'published' THEN 'draft' ELSE d.status END,
    revision = d.revision + 1,
    updated_at = NOW()
WHERE (d.status = 'published' AND d.item_id IS NULL)
   OR (
     d.item_id IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM phantasi_items i WHERE i.id = d.item_id)
   );
"#,
    )
    .await?;
    Ok(())
}

/// 笔记联合作者。发起人是 owner，同时写入或主动加入的管理员是 author。
pub(crate) async fn ensure_phantasi_note_authors_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS phantasi_note_authors (
    doc_id INTEGER NOT NULL REFERENCES phantasi_note_docs(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL,
    role VARCHAR NOT NULL DEFAULT 'author',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (doc_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_phantasi_note_authors_user
    ON phantasi_note_authors (user_id);
INSERT INTO phantasi_note_authors (doc_id, user_id, role)
SELECT id, user_id, 'owner' FROM phantasi_note_docs
ON CONFLICT (doc_id, user_id) DO NOTHING;
"#,
    )
    .await?;
    Ok(())
}

/// 友联 / 订阅申请。旧库靠 heal 补表，不新开 folded migration。
pub(crate) async fn ensure_phantasi_source_applications_table(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
CREATE TABLE IF NOT EXISTS phantasi_source_applications (
    id SERIAL PRIMARY KEY,
    kind VARCHAR NOT NULL DEFAULT 'friend',
    status VARCHAR NOT NULL DEFAULT 'pending',
    site_name TEXT NOT NULL,
    site_url TEXT NOT NULL,
    feed_url TEXT,
    description TEXT,
    message TEXT,
    applicant_name TEXT,
    applicant_email TEXT,
    applicant_user_id INTEGER,
    applicant_ip TEXT,
    result_source_id INTEGER,
    review_note TEXT,
    reviewed_by INTEGER,
    reviewed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_phantasi_source_applications_status
    ON phantasi_source_applications (status, created_at);
CREATE INDEX IF NOT EXISTS idx_phantasi_source_applications_site_url
    ON phantasi_source_applications (site_url);
"#,
    )
    .await?;
    Ok(())
}

/// 站点上传/生成媒体目录与引用/别名/迁移进度。外链 `cache_image` 不进。
pub(crate) async fn ensure_media_assets_table(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(include_str!("../../../migrations/media_asset_model.sql"))
        .await?;
    Ok(())
}

/// Editor preferences and bounded document history, shared with greenfield DDL.
pub(crate) async fn ensure_note_editor_history(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(include_str!("../../../migrations/note_editor.sql"))
        .await?;
    Ok(())
}

/// One shared Note catalog. Partial unique is not expressible in `get_expected_indexes`.
pub(crate) async fn ensure_phantasi_note_source_unique(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
DECLARE
    kept_id integer;
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_phantasi_sources_note_type')
          AND indisunique AND indisvalid
    ) THEN
        SELECT MIN(id) INTO kept_id FROM phantasi_sources WHERE source_type = 'note';
        IF kept_id IS NOT NULL THEN
            UPDATE phantasi_items
            SET source_id = kept_id
            WHERE source_id IN (
                SELECT id FROM phantasi_sources WHERE source_type = 'note' AND id <> kept_id
            );
            IF to_regclass('phantasi_source_applications') IS NOT NULL THEN
                UPDATE phantasi_source_applications
                SET result_source_id = kept_id
                WHERE result_source_id IN (
                    SELECT id FROM phantasi_sources WHERE source_type = 'note' AND id <> kept_id
                );
            END IF;
            DELETE FROM phantasi_sources WHERE source_type = 'note' AND id <> kept_id;
        END IF;
        CREATE UNIQUE INDEX idx_phantasi_sources_note_type
            ON phantasi_sources ((true)) WHERE source_type = 'note';
    END IF;
END $$;
"#,
    )
    .await?;
    Ok(())
}

/// Global RSSHub instances use NULL user_id; a plain UNIQUE allows duplicate NULLs.
pub(crate) async fn ensure_rsshub_global_url_unique(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_rsshub_instances_global_url')
          AND indisunique AND indisvalid
    ) THEN
        WITH ranked AS (
            SELECT id, url,
                   ROW_NUMBER() OVER (
                       PARTITION BY url
                       ORDER BY CASE health_status
                                    WHEN 'healthy' THEN 0
                                    WHEN 'degraded' THEN 1
                                    WHEN 'unknown' THEN 2
                                    ELSE 3
                                END,
                                last_health_check DESC NULLS LAST,
                                updated_at DESC NULLS LAST,
                                id DESC
                   ) AS rn
            FROM rsshub_instances
            WHERE user_id IS NULL
        ),
        loser AS (
            SELECT r.id AS lose_id, w.id AS keep_id
            FROM ranked r
            JOIN ranked w ON w.url = r.url AND w.rn = 1
            WHERE r.rn > 1
        ),
        merged AS (
            UPDATE rsshub_instances w
               SET total_requests = w.total_requests + s.add_total,
                   success_requests = w.success_requests + s.add_success
              FROM (
                    SELECT l.keep_id,
                           SUM(a.total_requests)::int AS add_total,
                           SUM(a.success_requests)::int AS add_success
                      FROM loser l
                      JOIN rsshub_instances a ON a.id = l.lose_id
                     GROUP BY l.keep_id
                   ) s
             WHERE w.id = s.keep_id
         RETURNING w.id
        )
        DELETE FROM rsshub_instances
         WHERE id IN (SELECT lose_id FROM loser);
        CREATE UNIQUE INDEX idx_rsshub_instances_global_url
            ON rsshub_instances (url) WHERE user_id IS NULL;
    END IF;
END $$;
"#,
    )
    .await?;
    Ok(())
}

/// Pending friend-link applications: one canonical site/feed URL at a time.
pub(crate) async fn ensure_phantasi_application_pending_unique(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_phantasi_source_applications_pending_site')
          AND indisunique AND indisvalid
    ) THEN
        DELETE FROM phantasi_source_applications a
        USING phantasi_source_applications b
        WHERE a.status = 'pending' AND b.status = 'pending'
          AND regexp_replace(a.site_url, '/+$', '') = regexp_replace(b.site_url, '/+$', '')
          AND a.id > b.id;
        CREATE UNIQUE INDEX idx_phantasi_source_applications_pending_site
            ON phantasi_source_applications (regexp_replace(site_url, '/+$', ''))
            WHERE status = 'pending';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_phantasi_source_applications_pending_feed')
          AND indisunique AND indisvalid
    ) THEN
        DELETE FROM phantasi_source_applications a
        USING phantasi_source_applications b
        WHERE a.status = 'pending' AND b.status = 'pending'
          AND a.feed_url IS NOT NULL AND btrim(a.feed_url) <> ''
          AND b.feed_url IS NOT NULL AND btrim(b.feed_url) <> ''
          AND regexp_replace(a.feed_url, '/+$', '') = regexp_replace(b.feed_url, '/+$', '')
          AND a.id > b.id;
        CREATE UNIQUE INDEX idx_phantasi_source_applications_pending_feed
            ON phantasi_source_applications (regexp_replace(feed_url, '/+$', ''))
            WHERE status = 'pending' AND feed_url IS NOT NULL AND btrim(feed_url) <> '';
    END IF;
END $$;
"#,
    )
    .await?;
    Ok(())
}

/// Owner-wide shortcut chord uniqueness. JSON keys are not in (user,tapp,key).
pub(crate) async fn ensure_tapp_shortcut_chord_unique(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_index
        WHERE indexrelid = to_regclass('idx_tapp_shortcuts_owner_chord')
          AND indisunique AND indisvalid
    ) THEN
        IF EXISTS (
            SELECT 1
            FROM tapp_storage a
            JOIN tapp_storage b
              ON a.user_id = b.user_id
             AND a.key <> b.key
             AND starts_with(a.key, '_shortcut:')
             AND starts_with(b.key, '_shortcut:')
             AND a.value->>'keys' IS NOT NULL
             AND a.value->>'keys' = b.value->>'keys'
        ) THEN
            RAISE EXCEPTION
                'shortcut chord collision across different bindings; refusing unique index';
        END IF;
        DELETE FROM tapp_storage a
        USING tapp_storage b
        WHERE a.user_id = b.user_id
          AND a.key = b.key
          AND starts_with(a.key, '_shortcut:')
          AND starts_with(b.key, '_shortcut:')
          AND a.value->>'keys' IS NOT NULL
          AND a.value->>'keys' = b.value->>'keys'
          AND a.id > b.id;
        CREATE UNIQUE INDEX idx_tapp_shortcuts_owner_chord
            ON tapp_storage (user_id, (value->>'keys'))
            WHERE starts_with(key, '_shortcut:');
    END IF;
END $$;
"#,
    )
    .await?;
    Ok(())
}

/// Retire derived snapshots and redundant indexes; approved permissions and per-user
/// reading states remain authoritative. `users.github_id` keeps its UNIQUE constraint
/// index, so the plain `idx_users_github_id` duplicate is dropped.
pub(crate) async fn ensure_read_projection_schema(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(migration::SOURCE_RECENT_INDEX_SQL)
        .await?;
    db.execute_unprepared("CREATE INDEX IF NOT EXISTS idx_metadata_history_user_date ON metadata_history (user_id, change_date);
        DROP INDEX IF EXISTS idx_phantasi_items_published;
        DROP INDEX IF EXISTS idx_metadata_history_user;
        DROP INDEX IF EXISTS idx_users_github_id;
        ALTER TABLE tapps DROP COLUMN IF EXISTS granted_permissions;
        ALTER TABLE platform_reports DROP COLUMN IF EXISTS expires_at;
        ALTER TABLE phantasi_sources DROP COLUMN IF EXISTS unread_count;").await?;
    Ok(())
}
