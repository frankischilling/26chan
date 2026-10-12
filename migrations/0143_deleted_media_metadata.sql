-- Keep consumed attachment identities, but erase descriptive metadata on whole
-- deletion. File-only deletion retains source-visible file metadata.
DO $$ BEGIN
 SET LOCAL ROLE board_thread_transfer_owner;
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content catalog is unavailable before media erasure.';
 END IF;
 RESET ROLE;
END $$;
ALTER TABLE content.post_media ADD COLUMN metadata_erased boolean NOT NULL DEFAULT false;
ALTER TABLE content.post_media ADD CONSTRAINT post_media_erased_payload CHECK(
 NOT metadata_erased OR (filename='[deleted]' AND bytes=1 AND width=1 AND height=1
 AND NOT spoiler AND file_deleted AND created_at='epoch'::timestamptz));
GRANT SELECT(post_id,metadata_erased),UPDATE(filename,bytes,width,height,spoiler,file_deleted,created_at,metadata_erased)
 ON content.post_media TO board_content_erasure_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_content_erasure_owner;
SET LOCAL ROLE board_content_erasure_owner;
CREATE FUNCTION post_secrets.guard_media_erasure() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE target_board text; erased boolean;
BEGIN
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Media mutation requires Read Committed.' USING ERRCODE='22023';
 END IF;
 IF TG_OP='UPDATE' THEN
  IF NEW.post_id<>OLD.post_id OR NEW.job_id<>OLD.job_id OR NEW.asset_id<>OLD.asset_id OR NEW.tim<>OLD.tim THEN
   RAISE EXCEPTION 'Attachment identity cannot be reassigned.' USING ERRCODE='23514';
  END IF;
  IF OLD.metadata_erased THEN
   IF to_jsonb(NEW) IS DISTINCT FROM to_jsonb(OLD) THEN
    RAISE EXCEPTION 'Erased media metadata cannot be restored.' USING ERRCODE='23514';
   END IF;
   RETURN NEW;
  END IF;
 END IF;
 SELECT board INTO target_board FROM content.posts WHERE id=NEW.post_id;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post is unavailable.' USING ERRCODE='23514'; END IF;
 -- A direct tuple-first write must not wait backwards against deletion.
 PERFORM slug FROM content.boards WHERE slug=target_board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23514'; END IF;
 SELECT content_erased INTO erased FROM content.posts WHERE id=NEW.post_id AND board=target_board;
 IF NOT FOUND THEN RAISE EXCEPTION 'Post changed during media mutation.' USING ERRCODE='23514'; END IF;
 IF erased THEN
  IF TG_OP='INSERT' THEN
   RAISE EXCEPTION 'Cannot attach media to erased content.' USING ERRCODE='23514';
  END IF;
  NEW.metadata_erased:=true;
  NEW.filename:='[deleted]'; NEW.bytes:=1; NEW.width:=1; NEW.height:=1;
  NEW.spoiler:=false; NEW.file_deleted:=true; NEW.created_at:='epoch'::timestamptz;
 ELSIF NEW.metadata_erased THEN
  RAISE EXCEPTION 'Media erasure requires whole-post deletion.' USING ERRCODE='23514';
 END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION post_secrets.erase_deleted_media() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_class WHERE oid='content.post_media'::regclass
  AND relkind='r' AND relpersistence='p' AND NOT relrowsecurity AND NOT relforcerowsecurity) THEN
  RAISE EXCEPTION 'Media erasure visibility is unavailable.' USING ERRCODE='42501';
 END IF;
 UPDATE content.post_media SET metadata_erased=true WHERE post_id=NEW.id AND NOT metadata_erased;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION post_secrets.guard_media_erasure(),post_secrets.erase_deleted_media() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION post_secrets.guard_media_erasure(),post_secrets.erase_deleted_media() TO board_migrator;
RESET ROLE;
CREATE TRIGGER z_guard_media_erasure BEFORE INSERT OR UPDATE ON content.post_media
 FOR EACH ROW EXECUTE FUNCTION post_secrets.guard_media_erasure();
CREATE TRIGGER erase_deleted_media AFTER UPDATE ON content.posts
 FOR EACH ROW WHEN(NOT OLD.content_erased AND NEW.content_erased)
 EXECUTE FUNCTION post_secrets.erase_deleted_media();
SET LOCAL ROLE board_content_erasure_owner;
REVOKE EXECUTE ON FUNCTION post_secrets.guard_media_erasure(),post_secrets.erase_deleted_media() FROM board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_content_erasure_owner;
-- Existing rows are left for an explicit, receipt-bound operator cleanup.
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content catalog is unavailable after media erasure.';
 END IF;
END $$;
RESET ROLE;
