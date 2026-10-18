-- Original image bytes are private and independent of processed-file identity.
-- A durable asset outlives its queue job; deleting the asset removes its receipt.
CREATE TABLE media.input_receipts (
    asset_id text PRIMARY KEY REFERENCES media.assets(id) ON DELETE CASCADE,
    component text NOT NULL CHECK (component IN ('upload','paired-image')),
    sha256 text NOT NULL CHECK (octet_length(sha256)=64 AND sha256 ~ '^[0-9a-f]{64}$'),
    bytes bigint NOT NULL CHECK (bytes BETWEEN 1 AND 10485760),
    md5 bytea NOT NULL CHECK (octet_length(md5)=16)
);
REVOKE ALL ON media.input_receipts FROM PUBLIC;

CREATE FUNCTION media.guard_input_receipt() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    RAISE EXCEPTION 'Original input receipts are immutable.' USING ERRCODE='42501';
END $$;
REVOKE ALL ON FUNCTION media.guard_input_receipt() FROM PUBLIC;
CREATE TRIGGER media_input_receipt_immutable BEFORE UPDATE ON media.input_receipts
    FOR EACH ROW EXECUTE FUNCTION media.guard_input_receipt();

-- Only a trusted publication host can submit a snapshot receipt. The worker and
-- public clients never receive this capability or access to the private table.
CREATE FUNCTION media.record_input_receipt(
    p_job text,p_token text,p_asset text,p_sha256 text,p_bytes bigint,p_md5 bytea,p_component text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
DECLARE
    j media.jobs%ROWTYPE;
    a media.assets%ROWTYPE;
    r media.input_receipts%ROWTYPE;
    have_job boolean;
    have_receipt boolean;
BEGIN
    IF p_component IS NULL OR p_component NOT IN ('upload','paired-image')
       OR p_sha256 IS NULL OR octet_length(p_sha256)<>64
       OR p_sha256 !~ '^[0-9a-f]{64}$' OR p_bytes IS NULL
       OR p_bytes NOT BETWEEN 1 AND 10485760
       OR p_md5 IS NULL OR octet_length(p_md5)<>16 THEN
        RAISE EXCEPTION 'Original input receipt is invalid.' USING ERRCODE='22023';
    END IF;
    -- Match the reservation/approval lock order, even when a retry discovers
    -- that terminal queue cleanup has already removed the job.
    SELECT * INTO j FROM media.jobs WHERE id=p_job FOR UPDATE;
    have_job := FOUND;
    SELECT * INTO a FROM media.assets WHERE id=p_asset FOR UPDATE;
    IF NOT FOUND OR a.job_id IS DISTINCT FROM p_job
       OR a.lease_token IS DISTINCT FROM p_token OR a.state='deleting' THEN
        RAISE EXCEPTION 'Original input receipt is unavailable.' USING ERRCODE='42501';
    END IF;
    IF (a.source_input_sha256 IS NOT NULL AND a.source_input_sha256<>p_sha256)
       OR (a.source_input_bytes IS NOT NULL AND a.source_input_bytes<>p_bytes) THEN
        RAISE EXCEPTION 'Original input differs from source provenance.' USING ERRCODE='42501';
    END IF;
    SELECT * INTO r FROM media.input_receipts WHERE asset_id=p_asset;
    have_receipt := FOUND;
    IF have_receipt AND (r.sha256,r.bytes,r.md5,r.component) IS DISTINCT FROM (p_sha256,p_bytes,p_md5,p_component) THEN
        RAISE EXCEPTION 'Original input receipt differs.' USING ERRCODE='42501';
    END IF;
    IF a.state='approved' THEN
        IF NOT have_receipt THEN
            RAISE EXCEPTION 'Historical input receipt is unknown.' USING ERRCODE='42501';
        END IF;
        RETURN;
    END IF;
    IF NOT have_job OR j.state<>'processing' OR j.lease_token IS DISTINCT FROM p_token
       OR j.expires_at IS NULL OR j.expires_at<=clock_timestamp()
       OR ((j.input_kind='image-v1' AND p_component='upload' AND j.input_bytes=p_bytes)
            OR (j.input_kind='paired-v2' AND p_component='paired-image' AND j.drawing_publication
                AND j.input_image_bytes=p_bytes AND j.input_image_sha256=p_sha256)) IS NOT TRUE THEN
        RAISE EXCEPTION 'Original input receipt requires a current lease.' USING ERRCODE='42501';
    END IF;
    IF NOT have_receipt THEN
        INSERT INTO media.input_receipts(asset_id,sha256,bytes,md5,component)
        VALUES(p_asset,p_sha256,p_bytes,p_md5,p_component);
    END IF;
END $$;
REVOKE ALL ON FUNCTION media.record_input_receipt(text,text,text,text,bigint,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media.record_input_receipt(text,text,text,text,bigint,bytea,text) TO board_media;

-- Receipt-aware approval checks existence without filling a missing receipt.
-- The caller retains these locks through the normal approval state transition.
CREATE FUNCTION media.require_input_receipt(
    p_job text,p_token text,p_asset text,p_sha256 text,p_bytes bigint,p_md5 bytea,p_component text
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog AS $$
BEGIN
    PERFORM 1 FROM media.jobs WHERE id=p_job FOR UPDATE;
    PERFORM 1 FROM media.assets WHERE id=p_asset FOR UPDATE;
    IF NOT EXISTS(SELECT 1 FROM media.input_receipts WHERE asset_id=p_asset) THEN
        RAISE EXCEPTION 'Original input receipt is missing.' USING ERRCODE='42501';
    END IF;
    PERFORM media.record_input_receipt(p_job,p_token,p_asset,p_sha256,p_bytes,p_md5,p_component);
END $$;
REVOKE ALL ON FUNCTION media.require_input_receipt(text,text,text,text,bigint,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION media.require_input_receipt(text,text,text,text,bigint,bytea,text) TO board_media;
ALTER FUNCTION media.record_input_receipt(text,text,text,text,bigint,bytea,text) OWNER TO board_migrator;
ALTER FUNCTION media.require_input_receipt(text,text,text,text,bigint,bytea,text) OWNER TO board_migrator;
