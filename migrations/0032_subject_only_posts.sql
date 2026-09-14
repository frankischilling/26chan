-- Source OP admission permits a nonempty subject with an empty comment.
-- Replies still need an authorized attachment when their stored comment is empty.
GRANT SELECT(subject) ON content.posts TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.require_attachment_for_empty_post() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    -- Read the final row so queued updates cannot preserve a cleared subject.
    IF EXISTS (SELECT 1 FROM content.posts p
        WHERE p.id = NEW.id AND p.comment = ''
          AND NOT (p.id = p.thread_id AND p.subject <> ''))
       AND NOT EXISTS (SELECT 1 FROM content.post_media m WHERE m.post_id = NEW.id) THEN
        RAISE EXCEPTION 'An empty comment requires an authorized attachment.'
            USING ERRCODE = '23514', CONSTRAINT = 'posts_empty_comment_attachment';
    END IF;
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION content.require_attachment_for_empty_post() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.require_attachment_for_empty_post() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

DROP TRIGGER posts_empty_comment_attachment ON content.posts;
CREATE CONSTRAINT TRIGGER posts_empty_comment_attachment
AFTER INSERT OR UPDATE OF comment, subject ON content.posts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW WHEN (NEW.comment = '')
EXECUTE FUNCTION content.require_attachment_for_empty_post();

SET LOCAL ROLE board_attachment_owner;
REVOKE EXECUTE ON FUNCTION content.require_attachment_for_empty_post() FROM board_migrator;
RESET ROLE;
