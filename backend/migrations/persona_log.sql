-- Her log on the server: the relay between host cores, the server's key ring,
-- what was forgotten, and her one lease (services/her_log; the rules are
-- crates/myriad-persona-log). Nothing reads or writes these tables unless
-- HER_LOG_ENABLED is on, but her lease: its grants say which end holds her,
-- log or no log. Nothing is ever stored in them in the clear: a row is an
-- envelope, its content is a payload sealed by its object's key.
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_persona_log_tables`; both run this same file.

-- Her log is one per account (2026-11-01): node ids are unique only per
-- account and audience (myriad-core docs/SHARED_CORE.md), so every table of
-- it is keyed by the account (`user_id`). Before that it was one log for the
-- whole site, keyed by row id. That log was never on anywhere
-- (HER_LOG_ENABLED, no client spoke it), and the partner's private talk kept
-- under an account id cannot be moved to a partner id (the core refuses that
-- move), so a log of the old shape is made again below, but only while it
-- is empty: nothing kept is deleted quietly. One that holds anything stays
-- as it is (the startup heal says so in its log, `ensure_persona_log_tables`)
-- until someone decides what becomes of it, before her log is turned on.
-- The schema check may have given the old tables a nullable
-- `user_id` first; it is the primary key that tells the shapes apart.
DO $$
DECLARE
    old_tables TEXT[] := ARRAY['agent_log_payloads', 'agent_log_rows', 'agent_log_object_keys',
        'agent_log_audience_keys', 'agent_log_counters', 'agent_log_full_links'];
    t TEXT;
    held BOOLEAN := false;
    one BOOLEAN;
BEGIN
    IF to_regclass('agent_log_rows') IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM pg_index i
        JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY (i.indkey)
        WHERE i.indrelid = to_regclass('agent_log_rows') AND i.indisprimary
          AND a.attname = 'user_id'
    ) THEN
        FOREACH t IN ARRAY old_tables LOOP
            IF to_regclass(t) IS NOT NULL THEN
                EXECUTE format('SELECT EXISTS (SELECT 1 FROM %I)', t) INTO one;
                held := held OR one;
            END IF;
        END LOOP;
        IF NOT held THEN
            DROP TABLE IF EXISTS agent_log_payloads, agent_log_rows, agent_log_object_keys,
                agent_log_audience_keys, agent_log_counters, agent_log_full_links;
        END IF;
    END IF;
    -- The ledger was one line per forgotten target; it is a list per
    -- audience now (`agent_log_ledgers`). The old table goes only empty.
    IF to_regclass('agent_log_ledger') IS NOT NULL THEN
        EXECUTE 'SELECT EXISTS (SELECT 1 FROM agent_log_ledger)' INTO one;
        IF NOT one THEN
            DROP TABLE agent_log_ledger;
        END IF;
    END IF;
END $$;

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

-- Her lease: every grant the server issued, one for all audiences, and the
-- one record of which end holds her (persona_sync::lease reads where she is
-- from it). The epoch only goes up; the newest row is the grant that stands.
CREATE TABLE IF NOT EXISTS agent_lease_grants (
    epoch BIGINT PRIMARY KEY CHECK (epoch >= 1),
    -- None while she moves house (a freeze): no end holds her.
    holder VARCHAR(64),
    issued VARCHAR(96) NOT NULL,
    how VARCHAR(12) NOT NULL
        CONSTRAINT agent_lease_grants_how_check
        CHECK (how IN ('first', 'handover', 'takeover', 'force', 'freeze', 'thaw')),
    -- A handover's or a freeze's {audience: last dseq} of the side that gave it up.
    through JSONB,
    created_at TIMESTAMPTZ NOT NULL,
    -- The holder it moved from (every grant but the first and a thaw; none
    -- on grants kept before it was written).
    from_holder VARCHAR(64),
    -- A freeze's move.
    move_id VARCHAR(64),
    CONSTRAINT agent_lease_grants_freeze_check
        CHECK ((how = 'freeze') = (holder IS NULL) AND (how = 'freeze') = (move_id IS NOT NULL))
);
ALTER TABLE agent_lease_grants ADD COLUMN IF NOT EXISTS from_holder VARCHAR(64);
-- Grants from before she could be frozen: a holder always, and two ways fewer.
ALTER TABLE agent_lease_grants ADD COLUMN IF NOT EXISTS move_id VARCHAR(64);
ALTER TABLE agent_lease_grants ALTER COLUMN holder DROP NOT NULL;
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'agent_lease_grants'::regclass AND conname = 'agent_lease_grants_freeze_check'
    ) THEN
        ALTER TABLE agent_lease_grants DROP CONSTRAINT IF EXISTS agent_lease_grants_how_check;
        ALTER TABLE agent_lease_grants ADD CONSTRAINT agent_lease_grants_how_check
            CHECK (how IN ('first', 'handover', 'takeover', 'force', 'freeze', 'thaw'));
        ALTER TABLE agent_lease_grants ADD CONSTRAINT agent_lease_grants_freeze_check
            CHECK ((how = 'freeze') = (holder IS NULL) AND (how = 'freeze') = (move_id IS NOT NULL));
    END IF;
END $$;
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
-- A new grant is announced on `myriad_her_host`, with her copy and her
-- moves, so each process reads where she is again as it commits.
CREATE OR REPLACE FUNCTION agent_lease_grants_issued() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('myriad_her_host', 'mind:' || NEW.epoch::text);
    RETURN NULL;
END $$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS agent_lease_grants_issued ON agent_lease_grants;
CREATE TRIGGER agent_lease_grants_issued AFTER INSERT ON agent_lease_grants
    FOR EACH ROW EXECUTE FUNCTION agent_lease_grants_issued();

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

-- What the partner and she said in private on another of the owner's ends,
-- kept in the site's own conversations as copies (D6-e): `origin` is the
-- end that wrote the line (none for a line said here), `log_row` the row of
-- her log it is a copy of, or, for the partner's line said here, the row it
-- was written as. A copy is never written to the log again. A line goes,
-- with what it taught her, when its row is forgotten (D6-g). A row is
-- copied once, into one conversation.
ALTER TABLE agent_messages ADD COLUMN IF NOT EXISTS origin VARCHAR(64);
ALTER TABLE agent_messages ADD COLUMN IF NOT EXISTS log_row VARCHAR(512);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_messages_log_row
    ON agent_messages (log_row) WHERE log_row IS NOT NULL;
-- Which of the site's conversations a segment of another end's talk goes
-- in, settled the first time the site sees it: the owner's private chat
-- last talked in within six hours before its first line, or a new one.
CREATE TABLE IF NOT EXISTS agent_her_segments (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    object VARCHAR(512) NOT NULL,
    session_id VARCHAR(64) NOT NULL,
    PRIMARY KEY (user_id, object)
);

-- Her records as her log last has them (b1 of protocol 2): each object's
-- digest, as the site wrote it or took it in, and the row from another end
-- last taken in of it (taken in once). A record whose digest differs is
-- written again; one gone from the site is written as gone.
CREATE TABLE IF NOT EXISTS agent_her_state_rows (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    object VARCHAR(512) NOT NULL,
    digest VARCHAR(128) NOT NULL,
    took VARCHAR(256),
    PRIMARY KEY (user_id, object)
);
ALTER TABLE agent_her_state_rows ADD COLUMN IF NOT EXISTS took VARCHAR(256);
