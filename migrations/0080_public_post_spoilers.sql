-- Initial spoiler selection follows SPOILERS, including on text posts. The
-- public role still cannot INSERT/UPDATE the state column or call the staff
-- setter. This trigger only initializes a new public post from its request.
CREATE FUNCTION content.initial_public_image_spoiler() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean;
BEGIN
    IF session_user='board_public' THEN
        SELECT b.comment_spoiler_cleanup INTO enabled FROM content.boards b
            WHERE b.slug=NEW.board FOR SHARE;
        NEW.image_spoiler=coalesce(enabled,false)
            AND coalesce(current_setting('board.post_image_spoiler',true),'')='true';
    END IF;
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.initial_public_image_spoiler() FROM PUBLIC;
CREATE TRIGGER initial_public_image_spoiler BEFORE INSERT ON content.posts
    FOR EACH ROW EXECUTE FUNCTION content.initial_public_image_spoiler();

-- Attachment insertion has its own capability-scoped SQL entry point. Check
-- the same board policy there so a forged direct call cannot select a spoiler
-- on a disabled board. Later staff UPDATEs retain the migration-78 setter.
CREATE FUNCTION content.initial_attachment_spoiler() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE enabled boolean;
BEGIN
    SELECT b.comment_spoiler_cleanup INTO enabled FROM content.posts p
        JOIN content.boards b ON b.slug=p.board
        WHERE p.id=NEW.post_id FOR SHARE OF b;
    NEW.spoiler=NEW.spoiler AND coalesce(enabled,false);
    RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.initial_attachment_spoiler() FROM PUBLIC;
CREATE TRIGGER initial_attachment_spoiler BEFORE INSERT ON content.post_media
    FOR EACH ROW EXECUTE FUNCTION content.initial_attachment_spoiler();
