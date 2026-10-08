-- Inactive paired intake: durable provenance without replay processing authority.
-- Existing image reservations retain their original size and function contract.
ALTER TABLE media.jobs
    ADD COLUMN input_kind text NOT NULL DEFAULT 'image-v1',
    ADD COLUMN input_sha256 text,
    ADD COLUMN input_image_bytes bigint,
    ADD COLUMN input_image_sha256 text,
    ADD COLUMN input_replay_bytes bigint,
    ADD COLUMN input_replay_sha256 text,
    DROP CONSTRAINT jobs_input_bytes_check,
    ADD CONSTRAINT media_input_shape CHECK (
      (input_kind='image-v1' AND (input_bytes IS NULL OR input_bytes BETWEEN 1 AND 8388608)
       AND input_sha256 IS NULL AND input_image_bytes IS NULL AND input_image_sha256 IS NULL
       AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL)
      OR
      (input_kind='paired-v2' AND state IN ('receiving','queued','failed')
       AND attempts=0 AND lease_token IS NULL AND output_sha256 IS NULL AND output_bytes IS NULL
       AND (
         (state IN ('receiving','failed') AND input_bytes IS NULL AND input_sha256 IS NULL
          AND input_image_bytes IS NULL AND input_image_sha256 IS NULL
          AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL)
         OR
         (state IN ('queued','failed') AND input_bytes IS NOT NULL AND input_sha256 IS NOT NULL
          AND input_image_bytes IS NOT NULL AND input_image_sha256 IS NOT NULL
          AND octet_length(input_sha256)=64 AND input_sha256 ~ '^[0-9a-f]{64}$'
          AND octet_length(input_image_sha256)=64 AND input_image_sha256 ~ '^[0-9a-f]{64}$'
          AND input_image_bytes BETWEEN 1 AND 8388608
          AND ((input_replay_bytes IS NULL AND input_replay_sha256 IS NULL)
            OR (input_replay_bytes IS NOT NULL AND input_replay_sha256 IS NOT NULL
                AND input_replay_bytes BETWEEN 1 AND 8388608
                AND octet_length(input_replay_sha256)=64 AND input_replay_sha256 ~ '^[0-9a-f]{64}$'))
          AND input_bytes BETWEEN 57 AND 16777272
          AND input_bytes=input_image_bytes+COALESCE(input_replay_bytes,0)+56)
       ))
    );

CREATE FUNCTION media.guard_input_descriptor() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF NEW.input_kind IS DISTINCT FROM OLD.input_kind THEN
            RAISE EXCEPTION 'Media input kind is immutable.' USING ERRCODE='42501';
        END IF;
        IF OLD.input_kind='paired-v2' AND OLD.state='failed' AND NEW.state <> 'failed' THEN
            RAISE EXCEPTION 'Terminal paired input cannot be requeued.' USING ERRCODE='42501';
        END IF;
        IF OLD.input_kind='paired-v2' AND OLD.input_bytes IS NOT NULL AND
           (NEW.input_bytes,NEW.input_sha256,NEW.input_image_bytes,NEW.input_image_sha256,
            NEW.input_replay_bytes,NEW.input_replay_sha256) IS DISTINCT FROM
           (OLD.input_bytes,OLD.input_sha256,OLD.input_image_bytes,OLD.input_image_sha256,
            OLD.input_replay_bytes,OLD.input_replay_sha256) THEN
            RAISE EXCEPTION 'Paired input provenance is immutable.' USING ERRCODE='42501';
        END IF;
        IF NEW.input_kind='paired-v2' AND OLD.input_bytes IS NULL
           AND (NEW.input_bytes IS NOT NULL OR NEW.state='queued')
           AND (current_user <> 'board_media_intake_owner' OR OLD.state <> 'receiving'
                OR NEW.state <> 'queued') THEN
            RAISE EXCEPTION 'Paired finalization requires intake authority.' USING ERRCODE='42501';
        END IF;
    ELSIF NEW.input_kind='paired-v2' AND
          (current_user <> 'board_media_intake_owner' OR NEW.state <> 'receiving'
           OR NEW.input_bytes IS NOT NULL) THEN
        RAISE EXCEPTION 'Paired reservation requires intake authority.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_input_descriptor() FROM PUBLIC;
CREATE TRIGGER media_input_descriptor_immutable BEFORE INSERT OR UPDATE ON media.jobs
    FOR EACH ROW EXECUTE FUNCTION media.guard_input_descriptor();

