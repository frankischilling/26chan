-- Check usable attachment context before content rules can commit effects.
DO $still_transfer_preflight$
BEGIN
    IF (pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NULL)
        <> (pg_catalog.to_regprocedure('content.text_thread_transfer_ready()') IS NULL) THEN
        RAISE EXCEPTION 'Text transfer catalog is incomplete before attachment admission preflight.';
    END IF;
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready for attachment admission preflight.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $still_transfer_preflight$;

GRANT SELECT(md5,source_profile) ON media.assets TO board_attachment_owner;
GRANT SELECT(input_kind,drawing_publication) ON media.jobs TO board_attachment_owner;
GRANT SELECT(asset_id,component,md5) ON media.input_receipts TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;

CREATE FUNCTION content.attachment_upload_filter_metadata(
    p_job text,p_capability text,p_board text,p_parent bigint,p_authorized boolean,p_badged boolean
) RETURNS TABLE(filename text,normalized_md5 text,original_md5 bytea,original_eligible boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE b record; t record; j record; a record; raw_md5 bytea; eligible boolean;
BEGIN
    IF current_setting('transaction_isolation')<>'read committed'
       OR p_parent IS NULL OR p_parent<0 OR p_authorized IS NULL OR p_badged IS NULL
       OR ((p_authorized OR p_badged) AND session_user<>'board_staff') THEN
        RAISE EXCEPTION 'Attachment admission is unavailable.' USING ERRCODE='P0002';
    END IF;
    SELECT image_limit,text_only,staff_only,upload_board,pdf_enabled INTO b
        FROM content.boards WHERE slug=p_board FOR UPDATE;
    IF NOT FOUND OR b.staff_only OR (p_parent>0 AND (b.text_only OR b.upload_board)) THEN
        RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
    END IF;
    IF b.image_limit=0 THEN
        IF session_user='board_staff' THEN
            RAISE EXCEPTION 'Attachment is unavailable.' USING ERRCODE='P0002';
        END IF;
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE='P0001';
    END IF;
    IF p_parent>0 THEN
        SELECT sticky,undead,closed INTO t FROM content.threads
            WHERE board=p_board AND id=p_parent AND NOT deleted AND archived_at IS NULL FOR UPDATE;
        IF NOT FOUND OR (t.closed AND NOT p_authorized) THEN
            RAISE EXCEPTION 'Thread is unavailable.' USING ERRCODE='P0002';
        END IF;
    END IF;
    SELECT q.state,q.filename,q.input_bytes,q.input_kind,q.drawing_publication INTO j
        FROM media.jobs q WHERE q.id=p_job FOR UPDATE;
    PERFORM content.check_attachment_upload(p_job,p_capability);
    IF j.state IS DISTINCT FROM 'published' THEN
        RAISE EXCEPTION 'Attachment is not ready.' USING ERRCODE='P0001';
    END IF;
    SELECT id,bytes,width,height,output_format,still_profile,video_duration_micros,
           video_has_audio,thumbnail_width,thumbnail_height,md5,source_profile INTO a
        FROM media.assets WHERE job_id=p_job AND state='approved' ORDER BY id LIMIT 1;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Attachment is not approved.' USING ERRCODE='P0001';
    END IF;
    IF a.still_profile IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM media.image_context c WHERE c.job_id=p_job AND c.board=p_board
        AND c.thread_id=p_parent
        AND (p_board<>'p' OR a.output_format<>'jpeg' OR c.capcode=p_badged)
    ) THEN
        RAISE EXCEPTION 'Upload context does not match this post. Upload the file again.' USING ERRCODE='23514';
    END IF;
    PERFORM content.check_attachment_board_policy(p_board,j.input_bytes,a.bytes,a.width,a.height,
        a.output_format,j.filename,a.video_duration_micros,a.video_has_audio);
    IF a.output_format IN ('webm','mp4') AND p_parent>0
       AND least(a.thumbnail_width,a.thumbnail_height)*125/greatest(125,a.thumbnail_width,a.thumbnail_height)=0 THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF (a.output_format='pdf' AND lower(right(j.filename,4))<>'.pdf')
       OR (b.pdf_enabled AND lower(right(j.filename,4))='.pdf' AND a.output_format<>'pdf') THEN
        RAISE EXCEPTION 'Error: Upload failed.' USING ERRCODE='23514';
    END IF;
    IF p_parent>0 THEN
      IF NOT p_authorized AND NOT t.sticky AND NOT t.undead
       AND (SELECT count(*) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
            WHERE p.board=p_board AND p.thread_id=p_parent AND p.id<>p_parent
            AND NOT p.deleted AND NOT m.file_deleted)>=b.image_limit THEN
        RAISE EXCEPTION 'This board or thread cannot accept another image.' USING ERRCODE='P0001';
      END IF;
    END IF;
    SELECT r.md5 INTO raw_md5 FROM media.input_receipts r WHERE r.asset_id=a.id
        AND ((j.input_kind='image-v1' AND r.component='upload')
          OR (j.input_kind='paired-v2' AND j.drawing_publication AND r.component='paired-image'));
    -- Original JPEG MD5 is captured only by base STRIP_EXIF. The separate
    -- capcode-only strip on /p does not assign the legacy original digest.
    -- A generic normalized PNG may have come from JPEG; do not guess its kind.
    eligible:=CASE
        WHEN a.output_format='pdf' THEN false
        WHEN a.output_format IN ('gif','webm','mp4') THEN true
        WHEN a.output_format='jpeg' THEN p_board<>'p'
        WHEN a.output_format='png' AND (a.still_profile='still-v6' OR a.source_profile='png-v1') THEN true
        ELSE NULL::boolean END;
    RETURN QUERY SELECT j.filename,a.md5,
        CASE WHEN eligible IS FALSE THEN NULL::bytea ELSE raw_md5 END,eligible;
END $$;
REVOKE ALL ON FUNCTION content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean) TO board_public,board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
DO $still_transfer_reseal$
BEGIN
    IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
        EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
        UPDATE post_secrets.thread_transfer_catalog
            SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
        IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
            RAISE EXCEPTION 'Text transfer catalog is not ready after attachment admission preflight.';
        END IF;
        EXECUTE 'RESET ROLE';
    END IF;
END $still_transfer_reseal$;
