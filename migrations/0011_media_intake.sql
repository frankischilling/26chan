-- Installed transactionally by the migrator. Runtime receives functions only.
CREATE SCHEMA media_intake;
REVOKE ALL ON SCHEMA media_intake FROM PUBLIC;
CREATE TABLE media_intake.handles (
    job_id text PRIMARY KEY REFERENCES media.jobs(id) ON DELETE CASCADE,
    capability_hash bytea NOT NULL CHECK (octet_length(capability_hash) = 32),
    upload_started_at timestamptz
);
REVOKE ALL ON media_intake.handles FROM PUBLIC;

GRANT USAGE ON SCHEMA media, media_intake TO board_media_intake_owner;
GRANT SELECT ON media.queue_policy TO board_media_intake_owner;
GRANT UPDATE(singleton) ON media.queue_policy TO board_media_intake_owner;
GRANT SELECT(id, state, input_bytes, expires_at) ON media.jobs TO board_media_intake_owner;
GRANT INSERT(id, filename, expires_at) ON media.jobs TO board_media_intake_owner;
GRANT UPDATE(state, input_bytes, expires_at, updated_at, failure) ON media.jobs TO board_media_intake_owner;
GRANT SELECT ON media_intake.handles TO board_media_intake_owner;
GRANT INSERT(job_id, capability_hash) ON media_intake.handles TO board_media_intake_owner;
GRANT UPDATE(upload_started_at) ON media_intake.handles TO board_media_intake_owner;
GRANT SELECT(id, job_id, state) ON media.assets TO board_media_intake_owner;

