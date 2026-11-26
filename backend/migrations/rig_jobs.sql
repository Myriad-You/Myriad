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
    -- The caller's key for one submission: the same key gives the same job.
    idempotency_key VARCHAR(255),
    -- SHA-256 of what was asked, to tell a key used again for something else.
    request_digest VARCHAR(64) NOT NULL,
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

-- NULL keys are distinct: only submissions that carry a key are joined by it.
CREATE UNIQUE INDEX IF NOT EXISTS idx_rig_jobs_caller_key
    ON rig_jobs (caller, idempotency_key);
CREATE INDEX IF NOT EXISTS idx_rig_jobs_caller_created
    ON rig_jobs (caller, created_at);
CREATE INDEX IF NOT EXISTS idx_rig_jobs_state_heartbeat
    ON rig_jobs (state, heartbeat_at);

CREATE TABLE IF NOT EXISTS rig_job_steps (
    job_id VARCHAR(32) NOT NULL REFERENCES rig_jobs(id) ON DELETE CASCADE,
    -- Order the step began in, within its job.
    seq INTEGER NOT NULL,
    name VARCHAR(64) NOT NULL,
    state VARCHAR(16) NOT NULL
        CHECK (state IN ('running', 'succeeded', 'failed', 'cancelled')),
    -- Calls made for the step: the first and each one tried again.
    attempts INTEGER NOT NULL DEFAULT 1,
    -- Where the step's work ran: `hf_space`, `image_provider`.
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