-- No current asset path, including legacy/manual manifests without provenance,
-- may approve a known pair. Later replay admission must replace this boundary.
CREATE FUNCTION media.guard_inactive_pair_asset() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog AS $$
BEGIN
    PERFORM 1 FROM media.jobs WHERE id=NEW.job_id AND input_kind='paired-v2' FOR UPDATE;
    IF FOUND THEN
        RAISE EXCEPTION 'Paired publication is not enabled.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_inactive_pair_asset() FROM PUBLIC;
CREATE TRIGGER media_inactive_pair_asset BEFORE INSERT OR UPDATE OF job_id ON media.assets
    FOR EACH ROW EXECUTE FUNCTION media.guard_inactive_pair_asset();

GRANT SELECT(input_kind,input_sha256,input_image_bytes,input_image_sha256,input_replay_bytes,input_replay_sha256)
    ON media.jobs TO board_media_intake_owner;
GRANT INSERT(input_kind) ON media.jobs TO board_media_intake_owner;
GRANT UPDATE(input_sha256,input_image_bytes,input_image_sha256,input_replay_bytes,input_replay_sha256)
    ON media.jobs TO board_media_intake_owner;

CREATE FUNCTION media_intake.reserve_pair(p_filename text)
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
    INSERT INTO media.jobs(id, filename, expires_at, input_kind)
        VALUES (v_id, p_filename, clock_timestamp() + interval '5 minutes', 'paired-v2');
    INSERT INTO media_intake.handles(job_id, capability_hash)
        VALUES (v_id, sha256(convert_to(v_capability, 'UTF8')));
    RETURN QUERY SELECT v_id, v_capability;
END
$$;