CREATE FUNCTION media_intake.reserve(p_filename text)
RETURNS TABLE(id text, capability text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_capacity integer;
    v_pending bigint;
    v_id text;
    v_capability text;
BEGIN
    -- Admission relies on a fresh count after acquiring the policy row lock.
    -- Support only Read Committed at this directly callable SQL boundary.
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Intake reservation requires Read Committed.' USING ERRCODE = '22023';
    END IF;
    IF p_filename IS NULL OR octet_length(p_filename) NOT BETWEEN 1 AND 255
       OR EXISTS (SELECT 1 FROM generate_series(1, length(p_filename)) AS c(i)
                  WHERE ascii(substr(p_filename, c.i, 1)) BETWEEN 0 AND 31
                     OR ascii(substr(p_filename, c.i, 1)) BETWEEN 127 AND 159) THEN
        RAISE EXCEPTION 'Invalid intake metadata.' USING ERRCODE = '22023';
    END IF;
    SELECT q.capacity INTO STRICT v_capacity FROM media.queue_policy q WHERE q.singleton FOR UPDATE;
    SELECT count(*) INTO v_pending FROM media.jobs j WHERE j.state IN ('receiving', 'queued', 'processing');
    IF v_pending >= v_capacity THEN
        RAISE EXCEPTION 'Media queue is full.' USING ERRCODE = 'P0001';
    END IF;
    v_id := replace(gen_random_uuid()::text, '-', '');
    v_capability := replace(gen_random_uuid()::text, '-', '') || replace(gen_random_uuid()::text, '-', '');
    INSERT INTO media.jobs(id, filename, expires_at)
        VALUES (v_id, p_filename, clock_timestamp() + interval '5 minutes');
    INSERT INTO media_intake.handles(job_id, capability_hash)
        VALUES (v_id, sha256(convert_to(v_capability, 'UTF8')));
    RETURN QUERY SELECT v_id, v_capability;
END
$$;

CREATE FUNCTION media_intake.begin_upload(p_id text, p_capability text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_state text;
    v_expires timestamptz;
    v_hash bytea;
    v_started timestamptz;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.expires_at INTO v_state, v_expires FROM media.jobs j WHERE j.id = p_id FOR UPDATE;
    SELECT h.capability_hash, h.upload_started_at INTO v_hash, v_started
        FROM media_intake.handles h WHERE h.job_id = p_id FOR UPDATE;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR (v_state IN ('receiving', 'queued') AND v_expires <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    IF v_state <> 'receiving' OR v_started IS NOT NULL THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE = 'P0001';
    END IF;
    UPDATE media_intake.handles SET upload_started_at = clock_timestamp() WHERE job_id = p_id;
END
$$;

CREATE FUNCTION media_intake.finish_upload(p_id text, p_capability text, p_input_bytes bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_state text;
    v_expires timestamptz;
    v_hash bytea;
    v_started timestamptz;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.expires_at INTO v_state, v_expires FROM media.jobs j WHERE j.id = p_id FOR UPDATE;
    SELECT h.capability_hash, h.upload_started_at INTO v_hash, v_started
        FROM media_intake.handles h WHERE h.job_id = p_id FOR UPDATE;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR (v_state IN ('receiving', 'queued') AND v_expires <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    IF p_input_bytes IS NULL OR p_input_bytes NOT BETWEEN 1 AND 8388608 THEN
        RAISE EXCEPTION 'Invalid intake size.' USING ERRCODE = '22023';
    END IF;
    IF v_state <> 'receiving' OR v_started IS NULL THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE = 'P0001';
    END IF;
    UPDATE media.jobs SET state = 'queued', input_bytes = p_input_bytes,
        expires_at = clock_timestamp() + interval '1 hour', updated_at = clock_timestamp()
        WHERE media.jobs.id = p_id;
END
$$;

CREATE FUNCTION media_intake.abort_upload(p_id text, p_capability text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_state text;
    v_expires timestamptz;
    v_hash bytea;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.expires_at INTO v_state, v_expires FROM media.jobs j WHERE j.id = p_id FOR UPDATE;
    SELECT h.capability_hash INTO v_hash FROM media_intake.handles h WHERE h.job_id = p_id FOR UPDATE;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR (v_state IN ('receiving', 'queued') AND v_expires <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    IF v_state <> 'receiving' THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE = 'P0001';
    END IF;
    UPDATE media.jobs SET state = 'failed', failure = 'intake_failed', expires_at = NULL,
        updated_at = clock_timestamp() WHERE media.jobs.id = p_id;
END
$$;

CREATE FUNCTION media_intake.status(p_id text, p_capability text)
RETURNS TABLE(id text, state text, input_bytes bigint, output_id text)
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    RETURN QUERY
        SELECT j.id, CASE WHEN j.state = 'receiving' AND h.upload_started_at IS NOT NULL THEN 'uploading' ELSE j.state END,
               j.input_bytes,
               (SELECT a.id FROM media.assets a WHERE a.job_id = j.id AND a.state = 'approved' ORDER BY a.id LIMIT 1)
        FROM media.jobs j JOIN media_intake.handles h ON h.job_id = j.id
        WHERE j.id = p_id AND h.capability_hash = sha256(convert_to(p_capability, 'UTF8'))
          AND (j.state NOT IN ('receiving', 'queued') OR j.expires_at > clock_timestamp());
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
END
$$;

CREATE FUNCTION media_intake.ready() RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
    SELECT EXISTS (SELECT 1 FROM media.queue_policy q WHERE q.singleton)
$$;

REVOKE ALL ON ALL FUNCTIONS IN SCHEMA media_intake FROM PUBLIC;
GRANT USAGE ON SCHEMA media_intake TO board_media_intake;
GRANT EXECUTE ON FUNCTION media_intake.reserve(text), media_intake.begin_upload(text, text),
    media_intake.finish_upload(text, text, bigint), media_intake.abort_upload(text, text),
    media_intake.status(text, text), media_intake.ready() TO board_media_intake;
-- Ownership transfer needs CREATE only during this transaction.
GRANT CREATE ON SCHEMA media_intake TO board_media_intake_owner;
ALTER FUNCTION media_intake.reserve(text) OWNER TO board_media_intake_owner;
ALTER FUNCTION media_intake.begin_upload(text, text) OWNER TO board_media_intake_owner;
ALTER FUNCTION media_intake.finish_upload(text, text, bigint) OWNER TO board_media_intake_owner;
ALTER FUNCTION media_intake.abort_upload(text, text) OWNER TO board_media_intake_owner;
ALTER FUNCTION media_intake.status(text, text) OWNER TO board_media_intake_owner;
ALTER FUNCTION media_intake.ready() OWNER TO board_media_intake_owner;
REVOKE CREATE ON SCHEMA media_intake FROM board_media_intake_owner;
