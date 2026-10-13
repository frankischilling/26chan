-- Drawing admission is an explicit enrollment within the paired wire format.
-- Historical and qualification-only pairs remain non-publishable by default.
ALTER TABLE media.jobs ADD COLUMN drawing_publication boolean NOT NULL DEFAULT false;

CREATE FUNCTION media.guard_drawing_enrollment() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF TG_OP='UPDATE' AND NEW.drawing_publication IS DISTINCT FROM OLD.drawing_publication THEN
        RAISE EXCEPTION 'Drawing enrollment is immutable.' USING ERRCODE='42501';
    END IF;
    IF TG_OP='UPDATE' AND NEW.drawing_publication AND NEW.state='published'
       AND current_user<>'board_migrator' THEN
        RAISE EXCEPTION 'Drawing completion requires typed authority.' USING ERRCODE='42501';
    END IF;
    IF TG_OP='INSERT' AND NEW.drawing_publication
       AND (current_user<>'board_media_intake_owner' OR NEW.input_kind<>'paired-v2'
            OR NEW.state<>'receiving' OR NEW.input_bytes IS NOT NULL) THEN
        RAISE EXCEPTION 'Drawing enrollment requires intake authority.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_drawing_enrollment() FROM PUBLIC;
CREATE TRIGGER media_drawing_enrollment BEFORE INSERT OR UPDATE ON media.jobs
 FOR EACH ROW EXECUTE FUNCTION media.guard_drawing_enrollment();
GRANT SELECT(drawing_publication),INSERT(drawing_publication) ON media.jobs TO board_media_intake_owner;

