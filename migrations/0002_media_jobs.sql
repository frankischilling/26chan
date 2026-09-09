CREATE SCHEMA media;
REVOKE ALL ON SCHEMA media FROM PUBLIC;

CREATE TABLE media.queue_policy (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    capacity integer NOT NULL CHECK (capacity BETWEEN 1 AND 1024)
);
INSERT INTO media.queue_policy (capacity) VALUES (64);

CREATE TABLE media.jobs (
    id text PRIMARY KEY CHECK (id ~ '^[0-9a-f]{32}$'),
    filename text NOT NULL CHECK (octet_length(filename) BETWEEN 1 AND 255),
    state text NOT NULL DEFAULT 'receiving'
        CHECK (state IN ('receiving', 'queued', 'processing', 'published', 'failed')),
    input_bytes bigint CHECK (input_bytes BETWEEN 1 AND 8388608),
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts BETWEEN 0 AND 3),
    lease_token text CHECK (lease_token ~ '^[0-9a-f]{32}$'),
    expires_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    output_sha256 text CHECK (output_sha256 ~ '^[0-9a-f]{64}$'),
    output_bytes bigint CHECK (output_bytes BETWEEN 1 AND 5242880),
    failure text CHECK (failure IN ('intake_failed', 'abandoned', 'processing_failed', 'invalid_output', 'retry_exhausted')),
    CHECK ((state IN ('receiving', 'processing')) = (expires_at IS NOT NULL)),
    CHECK ((state IN ('processing', 'published')) = (lease_token IS NOT NULL)),
    CHECK ((state = 'published') = (output_sha256 IS NOT NULL AND output_bytes IS NOT NULL)),
    CHECK ((state = 'failed') = (failure IS NOT NULL)),
    CHECK (state NOT IN ('queued', 'processing', 'published') OR input_bytes IS NOT NULL),
    CHECK (state NOT IN ('processing', 'published') OR attempts > 0)
);
CREATE INDEX media_pending ON media.jobs (created_at, id) WHERE state = 'queued';
CREATE INDEX media_expiration ON media.jobs (expires_at, id) WHERE expires_at IS NOT NULL;
CREATE INDEX media_terminal ON media.jobs (updated_at, id) WHERE state IN ('failed', 'published');

GRANT USAGE ON SCHEMA media TO board_media;
GRANT SELECT ON media.queue_policy TO board_media;
-- UPDATE privilege is required for the admission row lock, but the capacity is operator-owned.
GRANT UPDATE(singleton) ON media.queue_policy TO board_media;
GRANT SELECT, INSERT, UPDATE, DELETE ON media.jobs TO board_media;
