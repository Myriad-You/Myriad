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
The migrator has no 007+ files. Before SeaORM validates history, leftover
`digital_life_*` experiment tables (and matching enum / domain / composite
types, plus `_schema_versions` marks) are dropped by prefix, and any
`seaql_migrations` row without a file is deleted. Fold new structure into
001–006.

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
| agent_persona / addressee / diary / proactive | `004` | `ensure_agent_merope_tables` + TableDef |
| agent_intentions | `004` | `ensure_agent_intentions_table` + TableDef |
| agent_intentions unique source event | `004` | `ensure_agent_intentions_table` unique index |
| agent_autonomy_grants | `004` | `ensure_agent_autonomy_grants_table` + TableDef |
| inbox_receipts | `005` | `ensure_federation_inbox_receipts_table` + TableDef |
| phantasi_note_docs | `003` | `ensure_phantasi_note_docs_table` + TableDef |
| phantasi_note_authors | `003` | `ensure_phantasi_note_authors_table` + TableDef |
| phantasi_source_applications | `003` | `ensure_phantasi_source_applications_table` + TableDef |
| media_assets | `003` / `media_asset_model.sql` | `ensure_media_assets_table` + TableDef |
| media_references / media_url_aliases / media_migration_jobs | `003` / `media_asset_model.sql` | `ensure_media_assets_table` + TableDef |
| note editor history | `003` / `note_editor.sql` | `ensure_note_editor_history` + TableDef |
| user lifecycle FKs + subject-row delete trigger | `006` / `user_lifecycle.sql` | `ensure_user_lifecycle` (orphans of the listed FKs are removed / set NULL before the constraint is added) |

Older DBs that already applied a pre-feature migration version get tables via
`ensure_*` (`CREATE IF NOT EXISTS`). The `_schema_versions` mark does **not**
skip the safety check, and is written only after the final drift check succeeds.

## Applying migrations

Prefer backend startup (runs `Migrator::up` automatically), or:

```bash
cargo run -p migration
```
