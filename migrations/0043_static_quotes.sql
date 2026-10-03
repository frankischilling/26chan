-- Add static board/catalog references only to newly inserted posts. The saved
-- historical formatter and all existing policy bits retain their behavior.
ALTER TABLE content.posts DROP CONSTRAINT posts_comment_format_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_format_check CHECK (
    comment_format=0 OR comment_format BETWEEN 8 AND 15 OR comment_format BETWEEN 24 AND 31
    OR comment_format BETWEEN 40 AND 47 OR comment_format BETWEEN 56 AND 63
    OR comment_format BETWEEN 104 AND 111 OR comment_format BETWEEN 120 AND 127
);

CREATE OR REPLACE FUNCTION content.stamp_comment_format() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_format smallint;
BEGIN
    SELECT (104 + b.comment_spoiler_cleanup::integer
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
