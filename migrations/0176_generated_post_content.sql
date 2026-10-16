-- Generated dice/fortune markup is nonblank source content before admission.
-- Preserve existing rows and the deferred final-row attachment invariant.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before generated content migration'; END IF;
END $$;
RESET ROLE;

GRANT SELECT(dice_result,fortune_text) ON content.posts TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE OR REPLACE FUNCTION content.require_attachment_for_empty_post() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
    IF EXISTS(SELECT 1 FROM content.posts p WHERE p.id=NEW.id AND p.comment='' AND NOT p.content_erased
        AND NOT (p.id=p.thread_id AND p.subject<>'')
        AND p.dice_result IS NULL AND p.fortune_text IS NULL)
        AND NOT EXISTS(SELECT 1 FROM content.post_media m WHERE m.post_id=NEW.id) THEN
        RAISE EXCEPTION 'An empty comment requires an authorized attachment.'
            USING ERRCODE='23514',CONSTRAINT='posts_empty_comment_attachment';
    END IF;
    RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION content.require_attachment_for_empty_post() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.require_attachment_for_empty_post() TO board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

DROP TRIGGER posts_empty_comment_attachment ON content.posts;
CREATE CONSTRAINT TRIGGER posts_empty_comment_attachment
AFTER INSERT OR UPDATE OF comment,subject,dice_result,fortune_text,fortune_color ON content.posts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW WHEN (NEW.comment='')
EXECUTE FUNCTION content.require_attachment_for_empty_post();
SET LOCAL ROLE board_attachment_owner;
REVOKE EXECUTE ON FUNCTION content.require_attachment_for_empty_post() FROM board_migrator;
RESET ROLE;

SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Generated content migration did not preserve transfer authority'; END IF;
END $$;
RESET ROLE;
