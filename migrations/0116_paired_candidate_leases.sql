-- Inactive candidate-only processing. Publication and asset guards from 0115
-- remain intact; a checked candidate is a discarded result, never approval.
ALTER TABLE media.jobs DROP CONSTRAINT media_input_shape,
    ADD CONSTRAINT media_input_shape CHECK (
      (input_kind='image-v1' AND (input_bytes IS NULL OR input_bytes BETWEEN 1 AND 8388608)
       AND input_sha256 IS NULL AND input_image_bytes IS NULL AND input_image_sha256 IS NULL
       AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL)
      OR
      (input_kind='paired-v2' AND state IN ('receiving','queued','processing','failed')
       AND output_sha256 IS NULL AND output_bytes IS NULL
       AND ((state='processing' AND attempts BETWEEN 1 AND 3 AND lease_token IS NOT NULL)
         OR (state<>'processing' AND lease_token IS NULL))
       AND (
         (state IN ('receiving','failed') AND input_bytes IS NULL AND input_sha256 IS NULL
          AND input_image_bytes IS NULL AND input_image_sha256 IS NULL
          AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL)
         OR
         (state IN ('queued','processing','failed') AND input_bytes IS NOT NULL AND input_sha256 IS NOT NULL
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


ALTER TABLE media.jobs DROP CONSTRAINT jobs_failure_check,
 ADD CONSTRAINT jobs_failure_check CHECK (failure IN
 ('intake_failed','abandoned','processing_failed','invalid_output','retry_exhausted','candidate_checked')
 AND (failure<>'candidate_checked' OR input_kind='paired-v2'));

CREATE FUNCTION media.guard_paired_claim() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF NEW.input_kind='paired-v2' AND current_user <> 'board_migrator' THEN
        IF NEW.attempts IS DISTINCT FROM OLD.attempts
           OR (OLD.state='queued' AND NEW.state='queued'
               AND NEW.expires_at IS DISTINCT FROM OLD.expires_at)
           OR (OLD.state='processing' AND NEW.state='queued'
               AND (OLD.attempts>=3 OR NEW.expires_at IS NULL
                    OR NEW.expires_at>clock_timestamp()+interval '1 hour')) THEN
            RAISE EXCEPTION 'Paired attempt and retry bounds are immutable.' USING ERRCODE='42501';
        END IF;
    END IF;
    IF NEW.input_kind='paired-v2' AND NEW.state='processing'
       AND (OLD.state,OLD.attempts,OLD.lease_token,OLD.expires_at) IS DISTINCT FROM
           (NEW.state,NEW.attempts,NEW.lease_token,NEW.expires_at)
       AND current_user <> 'board_migrator' THEN
        RAISE EXCEPTION 'Paired processing requires typed claim authority.' USING ERRCODE='42501';
    END IF;
    IF NEW.failure='candidate_checked'
       AND (NEW.input_kind<>'paired-v2' OR current_user <> 'board_migrator') THEN
        RAISE EXCEPTION 'Candidate check requires typed completion authority.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_paired_claim() FROM PUBLIC;
CREATE TRIGGER media_paired_claim_guard BEFORE UPDATE ON media.jobs
 FOR EACH ROW EXECUTE FUNCTION media.guard_paired_claim();

CREATE FUNCTION media.claim_paired_candidate()
RETURNS SETOF media.jobs
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_id text;
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Candidate claim requires Read Committed.' USING ERRCODE='22023';
    END IF;
    SELECT j.id INTO v_id FROM media.jobs j
      WHERE j.input_kind='paired-v2' AND j.state='queued' AND j.attempts<3
        AND j.expires_at>clock_timestamp()
      ORDER BY j.created_at,j.id FOR UPDATE SKIP LOCKED LIMIT 1;
    IF NOT FOUND THEN RETURN; END IF;
    RETURN QUERY UPDATE media.jobs j SET state='processing',attempts=j.attempts+1,
      lease_token=replace(gen_random_uuid()::text,'-',''),
      expires_at=clock_timestamp()+interval '30 seconds',updated_at=clock_timestamp()
      WHERE j.id=v_id AND j.expires_at>clock_timestamp() RETURNING j.*;
END $$;

CREATE FUNCTION media.finish_paired_candidate(p_id text,p_token text,p_checked boolean)
RETURNS boolean
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_job media.jobs%ROWTYPE;
BEGIN
    IF p_id IS NULL OR octet_length(p_id)<>32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_token IS NULL OR octet_length(p_token)<>32 OR p_token !~ '^[0-9a-f]{32}$'
       OR p_checked IS NULL THEN RETURN false; END IF;
    SELECT j.* INTO v_job FROM media.jobs j WHERE j.id=p_id FOR UPDATE;
    IF v_job.id IS NULL OR v_job.input_kind<>'paired-v2' OR v_job.state<>'processing'
       OR v_job.lease_token<>p_token OR v_job.expires_at<=clock_timestamp() THEN RETURN false; END IF;
    UPDATE media.jobs SET state='failed',lease_token=NULL,expires_at=NULL,
      failure=CASE WHEN p_checked THEN 'candidate_checked' ELSE 'processing_failed' END,
      updated_at=clock_timestamp() WHERE id=p_id;
    RETURN true;
END $$;
REVOKE ALL ON FUNCTION media.claim_paired_candidate(),media.finish_paired_candidate(text,text,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media.claim_paired_candidate(),media.finish_paired_candidate(text,text,boolean) TO board_media;

ALTER FUNCTION media.claim_paired_candidate() OWNER TO board_migrator;
ALTER FUNCTION media.finish_paired_candidate(text,text,boolean) OWNER TO board_migrator;
