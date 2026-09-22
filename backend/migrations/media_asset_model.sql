-- Persistent media asset model. Idempotent for greenfield 003 and runtime heals.
-- New columns stay nullable/defaulted so existing catalog rows survive the
-- expansion phase. Tightening NOT NULL happens after backfill, not here.

CREATE TABLE IF NOT EXISTS media_assets (
    id SERIAL PRIMARY KEY,
    kind VARCHAR NOT NULL,
    url TEXT NOT NULL,
    mime TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    size BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    public_id UUID DEFAULT gen_random_uuid(),
    scope TEXT DEFAULT 'legacy_unknown',
    owner_user_id INTEGER,
    created_by INTEGER,
    storage_key TEXT,
    state TEXT,
    exposure TEXT DEFAULT 'public',
    first_published_at TIMESTAMPTZ,
    source TEXT DEFAULT 'legacy',
    derived_from_id INTEGER,
    checksum_sha256 TEXT,
    width INTEGER,
    height INTEGER,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    state_since TIMESTAMPTZ,
    references_complete BOOLEAN NOT NULL DEFAULT FALSE,
    write_token UUID,
    write_lease_until TIMESTAMPTZ,
    producer_key TEXT
);

ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS public_id UUID DEFAULT gen_random_uuid();
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS scope TEXT DEFAULT 'legacy_unknown';
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS owner_user_id INTEGER;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS created_by INTEGER;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS storage_key TEXT;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS state TEXT;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS exposure TEXT DEFAULT 'public';
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS first_published_at TIMESTAMPTZ;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS source TEXT DEFAULT 'legacy';
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS derived_from_id INTEGER;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS checksum_sha256 TEXT;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS width INTEGER;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS height INTEGER;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS state_since TIMESTAMPTZ;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS references_complete BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS write_token UUID;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS write_lease_until TIMESTAMPTZ;
ALTER TABLE media_assets ADD COLUMN IF NOT EXISTS producer_key TEXT;

UPDATE media_assets SET public_id = gen_random_uuid() WHERE public_id IS NULL;
UPDATE media_assets SET scope = 'legacy_unknown' WHERE scope IS NULL;
UPDATE media_assets SET exposure = 'public' WHERE exposure IS NULL;
UPDATE media_assets SET source = 'legacy' WHERE source IS NULL;
UPDATE media_assets SET updated_at = created_at WHERE updated_at IS NULL;
UPDATE media_assets SET references_complete = FALSE WHERE references_complete IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_size_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_size_check CHECK (size >= 0);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_scope_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_scope_check
            CHECK (scope IS NULL OR scope IN ('site', 'user', 'legacy_unknown'));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_state_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_state_check
            CHECK (state IS NULL OR state IN ('staging', 'ready', 'deleting', 'missing', 'deleted'));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_exposure_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_exposure_check
            CHECK (exposure IS NULL OR exposure IN ('private', 'public'));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_source_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_source_check
            CHECK (source IS NULL OR source IN ('upload', 'generated', 'channel', 'import', 'legacy'));
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_owner_scope_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_owner_scope_check CHECK (
            (scope = 'user' AND owner_user_id IS NOT NULL)
            OR (scope IN ('site', 'legacy_unknown') AND owner_user_id IS NULL)
            OR scope IS NULL
        );
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_width_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_width_check
            CHECK (width IS NULL OR width > 0);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_height_check'
    ) THEN
        ALTER TABLE media_assets ADD CONSTRAINT media_assets_height_check
            CHECK (height IS NULL OR height > 0);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'media_assets_derived_from_id_fkey'
    ) THEN
        ALTER TABLE media_assets
            ADD CONSTRAINT media_assets_derived_from_id_fkey
            FOREIGN KEY (derived_from_id) REFERENCES media_assets(id) ON DELETE SET NULL;
    END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_media_assets_url ON media_assets (url);
CREATE INDEX IF NOT EXISTS idx_media_assets_kind ON media_assets (kind);
CREATE UNIQUE INDEX IF NOT EXISTS idx_media_assets_public_id ON media_assets (public_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_media_assets_storage_key ON media_assets (storage_key);
CREATE INDEX IF NOT EXISTS idx_media_assets_owner_created
    ON media_assets (scope, owner_user_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_media_assets_state_since
    ON media_assets (state, state_since);
CREATE INDEX IF NOT EXISTS idx_media_assets_source_created
    ON media_assets (source, created_at);

-- Site rows have NULL owner; NULLS NOT DISTINCT prevents two site producers
-- from sharing a producer_key by relying on UNIQUE NULL semantics.
CREATE UNIQUE INDEX IF NOT EXISTS idx_media_assets_producer_key
    ON media_assets (scope, owner_user_id, producer_key) NULLS NOT DISTINCT
    WHERE producer_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS media_references (
    id SERIAL PRIMARY KEY,
    asset_id INTEGER NOT NULL REFERENCES media_assets(id) ON DELETE RESTRICT,
    consumer_type TEXT NOT NULL,
    consumer_id TEXT NOT NULL,
    slot TEXT NOT NULL,
    requires_public BOOLEAN NOT NULL DEFAULT FALSE,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_media_references_slot
    ON media_references (asset_id, consumer_type, consumer_id, slot);
CREATE INDEX IF NOT EXISTS idx_media_references_consumer
    ON media_references (consumer_type, consumer_id);

CREATE TABLE IF NOT EXISTS media_url_aliases (
    id SERIAL PRIMARY KEY,
    local_path TEXT NOT NULL,
    asset_id INTEGER NOT NULL REFERENCES media_assets(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_media_url_aliases_local_path
    ON media_url_aliases (local_path);
CREATE INDEX IF NOT EXISTS idx_media_url_aliases_asset
    ON media_url_aliases (asset_id);

CREATE TABLE IF NOT EXISTS media_migration_jobs (
    id SERIAL PRIMARY KEY,
    source_kind TEXT NOT NULL,
    source_key TEXT NOT NULL,
    asset_id INTEGER REFERENCES media_assets(id) ON DELETE RESTRICT,
    copy_state TEXT NOT NULL DEFAULT 'pending',
    verify_state TEXT NOT NULL DEFAULT 'pending',
    switch_state TEXT NOT NULL DEFAULT 'pending',
    error_code TEXT,
    cursor TEXT,
    batch_version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_media_migration_jobs_source
    ON media_migration_jobs (source_kind, source_key);
CREATE INDEX IF NOT EXISTS idx_media_migration_jobs_copy_state
    ON media_migration_jobs (copy_state, id);

CREATE INDEX IF NOT EXISTS idx_media_assets_created_id ON media_assets (created_at, id);
