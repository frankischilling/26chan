DO $gif_transfer_preflight$
BEGIN
    IF (pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NULL)
        <> (pg_catalog.to_regprocedure('content.text_thread_transfer_ready()') IS NULL) THEN
        RAISE EXCEPTION 'Text transfer catalog is incomplete before source GIF admission.';
    END IF;
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready for source GIF admission.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $gif_transfer_preflight$;

-- Context-bound GIF-v7 retains validated GIF bytes and a JPEG thumbnail.
-- Existing GIF-v3 files keep their 20 MiB normalized-output limit and PNG thumbnails.
ALTER TABLE media.assets
    DROP CONSTRAINT gif_manifest_check,
    ADD CONSTRAINT gif_manifest_check CHECK (
        output_format<>'gif' OR (md5 IS NOT NULL AND thumbnail_sha256 IS NOT NULL
            AND ((source_profile IS NULL AND still_profile IS NULL)
              OR (source_profile IS NOT NULL AND source_profile='gif-v7'
                AND still_profile IS NOT NULL AND still_profile='gif-v7')))),
    DROP CONSTRAINT assets_bytes_check,
    ADD CONSTRAINT assets_bytes_check CHECK(bytes BETWEEN 1 AND CASE
        WHEN still_profile='gif-v7' THEN 8388608
        WHEN still_profile='still-v6' THEN 10485760
        WHEN output_format='gif' THEN 20971520
        WHEN output_format IN ('pdf','webm','mp4') THEN 8388608 ELSE 5242880 END),
    DROP CONSTRAINT assets_width_check,
    DROP CONSTRAINT assets_height_check,
    ADD CONSTRAINT assets_width_check CHECK(width BETWEEN 1 AND CASE
        WHEN still_profile IN ('still-v6','gif-v7') THEN 10000
        WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
    ADD CONSTRAINT assets_height_check CHECK(height BETWEEN 1 AND CASE
        WHEN still_profile IN ('still-v6','gif-v7') THEN 10000
        WHEN output_format IN ('webm','mp4') THEN 2048 ELSE 1024 END),
    DROP CONSTRAINT still_manifest_check,
    ADD CONSTRAINT still_manifest_check CHECK (
        (still_profile IS NULL AND output_format<>'jpeg' AND thumbnail_format='png'
         AND mobile_sha256 IS NULL AND mobile_bytes IS NULL AND mobile_width IS NULL AND mobile_height IS NULL)
        OR (still_profile IS NOT NULL
         AND ((still_profile='still-v6' AND output_format IN ('png','jpeg'))
           OR (still_profile='gif-v7' AND output_format='gif'
             AND mobile_sha256 IS NULL AND mobile_bytes IS NULL AND mobile_width IS NULL AND mobile_height IS NULL))
         AND thumbnail_format='jpeg' AND thumbnail_sha256 IS NOT NULL
         AND thumbnail_bytes IS NOT NULL AND thumbnail_bytes BETWEEN 1 AND 1048576
         AND md5 IS NOT NULL AND source_profile IS NOT NULL AND source_profile=still_profile
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
               AND source_retained_bytes BETWEEN 20 AND 10485760)
           OR (source_profile='gif-v7' AND still_profile IS NOT NULL AND still_profile='gif-v7'
               AND source_input_bytes BETWEEN 20 AND 8388608
               AND source_retained_bytes BETWEEN 20 AND 8388608)))
    );

GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.check_image_board_policy(
    p_board text,p_input_bytes bigint,p_width integer,p_height integer,p_format text
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record;
BEGIN
    SELECT max_image_bytes,min_image_width,min_image_height,max_image_dimension,gif_only,pdf_enabled,upload_board
        INTO b FROM content.boards WHERE slug=p_board;
    IF NOT FOUND OR p_input_bytes IS NULL OR p_width IS NULL OR p_height IS NULL
        OR p_format IS NULL OR p_format NOT IN ('png','jpeg','gif','pdf','webm','mp4') THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF p_format='gif' AND b.upload_board THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF p_input_bytes<1 OR p_input_bytes>least(b.max_image_bytes,CASE WHEN p_format IN ('png','jpeg') THEN 10485760 ELSE 8388608 END) THEN
        RAISE EXCEPTION 'Error: File too large.' USING ERRCODE='23514';
    END IF;
    IF p_format='pdf' AND (NOT b.pdf_enabled OR p_width<>1 OR p_height<>1) THEN
        RAISE EXCEPTION 'Error: Corrupted file or unsupported file type.' USING ERRCODE='23514';
    END IF;
    -- The source PDF branch precedes its image-only GIF restriction.
    IF b.gif_only AND p_format NOT IN ('gif','pdf','webm','mp4') THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF p_width<b.min_image_width OR p_height<b.min_image_height THEN
        RAISE EXCEPTION 'Error: Image resolution is too small.' USING ERRCODE='23514';
    END IF;
    IF p_width>b.max_image_dimension OR p_height>b.max_image_dimension THEN
        RAISE EXCEPTION 'Error: Image resolution is too large.' USING ERRCODE='23514';
    END IF;
END $$;
REVOKE ALL ON FUNCTION content.check_image_board_policy(text,bigint,integer,integer,text)
    FROM PUBLIC,board_public,board_staff,board_auth,board_media,board_media_read,board_media_intake,board_monitor;

RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

DO $gif_transfer_reseal$
BEGIN
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        UPDATE post_secrets.thread_transfer_catalog
            SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready after source GIF admission.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $gif_transfer_reseal$;
