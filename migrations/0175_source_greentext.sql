-- Correct source greentext prefix recognition for new posts only.
-- Existing rows, frozen presentations and moderation snapshots keep their profile.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before text formatting migration'; END IF;
END $$;
RESET ROLE;
ALTER TABLE content.posts DROP CONSTRAINT posts_comment_format_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_format_check CHECK (
    comment_format=0 OR comment_format BETWEEN 8 AND 15 OR comment_format BETWEEN 24 AND 31
    OR comment_format BETWEEN 40 AND 47 OR comment_format BETWEEN 56 AND 63
    OR comment_format BETWEEN 104 AND 111 OR comment_format BETWEEN 120 AND 127
    OR comment_format BETWEEN 232 AND 239 OR comment_format BETWEEN 248 AND 255
);

CREATE OR REPLACE FUNCTION content.stamp_comment_format() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_format smallint;
BEGIN
    SELECT (232 + b.comment_spoiler_cleanup::integer
        + 2 * b.comment_code_spacing::integer
        + 4 * b.comment_sjis_spacing::integer
        + 16 * (b.op_markup AND (NEW.id=NEW.thread_id
            OR COALESCE(current_setting('board.source_op_reply', true)='true', false)))::integer)::smallint
        INTO v_format FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE = '23503';
    END IF;
    NEW.comment_format := v_format;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_comment_format() FROM PUBLIC;

ALTER TABLE content.moderation_audit DROP CONSTRAINT moderation_audit_snapshot_comment_format_check;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_snapshot_comment_format_check CHECK (
    snapshot_comment_format=0 OR snapshot_comment_format BETWEEN 8 AND 15
    OR snapshot_comment_format BETWEEN 24 AND 31 OR snapshot_comment_format BETWEEN 40 AND 47
    OR snapshot_comment_format BETWEEN 56 AND 63 OR snapshot_comment_format BETWEEN 104 AND 111
    OR snapshot_comment_format BETWEEN 120 AND 127 OR snapshot_comment_format BETWEEN 232 AND 239
    OR snapshot_comment_format BETWEEN 248 AND 255
);

ALTER TABLE content.posts DROP CONSTRAINT posts_saved_presentation_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_saved_presentation_check CHECK (
    (saved_presentation IS NULL AND saved_presentation_search IS NULL)
    OR (saved_presentation IS NOT NULL AND saved_presentation_search IS NOT NULL
        AND wordfilter_payload IS NULL AND wordfilter_search IS NULL
        AND (comment_format BETWEEN 104 AND 111 OR comment_format BETWEEN 120 AND 127
            OR comment_format BETWEEN 232 AND 239 OR comment_format BETWEEN 248 AND 255)
        AND octet_length(saved_presentation) BETWEEN 9 AND
            CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END
        AND substring(saved_presentation FROM 1 FOR 4)=decode(
            CASE WHEN staff_authorized_limits THEN '46503032' ELSE '46503031' END,'hex')
        AND get_byte(saved_presentation,4) BETWEEN 0 AND 1
        AND octet_length(saved_presentation_search)<=
            CASE WHEN staff_authorized_limits THEN 524288 ELSE 131072 END
        AND octet_length(comment)<=CASE WHEN staff_authorized_limits THEN 2097152 ELSE 524288 END));

