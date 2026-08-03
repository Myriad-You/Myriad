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
4. `004_agent_system` - Agent tasks, memory and notification state
5. `005_federation` - Federation identities and messages
6. `006_oauth_identities` - OAuth/OIDC identity bindings

Base CREATE tables (001–006) include the current column set for greenfield installs.
Thin ALTER-only migrations that only added columns or healed data were retired:

- `007_notification_preferences` → `users.notification_preferences` in 001
- `008_tapp_approved_permissions` → `tapps.approved_permissions` in 002; missing column via generic `get_expected_schema` ADD only (no dedicated backfill)
- `009_user_presence` → `users.last_seen_at` / `online_seconds` in 001 + schema_check
- `010_user_owner` / `011_owner_is_admin` → `users.is_owner` in 001 + `ensure_single_owner`

Also retired from `seaql_migrations` when present on older/local DBs (files not on mainline):

- `008_tapp_runtime_registry`, `009_activity_events`
- digital_life experiment: `007_digital_life` … `011_digital_life_asset_subjects`

On reconcile, temporary **digital_life_*** experiment tables are also
`DROP TABLE IF EXISTS … CASCADE` (product: throwaway feature). Known names live
in `RETIRED_DIGITAL_LIFE_TABLES`; any remaining `public.digital_life_%` table /
type is dropped by prefix scan. Generically named companions from that
experiment (`image_generation_jobs`, `image_assets`) are **not** auto-dropped.

Authoritative lists: `RETIRED_MIGRATION_VERSIONS` /
`RETIRED_DIGITAL_LIFE_TABLES` in `backend/src/db/schema_check/seeds.rs`.

Whole tables are created by Migrator (001–006) — the numbered series is the
**complete greenfield source of truth**. Runtime `schema_check` only heals
**recent (~1 month) features** plus ongoing data/object jobs (platform seeds,
single owner, storage-quota trigger).

**Setup path:** `api/setup::init_database` runs `Migrator::up` then
`schema_check::ensure_schema` so first-boot seeds/heals do not require a
process restart (same order as `main` after DB connect).

**Startup policy (full mode):** default is warn + continue on migration /
`ensure_schema` failure so local DBs with retired history noise still boot.
Missing applied migration *files* are always treated as non-fatal history hygiene.
Set `MYRIAD_STRICT_SCHEMA=1` to refuse start on other migration/schema errors
(eventual hard prod gate). `MYRIAD_ALLOW_SCHEMA_DRIFT=1` remains a deliberate
recovery override under strict mode.

Recent tables:

| Feature | Migration | schema_check |
|---------|-----------|--------------|
| Site analytics (+ country) | `001` §8 | `ensure_analytics_tables` + TableDef |
| heartbeat_claims | `004` | `ensure_heartbeat_claims_table` + TableDef |
| content_filters / policy / domain_aliases / object_interactions | `005` 扩展段 | 对应 `ensure_*` + TableDef |

Older DBs that already applied a pre-feature migration version get tables via
`ensure_*` (`CREATE IF NOT EXISTS`). The `_schema_versions` mark does **not**
skip the safety check. Retired migration history rows are removed by
`reconcile_retired_migration_history` before `Migrator::up`.

## Applying migrations

Prefer backend startup (runs `Migrator::up` automatically), or:

```bash
cargo run -p migration
```
