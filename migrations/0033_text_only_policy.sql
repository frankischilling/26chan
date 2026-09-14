-- Active source TEXT_ONLY defaults: news only; other/future boards default off.
ALTER TABLE content.boards ADD COLUMN text_only boolean NOT NULL DEFAULT false;
UPDATE content.boards SET text_only=true WHERE slug='news';

-- The existing attachment function already locks the board before the thread
-- and job. This invoker trigger also holds that policy through its insertion;
-- it cannot grant the caller any additional read or mutation authority.
GRANT SELECT(text_only) ON content.boards TO board_attachment_owner;
CREATE FUNCTION content.check_text_only_attachment() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    v_text_only boolean;
    v_thread bigint;
BEGIN
    SELECT b.text_only,p.thread_id INTO v_text_only,v_thread
        FROM content.posts p JOIN content.boards b ON b.slug=p.board
        WHERE p.id=NEW.post_id FOR SHARE OF b;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='23503';
    END IF;
    IF v_text_only AND NEW.post_id<>v_thread THEN
        RAISE EXCEPTION 'You cannot upload files on this board' USING ERRCODE='23514';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.check_text_only_attachment() FROM PUBLIC;
CREATE TRIGGER check_text_only_attachment BEFORE INSERT ON content.post_media
FOR EACH ROW EXECUTE FUNCTION content.check_text_only_attachment();