ALTER TABLE media.jobs DROP CONSTRAINT media_input_shape,
    ADD CONSTRAINT media_input_shape CHECK (
      (input_kind='image-v1' AND NOT drawing_publication AND (input_bytes IS NULL OR input_bytes BETWEEN 1 AND 8388608)
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

CREATE FUNCTION media_intake.reserve_drawing(p_filename text)
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
    INSERT INTO media.jobs(id, filename, expires_at, input_kind, drawing_publication)
        VALUES (v_id, p_filename, clock_timestamp() + interval '5 minutes', 'paired-v2', true);
    INSERT INTO media_intake.handles(job_id, capability_hash)
        VALUES (v_id, sha256(convert_to(v_capability, 'UTF8')));
    RETURN QUERY SELECT v_id, v_capability;
END
$$;

CREATE OR REPLACE FUNCTION media.claim_paired_candidate()
RETURNS SETOF media.jobs
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_id text;
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Candidate claim requires Read Committed.' USING ERRCODE='22023';
    END IF;
    SELECT j.id INTO v_id FROM media.jobs j
      WHERE j.input_kind='paired-v2' AND NOT j.drawing_publication AND j.state='queued' AND j.attempts<3
        AND j.expires_at>clock_timestamp()
      ORDER BY j.created_at,j.id FOR UPDATE SKIP LOCKED LIMIT 1;
    IF NOT FOUND THEN RETURN; END IF;
    RETURN QUERY UPDATE media.jobs j SET state='processing',attempts=j.attempts+1,
      lease_token=replace(gen_random_uuid()::text,'-',''),
      expires_at=clock_timestamp()+interval '30 seconds',updated_at=clock_timestamp()
      WHERE j.id=v_id AND j.expires_at>clock_timestamp() RETURNING j.*;
END $$;

CREATE FUNCTION media.claim_drawing()
RETURNS SETOF media.jobs
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_id text;
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Candidate claim requires Read Committed.' USING ERRCODE='22023';
    END IF;
    SELECT j.id INTO v_id FROM media.jobs j
      WHERE j.input_kind='paired-v2' AND j.drawing_publication AND j.state='queued' AND j.attempts<3
        AND j.expires_at>clock_timestamp()
      ORDER BY j.created_at,j.id FOR UPDATE SKIP LOCKED LIMIT 1;
    IF NOT FOUND THEN RETURN; END IF;
    RETURN QUERY UPDATE media.jobs j SET state='processing',attempts=j.attempts+1,
      lease_token=replace(gen_random_uuid()::text,'-',''),
      expires_at=clock_timestamp()+interval '30 seconds',updated_at=clock_timestamp()
      WHERE j.id=v_id AND j.expires_at>clock_timestamp() RETURNING j.*;
END $$;

REVOKE ALL ON FUNCTION media_intake.reserve_drawing(text),media.claim_drawing() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media_intake.reserve_drawing(text) TO board_media_intake;
GRANT EXECUTE ON FUNCTION media.claim_drawing() TO board_media;
GRANT CREATE ON SCHEMA media_intake TO board_media_intake_owner;
ALTER FUNCTION media_intake.reserve_drawing(text) OWNER TO board_media_intake_owner;
REVOKE CREATE ON SCHEMA media_intake FROM board_media_intake_owner;
ALTER FUNCTION media.claim_drawing() OWNER TO board_migrator;

-- A canonical replay is a companion of one immutable image approval. Its
-- existence alone does not expose it: the reader view requires a visible post.
CREATE TABLE media.replay_assets (
    asset_id text PRIMARY KEY REFERENCES media.assets(id) ON DELETE CASCADE,
    sha256 text NOT NULL CHECK (octet_length(sha256)=64 AND sha256 ~ '^[0-9a-f]{64}$'),
    bytes bigint NOT NULL CHECK (bytes BETWEEN 288 AND 262400 AND (bytes-256)%16=0),
    width integer NOT NULL CHECK (width BETWEEN 1 AND 1024),
    height integer NOT NULL CHECK (height BETWEEN 1 AND 1024)
);
REVOKE ALL ON TABLE media.replay_assets FROM PUBLIC;

CREATE FUNCTION media.guard_replay_asset() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        RAISE EXCEPTION 'Replay metadata is immutable.' USING ERRCODE='42501';
    END IF;
    IF current_user<>'board_migrator' OR NOT EXISTS (
        SELECT 1 FROM media.assets a JOIN media.jobs j ON j.id=a.job_id
        WHERE a.id=NEW.asset_id AND a.state='pending' AND a.output_format='png'
          AND a.width=NEW.width AND a.height=NEW.height
          AND j.input_kind='paired-v2' AND j.drawing_publication
          AND j.state='processing' AND j.lease_token=a.lease_token
          AND j.expires_at>clock_timestamp() AND j.input_replay_bytes IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'Replay metadata requires a current drawing reservation.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_replay_asset() FROM PUBLIC;
CREATE TRIGGER media_replay_asset_guard BEFORE INSERT OR UPDATE ON media.replay_assets
 FOR EACH ROW EXECUTE FUNCTION media.guard_replay_asset();

CREATE VIEW media.approved_post_replays WITH (security_barrier=true) AS
 SELECT a.board,a.tim,r.asset_id AS id,r.sha256,r.bytes,r.width,r.height
 FROM media.approved_post_assets a JOIN media.replay_assets r ON r.asset_id=a.id
 WHERE EXISTS(SELECT 1 FROM content.boards b WHERE b.slug=a.board AND b.oekaki_replays);
GRANT SELECT ON media.approved_post_replays TO board_media_read;

CREATE OR REPLACE FUNCTION media.guard_inactive_pair_asset() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
DECLARE v_job media.jobs%ROWTYPE;
BEGIN
    SELECT * INTO v_job FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
    IF v_job.input_kind='paired-v2' AND (
        NOT v_job.drawing_publication OR current_user<>'board_migrator'
        OR NEW.state<>'pending' OR v_job.state<>'processing'
        OR v_job.lease_token IS DISTINCT FROM NEW.lease_token
        OR v_job.expires_at<=clock_timestamp()) THEN
        RAISE EXCEPTION 'Paired publication requires drawing authority.' USING ERRCODE='42501';
    END IF;
    RETURN NEW;
END $$;

CREATE OR REPLACE FUNCTION media.guard_source_provenance() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (NEW.source_input_sha256,NEW.source_input_bytes,NEW.source_profile,
            NEW.source_retained_bytes,NEW.source_md5) IS DISTINCT FROM
           (OLD.source_input_sha256,OLD.source_input_bytes,OLD.source_profile,
            OLD.source_retained_bytes,OLD.source_md5) THEN
            RAISE EXCEPTION 'Media source provenance is immutable.' USING ERRCODE='42501';
        END IF;
    ELSIF NEW.source_input_sha256 IS NOT NULL OR NEW.source_input_bytes IS NOT NULL
       OR NEW.source_profile IS NOT NULL OR NEW.source_retained_bytes IS NOT NULL
       OR NEW.source_md5 IS NOT NULL THEN
        PERFORM 1 FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
        IF NEW.state<>'pending' OR NOT EXISTS (
            SELECT 1 FROM media.jobs j WHERE j.id=NEW.job_id AND j.state='processing'
              AND j.lease_token=NEW.lease_token AND j.expires_at>clock_timestamp()
              AND ((j.input_kind='image-v1' AND j.input_bytes=NEW.source_input_bytes)
                OR (j.input_kind='paired-v2' AND j.drawing_publication
                    AND current_user='board_migrator'
                    AND j.input_image_bytes=NEW.source_input_bytes
                    AND j.input_image_sha256=NEW.source_input_sha256))
        ) THEN
            RAISE EXCEPTION 'Media source provenance requires a current processing lease.' USING ERRCODE='42501';
        END IF;
    END IF;
    RETURN NEW;
END $$;

CREATE FUNCTION media.prepare_drawing_output(
 p_job text,p_token text,p_sha text,p_bytes bigint,p_width integer,p_height integer,
 p_md5 text,p_thumb_sha text,p_thumb_bytes bigint,p_thumb_width integer,p_thumb_height integer,
 p_source_retained bigint,p_source_md5 bytea,p_replay_sha text,p_replay_bytes bigint
) RETURNS SETOF media.assets
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE j media.jobs%ROWTYPE; a media.assets%ROWTYPE; r media.replay_assets%ROWTYPE;
BEGIN
    SELECT * INTO j FROM media.jobs WHERE id=p_job FOR UPDATE;
    IF j.id IS NULL OR j.input_kind<>'paired-v2' OR NOT j.drawing_publication
       OR j.state<>'processing' OR j.lease_token IS DISTINCT FROM p_token
       OR j.expires_at<=clock_timestamp() THEN
        RAISE EXCEPTION 'Drawing lease is no longer current.' USING ERRCODE='P0001';
    END IF;
    IF p_sha IS NULL OR octet_length(p_sha)<>64 OR p_sha !~ '^[0-9a-f]{64}$'
       OR p_bytes IS NULL OR p_bytes NOT BETWEEN 1 AND 5242880
       OR p_width IS NULL OR p_width NOT BETWEEN 1 AND 1024
       OR p_height IS NULL OR p_height NOT BETWEEN 1 AND 1024
       OR p_md5 IS NULL OR octet_length(p_md5)<>32 OR p_md5 !~ '^[0-9a-f]{32}$'
       OR p_thumb_sha IS NULL OR octet_length(p_thumb_sha)<>64 OR p_thumb_sha !~ '^[0-9a-f]{64}$'
       OR p_thumb_bytes IS NULL OR p_thumb_bytes NOT BETWEEN 1 AND 5242880
       OR p_thumb_width IS NULL OR p_thumb_width NOT BETWEEN 1 AND LEAST(250,p_width)
       OR p_thumb_height IS NULL OR p_thumb_height NOT BETWEEN 1 AND LEAST(250,p_height)
       OR p_source_retained IS NULL OR p_source_retained NOT BETWEEN 20 AND j.input_image_bytes
       OR p_source_md5 IS NULL OR octet_length(p_source_md5)<>16
       OR ((p_replay_sha IS NULL) <> (p_replay_bytes IS NULL))
       OR ((j.input_replay_bytes IS NULL) <> (p_replay_bytes IS NULL))
       OR (p_replay_bytes IS NOT NULL AND (p_replay_bytes NOT BETWEEN 288 AND 262400
          OR (p_replay_bytes-256)%16<>0 OR octet_length(p_replay_sha)<>64 OR p_replay_sha !~ '^[0-9a-f]{64}$')) THEN
        RAISE EXCEPTION 'Invalid drawing output metadata.' USING ERRCODE='22023';
    END IF;
    SELECT * INTO a FROM media.assets WHERE job_id=p_job AND lease_token=p_token FOR UPDATE;
    IF a.id IS NOT NULL THEN
        SELECT * INTO r FROM media.replay_assets WHERE asset_id=a.id;
        IF a.state<>'pending' OR a.output_format<>'png'
           OR (a.sha256,a.bytes,a.width,a.height,a.md5,a.thumbnail_sha256,a.thumbnail_bytes,
               a.thumbnail_width,a.thumbnail_height,a.source_input_sha256,a.source_input_bytes,
               a.source_profile,a.source_retained_bytes,a.source_md5) IS DISTINCT FROM
              (p_sha,p_bytes,p_width,p_height,p_md5,p_thumb_sha,p_thumb_bytes,p_thumb_width,p_thumb_height,
               j.input_image_sha256,j.input_image_bytes,'png-v1'::text,p_source_retained,p_source_md5)
           OR (r.sha256,r.bytes) IS DISTINCT FROM (p_replay_sha,p_replay_bytes) THEN
            RAISE EXCEPTION 'Drawing reservation conflicts.' USING ERRCODE='P0001';
        END IF;
        RETURN NEXT a; RETURN;
    END IF;
    INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,output_format,
      md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height,
      source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5)
    VALUES(replace(gen_random_uuid()::text,'-',''),p_job,p_token,p_sha,p_bytes,p_width,p_height,'png',
      p_md5,p_thumb_sha,p_thumb_bytes,p_thumb_width,p_thumb_height,
      j.input_image_sha256,j.input_image_bytes,'png-v1',p_source_retained,p_source_md5) RETURNING * INTO a;
    IF p_replay_bytes IS NOT NULL THEN
        INSERT INTO media.replay_assets VALUES(a.id,p_replay_sha,p_replay_bytes,p_width,p_height);
    END IF;
    RETURN NEXT a;
END $$;

CREATE FUNCTION media.approve_drawing_output(p_job text,p_token text,p_asset text)
RETURNS SETOF media.assets
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE j media.jobs%ROWTYPE; a media.assets%ROWTYPE;
BEGIN
    SELECT * INTO j FROM media.jobs WHERE id=p_job FOR UPDATE;
    SELECT * INTO a FROM media.assets WHERE id=p_asset AND job_id=p_job AND lease_token=p_token FOR UPDATE;
    IF j.id IS NULL OR a.id IS NULL OR j.input_kind<>'paired-v2' OR NOT j.drawing_publication
       OR j.lease_token IS DISTINCT FROM p_token THEN
        RAISE EXCEPTION 'Drawing approval is unavailable.' USING ERRCODE='P0001';
    END IF;
    IF j.state='published' AND a.state='approved' THEN RETURN NEXT a; RETURN; END IF;
    IF j.state<>'processing' OR j.expires_at<=clock_timestamp() OR a.state<>'pending'
       OR ((j.input_replay_bytes IS NOT NULL) <> EXISTS(SELECT 1 FROM media.replay_assets WHERE asset_id=a.id)) THEN
        RAISE EXCEPTION 'Drawing approval is unavailable.' USING ERRCODE='P0001';
    END IF;
    UPDATE media.jobs SET state='published',output_sha256=a.sha256,output_bytes=a.bytes,
      expires_at=NULL,updated_at=clock_timestamp() WHERE id=j.id;
    UPDATE media.assets SET state='approved',approved_at=clock_timestamp(),updated_at=clock_timestamp()
      WHERE id=a.id RETURNING * INTO a;
    RETURN NEXT a;
END $$;
REVOKE ALL ON FUNCTION media.prepare_drawing_output(text,text,text,bigint,integer,integer,text,text,bigint,integer,integer,bigint,bytea,text,bigint),media.approve_drawing_output(text,text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media.prepare_drawing_output(text,text,text,bigint,integer,integer,text,text,bigint,integer,integer,bigint,bytea,text,bigint),media.approve_drawing_output(text,text,text) TO board_media;
ALTER FUNCTION media.prepare_drawing_output(text,text,text,bigint,integer,integer,text,text,bigint,integer,integer,bigint,bytea,text,bigint) OWNER TO board_migrator;
ALTER FUNCTION media.approve_drawing_output(text,text,text) OWNER TO board_migrator;

CREATE FUNCTION media.guard_drawing_asset_approval() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
DECLARE j media.jobs%ROWTYPE;
BEGIN
    IF NEW.state='approved' AND OLD.state<>'approved' THEN
        SELECT * INTO j FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
        IF j.drawing_publication AND (current_user<>'board_migrator' OR j.state<>'published'
           OR j.lease_token IS DISTINCT FROM NEW.lease_token
           OR j.output_sha256 IS DISTINCT FROM NEW.sha256 OR j.output_bytes IS DISTINCT FROM NEW.bytes) THEN
            RAISE EXCEPTION 'Drawing approval requires joint completion.' USING ERRCODE='42501';
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_drawing_asset_approval() FROM PUBLIC;
CREATE TRIGGER media_drawing_asset_approval BEFORE UPDATE OF state ON media.assets
 FOR EACH ROW EXECUTE FUNCTION media.guard_drawing_asset_approval();

CREATE FUNCTION media_intake.begin_drawing_upload(p_id text, p_capability text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_state text;
    v_kind text;
    v_drawing boolean;
    v_expires timestamptz;
    v_hash bytea;
    v_started timestamptz;
BEGIN
    IF p_id IS NULL OR octet_length(p_id) <> 32 OR p_id !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.state, j.expires_at, j.input_kind,j.drawing_publication INTO v_state, v_expires, v_kind,v_drawing FROM media.jobs j WHERE j.id = p_id FOR UPDATE;
    SELECT h.capability_hash, h.upload_started_at INTO v_hash, v_started
        FROM media_intake.handles h WHERE h.job_id = p_id FOR UPDATE;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability, 'UTF8'))
       OR (v_state IN ('receiving', 'queued') AND v_expires <= clock_timestamp()) THEN
        RAISE EXCEPTION 'Intake handle not found.' USING ERRCODE = 'P0002';
    END IF;
    IF v_kind <> 'paired-v2' OR NOT v_drawing OR v_state <> 'receiving' OR v_started IS NOT NULL THEN
        RAISE EXCEPTION 'Intake state conflict.' USING ERRCODE = 'P0001';
    END IF;
    UPDATE media_intake.handles SET upload_started_at = clock_timestamp() WHERE job_id = p_id;
END
$$;

REVOKE ALL ON FUNCTION media_intake.begin_drawing_upload(text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media_intake.begin_drawing_upload(text,text) TO board_media_intake;
GRANT CREATE ON SCHEMA media_intake TO board_media_intake_owner;
ALTER FUNCTION media_intake.begin_drawing_upload(text,text) OWNER TO board_media_intake_owner;
REVOKE CREATE ON SCHEMA media_intake FROM board_media_intake_owner;
