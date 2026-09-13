-- Cancel attachment authorization, not the processor's lease. An already
-- running isolated job may finish; its output cannot attach after revocation.
GRANT DELETE ON media_intake.handles TO board_attachment_owner;
-- Status is a point-in-time authorization check. Posting still checks and
-- consumes the capability under locks in its own transaction.
CREATE FUNCTION content.check_attachment_upload(p_job text, p_capability text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE v_hash bytea; v_created timestamptz;
BEGIN
    IF p_job IS NULL OR octet_length(p_job) <> 32 OR p_job !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Upload is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    SELECT h.capability_hash,j.created_at INTO v_hash,v_created
        FROM media_intake.handles h JOIN media.jobs j ON j.id=h.job_id WHERE j.id=p_job;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability,'UTF8'))
       OR v_created + interval '2 hours' <= clock_timestamp() THEN
        RAISE EXCEPTION 'Upload is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    IF EXISTS (SELECT 1 FROM content.post_media m WHERE m.job_id=p_job) THEN
        RAISE EXCEPTION 'Upload was already attached.' USING ERRCODE = 'P0001';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.check_attachment_upload(text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.check_attachment_upload(text,text) TO board_public;

CREATE FUNCTION content.cancel_attachment_upload(p_job text, p_capability text) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE v_hash bytea; v_created timestamptz;
BEGIN
    IF current_setting('transaction_isolation') <> 'read committed' THEN
        RAISE EXCEPTION 'Cancellation requires read committed isolation.' USING ERRCODE = '22023';
    END IF;
    IF p_job IS NULL OR octet_length(p_job) <> 32 OR p_job !~ '^[0-9a-f]{32}$'
       OR p_capability IS NULL OR octet_length(p_capability) <> 64 OR p_capability !~ '^[0-9a-f]{64}$' THEN
        RAISE EXCEPTION 'Upload is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    SELECT j.created_at INTO v_created FROM media.jobs j WHERE j.id=p_job FOR UPDATE;
    SELECT h.capability_hash INTO v_hash FROM media_intake.handles h WHERE h.job_id=p_job;
    IF v_hash IS NULL OR v_hash <> sha256(convert_to(p_capability,'UTF8'))
       OR v_created + interval '2 hours' <= clock_timestamp() THEN
        RAISE EXCEPTION 'Upload is unavailable.' USING ERRCODE = 'P0002';
    END IF;
    IF EXISTS (SELECT 1 FROM content.post_media m WHERE m.job_id=p_job) THEN
        RAISE EXCEPTION 'Upload was already attached.' USING ERRCODE = 'P0001';
    END IF;
    DELETE FROM media_intake.handles WHERE job_id=p_job;
END $$;
REVOKE ALL ON FUNCTION content.cancel_attachment_upload(text,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.cancel_attachment_upload(text,text) TO board_public;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
ALTER FUNCTION content.check_attachment_upload(text,text) OWNER TO board_attachment_owner;
ALTER FUNCTION content.cancel_attachment_upload(text,text) OWNER TO board_attachment_owner;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
