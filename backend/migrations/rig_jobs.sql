-- Generation jobs for character rigs: one row per job, whoever asked for it,
-- and one row per step the job ran. The rows are the job's state; the
-- process running a job writes its progress here and renews its lease
-- (`heartbeat_at`), and a running job whose lease has lapsed is one whose
-- process went away.
-- Created by 004 and by schema_check::ensure_rig_jobs_tables; safe to run
-- again.
CREATE TABLE IF NOT EXISTS rig_jobs (
    id VARCHAR(32) PRIMARY KEY,
    -- What the job makes: `bust`, `plain`, `figure`, `refit`, `load`.
    kind VARCHAR(32) NOT NULL,
    -- Who asked, as `user:<id>`; the key of everything below that is per caller.
    caller VARCHAR(96) NOT NULL,
    -- The site account the job belongs to, when the caller is one.
    owner_user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    -- What a running job of the same input is joined by instead of started again.
    subject VARCHAR(512),
    -- What the job was asked to work from and how: references, never the bytes.
    input JSONB NOT NULL,
    state VARCHAR(16) NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    -- Where the work stands, as the pipeline reports it.
    progress JSONB NOT NULL DEFAULT '{}'::jsonb,
    -- Times a process began running the job.
    attempts INTEGER NOT NULL DEFAULT 0,
    error_code VARCHAR(64),
    error_message TEXT,
    -- The failure as the API answered it.
    error_detail JSONB,
    -- Where the result is kept, e.g. `turn-archive:<id>`.
    result_ref VARCHAR(255),
    -- `ai_cost_ledger.task_id` of every paid call the job made.
    ledger_task_id VARCHAR(160) NOT NULL,
    -- The process running the job and when it last said it still is.
    runner VARCHAR(64),
    heartbeat_at TIMESTAMPTZ,
    cancel_requested_at TIMESTAMPTZ,
    -- Which parts of its end the caller has had: `psd`, `keys`, `failure`.
    delivered JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    ended_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rig_jobs_caller_created
    ON rig_jobs (caller, created_at);
CREATE INDEX IF NOT EXISTS idx_rig_jobs_owner
    ON rig_jobs (owner_user_id);
CREATE INDEX IF NOT EXISTS idx_rig_jobs_ended
    ON rig_jobs (ended_at);
-- The jobs holding a place, by when their lease was last renewed.
CREATE INDEX IF NOT EXISTS idx_rig_jobs_active_heartbeat
    ON rig_jobs (heartbeat_at) WHERE state IN ('queued', 'running');

-- A caller's key for one submission and the job it gave. The same key with
-- the same request gives the same job; with another request it is refused.
-- A key whose job failed or was cancelled is let go by the next submission
-- under it; keys expire after a day (`services::rig_jobs::KEY_TTL`).
CREATE TABLE IF NOT EXISTS rig_job_keys (
    caller VARCHAR(96) NOT NULL,
    idempotency_key VARCHAR(255) NOT NULL,
    job_id VARCHAR(32) NOT NULL REFERENCES rig_jobs(id) ON DELETE CASCADE,
    -- SHA-256 of what was asked under the key.
    request_digest VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (caller, idempotency_key)
);
CREATE INDEX IF NOT EXISTS idx_rig_job_keys_job ON rig_job_keys (job_id);
CREATE INDEX IF NOT EXISTS idx_rig_job_keys_created ON rig_job_keys (created_at);

CREATE TABLE IF NOT EXISTS rig_job_steps (
    job_id VARCHAR(32) NOT NULL REFERENCES rig_jobs(id) ON DELETE CASCADE,
    -- Order the step began in, within its job.
    seq INTEGER NOT NULL,
    name VARCHAR(64) NOT NULL,
    state VARCHAR(16) NOT NULL
        CHECK (state IN ('running', 'succeeded', 'failed', 'cancelled')),
    -- Calls made for the step: the first and each one tried again.
    attempts INTEGER NOT NULL DEFAULT 1,
    -- Where the step's work ran: `hf_space`, `image_provider`, or
    -- `hf_space+image_provider` for a step that asks the Space and may draw again.
    backend VARCHAR(32) NOT NULL,
    -- Which one: the Space's name, the image provider and model.
    backend_target VARCHAR(255),
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    wall_seconds DOUBLE PRECISION,
    -- As the backend reported it; NULL when it reports none.
    gpu_seconds DOUBLE PRECISION,
    PRIMARY KEY (job_id, seq)
);
