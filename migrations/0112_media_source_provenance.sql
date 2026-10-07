-- Source identity is independent of normalized output identity. Historical
-- approvals remain NULL; it must never be inferred from normalized bytes.
ALTER TABLE media.assets
    ADD COLUMN source_input_sha256 text,
    ADD COLUMN source_input_bytes bigint,
    ADD COLUMN source_profile text,
    ADD COLUMN source_retained_bytes bigint,
    ADD COLUMN source_md5 bytea,
    ADD CONSTRAINT source_provenance CHECK (
        (source_input_sha256 IS NULL AND source_input_bytes IS NULL
         AND source_profile IS NULL AND source_retained_bytes IS NULL AND source_md5 IS NULL)
        OR
        (source_input_sha256 IS NOT NULL AND octet_length(source_input_sha256)=64
         AND source_input_sha256 ~ '^[0-9a-f]{64}$'
         AND source_input_bytes IS NOT NULL AND source_input_bytes BETWEEN 20 AND 8388608
         AND source_profile IS NOT NULL AND source_profile='png-v1'
         AND source_retained_bytes IS NOT NULL AND source_retained_bytes BETWEEN 20 AND source_input_bytes
         AND source_md5 IS NOT NULL AND octet_length(source_md5)=16)
    );

-- Independent of the normalized-manifest migration exception: even the
-- migrator cannot backfill or change a source tuple on an existing asset.
CREATE FUNCTION media.guard_source_provenance() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path = pg_catalog AS $$
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
        -- Acquire the job before any conflicting asset row, retaining it through
        -- insertion. Do not give invokers any additional table authority.
        PERFORM 1 FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
        IF NEW.state <> 'pending' OR NOT EXISTS (
            SELECT 1 FROM media.jobs WHERE id=NEW.job_id AND state='processing'
            AND lease_token=NEW.lease_token AND input_bytes=NEW.source_input_bytes
            AND expires_at > clock_timestamp()
        ) THEN
            RAISE EXCEPTION 'Media source provenance requires a current processing lease.'
                USING ERRCODE='42501';
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_source_provenance() FROM PUBLIC;
CREATE TRIGGER media_source_provenance_immutable BEFORE INSERT OR UPDATE ON media.assets
    FOR EACH ROW EXECUTE FUNCTION media.guard_source_provenance();
