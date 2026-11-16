-- What each private chat message means, as a vector: on each private turn
-- she finds the few lines of the whole talk nearest what was just said
-- (`merope::chat_near`). Kept beside the line and deleted with it: whatever
-- deletes a line (forgetting it, or a memory it taught her, or its person)
-- takes its vector in the same statement. `digest` is of the text embedded.
-- Worked out where she is, never synced. Created by migration 004 and, for
-- databases already past it, by the startup heal
-- `ensure_message_embeddings_table`; both run this same file.
CREATE TABLE IF NOT EXISTS agent_message_embeddings (
    message_id INTEGER PRIMARY KEY REFERENCES agent_messages(id) ON DELETE CASCADE,
    model VARCHAR(128) NOT NULL,
    digest VARCHAR(64) NOT NULL,
    vector BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
