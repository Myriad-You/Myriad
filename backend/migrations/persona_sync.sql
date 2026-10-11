-- Syncing her with other platforms (crates/myriad-persona-sync).
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_persona_sync_tables`; both run this same file.

-- A device that syncs her with the site, one row each. The token is kept
-- only as its sha256; `token_hint` is the `mys_` prefix and 8 characters.
-- `role`: the parent end is where her own life happens. At most one live
-- device is the parent; with none, the site is.
CREATE TABLE IF NOT EXISTS agent_sync_devices (
    id VARCHAR(40) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(80) NOT NULL,
    scope VARCHAR(32) NOT NULL DEFAULT 'persona-sync',
    role VARCHAR(8) NOT NULL DEFAULT 'child' CHECK (role IN ('parent', 'child')),
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    token_hint VARCHAR(12) NOT NULL,
    lineage VARCHAR(36),
    created_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoked_reason VARCHAR(32)
);
CREATE INDEX IF NOT EXISTS idx_agent_sync_devices_user_created
    ON agent_sync_devices (user_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_sync_devices_one_parent
    ON agent_sync_devices (user_id) WHERE role = 'parent' AND revoked_at IS NULL;

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
