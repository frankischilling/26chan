-- The operator selected 30 days for independent moderation content snapshots.
-- Preserve audit identity/action metadata. Migration assigns deadlines but never
-- erases payload; the narrow retention worker performs bounded expiry batches.
DO $$ BEGIN
 SET LOCAL ROLE board_thread_transfer_owner;
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content catalog is unavailable before snapshot retention.';
 END IF;
 RESET ROLE;
END $$;

ALTER TABLE content.moderation_audit ADD COLUMN snapshot_expires_at timestamptz;
UPDATE content.moderation_audit
 SET snapshot_expires_at=least(created_at,clock_timestamp())+interval '720 hours'
 WHERE snapshot_version IS NOT NULL;
ALTER TABLE content.moderation_audit ADD CONSTRAINT moderation_audit_snapshot_deadline
 CHECK(snapshot_version IS NULL OR snapshot_expires_at IS NOT NULL);
CREATE INDEX moderation_snapshot_expiration ON content.moderation_audit(snapshot_expires_at,id)
 WHERE snapshot_version IS NOT NULL;

GRANT USAGE ON SCHEMA content TO board_moderation_retention;
GRANT USAGE ON SCHEMA content,post_secrets TO board_moderation_retention_owner;
GRANT SELECT(id,snapshot_version,snapshot_expires_at) ON content.moderation_audit TO board_moderation_retention_owner;
GRANT UPDATE(snapshot_version,snapshot_name,snapshot_trip,snapshot_capcode,snapshot_subject,
 snapshot_comment,snapshot_comment_format,snapshot_staff_authorized_limits,snapshot_wordfiltered,
 snapshot_image_spoiler,snapshot_filename,snapshot_dice_result,snapshot_fortune_text,
 snapshot_fortune_color,snapshot_drawing_time_seconds,snapshot_drawing_source_post_id)
 ON content.moderation_audit TO board_moderation_retention_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_moderation_retention_owner;
SET LOCAL ROLE board_moderation_retention_owner;

CREATE FUNCTION post_secrets.moderation_snapshot_deadline() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 -- A runtime cannot extend retention with a supplied audit timestamp/deadline.
 NEW.snapshot_expires_at:=CASE WHEN NEW.snapshot_version IS NOT NULL
  THEN clock_timestamp()+interval '720 hours' ELSE NULL END;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION post_secrets.moderation_snapshot_deadline() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION post_secrets.moderation_snapshot_deadline() TO board_migrator;
RESET ROLE;
CREATE TRIGGER moderation_snapshot_deadline BEFORE INSERT ON content.moderation_audit
 FOR EACH ROW EXECUTE FUNCTION post_secrets.moderation_snapshot_deadline();
SET LOCAL ROLE board_moderation_retention_owner;
REVOKE EXECUTE ON FUNCTION post_secrets.moderation_snapshot_deadline() FROM board_migrator;

CREATE TABLE post_secrets.moderation_retention_catalog (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON post_secrets.moderation_retention_catalog FROM PUBLIC,board_public,board_staff,board_auth,
 board_moderation_retention,board_migrator;

CREATE FUNCTION post_secrets.moderation_retention_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT sha256(convert_to(coalesce(jsonb_agg(jsonb_build_array(
  p.oid::regprocedure::text,pg_get_userbyid(p.proowner),pg_get_functiondef(p.oid),
  (SELECT jsonb_agg(jsonb_build_array(pg_get_userbyid(a.grantor),
   CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type,a.is_grantable)
   ORDER BY pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type)
   FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a)) ORDER BY p.oid::regprocedure::text),'[]'::jsonb)::text,'UTF8'))
 FROM pg_proc p WHERE p.proowner='board_moderation_retention_owner'::regrole
$$;
REVOKE ALL ON FUNCTION post_secrets.moderation_retention_catalog_hash() FROM PUBLIC;

CREATE FUNCTION content.moderation_retention_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_moderation_retention_owner'
  AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member='board_moderation_retention_owner'::regrole)
 AND NOT has_schema_privilege('board_moderation_retention_owner','content','CREATE')
 AND NOT has_schema_privilege('board_moderation_retention_owner','post_secrets','CREATE')
 AND NOT EXISTS(SELECT 1 FROM (VALUES('board_public'),('board_staff'),('board_auth'),('board_media'),
  ('board_media_read'),('board_media_intake'),('board_monitor'),('board_moderation_retention')) r(name)
  WHERE pg_has_role(r.name,'board_moderation_retention_owner','MEMBER'))
 AND NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_moderation_retention'
  AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls))
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members WHERE member='board_moderation_retention'::regrole)
 AND NOT has_any_column_privilege('board_moderation_retention','content.moderation_audit','SELECT,INSERT,UPDATE,REFERENCES')
 AND NOT has_table_privilege('board_moderation_retention','content.moderation_audit','DELETE,TRUNCATE,TRIGGER')
 AND coalesce((SELECT definition_hash=post_secrets.moderation_retention_catalog_hash()
  FROM post_secrets.moderation_retention_catalog WHERE singleton),false)
 AND content.text_thread_transfer_ready()
$$;
REVOKE ALL ON FUNCTION content.moderation_retention_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.moderation_retention_ready() TO board_moderation_retention,board_staff,board_migrator;

CREATE FUNCTION content.expire_moderation_snapshots() RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE affected bigint; invoker text;
BEGIN
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 IF invoker NOT IN('board_moderation_retention','board_migrator')
  OR content.moderation_retention_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Moderation retention is unavailable.' USING ERRCODE='42501';
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Moderation retention requires Read Committed.' USING ERRCODE='22023';
 END IF;
 WITH expired AS MATERIALIZED (
  SELECT id FROM content.moderation_audit
  WHERE snapshot_version IS NOT NULL AND snapshot_expires_at<=transaction_timestamp()
  ORDER BY snapshot_expires_at,id LIMIT 64 FOR UPDATE SKIP LOCKED
 ) UPDATE content.moderation_audit a SET snapshot_version=NULL,snapshot_name=NULL,
  snapshot_trip=NULL,snapshot_capcode=NULL,snapshot_subject=NULL,snapshot_comment=NULL,
  snapshot_comment_format=NULL,snapshot_staff_authorized_limits=NULL,snapshot_wordfiltered=NULL,
  snapshot_image_spoiler=NULL,snapshot_filename=NULL,snapshot_dice_result=NULL,
  snapshot_fortune_text=NULL,snapshot_fortune_color=NULL,snapshot_drawing_time_seconds=NULL,
  snapshot_drawing_source_post_id=NULL
 FROM expired e WHERE a.id=e.id;
 GET DIAGNOSTICS affected=ROW_COUNT;
 RETURN affected;
END $$;
REVOKE ALL ON FUNCTION content.expire_moderation_snapshots() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.expire_moderation_snapshots() TO board_moderation_retention,board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_moderation_retention_owner;
SET LOCAL ROLE board_thread_transfer_owner;
GRANT EXECUTE ON FUNCTION content.text_thread_transfer_ready() TO board_moderation_retention_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash()
 WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Content catalog is unavailable after snapshot retention.';
 END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_moderation_retention_owner;
INSERT INTO post_secrets.moderation_retention_catalog
 VALUES(true,post_secrets.moderation_retention_catalog_hash());
DO $$ BEGIN
 IF content.moderation_retention_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Moderation retention is unavailable after migration.';
 END IF;
END $$;
RESET ROLE;