GRANT CREATE ON SCHEMA post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION post_secrets.transfer_lock_source(p_source text,p_thread bigint,p_destination text)
RETURNS integer LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_count integer; v_bytes bigint; v_all_bytes bigint;
BEGIN
    IF p_source IS NULL OR p_destination IS NULL OR p_source !~ '^[a-z0-9]{1,10}$'
        OR p_destination !~ '^[a-z0-9]{1,10}$' OR p_source=p_destination OR p_source='b'
        OR p_destination IN('f','j') OR p_thread IS NULL OR p_thread<=0 THEN
        RAISE EXCEPTION 'Invalid transfer target.' USING ERRCODE='P131I';
    END IF;
    PERFORM b.slug FROM content.boards b WHERE b.slug IN(p_source,p_destination)
        ORDER BY b.slug FOR UPDATE NOWAIT;
    IF (SELECT count(*) FROM content.boards b WHERE b.slug IN(p_source,p_destination)
        AND NOT b.staff_only AND NOT b.upload_board)<>2 THEN
        RAISE EXCEPTION 'Transfer board is unavailable.' USING ERRCODE='P131N';
    END IF;
    PERFORM t.id FROM content.threads t WHERE t.board=p_source AND t.id=p_thread
        AND NOT t.deleted AND NOT t.content_erased AND NOT t.closed AND t.archived_at IS NULL FOR UPDATE NOWAIT;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer source is unavailable.' USING ERRCODE='P131N'; END IF;
    -- Retained sticky tombstones can outgrow the live window. Bound all work,
    -- including structural-media inspection, rather than sweeping that history.
    IF (SELECT count(*) FROM (SELECT 1 FROM content.posts p WHERE p.board=p_source
        AND p.thread_id=p_thread LIMIT 1002) bounded)>1001 THEN
        RAISE EXCEPTION 'Transfer source exceeds its bounded window.' USING ERRCODE='P131L';
    END IF;
    PERFORM p.id FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread ORDER BY p.id FOR UPDATE NOWAIT;
    IF NOT EXISTS(SELECT 1 FROM content.posts p WHERE p.board=p_source AND p.id=p_thread
        AND p.thread_id=p_thread AND NOT p.deleted AND NOT p.content_erased) THEN
        RAISE EXCEPTION 'Transfer source is unavailable.' USING ERRCODE='P131N';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p JOIN content.post_media m ON m.post_id=p.id
        WHERE p.board=p_source AND p.thread_id=p_thread) THEN
        RAISE EXCEPTION 'Only media-free threads can be transferred.' USING ERRCODE='P131F';
    END IF;
    SELECT count(*),coalesce(sum(octet_length(p.comment)::bigint+coalesce(octet_length(p.wordfilter_payload),0)
        +coalesce(octet_length(p.wordfilter_search),0)+coalesce(octet_length(p.saved_presentation),0)
        +coalesce(octet_length(p.saved_presentation_search),0)),0),
        coalesce(sum(octet_length(p.comment)::bigint+coalesce(octet_length(p.wordfilter_payload),0)
        +coalesce(octet_length(p.wordfilter_search),0)+coalesce(octet_length(p.saved_presentation),0)
        +coalesce(octet_length(p.saved_presentation_search),0)+octet_length(p.name)+octet_length(p.subject)+2048),0)
        INTO v_count,v_bytes,v_all_bytes
    FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted;
    IF v_count NOT BETWEEN 1 AND 1001 OR v_bytes>64064000 OR v_all_bytes>67108864 THEN
        RAISE EXCEPTION 'Transfer exceeds its capacity.' USING ERRCODE='P131L';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted
        AND (p.content_erased OR NOT isfinite(p.created_at)
            OR NOT (p.comment_format BETWEEN 104 AND 111 OR p.comment_format BETWEEN 120 AND 127
                OR p.comment_format BETWEEN 232 AND 239 OR p.comment_format BETWEEN 248 AND 255)
            OR p.drawing_time_seconds IS NOT NULL OR p.drawing_source_post_id IS NOT NULL
            OR p.dice_result IS NOT NULL OR p.fortune_text IS NOT NULL OR p.fortune_color IS NOT NULL
            OR (p.saved_presentation IS NOT NULL AND p.wordfilter_payload IS NOT NULL))) THEN
        RAISE EXCEPTION 'Source presentation cannot be transferred.' USING ERRCODE='P131F';
    END IF;
    IF EXISTS(SELECT 1 FROM content.posts p JOIN content.boards b ON b.slug=p.board
        WHERE p.board=p_source AND p.thread_id=p_thread AND NOT p.deleted
        AND b.country_flags AND p.board_flag IS NULL AND (p.country IS NULL OR p.country_name IS NULL)) THEN
        RAISE EXCEPTION 'Source country evidence is unavailable.' USING ERRCODE='P131F';
    END IF;
    RETURN v_count;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_thread_transfer_owner;

SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Text formatting migration did not preserve transfer authority'; END IF;
END $$;
RESET ROLE;
