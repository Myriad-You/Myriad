-- The topics she keeps what she knows of someone under (`myriad_merope::topics`):
-- a kind of thing in their life they come back to, never one item of it.
-- A topic is a node of hers in the shared core's sense (a topic node with a
-- minted key, its id made by `myriad_nodes::node_id`), kept here while her
-- log is off; the fact itself stays on the memory row, and a memory points at
-- the topic it goes under (`agent_memory_topics`). Forgetting a memory takes
-- its pointer; a topic none points at any more is forgotten with the last
-- one, for its name can say as much as what was under it. A pointer with no
-- topic is a memory she sorted under none (only about them, or of no kind),
-- so sorting what she knew before topics (`chat_remember::sort`) does not ask
-- of it again. Created by
-- migration 004 and, for databases already past it, by the startup heal
-- `ensure_topic_nodes_tables`; both run this same file.
CREATE TABLE IF NOT EXISTS agent_topic_nodes (
    id VARCHAR(64) NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    audience TEXT NOT NULL,
    node_key TEXT NOT NULL,
    name TEXT NOT NULL,
    aliases JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL,
    tidied_at TIMESTAMPTZ,
    PRIMARY KEY (user_id, id)
);
CREATE TABLE IF NOT EXISTS agent_memory_topics (
    memory_id VARCHAR(64) PRIMARY KEY REFERENCES agent_memories(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL,
    topic_id VARCHAR(64),
    linked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (user_id, topic_id) REFERENCES agent_topic_nodes(user_id, id) ON DELETE CASCADE
);
-- Under none, and when a topic was last tidied and a memory put under it
-- (`chat_remember::tidy`: only a topic changed since it was tidied is looked
-- at again), were added after the first version of these tables.
ALTER TABLE agent_memory_topics ALTER COLUMN topic_id DROP NOT NULL;
ALTER TABLE agent_topic_nodes ADD COLUMN IF NOT EXISTS tidied_at TIMESTAMPTZ;
ALTER TABLE agent_memory_topics
    ADD COLUMN IF NOT EXISTS linked_at TIMESTAMPTZ NOT NULL DEFAULT now();
CREATE INDEX IF NOT EXISTS idx_agent_memory_topics_topic
    ON agent_memory_topics (user_id, topic_id);
CREATE OR REPLACE FUNCTION agent_memory_topics_forget() RETURNS trigger AS $$
BEGIN
    DELETE FROM agent_topic_nodes t
     WHERE t.user_id = OLD.user_id AND t.id = OLD.topic_id
       AND NOT EXISTS (
           SELECT 1 FROM agent_memory_topics m
            WHERE m.user_id = OLD.user_id AND m.topic_id = OLD.topic_id
       );
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS agent_memory_topics_forget ON agent_memory_topics;
CREATE TRIGGER agent_memory_topics_forget AFTER DELETE ON agent_memory_topics
    FOR EACH ROW EXECUTE FUNCTION agent_memory_topics_forget();