GRANT CREATE ON SCHEMA media_intake TO board_media_intake_owner;
SET ROLE board_media_intake_owner;
CREATE OR REPLACE FUNCTION media_intake.begin_upload(p_id text, p_capability text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_state text;
    v_kind text;
    v_expires timestamptz;
    v_hash bytea;
    v_started timestamptz;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.expires_at, j.input_kind INTO v_state, v_expires, v_kind FROM media.jobs j WHERE j.id = p_id FOR UPDATE;
    SELECT h.capability_hash, h.upload_started_at INTO v_hash, v_started
        FROM media_intake.handles h WHERE h.job_id = p_id FOR UPDATE;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR (v_state IN ('receiving', 'queued') AND v_expires <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    IF v_kind <> 'image-v1' OR v_state <> 'receiving' OR v_started IS NOT NULL THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE = 'P0001';
    END IF;
    UPDATE media_intake.handles SET upload_started_at = clock_timestamp() WHERE job_id = p_id;
END
$$;
RESET ROLE;

CREATE FUNCTION media_intake.begin_pair_upload(p_id text, p_capability text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_state text;
    v_kind text;
    v_expires timestamptz;
    v_hash bytea;
    v_started timestamptz;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.expires_at, j.input_kind INTO v_state, v_expires, v_kind FROM media.jobs j WHERE j.id = p_id FOR UPDATE;
    SELECT h.capability_hash, h.upload_started_at INTO v_hash, v_started
        FROM media_intake.handles h WHERE h.job_id = p_id FOR UPDATE;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR (v_state IN ('receiving', 'queued') AND v_expires <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    IF v_kind <> 'paired-v2' OR v_state <> 'receiving' OR v_started IS NOT NULL THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE = 'P0001';
    END IF;
    UPDATE media_intake.handles SET upload_started_at = clock_timestamp() WHERE job_id = p_id;
END
$$;

SET ROLE board_media_intake_owner;
CREATE OR REPLACE FUNCTION media_intake.finish_upload(p_id text, p_capability text, p_input_bytes bigint) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_state text;
    v_kind text;
    v_expires timestamptz;
    v_hash bytea;
    v_started timestamptz;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.expires_at, j.input_kind INTO v_state, v_expires, v_kind FROM media.jobs j WHERE j.id = p_id FOR UPDATE;
    SELECT h.capability_hash, h.upload_started_at INTO v_hash, v_started
        FROM media_intake.handles h WHERE h.job_id = p_id FOR UPDATE;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR (v_state IN ('receiving', 'queued') AND v_expires <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    IF p_input_bytes IS NULL OR p_input_bytes NOT BETWEEN 1 AND 8388608 THEN
        RAISE EXCEPTION 'Invalid intake size.' USING ERRCODE = '22023';
    END IF;
    IF v_kind <> 'image-v1' OR v_state <> 'receiving' OR v_started IS NULL THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE = 'P0001';
    END IF;
    UPDATE media.jobs SET state = 'queued', input_bytes = p_input_bytes,
        expires_at = clock_timestamp() + interval '1 hour', updated_at = clock_timestamp()
        WHERE media.jobs.id = p_id;
END
$$;
RESET ROLE;

CREATE FUNCTION media_intake.finish_pair_upload(
    p_id text, p_capability text, p_bytes bigint, p_sha256 text,
    p_image_bytes bigint, p_image_sha256 text, p_replay_bytes bigint, p_replay_sha256 text
) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_job record;
    v_hash bytea;
    v_started timestamptz;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE='P0002';
    END IF;
    SELECT j.state,j.expires_at,j.input_kind,j.input_bytes,j.input_sha256,
           j.input_image_bytes,j.input_image_sha256,j.input_replay_bytes,j.input_replay_sha256
        INTO v_job FROM media.jobs j WHERE j.id=p_id FOR UPDATE;
    SELECT h.capability_hash,h.upload_started_at INTO v_hash,v_started
        FROM media_intake.handles h WHERE h.job_id=p_id FOR UPDATE;
    -- Authentication and fresh expiry checks precede metadata diagnostics.
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability,'UTF8'))
       OR (v_job.state IN ('receiving','queued') AND v_job.expires_at <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE='P0002';
    END IF;
    IF p_bytes IS NULL OR p_bytes NOT BETWEEN 57 AND 16777272
       OR p_sha256 IS NULL OR octet_length(p_sha256) <> 64 OR p_sha256 !~ '^[0-9a-f]{64}$'
       OR p_image_bytes IS NULL OR p_image_bytes NOT BETWEEN 1 AND 8388608
       OR p_image_sha256 IS NULL OR octet_length(p_image_sha256) <> 64 OR p_image_sha256 !~ '^[0-9a-f]{64}$'
       OR ((p_replay_bytes IS NULL) <> (p_replay_sha256 IS NULL))
       OR (p_replay_bytes IS NOT NULL AND (p_replay_bytes NOT BETWEEN 1 AND 8388608
           OR octet_length(p_replay_sha256) <> 64 OR p_replay_sha256 !~ '^[0-9a-f]{64}$')) THEN
        RAISE EXCEPTION 'Invalid paired input descriptor.' USING ERRCODE='22023';
    END IF;
    -- Arithmetic follows component validation, so hostile bigint declarations
    -- cannot overflow before the bounded error above.
    IF p_bytes <> p_image_bytes+COALESCE(p_replay_bytes,0)+56 THEN
        RAISE EXCEPTION 'Invalid paired input length.' USING ERRCODE='22023';
    END IF;
    IF v_job.input_kind <> 'paired-v2' OR v_started IS NULL THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE='P0001';
    END IF;
    IF v_job.state='queued' AND
       (v_job.input_bytes,v_job.input_sha256,v_job.input_image_bytes,v_job.input_image_sha256,
        v_job.input_replay_bytes,v_job.input_replay_sha256) IS NOT DISTINCT FROM
       (p_bytes,p_sha256,p_image_bytes,p_image_sha256,p_replay_bytes,p_replay_sha256) THEN
        -- Reconcile an uncertain commit without extending expiry or rewriting data.
        RETURN;
    END IF;
    IF v_job.state <> 'receiving' THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE='P0001';
    END IF;
    UPDATE media.jobs SET state='queued',input_bytes=p_bytes,input_sha256=p_sha256,
        input_image_bytes=p_image_bytes,input_image_sha256=p_image_sha256,
        input_replay_bytes=p_replay_bytes,input_replay_sha256=p_replay_sha256,
        expires_at=clock_timestamp()+interval '1 hour',updated_at=clock_timestamp()
        WHERE id=p_id;
END $$;
REVOKE ALL ON FUNCTION media_intake.reserve_pair(text),media_intake.begin_pair_upload(text,text),
    media_intake.finish_pair_upload(text,text,bigint,text,bigint,text,bigint,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media_intake.reserve_pair(text),media_intake.begin_pair_upload(text,text),
    media_intake.finish_pair_upload(text,text,bigint,text,bigint,text,bigint,text) TO board_media_intake;
GRANT CREATE ON SCHEMA media_intake TO board_media_intake_owner;
ALTER FUNCTION media_intake.reserve_pair(text) OWNER TO board_media_intake_owner;
ALTER FUNCTION media_intake.begin_pair_upload(text,text) OWNER TO board_media_intake_owner;
ALTER FUNCTION media_intake.finish_pair_upload(text,text,bigint,text,bigint,text,bigint,text) OWNER TO board_media_intake_owner;
REVOKE CREATE ON SCHEMA media_intake FROM board_media_intake_owner;
