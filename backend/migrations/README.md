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
3. `003_phantasi_system` - Phantasi sources, items, annotations, cloud note docs (including reading-state `revision` and article `content_revision`), media catalog, and friend-link / subscription applications
4. `004_agent_system` - Agent tasks, memory, notification state, and Merope persona tables
5. `005_federation` - Federation identities, messages, delivery queue, and inbox receipts
6. `006_oauth_identities` - OAuth/OIDC identity bindings, plus the user lifecycle (`user_lifecycle.sql`)

Base CREATE tables (001–006) include the current column set for greenfield installs.
The migrator has no 007+ files. Before SeaORM validates history, any
`seaql_migrations` row without a file is deleted. Fold new structure into
001–006.

**Support floor:** upgrades start from 0.6.1 or later. An older instance
upgrades to 0.6.1 first, then to the current release; the renames and heals
for anything older are gone. `Migrator::up` refuses an existing database
without the 0.6.1 schema mark (`SUPPORT_FLOOR_SCHEMA_MARK`) instead of
starting on a shape nothing heals.

Whole tables are created by Migrator (001–006) — the numbered series is the
**complete greenfield source of truth**. Runtime `schema_check` heals missing
columns and indexes generically, plus ongoing data/object jobs (platform and
config seeds, single owner, user lifecycle, the agent task status CHECK, note
docs whose article was deleted).

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

A new table or structure goes into 001–006 for new databases, and gets an
`ensure_*` heal (`CREATE IF NOT EXISTS`) for databases already past that
migration — until the support floor moves past the release that brought it.
The `_schema_versions` mark does **not** skip the safety check, and is written
only after the final drift check succeeds.

| Ongoing | Migration | schema_check |
|---------|-----------|--------------|
| user lifecycle FKs + subject-table guard / delete triggers | `006` / `user_lifecycle.sql` (down: `user_lifecycle_down.sql`) | `ensure_user_lifecycle` (orphans of the listed FKs are removed / set NULL before the constraint is added) |
| agent task status CHECK | — | `ensure_agent_tasks_status_check` |

### Removed when the floor moves past 0.6.2

These exist only to bring a 0.6.1 database to the current shape, and were
added after 0.6.1, so 0.6.2 must still carry them. Once the support floor is
0.6.2 or later (bump `SUPPORT_FLOOR_SCHEMA_MARK` to the mark that release
writes), delete them together with their tests:

- `src/db/schema_check/old_rows.rs`: rows stored in old shapes, rewritten at
  startup, and the column constraints it adds to existing databases.
- `src/services/media/retire.rs`, `REFUSE_UNFINISHED_MEDIA_UPGRADE_SQL` in
  `lib.rs` and its `MYRIAD_ACCEPT_MISSING_MEDIA` switch: the old media
  addresses and their two tables.
- The startup calls of `ConfigService::upgrade_legacy_ai_settings` and
  `retired_configuration::drop_retired_rows` (`orchestrator.rs`): old
  settings rows. Their tables and `legacy_ai_settings::upgrade` stay, because
  restoring a settings backup still reads the old keys (see below).

### Kept regardless of the floor

Inputs a newer release still receives from outside the database:

- Settings backups: `legacy_ai_settings::upgrade`, the retired-key lists and
  the old cache spelling in `media::cite` (`current_cache_spelling`,
  `is_retired_address`). Backups are kept for years; drop them by the
  backup's `product_version` when that is wanted, not by the floor.
- Third-party Tapp packages: the older category names
  (`TAPP_CATEGORY_ALIASES`); install logs say which packages still use one.
- Operator environment: `config_service::env_seed`.

Key material, also kept: `rewrap_legacy_private_keys`
(`src/federation/keys.rs`) and the `MYRIAD_MIGRATE_DATA_KEY_FROM_JWT` path in
`crates/myriad-data-key`. They guard key material; a key sealed the old way
that can no longer be opened is data or a federation identity lost for good,
and a restored backup can still carry one.

## Applying migrations

Prefer backend startup (runs `Migrator::up` automatically), or:

```bash
cargo run -p migration
```
