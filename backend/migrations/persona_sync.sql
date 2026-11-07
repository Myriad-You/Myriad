-- Syncing her with other platforms (crates/myriad-persona-sync), and where
-- she lives.
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_persona_sync_tables`; both run this same file.

-- A device that syncs her with the site, one row each. The token is kept
-- only as its sha256; `token_hint` is the `mys_` prefix and 8 characters.
-- Where she lives is not a device's: it is her lease and the copy below.
CREATE TABLE IF NOT EXISTS agent_sync_devices (
    id VARCHAR(40) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(80) NOT NULL,
    scope VARCHAR(32) NOT NULL DEFAULT 'persona-sync',
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    token_hint VARCHAR(12) NOT NULL,
    lineage VARCHAR(36),
    created_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoked_reason VARCHAR(32),
    -- When her link last closed it for speaking another protocol (its app is
    -- to be updated); cleared by its next hello the site takes.
    app_outdated_at TIMESTAMPTZ
);
ALTER TABLE agent_sync_devices ADD COLUMN IF NOT EXISTS app_outdated_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS idx_agent_sync_devices_user_created
    ON agent_sync_devices (user_id, created_at DESC);
-- The parent/child flag of the old design, retired: a database from before
-- loses the column and its one-parent index here.
DROP INDEX IF EXISTS idx_agent_sync_devices_one_parent;
ALTER TABLE agent_sync_devices DROP COLUMN IF EXISTS role;

-- Where she lives (services/persona_sync/lease.rs). Which end holds her is
-- her lease (`agent_lease_grants`, persona_log.sql), and only that; these two
-- keep the rest. The old one-row lease (`agent_host_lease`) is taken apart
-- into them and the grants once, at start (`lease::old_lease`).
--
-- The site's copy of her, one row, because the site has one her. `lineage`
-- is her birth (kept across every move: a new birth is a new row); `state`
-- is live (she lives here, whichever end her mind is on), away (she lives
-- on the device the grant names, the site let its copy go or has none yet),
-- vacant (she is not here and the site does not know where: the device she
-- lived on was removed, or the site let her go after she moved out) or
-- moved_out (exported to be brought in elsewhere, waiting for the owner to
-- let this copy go or undo it). No key, token or grant is ever in it.
CREATE TABLE IF NOT EXISTS agent_her_copy (
    id VARCHAR(8) PRIMARY KEY CHECK (id = 'her'),
    lineage VARCHAR(36) NOT NULL,
    state VARCHAR(12) NOT NULL
        CONSTRAINT agent_her_copy_state_check
        CHECK (state IN ('live', 'away', 'vacant', 'moved_out')),
    state_at TIMESTAMPTZ NOT NULL
);
CREATE OR REPLACE FUNCTION agent_her_copy_guard() RETURNS trigger AS $$
BEGIN
    IF NEW.lineage <> OLD.lineage THEN
        RAISE EXCEPTION 'agent_her_copy: the lineage does not change in place';
    END IF;
    RETURN NEW;
END $$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS agent_her_copy_guard ON agent_her_copy;
CREATE TRIGGER agent_her_copy_guard BEFORE UPDATE ON agent_her_copy
    FOR EACH ROW EXECUTE FUNCTION agent_her_copy_guard();

-- Her moves, one row each: proposed → frozen → transferring → applied →
-- committed → retired. While one is frozen no end holds her (a freeze
-- grant). One that is called off goes; one that finished stays (`done_at`,
-- and `done_epoch` the epoch it left), so a commit asked again is answered.
-- At most one is under way.
CREATE TABLE IF NOT EXISTS agent_her_moves (
    id VARCHAR(64) PRIMARY KEY,
    kind VARCHAR(8) NOT NULL CHECK (kind IN ('move', 'merge', 'recover')),
    from_holder VARCHAR(64) NOT NULL,
    to_holder VARCHAR(64) NOT NULL,
    phase VARCHAR(12) NOT NULL
        CHECK (phase IN ('proposed', 'frozen', 'transferring', 'applied', 'committed', 'retired')),
    started_by VARCHAR(8) NOT NULL CHECK (started_by IN ('owner', 'device')),
    digest VARCHAR(128),
    started_at TIMESTAMPTZ NOT NULL,
    confirmed_at TIMESTAMPTZ,
    agreed BOOLEAN,
    import_id VARCHAR(64),
    via VARCHAR(64),
    was_state VARCHAR(12),
    updated_at TIMESTAMPTZ NOT NULL,
    done_at TIMESTAMPTZ,
    done_epoch BIGINT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_her_moves_one_under_way
    ON agent_her_moves ((true)) WHERE done_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_agent_her_moves_done
    ON agent_her_moves (done_at DESC);

-- Every change to either is announced on `myriad_her_host` as it commits
-- (so is every grant), so each process (and each of its links and drivers)
-- reads where she is again.
CREATE OR REPLACE FUNCTION agent_her_place_changed() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('myriad_her_host', TG_TABLE_NAME);
    RETURN NULL;
END $$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS agent_her_copy_changed ON agent_her_copy;
CREATE TRIGGER agent_her_copy_changed AFTER INSERT OR UPDATE OR DELETE ON agent_her_copy
    FOR EACH ROW EXECUTE FUNCTION agent_her_place_changed();
DROP TRIGGER IF EXISTS agent_her_moves_changed ON agent_her_moves;
CREATE TRIGGER agent_her_moves_changed AFTER INSERT OR UPDATE OR DELETE ON agent_her_moves
    FOR EACH ROW EXECUTE FUNCTION agent_her_place_changed();

-- What moved her affect toward each person, one row per change, appended in
-- the same transaction as the change itself (store/addressee.rs). `at` is
-- the moment the change is stamped with; `baseline_*` what she settled
-- toward before it; `before`/`after` are for audit and display, replay
-- reads neither. Everyone's are kept (folded at night); only the owner's
-- cross to other sides.
CREATE TABLE IF NOT EXISTS agent_affect_events (
    id VARCHAR(40) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    at TIMESTAMPTZ NOT NULL,
    cause VARCHAR(32) NOT NULL,
    impulse JSONB NOT NULL,
    baseline_mood DOUBLE PRECISION NOT NULL,
    baseline_arousal DOUBLE PRECISION NOT NULL,
    before JSONB NOT NULL,
    after JSONB NOT NULL,
    late BOOLEAN NOT NULL DEFAULT false,
    origin VARCHAR(64) NOT NULL DEFAULT 'site',
    rev VARCHAR(96),
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_affect_events_user_at
    ON agent_affect_events (user_id, at);
-- Everyone's changes since she got up, for how her talks with others went
-- (myriad_merope::elsewhere), read on every turn.
CREATE INDEX IF NOT EXISTS idx_agent_affect_events_at
    ON agent_affect_events (at);

-- Where replaying her affect toward a person starts: the affect right after
-- the change `through_event`, at `at`. Written with the first change and
-- moved forward when old changes are folded in; only the site writes it.
CREATE TABLE IF NOT EXISTS agent_affect_anchor (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    mood DOUBLE PRECISION NOT NULL,
    arousal DOUBLE PRECISION NOT NULL,
    emotion DOUBLE PRECISION NOT NULL,
    emotion_arousal DOUBLE PRECISION NOT NULL,
    at TIMESTAMPTZ NOT NULL,
    through_event VARCHAR(40),
    rev VARCHAR(96) NOT NULL
);

-- Values her persona's fields held until an edit replaced them, to look back
-- on and put back (putting one back is one more edit). `rev`/`origin` are
-- the replaced value's; `reason`: edit | lww | subject | trimmed.
CREATE TABLE IF NOT EXISTS agent_persona_field_history (
    id BIGSERIAL PRIMARY KEY,
    field VARCHAR(64) NOT NULL,
    value JSONB,
    rev VARCHAR(96) NOT NULL,
    origin VARCHAR(64) NOT NULL,
    replaced_at TIMESTAMPTZ NOT NULL,
    replaced_by_rev VARCHAR(96) NOT NULL,
    reason VARCHAR(16) NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_persona_field_history_field
    ON agent_persona_field_history (field, replaced_at DESC);

-- What came by chat app while she was not on the site (moving, or living on
-- another end), held in the order it came until she is back (D3): nobody
-- is answered meanwhile, not even with a refusal. `inbound_id` is the
-- platform message as the channel claimed it, so one message is held once.
-- No expiry: a row goes when she reads it, when its pairing is gone, or
-- when the owner clears what is held.
CREATE TABLE IF NOT EXISTS agent_channel_held (
    id BIGSERIAL PRIMARY KEY,
    platform VARCHAR(16) NOT NULL,
    session_key VARCHAR(255) NOT NULL,
    inbound_id VARCHAR(255) NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    images JSONB NOT NULL DEFAULT '[]'::jsonb,
    arrived_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_channel_held_inbound
    ON agent_channel_held (platform, inbound_id);
CREATE INDEX IF NOT EXISTS idx_agent_channel_held_session
    ON agent_channel_held (session_key, id);

-- Bringing her into a site that has none (services/persona_sync/import.rs):
-- from a device that holds her, or from an export file the owner uploads.
-- One session at a time. A device's session waits for the owner to confirm
-- it on the site (`proposed`), then takes her page by page (`confirmed`,
-- `transferring`), checked against the manifest the owner was shown; she
-- lands in one transaction and the session is `committed`. A file's
-- session takes the sealed file in pieces (`uploading`) and opens it with
-- the passphrase given once. Nothing secret is kept here: no token, no
-- passphrase, no key.
CREATE TABLE IF NOT EXISTS agent_host_import (
    id VARCHAR(40) PRIMARY KEY,
    source VARCHAR(8) NOT NULL CHECK (source IN ('device', 'file')),
    device_id VARCHAR(40),
    lineage VARCHAR(36),
    epoch BIGINT,
    name TEXT,
    manifest JSONB,
    phase VARCHAR(16) NOT NULL,
    received BIGINT NOT NULL DEFAULT 0,
    total BIGINT,
    created_at TIMESTAMPTZ NOT NULL,
    confirmed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL
);
-- The first meeting of two of her (H3d, E7): a device's her merged into the
-- site's. `kind` says so; `subject` is who she is when her names differ
-- (`site` or `device`, chosen on the device); `preview` is the device's
-- persona and wardrobe as the owner is shown them before confirming;
-- `retired` names the file the site sealed of its own her when the owner
-- confirmed (the passphrase is not kept).
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS kind VARCHAR(8) NOT NULL DEFAULT 'import';
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS subject VARCHAR(8);
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS preview JSONB;
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS retired TEXT;
-- What a device has sent of her so far, each entity in its canonical form
-- (`kind` = `embedding` for a vector, by memory id), until she lands.
CREATE TABLE IF NOT EXISTS agent_host_import_staging (
    import_id VARCHAR(40) NOT NULL REFERENCES agent_host_import(id) ON DELETE CASCADE,
    kind VARCHAR(24) NOT NULL,
    entity_id VARCHAR(160) NOT NULL,
    canonical JSONB NOT NULL,
    PRIMARY KEY (import_id, kind, entity_id)
);

-- The daily backups of her a device she lives on pushes (D4): each a sealed
-- `myriad.her-archive` the site cannot open (the passphrase never leaves the
-- device), kept under `data/agent/backups/<device>/`. One per device and
-- day; the newest seven of her lineage are kept, whichever device pushed
-- them. `received` counts the bytes in so far while it comes in pieces;
-- `complete` once all `bytes` are here. Removing the device keeps them:
-- they are how she is taken back.
CREATE TABLE IF NOT EXISTS agent_host_backups (
    id VARCHAR(40) PRIMARY KEY,
    device_id VARCHAR(40) NOT NULL,
    day VARCHAR(10) NOT NULL,
    lineage VARCHAR(36) NOT NULL,
    epoch BIGINT NOT NULL,
    digest VARCHAR(64) NOT NULL,
    bytes BIGINT NOT NULL,
    received BIGINT NOT NULL DEFAULT 0,
    complete BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_host_backups_device_day
    ON agent_host_backups (device_id, day);
CREATE INDEX IF NOT EXISTS idx_agent_host_backups_lineage
    ON agent_host_backups (lineage, created_at DESC);

-- What the site's own Work did for the owner while she lived on another end
-- (H3e-9): Work runs there as the plain assistant, and how it went is kept
-- here for her, in the order it ended, until she is back on the site (or,
-- later, her host takes it over the link). Nothing secret is in it.
CREATE TABLE IF NOT EXISTS agent_kept_for_her (
    id BIGSERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_key VARCHAR(64) NOT NULL,
    summary TEXT NOT NULL,
    happened_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
