-- What she was made to forget, so a copy elsewhere (a device's her merged
-- in) cannot bring it back. Holds no words: a memory by its id, a line by a
-- digest of who said it, when and what. Goes with the person.
-- Created by 004 and by schema_check::ensure_forgotten_table; safe to run
-- again.
CREATE TABLE IF NOT EXISTS agent_forgotten (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    what VARCHAR(16) NOT NULL,
    mark VARCHAR(128) NOT NULL,
    forgotten_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, what, mark)
);
