-- Zero preserves the historical formatter; 8..15 select the first source
-- markup version and its three posting-time flags. Never backfill history.
ALTER TABLE content.posts ADD COLUMN comment_format smallint NOT NULL DEFAULT 0
    CHECK (comment_format = 0 OR comment_format BETWEEN 8 AND 15);

-- The attachment inserter already reads board image policy and locks its row.
-- It needs only these additional non-private policy reads for the same stamp.
GRANT SELECT(comment_spoiler_cleanup,comment_code_spacing,comment_sjis_spacing)
    ON content.boards TO board_attachment_owner;

CREATE FUNCTION content.stamp_comment_format() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_format smallint;
BEGIN
    -- Same board-first order as public posting and attachment insertion. Hold
    -- policy stable through commit even for an independent authorized INSERT.
    SELECT (8 + b.comment_spoiler_cleanup::integer
        + 2 * b.comment_code_spacing::integer
        + 4 * b.comment_sjis_spacing::integer)::smallint
        INTO v_format FROM content.boards b WHERE b.slug=NEW.board FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE = '23503';
    END IF;
    NEW.comment_format := v_format;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.stamp_comment_format() FROM PUBLIC;
CREATE TRIGGER stamp_comment_format BEFORE INSERT ON content.posts
FOR EACH ROW EXECUTE FUNCTION content.stamp_comment_format();
