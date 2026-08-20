-- Federation foreign-key orphan report (READ-ONLY)
--
-- Purpose: before adding FKs to federation_* tables, measure orphan rows.
-- Never deletes or mutates data. Safe to run on production replicas.
--
-- Backend heal (conservative default):
--   • Boot always runs orphan counts and logs a summary.
--   • ADD CONSTRAINT only when MYRIAD_FEDERATION_APPLY_FKS=1 AND orphans=0.
--   • Heal never DELETE / SET NULL to "fix" orphans.
--
-- Usage:
--   psql "$DATABASE_URL" -f scripts/extra/federation-fk-orphan-report.sql
--   docker exec -i myriad-postgres-dev psql -U myriad -d myriad \
--     -f - < scripts/extra/federation-fk-orphan-report.sql
--
-- Interpretation (conservative remediation order):
--   orphans = 0  → safe to set MYRIAD_FEDERATION_APPLY_FKS=1 (or leave unconstrained)
--   orphans > 0  → do NOT enable APPLY until reviewed:
--     1) leave unconstrained (safest)
--     2) nullable columns: SET NULL after backup (keeps row)
--     3) DELETE only for confirmed dead rows after explicit review
--
-- Intentionally NOT proposed as hard FKs (see comments in schema_check):
--   • federation_timeline.activity_id → activities.activity_id
--     (inbound feed rows often store remote activity ids without a local activities row)
--   • federation_remote_actors.domain → instances.domain
--     (soft cache; domain may appear before an instances row)
--   • federation_file_transfers.channel_id → channels.channel_id
--     (room transfers use empty-string channel_id; normalize to NULL before FK)

\pset format aligned
\pset border 2

SELECT '=== existing FKs involving federation_* ===' AS section;
SELECT
  tc.table_name,
  kcu.column_name,
  ccu.table_name AS foreign_table,
  ccu.column_name AS foreign_column,
  rc.delete_rule,
  tc.constraint_name
FROM information_schema.table_constraints tc
JOIN information_schema.key_column_usage kcu
  ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
JOIN information_schema.constraint_column_usage ccu
  ON ccu.constraint_name = tc.constraint_name AND ccu.table_schema = tc.table_schema
JOIN information_schema.referential_constraints rc
  ON rc.constraint_name = tc.constraint_name AND rc.constraint_schema = tc.table_schema
WHERE tc.constraint_type = 'FOREIGN KEY'
  AND tc.table_schema = 'public'
  AND (tc.table_name LIKE 'federation_%' OR ccu.table_name LIKE 'federation_%')
ORDER BY tc.table_name, kcu.column_name;

