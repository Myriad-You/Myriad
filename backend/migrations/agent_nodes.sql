-- Her nodes on her host: what she keeps about one thing the owner does on a
-- child end, a repository another coding agent works in or their talks with
-- other AIs, written from what she decided about each moment that end told
-- her (her link's `moment`). Her own words and references only, never the
-- originals; only ever the owner's own (`user_id`), and gone with them. The
-- log is append-only: one row per change, numbered within its node.
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_agent_nodes_tables`; both run this same file.
CREATE TABLE IF NOT EXISTS agent_nodes (
    id VARCHAR(64) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    form VARCHAR(16) NOT NULL,
    node_key TEXT NOT NULL,
    title TEXT NOT NULL,
    origin VARCHAR(64) NOT NULL,
    notes JSONB NOT NULL DEFAULT '[]'::jsonb,
    refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    logged BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_node_events (
    node_id VARCHAR(64) NOT NULL REFERENCES agent_nodes(id) ON DELETE CASCADE,
    seq BIGINT NOT NULL,
    change VARCHAR(32) NOT NULL,
    fields JSONB NOT NULL,
    at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (node_id, seq)
);
