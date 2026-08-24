# Migration CLI

Run database migrations for Myriad backend.

## Usage

```bash
# From repo root (workspace member `migration`)
cargo run -p migration

# Or from backend/
cd backend
cargo run --manifest-path migrations/Cargo.toml

# Or create a new migration
sea-orm-cli migrate generate create_new_table
```

## Available Migrations

1. `001_initial_schema` - Core users, platforms, profiles, reports, activity events, site analytics and configuration
2. `002_tapp_system` - Tapp installations, storage, widgets, quota, scheduler and shared runtime state
3. `003_brew_system` - Brew sources, items and annotations
4. `004_agent_system` - Agent tasks, memory, notification state, and Merope persona tables
5. `005_federation` - Federation identities, messages, delivery queue, and inbox receipts
6. `006_oauth_identities` - OAuth/OIDC identity bindings
7. `016_tapp_legacy_grant_clear` - Remove retired permission strings from installed TAPP rows and durably flag affected installs as needing re-authorization (`tapps.needs_reauthorization`; data cleanup for #336, not a permission mapping)

Base CREATE tables (001–006) include the current column set for greenfield installs.
`Migrator::up` deletes folded 007–015 names from `seaql_migrations` **before**
SeaORM validates history, drops leftover `digital_life_*` experiment tables
(prefix scan, local/dev only), then applies 001–006 + 016. Those names are not
kept as no-op files:

- `007_notification_preferences` → `users.notification_preferences` in 001
- `008_tapp_approved_permissions` → `tapps.approved_permissions` in 002; missing column via generic `get_expected_schema` ADD only (no dedicated backfill)
- `009_user_presence` → `users.last_seen_at` / `online_seconds` in 001 + schema_check
- `010_user_owner` / `011_owner_is_admin` → `users.is_owner` in 001 + `ensure_single_owner`
- `012_federation_inbox_receipts` / `013_federation_inbox_receipts_v2` → `federation_inbox_receipts` in 005; missing / scope-less table via `ensure_federation_inbox_receipts_table`
- `014_federation_delivery_leases` → `federation_delivery_queue.lease_token` / `lease_expires_at` + `idx_delivery_lease_expiry` in 005
- `015_federation_delivery_health` → `federation_instances.failing_since` + `idx_delivery_queue_target_domain` in 005
- `008_tapp_runtime_registry`, `009_activity_events` — also folded into 002 / 001
- `007_digital_life` / `008_digital_life_phase_two` / `009_digital_life_phase_three` /
  `010_digital_life_phase_four` / `011_digital_life_asset_subjects`：本地实验名，表已并入
  `004` 的 `agent_persona` / `agent_addressee_state` / `agent_diary` /
  `agent_proactive_messages`

Startup drops leftover `digital_life_*` experiment tables (and matching enum /
domain / composite types, plus `_schema_versions` marks). Other retired feature
tables stay. A future migration must use a new unique version name. 001–006 and
016 rows in `seaql_migrations` stay.

Whole tables are created by Migrator (001–006) — the numbered series is the
**complete greenfield source of truth**. Runtime `schema_check` only heals
**recent (~1 month) features** plus ongoing data/object jobs (platform seeds,
single owner, storage-quota trigger).

**Setup path:** `api/setup::init_database` runs `Migrator::up` then
`schema_check::ensure_schema` so first-boot seeds/heals do not require a
process restart (same order as `main` after DB connect).

**Startup policy (full mode):** migration or `ensure_schema` failure is fatal.
An instance waits for the session-scoped schema advisory lock up to a bounded
timeout, runs all repairs, then performs a final read-only drift check. It is not
marked ready and cannot serve normal traffic until all stages succeed. Environment
overrides cannot convert schema failure into readiness.

This is not a claim that every feature must always share one availability
domain. A future degraded mode is safe only after routes declare their schema
capabilities, affected routes fail with an explicit `503`, and health reports
those capabilities. Until that isolation exists, classifying drift as
"non-critical" or adding an operator bypass merely moves a deterministic
startup failure into partial writes and request-time SQL errors.

Recent tables:

| Feature | Migration | schema_check |
|---------|-----------|--------------|
| Site analytics (+ country) | `001` §8 | `ensure_analytics_tables` + TableDef |
| heartbeat_claims | `004` | `ensure_heartbeat_claims_table` + TableDef |
| content_filters / policy / domain_aliases / object_interactions / inbox_receipts | `005` 扩展段 | 对应 `ensure_*` + TableDef |
| delivery lease / health streak | `005`（原 014/015） | TableDef + generic ADD / CREATE INDEX |

Older DBs that already applied a pre-feature migration version get tables via
`ensure_*` (`CREATE IF NOT EXISTS`). The `_schema_versions` mark does **not**
skip the safety check, and is written only after the final drift check succeeds.

## Applying migrations

Prefer backend startup (runs `Migrator::up` automatically), or:

```bash
cargo run -p migration
```
