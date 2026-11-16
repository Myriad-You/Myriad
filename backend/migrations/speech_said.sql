-- What she said (`speech.said`), kept on the site while her log is not: one
-- row a line she said to someone, unasked or called on, and how much of it
-- went out. Every judgment of whether to speak reads it (`speech` in the
-- shared core). A row is the log's: `audience` is `personal:<user id>` (a
-- private talk with that user) or `group:<venue>`, `payload` is the
-- `speech.said` payload as her log keeps it (`at` in milliseconds), so the
-- core's `speech.said_from_rows` reads the rows as they are. Append-only.
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_speech_said_table`; both run this same file.
CREATE TABLE IF NOT EXISTS agent_speech_said (
    id VARCHAR(64) PRIMARY KEY,
    audience TEXT NOT NULL,
    user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    thing TEXT,
    called BOOLEAN NOT NULL,
    at TIMESTAMPTZ NOT NULL,
    payload JSONB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_speech_said_audience
    ON agent_speech_said (audience, at);
CREATE INDEX IF NOT EXISTS idx_agent_speech_said_thing
    ON agent_speech_said (thing, at);
