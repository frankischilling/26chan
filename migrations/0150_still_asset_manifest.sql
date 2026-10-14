-- Still-v6 reservations describe retained source bytes and JPEG companions.
-- Historical normalized PNG and other media reservations keep their old bounds.
ALTER TABLE media.assets
    ADD COLUMN still_profile text,
    ADD COLUMN thumbnail_format text NOT NULL DEFAULT 'png',
    ADD COLUMN mobile_sha256 text,
    ADD COLUMN mobile_bytes bigint,
    ADD COLUMN mobile_width integer,
    ADD COLUMN mobile_height integer,
    DROP CONSTRAINT assets_output_format_check,
    ADD CONSTRAINT assets_output_format_check CHECK(output_format IN ('png','jpeg','gif','pdf','webm','mp4')),
    DROP CONSTRAINT assets_bytes_check,
    ADD CONSTRAINT assets_bytes_check CHECK(bytes BETWEEN 1 AND CASE
        WHEN still_profile='still-v6' THEN 10485760
        WHEN output_format='gif' THEN 20971520
        WHEN output_format IN ('pdf','webm','mp4') THEN 8388608 ELSE 5242880 END),
    DROP CONSTRAINT assets_width_check,
    DROP CONSTRAINT assets_height_check,
    ADD CONSTRAINT assets_width_check CHECK(width BETWEEN 1 AND CASE
        WHEN still_profile='still-v6' THEN 10000 WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
    ADD CONSTRAINT assets_height_check CHECK(height BETWEEN 1 AND CASE
        WHEN still_profile='still-v6' THEN 10000 WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
    ADD CONSTRAINT still_manifest_check CHECK (
        (still_profile IS NULL AND output_format<>'jpeg' AND thumbnail_format='png'
         AND mobile_sha256 IS NULL AND mobile_bytes IS NULL AND mobile_width IS NULL AND mobile_height IS NULL)
        OR (still_profile IS NOT NULL AND still_profile='still-v6' AND output_format IN ('png','jpeg')
         AND thumbnail_format='jpeg' AND thumbnail_sha256 IS NOT NULL
         AND thumbnail_bytes IS NOT NULL AND thumbnail_bytes BETWEEN 1 AND 1048576
         AND md5 IS NOT NULL AND source_profile IS NOT NULL AND source_profile='still-v6'
         AND source_retained_bytes IS NOT NULL AND source_retained_bytes=bytes
         AND source_md5 IS NOT NULL AND encode(source_md5,'hex')=md5
         AND ((mobile_sha256 IS NULL AND mobile_bytes IS NULL AND mobile_width IS NULL AND mobile_height IS NULL)
           OR (mobile_sha256 IS NOT NULL AND octet_length(mobile_sha256)=64 AND mobile_sha256 ~ '^[0-9a-f]{64}$'
             AND mobile_bytes IS NOT NULL AND mobile_bytes BETWEEN 1 AND 4194304
             AND mobile_width IS NOT NULL AND mobile_width BETWEEN 1 AND 1024 AND mobile_width<=width
             AND mobile_height IS NOT NULL AND mobile_height BETWEEN 1 AND 1024 AND mobile_height<=height
             AND greatest(mobile_width,mobile_height)=1024)))
    ),
    DROP CONSTRAINT source_provenance,
    ADD CONSTRAINT source_provenance CHECK (
        (source_input_sha256 IS NULL AND source_input_bytes IS NULL
         AND source_profile IS NULL AND source_retained_bytes IS NULL AND source_md5 IS NULL)
        OR (source_input_sha256 IS NOT NULL AND octet_length(source_input_sha256)=64
         AND source_input_sha256 ~ '^[0-9a-f]{64}$'
         AND source_input_bytes IS NOT NULL AND source_retained_bytes IS NOT NULL
         AND source_profile IS NOT NULL AND source_md5 IS NOT NULL AND octet_length(source_md5)=16
         AND ((source_profile='png-v1' AND source_input_bytes BETWEEN 20 AND 8388608
               AND source_retained_bytes BETWEEN 20 AND source_input_bytes)
           OR (source_profile='still-v6' AND still_profile IS NOT NULL AND still_profile='still-v6'
               AND source_input_bytes BETWEEN 20 AND 10485760
               AND source_retained_bytes BETWEEN 20 AND 10485760)))
    );

CREATE FUNCTION media.guard_still_manifest() RETURNS trigger
LANGUAGE plpgsql SECURITY INVOKER SET search_path=pg_catalog AS $$
BEGIN
    IF TG_OP='UPDATE' THEN
        IF (NEW.still_profile,NEW.thumbnail_format,NEW.mobile_sha256,NEW.mobile_bytes,NEW.mobile_width,NEW.mobile_height)
          IS DISTINCT FROM
           (OLD.still_profile,OLD.thumbnail_format,OLD.mobile_sha256,OLD.mobile_bytes,OLD.mobile_width,OLD.mobile_height) THEN
            RAISE EXCEPTION 'Still image manifest is immutable.' USING ERRCODE='42501';
        END IF;
    ELSIF NEW.still_profile IS NOT NULL THEN
        PERFORM 1 FROM media.jobs WHERE id=NEW.job_id FOR UPDATE;
        IF NEW.state<>'pending' OR NOT EXISTS (
            SELECT 1 FROM media.jobs j JOIN media.image_context c ON c.job_id=j.id
            WHERE j.id=NEW.job_id AND j.input_kind='image-v1' AND j.state='processing'
              AND j.lease_token=NEW.lease_token AND j.expires_at>clock_timestamp()
              AND j.input_bytes=NEW.source_input_bytes
        ) THEN
            RAISE EXCEPTION 'Still image manifest requires a context-bound processing lease.' USING ERRCODE='42501';
        END IF;
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION media.guard_still_manifest() FROM PUBLIC;
CREATE TRIGGER media_still_manifest_immutable BEFORE INSERT OR UPDATE ON media.assets
    FOR EACH ROW EXECUTE FUNCTION media.guard_still_manifest();
