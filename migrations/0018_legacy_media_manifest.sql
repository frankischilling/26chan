-- Only the offline migration identity can fill a legacy NULL manifest once.
-- Full-file identity and every existing manifest remain immutable. Runtime
-- publication, retirement, and reader grants are unchanged.
CREATE OR REPLACE FUNCTION media.guard_asset_mutation() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog AS $$
BEGIN
    IF current_user = 'board_media' AND OLD.state = 'approved' THEN
        RAISE EXCEPTION 'Approved media records are immutable.' USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'UPDATE' THEN
        IF (NEW.id,NEW.job_id,NEW.lease_token,NEW.sha256,NEW.bytes,NEW.width,NEW.height,NEW.created_at)
           IS DISTINCT FROM
           (OLD.id,OLD.job_id,OLD.lease_token,OLD.sha256,OLD.bytes,OLD.width,OLD.height,OLD.created_at) THEN
            RAISE EXCEPTION 'Media reservation metadata is immutable.' USING ERRCODE = '42501';
        END IF;
        IF (NEW.md5,NEW.thumbnail_sha256,NEW.thumbnail_bytes,NEW.thumbnail_width,NEW.thumbnail_height)
           IS DISTINCT FROM
           (OLD.md5,OLD.thumbnail_sha256,OLD.thumbnail_bytes,OLD.thumbnail_width,OLD.thumbnail_height)
           AND NOT (current_user='board_migrator' AND OLD.state='approved' AND NEW.state='approved'
               AND OLD.md5 IS NULL AND OLD.thumbnail_sha256 IS NULL AND OLD.thumbnail_bytes IS NULL
               AND OLD.thumbnail_width IS NULL AND OLD.thumbnail_height IS NULL
               AND NEW.md5 IS NOT NULL AND NEW.thumbnail_sha256 IS NOT NULL AND NEW.thumbnail_bytes IS NOT NULL
               AND NEW.thumbnail_width IS NOT NULL AND NEW.thumbnail_height IS NOT NULL) THEN
            RAISE EXCEPTION 'Media reservation metadata is immutable.' USING ERRCODE = '42501';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
