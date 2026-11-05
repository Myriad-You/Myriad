-- Her nodes on the site as the shared core keeps them (`myriad_nodes`): the
-- rows of her log about `node:<id>`, one audience at a time, and what they
-- fold into. The memory rows (`agent_memories` and the other sources) stay
-- where they are and stay the one place of a memory's strength and recency;
-- an entry on a node carries the memory's text and names it (`memory:<id>`).
--
-- Every table is keyed by the account whose copy of her it is (`user_id`)
-- and the audience: node ids are unique only per account and audience. A
-- private talk is its account's; a group and her own are the site owner's.
--
-- `agent_node_rows` is append-only: one row per core row, its envelope as
-- the core writes it (clock, side, count, lease epoch), its body as the
-- exact canonical bytes `Body::read` gives back. `agent_node_heads` is the
-- fold of an audience's rows, one per node, rewritten in the same
-- transaction as the rows it follows from. `agent_node_placements` is where
-- a memory row was put: on a node, on 「关于你」, or nowhere (decided, not
-- asked again). `agent_node_inbox` holds, per audience and day, what she was
-- unsure where to put.
--
-- Nothing reads or writes these while `merope_memory_nodes` is off.
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_memory_nodes_tables`; both run this same file.
CREATE TABLE IF NOT EXISTS agent_node_rows (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    node_id VARCHAR(64) NOT NULL,
    row_id VARCHAR(512) NOT NULL,
    kind VARCHAR(32) NOT NULL,
    body TEXT NOT NULL,
    said_by JSONB NOT NULL DEFAULT '[]'::jsonb,
    author VARCHAR(8) NOT NULL,
    at TIMESTAMPTZ NOT NULL,
    hlc TEXT NOT NULL,
    origin VARCHAR(64) NOT NULL,
    dseq BIGINT,
    epoch BIGINT,
    envelope JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, row_id)
);
CREATE INDEX IF NOT EXISTS idx_agent_node_rows_node
    ON agent_node_rows (user_id, audience, node_id);
CREATE TABLE IF NOT EXISTS agent_node_heads (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    node_id VARCHAR(64) NOT NULL,
    node_key TEXT,
    kind VARCHAR(16),
    name TEXT,
    aliases JSONB NOT NULL DEFAULT '[]'::jsonb,
    status VARCHAR(16) NOT NULL,
    node JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, audience, node_id)
);
CREATE INDEX IF NOT EXISTS idx_agent_node_heads_status
    ON agent_node_heads (user_id, audience, status);
CREATE TABLE IF NOT EXISTS agent_node_placements (
    memory_id VARCHAR(64) PRIMARY KEY REFERENCES agent_memories(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    node_id VARCHAR(64),
    how VARCHAR(8) NOT NULL,
    decided_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_node_placements_node
    ON agent_node_placements (user_id, audience, node_id);
CREATE TABLE IF NOT EXISTS agent_node_inbox (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    day DATE NOT NULL,
    id VARCHAR(64) NOT NULL,
    item JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    placed_at TIMESTAMPTZ,
    PRIMARY KEY (user_id, audience, day, id)
);
-- What only the site keeps of the partner's instruction beside its
-- `node.intent` row (her intents, S2b): the message it was given in
-- (`message:<id>`), her line back at once, who, where and under which
-- runtime grants it was given (checked again when it comes due), and what
-- came of it (it ran, her hand went up instead, or it was called off).
-- Goes when the message it rests on is forgotten.
CREATE TABLE IF NOT EXISTS agent_node_instructions (
    intent_id VARCHAR(64) NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    node_id VARCHAR(64) NOT NULL,
    message_ref VARCHAR(64) NOT NULL,
    echo TEXT NOT NULL,
    owner VARCHAR(64) NOT NULL,
    persona VARCHAR(128) NOT NULL,
    device VARCHAR(64) NOT NULL,
    granted JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    outcome_way VARCHAR(16),
    outcome_line TEXT,
    outcome_at TIMESTAMPTZ,
    succeeded BOOLEAN,
    work_session_id VARCHAR(64),
    PRIMARY KEY (user_id, intent_id)
);
CREATE INDEX IF NOT EXISTS idx_agent_node_instructions_message
    ON agent_node_instructions (message_ref);
