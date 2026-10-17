-- File-only deletion keeps the post's normalized digest for later staff bans.
-- Whole-post erasure still removes this private attachment metadata.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content catalog is unavailable before retained upload identity';
 END IF;
END $$;
RESET ROLE;

ALTER TABLE content.post_media ADD COLUMN normalized_md5 bytea
 CHECK(normalized_md5 IS NULL OR octet_length(normalized_md5)=16);
ALTER TABLE content.post_media ADD CONSTRAINT erased_upload_digest
 CHECK(NOT metadata_erased OR normalized_md5 IS NULL);
-- Existing attachments remain unknown. This migration does not reconstruct
-- historical attachment or ban provenance from a current asset or input receipt.
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE FUNCTION content.retain_attachment_digest() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE digest text;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'post_media' OR TG_LEVEL<>'ROW'
    OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 OR TG_OP NOT IN('INSERT','UPDATE') THEN
  RAISE EXCEPTION 'Invalid attachment identity trigger' USING ERRCODE='23514';
 END IF;
 IF TG_OP='INSERT' THEN
  -- Both private attachment writers hold the publication job lock. Read the
  -- exact approved normalized output, never caller text or original input MD5.
  SELECT a.md5 INTO digest FROM media.assets a
   WHERE a.id=NEW.asset_id AND a.job_id=NEW.job_id AND a.state='approved';
  NEW.normalized_md5:=CASE WHEN octet_length(digest)=32 AND digest ~ '^[0-9a-f]{32}$'
   THEN decode(digest,'hex') ELSE NULL::bytea END;
 ELSIF NEW.metadata_erased THEN
  -- The preceding erasure guard requires actual whole-post content deletion.
  NEW.normalized_md5:=NULL;
 ELSIF NEW.normalized_md5 IS DISTINCT FROM OLD.normalized_md5 THEN
  RAISE EXCEPTION 'Retained attachment identity is immutable' USING ERRCODE='42501';
 END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.retain_attachment_digest() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.retain_attachment_digest() TO board_migrator;
RESET ROLE;
-- PostgreSQL runs same-kind triggers alphabetically. Whole-post erasure must
-- establish metadata_erased before the retained digest is cleared.
CREATE TRIGGER zz_retain_attachment_digest BEFORE INSERT OR UPDATE ON content.post_media
 FOR EACH ROW EXECUTE FUNCTION content.retain_attachment_digest();
SET LOCAL ROLE board_attachment_owner;
REVOKE EXECUTE ON FUNCTION content.retain_attachment_digest() FROM board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;

SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Retained upload identity changed content authority';
 END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF content.restrictions_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Retained upload identity changed restriction authority';
 END IF;
END $$;
RESET ROLE;
