# Migration CLI

Run database migrations for Myriad backend.

## Usage

```bash
# From backend directory
cd backend

# Run all pending migrations
cargo run --manifest-path migrations/Cargo.toml

# Or create a new migration
sea-orm-cli migrate generate create_new_table
```

## Available Migrations

1. `001_initial_schema` - Core users, platforms, profiles, reports, activity events and configuration
2. `002_tapp_system` - Tapp installations, storage, widgets, quota, scheduler and shared runtime state
3. `003_brew_system` - Brew sources, items and annotations
4. `004_agent_system` - Agent tasks, memory and notification state
5. `005_federation` - Federation identities and messages
6. `006_oauth_identities` - OAuth/OIDC identity bindings

Base CREATE tables (001–006) include the current column set for greenfield installs.
Thin ALTER-only migrations that only added columns or healed data were retired:

- `007_notification_preferences` → `users.notification_preferences` in 001
- `008_tapp_approved_permissions` → `tapps.approved_permissions` in 002 + generic column reconcile in schema_check (`get_expected_schema` / `generate_add_column_ddl`); optional NULL→`granted_permissions` data heal only
- `009_user_presence` → `users.last_seen_at` / `online_seconds` in 001 + schema_check
- `010_user_owner` / `011_owner_is_admin` → `users.is_owner` in 001 + `ensure_single_owner`

Existing deployments receive missing tables, columns, indexes, owner/admin heals,
and default platform seeds through the backend's independent startup schema
reconciliation (`schema_check`). The `_schema_versions` mark records the baseline
but does **not** skip the safety check — full reconcile still runs every boot.
Retired migration history rows are removed by `reconcile_retired_migration_history`
before `Migrator::up`.

## Manual SQL Migration

Alternatively, you can apply the SQL schema directly:

```bash
psql -U myriad -d myriad -f ../database/schema.sql
```