SELECT '=== orphan counts (candidate hard FKs) ===' AS section;
WITH checks AS (
  SELECT 'federation_keys.user_id → users.id' AS pair,
         COUNT(*)::bigint AS total,
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = k.user_id))::bigint AS orphans
  FROM federation_keys k
  UNION ALL
  SELECT 'federation_follows.user_id → users.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = f.user_id))
  FROM federation_follows f
  UNION ALL
  SELECT 'federation_follows.remote_actor_id → remote_actors.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = f.remote_actor_id))
  FROM federation_follows f
  UNION ALL
  SELECT 'federation_activities.user_id → users.id (nullable)',
         COUNT(*) FILTER (WHERE user_id IS NOT NULL),
         COUNT(*) FILTER (WHERE user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = a.user_id))
  FROM federation_activities a
  UNION ALL
  SELECT 'federation_activities.remote_actor_id → remote_actors.id (nullable)',
         COUNT(*) FILTER (WHERE remote_actor_id IS NOT NULL),
         COUNT(*) FILTER (WHERE remote_actor_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = a.remote_actor_id))
  FROM federation_activities a
  UNION ALL
  SELECT 'federation_delivery_queue.activity_id → activities.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_activities a WHERE a.id = d.activity_id))
  FROM federation_delivery_queue d
  UNION ALL
  SELECT 'federation_channels.user_id → users.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = c.user_id))
  FROM federation_channels c
  UNION ALL
  SELECT 'federation_channels.remote_actor_id → remote_actors.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = c.remote_actor_id))
  FROM federation_channels c
  UNION ALL
  SELECT 'federation_channel_messages.channel_id → channels.channel_id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_channels c WHERE c.channel_id = m.channel_id))
  FROM federation_channel_messages m
  UNION ALL
  SELECT 'federation_room_members.room_id → rooms.room_id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_rooms r WHERE r.room_id = m.room_id))
  FROM federation_room_members m
  UNION ALL
  SELECT 'federation_room_members.local_user_id → users.id (nullable)',
         COUNT(*) FILTER (WHERE local_user_id IS NOT NULL),
         COUNT(*) FILTER (WHERE local_user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = m.local_user_id))
  FROM federation_room_members m
  UNION ALL
  SELECT 'federation_room_messages.room_id → rooms.room_id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_rooms r WHERE r.room_id = m.room_id))
  FROM federation_room_messages m
  UNION ALL
  SELECT 'federation_published_content.user_id → users.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = p.user_id))
  FROM federation_published_content p
  UNION ALL
  SELECT 'federation_published_content.activity_id → activities.activity_id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_activities a WHERE a.activity_id = p.activity_id))
  FROM federation_published_content p
  UNION ALL
  SELECT 'federation_timeline.user_id → users.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = t.user_id))
  FROM federation_timeline t
  UNION ALL
  SELECT 'federation_timeline.remote_actor_id → remote_actors.id (nullable)',
         COUNT(*) FILTER (WHERE remote_actor_id IS NOT NULL),
         COUNT(*) FILTER (WHERE remote_actor_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM federation_remote_actors r WHERE r.id = t.remote_actor_id))
  FROM federation_timeline t
  UNION ALL
  SELECT 'federation_file_transfers.owner_user_id → users.id (nullable)',
         COUNT(*) FILTER (WHERE owner_user_id IS NOT NULL),
         COUNT(*) FILTER (WHERE owner_user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = f.owner_user_id))
  FROM federation_file_transfers f
  UNION ALL
  SELECT 'federation_file_transfers.room_id → rooms.room_id (nullable non-empty)',
         COUNT(*) FILTER (WHERE room_id IS NOT NULL AND btrim(room_id) <> ''),
         COUNT(*) FILTER (
           WHERE room_id IS NOT NULL AND btrim(room_id) <> ''
             AND NOT EXISTS (SELECT 1 FROM federation_rooms r WHERE r.room_id = f.room_id)
         )
  FROM federation_file_transfers f
  UNION ALL
  SELECT 'federation_object_interactions.user_id → users.id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM users u WHERE u.id = o.user_id))
  FROM federation_object_interactions o
  -- Soft / deferred (reported for visibility only)
  UNION ALL
  SELECT 'SOFT federation_timeline.activity_id → activities.activity_id',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_activities a WHERE a.activity_id = t.activity_id))
  FROM federation_timeline t
  UNION ALL
  SELECT 'SOFT federation_remote_actors.domain → instances.domain',
         COUNT(*),
         COUNT(*) FILTER (WHERE NOT EXISTS (SELECT 1 FROM federation_instances i WHERE i.domain = ra.domain))
  FROM federation_remote_actors ra
  UNION ALL
  SELECT 'SOFT federation_file_transfers.channel_id → channels (non-empty only)',
         COUNT(*) FILTER (WHERE channel_id IS NOT NULL AND btrim(channel_id) <> ''),
         COUNT(*) FILTER (
           WHERE channel_id IS NOT NULL AND btrim(channel_id) <> ''
             AND NOT EXISTS (SELECT 1 FROM federation_channels c WHERE c.channel_id = f.channel_id)
         )
  FROM federation_file_transfers f
)
SELECT pair,
       total,
       orphans,
       CASE WHEN total = 0 THEN 0
            ELSE ROUND(100.0 * orphans / total, 1) END AS orphan_pct,
       CASE
         WHEN pair LIKE 'SOFT %' AND orphans > 0 THEN 'soft — do not hard-FK'
         WHEN orphans = 0 THEN 'clean — FK ok'
         ELSE 'HAS ORPHANS — decide before FK'
       END AS recommendation
FROM checks
ORDER BY
  CASE WHEN pair LIKE 'SOFT %' THEN 1 ELSE 0 END,
  orphans DESC,
  pair;
