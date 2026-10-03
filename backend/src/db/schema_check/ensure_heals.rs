//! Runtime schema heal helpers.
//!
//! **Support floor: product ≥ 0.6.1.** Every supported database has booted
//! 0.6.1, so the heals for older shapes are gone; missing columns go through
//! `get_expected_schema` + generic DDL. Heals here are ongoing: the user
//! lifecycle, opt-in federation FKs, the agent task status CHECK (not in 004),
//! and note docs whose article was deleted.
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr};

/// The unified memory tables as 004 creates them, for database tests.
#[cfg(test)]
pub(crate) const AGENT_MEMORIES_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_memories (
    id VARCHAR(64) PRIMARY KEY,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    kind VARCHAR(16) NOT NULL,
    content TEXT NOT NULL,
    evidence TEXT,
    speaker VARCHAR(16) NOT NULL DEFAULT 'user',
    source VARCHAR(16) NOT NULL,
    venue VARCHAR(96) NOT NULL DEFAULT 'private',
    audience JSONB NOT NULL DEFAULT '[]'::jsonb,
    concepts JSONB NOT NULL DEFAULT '[]'::jsonb,
    importance DOUBLE PRECISION NOT NULL DEFAULT 0.5,
    access_count INTEGER NOT NULL DEFAULT 0,
    last_accessed_at TIMESTAMPTZ,
    valid_from TIMESTAMPTZ NOT NULL,
    invalid_at TIMESTAMPTZ,
    invalid_reason VARCHAR(16),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_memories_user_created
    ON agent_memories (user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_agent_memories_user_kind
    ON agent_memories (user_id, kind);
CREATE INDEX IF NOT EXISTS idx_agent_memories_venue_created
    ON agent_memories (venue, created_at DESC);
-- What each memory means, as a vector: recall finds a memory by what it
-- means as well as by its words. Kept apart so recall reads vectors only when
-- it uses them; `digest` is of the text embedded, so an edited memory is
-- embedded again rather than found by what it used to say.
CREATE TABLE IF NOT EXISTS agent_memory_embeddings (
    memory_id VARCHAR(64) PRIMARY KEY REFERENCES agent_memories(id) ON DELETE CASCADE,
    model VARCHAR(128) NOT NULL,
    digest VARCHAR(64) NOT NULL,
    vector BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
"#;

/// User lifecycle owned by the schema (`migrations/user_lifecycle.sql`): FK
/// cascades for account-owned rows; for subject-keyed tables an insert-side
/// guard plus an AFTER DELETE trigger on `users`. Unlike the federation candidates below this heal always applies:
/// it only removes rows whose owning user is already gone (or detaches
/// nullable references), which is what deleting that user does.
pub(crate) async fn ensure_user_lifecycle(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(include_str!("../../../migrations/user_lifecycle.sql"))
        .await?;
    Ok(())
}

/// Federation foreign keys — **conservative by default**.
///
/// # Policy (strict, no data mutation)
///
/// 005 SeaORM 主表没有 `ForeignKey::create`（本函数的候选 FK）。
/// 扩展 SQL 里 `federation_object_interactions.user_id` 已 `REFERENCES users`。
/// 指向 `users` 的归属 FK（及 channel → messages 级联）属于用户生命周期，
/// 由 [`ensure_user_lifecycle`] 默认修复，不在这里。
/// 孤儿行清理（DELETE / SET NULL）从不自动执行。
///
/// **Default (`MYRIAD_FEDERATION_APPLY_FKS` unset/false): report-only.**
/// For each candidate FK, count orphans and log whether the constraint is
/// missing. No `ALTER TABLE`. Safe for every production boot.
///
/// **Opt-in apply (`MYRIAD_FEDERATION_APPLY_FKS=1` or `true`):**
/// 1. Skip if the constraint already exists (one `pg_constraint` query for all).
/// 2. Count orphans for the missing ones (SELECT only, one UNION ALL query).
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
    let mut add_failed: u32 = 0;

    /// Which of `names` already exist as public-schema foreign keys.
    async fn existing_fk_names(
        db: &DatabaseConnection,
        names: Vec<String>,
    ) -> Result<std::collections::HashSet<String>, DbErr> {
        db.query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            r#"
SELECT c.conname::text AS name
FROM pg_constraint c
JOIN pg_namespace n ON n.oid = c.connamespace
WHERE n.nspname = 'public'
  AND c.contype = 'f'
  AND c.conname::text = ANY($1)
"#,
            [names.into()],
        ))
        .await?
        .iter()
        .map(|row| row.try_get::<String>("", "name"))
        .collect()
    }

    // One catalog round-trip for every candidate constraint name.
    let existing =
        existing_fk_names(db, FKS.iter().map(|fk| fk.name.to_string()).collect()).await?;

    let missing: Vec<&FedFk> = FKS
        .iter()
        .filter(|fk| !existing.contains(fk.name))
        .collect();
    let present = (FKS.len() - missing.len()) as u32;

    // One orphan-count round-trip for the missing ones only. Branch SQL is the
    // static `orphan_sql` above; `idx` maps each row back to `missing`.
    let mut orphan_counts: Vec<Option<i64>> = vec![None; missing.len()];
    if !missing.is_empty() {
        let sql = missing
            .iter()
            .enumerate()
            .map(|(idx, fk)| {
                format!(
                    "SELECT {idx}::int AS idx, q.orphans FROM ({}) AS q",
                    fk.orphan_sql
                )
            })
            .collect::<Vec<_>>()
            .join("\nUNION ALL\n");
        let rows = db
            .query_all_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                sql,
            ))
            .await?;
        for row in rows {
            let idx: i32 = row.try_get("", "idx")?;
            let orphans: i64 = row.try_get("", "orphans")?;
            let slot = usize::try_from(idx)
                .ok()
                .and_then(|idx| orphan_counts.get_mut(idx))
                .ok_or_else(|| DbErr::Custom(format!("unexpected orphan-count row {idx}")))?;
            *slot = Some(orphans);
        }
    }

    for (fk, orphans) in missing.into_iter().zip(orphan_counts) {
        let orphans = orphans.ok_or_else(|| {
            DbErr::Custom(format!("orphan count missing for constraint {}", fk.name))
        })?;

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
                // Another replica may have added it concurrently: that is success
                // only if this exact constraint now exists. Anything else is a
                // real failure — reported as such, but boot is not aborted for
                // an opt-in, non-critical constraint (retried next boot).
                let now_exists = existing_fk_names(db, vec![fk.name.to_string()])
                    .await
                    .map(|found| found.contains(fk.name))
                    .unwrap_or(false);
                if now_exists {
                    tracing::info!(
                        constraint = fk.name,
                        "Federation FK added concurrently by another instance"
                    );
                } else {
                    add_failed += 1;
                    tracing::error!(
                        constraint = fk.name,
                        error = %e,
                        "Failed to add federation FK (not applied; will retry next boot \
                         if MYRIAD_FEDERATION_APPLY_FKS is still set)"
                    );
                }
            }
        }
    }

    if add_failed > 0 {
        tracing::error!(
            present,
            missing_clean,
            missing_orphans,
            add_failed,
            apply,
            "Federation FK heal incomplete: some constraints could not be added"
        );
    } else {
        tracing::info!(
            present,
            missing_clean,
            missing_orphans,
            apply,
            "Federation FK heal summary (conservative: no orphan cleanup)"
        );
    }

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

/// Note docs have no foreign key to their article: one deleted with its
/// source leaves a published doc pointing nowhere. Back to draft, unlinked.
pub(crate) async fn unlink_note_docs_without_article(db: &DatabaseConnection) -> Result<(), DbErr> {
    db.execute_unprepared(
        r#"
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
