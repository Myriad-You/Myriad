-- Her log on the server: the relay between host cores, the server's key ring,
-- what was forgotten, and her one lease (services/her_log; the rules are
-- crates/myriad-persona-log). Nothing reads or writes these tables unless
-- HER_LOG_ENABLED is on, and nothing is ever stored in them in the clear: a
-- row is an envelope, its content is a payload sealed by its object's key.
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_persona_log_tables`; both run this same file.

-- Her log is one per account (2026-11-01): node ids are unique only per
-- account and audience (myriad-core docs/SHARED_CORE.md), so every table of
-- it is keyed by the account (`user_id`). Before that it was one log for the
-- whole site, keyed by row id. That log was never on anywhere
-- (HER_LOG_ENABLED, no client spoke it), and the partner's private talk kept
-- under an account id cannot be moved to a partner id (the core refuses that
-- move), so a log of the old shape goes whole here and is made again below.
-- The schema check may have given the old tables a nullable `user_id`
-- first; it is the primary key that tells the shapes apart.
DO $$
BEGIN
    IF to_regclass('agent_log_rows') IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM pg_index i
        JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY (i.indkey)
        WHERE i.indrelid = to_regclass('agent_log_rows') AND i.indisprimary
          AND a.attname = 'user_id'
    ) THEN
        DROP TABLE IF EXISTS agent_log_payloads, agent_log_rows, agent_log_object_keys,
            agent_log_audience_keys, agent_log_counters, agent_log_full_links;
    END IF;
END $$;
-- The ledger was one line per forgotten target; it is a list per audience now.
DROP TABLE IF EXISTS agent_log_ledger;

-- The owner's account for her log. `region` is where it is kept, one per
-- account (problem 13 of the sync plan: not decided yet, so it is empty
-- until it is); `stopped_at` is when the owner turned syncing off, from
-- which the retention period counts.
CREATE TABLE IF NOT EXISTS agent_log_accounts (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    region VARCHAR(16),
    started_at TIMESTAMPTZ NOT NULL,
    stopped_at TIMESTAMPTZ
);

-- The partner's one id for each account (`partner_` and 32 hex digits,
-- `partner.mint`): minted here once, the first time a device of the account
-- asks for it, and never changed or reused. Her log's private talk with the
-- partner is `personal:<this id>` on every side, and the server's side of
-- the account goes by it. An id, not a credential. Not deleted by the
-- retention sweep: a new id would cut every device of the account off.
CREATE TABLE IF NOT EXISTS agent_log_partners (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    partner_id VARCHAR(64) NOT NULL UNIQUE,
    minted_at TIMESTAMPTZ NOT NULL
);

-- One row of an account's log, per audience (`personal:…`, `own:`,
-- `group:…`), as the log rules write it: `envelope` is the row itself, the
-- other columns are copies of its fields to look it up by. The server keeps
-- shared rows only, so every row has its place (`origin`, `dseq`) in its
-- audience. A row is never changed, except into a stub (`forgotten`, no
-- payload).
CREATE TABLE IF NOT EXISTS agent_log_rows (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    id VARCHAR(512) NOT NULL,
    audience VARCHAR(512) NOT NULL,
    origin VARCHAR(64) NOT NULL,
    dseq BIGINT NOT NULL,
    hlc VARCHAR(96) NOT NULL,
    object VARCHAR(512) NOT NULL,
    subject VARCHAR(512),
    kind VARCHAR(64) NOT NULL,
    forgotten BOOLEAN NOT NULL DEFAULT false,
    envelope JSONB NOT NULL,
    received_from VARCHAR(64) NOT NULL,
    received_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, id)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_log_rows_place
    ON agent_log_rows (user_id, audience, origin, dseq);
CREATE INDEX IF NOT EXISTS idx_agent_log_rows_object
    ON agent_log_rows (user_id, audience, object);
CREATE INDEX IF NOT EXISTS idx_agent_log_rows_subject
    ON agent_log_rows (user_id, audience, subject);

-- What a row's payload said, sealed with its object's key (AES-256-GCM,
-- nonce first). Deleted when the row becomes a stub.
CREATE TABLE IF NOT EXISTS agent_log_payloads (
    user_id INTEGER NOT NULL,
    row_id VARCHAR(512) NOT NULL,
    sealed BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, row_id),
    FOREIGN KEY (user_id, row_id) REFERENCES agent_log_rows (user_id, id) ON DELETE CASCADE
);

-- The server's key ring (problem 5: the server can read, so it holds the
-- keys). One key per audience of an account, sealed by the installation's
-- data key (myriad-data-key, kept outside the database); one key per
-- object, sealed by its audience's key. Forgetting an object deletes its
-- key, so its payload cannot be read back from any copy of the database.
CREATE TABLE IF NOT EXISTS agent_log_audience_keys (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience VARCHAR(512) NOT NULL,
    sealed TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, audience)
);
CREATE TABLE IF NOT EXISTS agent_log_object_keys (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience VARCHAR(512) NOT NULL,
    object VARCHAR(512) NOT NULL,
    subject VARCHAR(512),
    sealed BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, audience, object)
);

-- An audience's ledger, as the core gives it (`[{audience, target, at,
-- kind?, homes?, to?, takes?}]`): what was forgotten or taken back from
-- sync, where and when, and the other names of its nodes. Never a word of
-- what anything said. Kept after the rows are stubs, so a side that comes
-- back with old rows is answered with the forget first.
CREATE TABLE IF NOT EXISTS agent_log_ledgers (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience VARCHAR(512) NOT NULL,
    entries JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, audience)
);

-- Nodes (`node:<id>`) the server learnt are a person's or a group's in her
-- own audience (`nodes.wire_take` `elsewhere`): no body of theirs is taken
-- or sent again, in whatever batch it comes.
CREATE TABLE IF NOT EXISTS agent_log_elsewhere (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience VARCHAR(512) NOT NULL,
    node VARCHAR(512) NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, audience, node)
);

-- The server's own count of the rows it writes (`site`), per account and
-- audience, and its clock: the last revision it stamped.
CREATE TABLE IF NOT EXISTS agent_log_counters (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience VARCHAR(512) NOT NULL,
    dseq BIGINT NOT NULL CHECK (dseq >= 1),
    PRIMARY KEY (user_id, audience)
);
CREATE TABLE IF NOT EXISTS agent_log_clock (
    node VARCHAR(64) PRIMARY KEY,
    last VARCHAR(96) NOT NULL
);

-- Single-use links for a new host core's first full sync over HTTPS
-- (`log.full`). Only the sha256 of the link's secret part is kept.
-- `peer_abi` is the device's core ABI as its link said (2 when its hello
-- listed `log2`): what may go to it.
CREATE TABLE IF NOT EXISTS agent_log_full_links (
    token_hash VARCHAR(64) PRIMARY KEY,
    device_id VARCHAR(64) NOT NULL,
    audience VARCHAR(512) NOT NULL,
    peer_abi SMALLINT NOT NULL DEFAULT 1 CHECK (peer_abi >= 1),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ
);

-- Her lease: every grant the server issued, one for all audiences. The
-- epoch only goes up; the newest row is the grant that stands.
CREATE TABLE IF NOT EXISTS agent_lease_grants (
    epoch BIGINT PRIMARY KEY CHECK (epoch >= 1),
    holder VARCHAR(64) NOT NULL,
    issued VARCHAR(96) NOT NULL,
    how VARCHAR(12) NOT NULL
        CONSTRAINT agent_lease_grants_how_check
        CHECK (how IN ('first', 'handover', 'takeover', 'force')),
    -- A handover's {audience: last dseq} of the side that gave it up.
    through JSONB,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE OR REPLACE FUNCTION agent_lease_grants_guard() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'UPDATE' OR TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'agent_lease_grants: a grant is never changed or taken back';
    END IF;
    IF EXISTS (SELECT 1 FROM agent_lease_grants WHERE epoch >= NEW.epoch) THEN
        RAISE EXCEPTION 'agent_lease_grants: epoch % is not past the newest', NEW.epoch;
    END IF;
    RETURN NEW;
END $$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS agent_lease_grants_guard ON agent_lease_grants;
CREATE TRIGGER agent_lease_grants_guard BEFORE INSERT OR UPDATE OR DELETE ON agent_lease_grants
    FOR EACH ROW EXECUTE FUNCTION agent_lease_grants_guard();

-- What the server knows of each of the owner's ends for moving the lease:
-- whether it is a host core, when it last heard from it, the place it said
-- it is in and since when, and when the owner was last seen there. Places
-- are the devices' own opaque tokens, never a name.
CREATE TABLE IF NOT EXISTS agent_lease_ends (
    device_id VARCHAR(64) PRIMARY KEY,
    core VARCHAR(8) NOT NULL
        CONSTRAINT agent_lease_ends_core_check CHECK (core IN ('host', 'child')),
    heard_at BIGINT,
    place VARCHAR(128),
    owner_since BIGINT,
    owner_seen_at BIGINT,
    stopped_epoch BIGINT,
    stopped_at BIGINT
);
