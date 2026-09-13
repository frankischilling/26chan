-- Empty comments require a durable, authorized attachment association. Runtime
-- roles cannot create or remove these associations except through scoped code.
ALTER TABLE content.posts DROP CONSTRAINT posts_comment_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_comment_check
    CHECK (char_length(comment) BETWEEN 0 AND 16000 AND octet_length(comment) <= 64000);

GRANT SELECT(comment) ON content.posts TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE FUNCTION content.require_attachment_for_empty_post() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    -- Inspect the final row, not merely the queued NEW value. Attachment
    -- insertion follows post insertion in the same authorized transaction.
    IF EXISTS (SELECT 1 FROM content.posts p WHERE p.id = NEW.id AND p.comment = '')
       AND NOT EXISTS (SELECT 1 FROM content.post_media m WHERE m.post_id = NEW.id) THEN
        RAISE EXCEPTION 'An empty comment requires an authorized attachment.'
            USING ERRCODE = '23514', CONSTRAINT = 'posts_empty_comment_attachment';
    END IF;
    RETURN NULL;
END
$$;
REVOKE ALL ON FUNCTION content.require_attachment_for_empty_post() FROM PUBLIC;
-- The migrator owns the table but does not inherit the function owner's rights.
GRANT EXECUTE ON FUNCTION content.require_attachment_for_empty_post() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

CREATE CONSTRAINT TRIGGER posts_empty_comment_attachment
AFTER INSERT OR UPDATE OF comment ON content.posts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW WHEN (NEW.comment = '')
EXECUTE FUNCTION content.require_attachment_for_empty_post();

SET LOCAL ROLE board_attachment_owner;
REVOKE EXECUTE ON FUNCTION content.require_attachment_for_empty_post() FROM board_migrator;
RESET ROLE;

-- File deletion retains post_media as a one-use tombstone, so an image-only
-- post can keep displaying the deleted-file marker without restoring media.
