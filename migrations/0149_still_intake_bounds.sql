-- Only context-bound image intake can use the source ten-MiB ceiling.
-- Paired jobs and legacy image reservations retain their existing byte bounds.
GRANT SELECT(job_id) ON media.image_context TO board_media_intake_owner;

ALTER TABLE media.jobs DROP CONSTRAINT media_input_shape,
    ADD CONSTRAINT media_input_shape CHECK (
      (input_kind='image-v1' AND NOT drawing_publication AND (input_bytes IS NULL OR input_bytes BETWEEN 1 AND 10485760)
       AND input_sha256 IS NULL AND input_image_bytes IS NULL AND input_image_sha256 IS NULL
       AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL)
      OR
      (input_kind='paired-v2' AND state IN ('receiving','queued','processing','failed','published')
       AND (state<>'published' OR drawing_publication)
       AND ((output_sha256 IS NULL AND output_bytes IS NULL) OR (drawing_publication AND state='published'))
       AND ((state IN ('processing','published') AND attempts BETWEEN 1 AND 3 AND lease_token IS NOT NULL)
         OR (state NOT IN ('processing','published') AND lease_token IS NULL))
       AND (
         (state IN ('receiving','failed') AND input_bytes IS NULL AND input_sha256 IS NULL
          AND input_image_bytes IS NULL AND input_image_sha256 IS NULL
          AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL)
         OR
         (state IN ('queued','processing','failed','published') AND input_bytes IS NOT NULL AND input_sha256 IS NOT NULL
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

CREATE FUNCTION media.guard_still_input_bound() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF NEW.input_kind='image-v1' AND NEW.input_bytes>8388608
        AND NOT EXISTS(SELECT 1 FROM media.image_context WHERE job_id=NEW.id) THEN
        RAISE EXCEPTION 'Larger input requires an image posting context.' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_still_input_bound() FROM PUBLIC;
CREATE TRIGGER media_still_input_bound BEFORE INSERT OR UPDATE ON media.jobs
    FOR EACH ROW EXECUTE FUNCTION media.guard_still_input_bound();

CREATE FUNCTION media_intake.input_limit(p_id text,p_capability text) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    -- Validate the capability and expiration before revealing a context bound.
    PERFORM 1 FROM media_intake.status(p_id,p_capability);
    RETURN CASE WHEN EXISTS(SELECT 1 FROM media.image_context WHERE job_id=p_id)
        THEN 10485760::bigint ELSE 8388608::bigint END;
END $$;
REVOKE ALL ON FUNCTION media_intake.input_limit(text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media_intake.input_limit(text,text) TO board_media_intake;
GRANT CREATE ON SCHEMA media_intake TO board_media_intake_owner;
ALTER FUNCTION media_intake.input_limit(text,text) OWNER TO board_media_intake_owner;
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
    IF p_input_bytes IS NULL OR p_input_bytes NOT BETWEEN 1 AND media_intake.input_limit(p_id,p_capability) THEN
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
REVOKE CREATE ON SCHEMA media_intake FROM board_media_intake_owner;
