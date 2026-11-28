-- The rig API's customers: tenants, the keys they call with, and what they
-- have used of their quotas. A tenant belongs to a site account; its keys
-- are kept only as a keyed hash. The quota rows are the counters every
-- backend process checks and moves under the tenant's row lock or in one
-- statement, so the limits hold across processes.
-- Created by 004 and by schema_check::ensure_rig_api_tables; safe to run
-- again.
CREATE TABLE IF NOT EXISTS rig_api_tenants (
    -- `ten_<uuid simple>`.
    id VARCHAR(40) PRIMARY KEY,
    owner_user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(120) NOT NULL,
    -- Limits of this tenant's own; NULL is the site's default for it.
    max_concurrent_jobs INTEGER CHECK (max_concurrent_jobs >= 0),
    max_jobs_per_period INTEGER CHECK (max_jobs_per_period >= 0),
    job_period VARCHAR(8) CHECK (job_period IN ('day', 'month')),
    requests_per_minute INTEGER CHECK (requests_per_minute >= 0),
    -- A disabled tenant's keys are refused.
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_rig_api_tenants_owner ON rig_api_tenants (owner_user_id);

CREATE TABLE IF NOT EXISTS rig_api_keys (
    -- `rak_<uuid simple>`.
    id VARCHAR(40) PRIMARY KEY,
    tenant_id VARCHAR(40) NOT NULL REFERENCES rig_api_tenants(id) ON DELETE CASCADE,
    name VARCHAR(80) NOT NULL DEFAULT '',
    -- The key's public part, `mrk_` and 12 characters: how it is found and
    -- told apart. Not a secret.
    prefix VARCHAR(24) NOT NULL,
    -- HMAC-SHA256 (hex) of the whole key under a key derived from the
    -- installation's data key.
    secret_hash VARCHAR(64) NOT NULL,
    -- Which hashing key that was: a fingerprint of it, not the key. A row
    -- whose fingerprint is not the current one was hashed under another
    -- data key and can no longer be checked.
    hash_key_id VARCHAR(16) NOT NULL,
    created_by_user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_rig_api_keys_prefix ON rig_api_keys (prefix);
CREATE INDEX IF NOT EXISTS idx_rig_api_keys_tenant ON rig_api_keys (tenant_id);
CREATE INDEX IF NOT EXISTS idx_rig_api_keys_created_by ON rig_api_keys (created_by_user_id);

-- Jobs a tenant was let start, per UTC day. A period (a day or a month) is
-- the sum of its days, so changing a tenant's period counts what it started
-- earlier in the new one. Days older than a year and a month are removed.
CREATE TABLE IF NOT EXISTS rig_api_job_usage (
    tenant_id VARCHAR(40) NOT NULL REFERENCES rig_api_tenants(id) ON DELETE CASCADE,
    day DATE NOT NULL,
    jobs INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, day)
);

-- Requests a tenant made in each minute. A tenant's older rows are removed
-- by its first request of a new minute.
CREATE TABLE IF NOT EXISTS rig_api_request_windows (
    tenant_id VARCHAR(40) NOT NULL REFERENCES rig_api_tenants(id) ON DELETE CASCADE,
    window_start TIMESTAMPTZ NOT NULL,
    requests INTEGER NOT NULL,
    PRIMARY KEY (tenant_id, window_start)
);
