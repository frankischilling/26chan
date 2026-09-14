-- Source OP_MARKUP is independent of ordinary spoiler/code/SJIS policy.
ALTER TABLE content.boards ADD COLUMN op_markup boolean NOT NULL DEFAULT false;
UPDATE content.boards SET op_markup=true WHERE slug IN ('qst','test');
ALTER TABLE content.posts DROP CONSTRAINT posts_comment_format_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_format_check CHECK (
    comment_format=0 OR comment_format BETWEEN 8 AND 15 OR comment_format BETWEEN 24 AND 31
);
GRANT SELECT(op_markup) ON content.boards TO board_attachment_owner;

CREATE OR REPLACE FUNCTION content.stamp_comment_format() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_format smallint;
BEGIN
    SELECT (8 + b.comment_spoiler_cleanup::integer
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
