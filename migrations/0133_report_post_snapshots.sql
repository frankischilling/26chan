-- modes/report.php:546-566 captures the public post when a report is admitted.
-- Copy only presentation data. Reporter identity, passwords, addresses and
-- admission authority never enter this snapshot. Historical rows are not filled.
-- Transfer fingerprints the reviewed board ACL/policy surface. Reject existing
-- drift before granting this capture owner its narrow parent lock permissions.
DO $snapshot_transfer_preflight$
BEGIN
 IF (pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NULL)
     <> (pg_catalog.to_regprocedure('content.text_thread_transfer_ready()') IS NULL) THEN
  RAISE EXCEPTION 'Text transfer catalog is incomplete before report capture.';
 END IF;
 IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
  EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
  PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
  IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
   RAISE EXCEPTION 'Text transfer catalog is not ready before report capture.';
  END IF;
  EXECUTE 'RESET ROLE';
 END IF;
END $snapshot_transfer_preflight$;
GRANT USAGE ON SCHEMA content,post_secrets TO board_report_snapshot_owner;
GRANT SELECT(slug),UPDATE(slug) ON content.boards TO board_report_snapshot_owner;
CREATE POLICY report_snapshot_board_read ON content.boards FOR SELECT
    TO board_report_snapshot_owner USING(true);
CREATE POLICY report_snapshot_board_lock ON content.boards FOR UPDATE
    TO board_report_snapshot_owner USING(true) WITH CHECK(true);

-- The narrow view hides every other post column. Its JSON lookup of the optional
-- transfer field works before or after 0131, without a grant on the whole post.
CREATE VIEW content.report_snapshot_source WITH (security_barrier=true) AS
SELECT p.board,p.id AS post_id,
 jsonb_build_object(
  'board',p.board,'post_id',p.id,'thread_id',p.thread_id,
  'created_at',p.created_at,'name',p.name,'trip',p.trip,'poster_id',p.poster_id,
  'capcode',p.capcode,'country',p.country,'country_name',p.country_name,
  'board_flag',p.board_flag,'board_flag_type',p.board_flag_type,'flag_name',p.flag_name,
  'subject',p.subject,'comment',p.comment,'comment_format',p.comment_format,'file_deleted',p.file_deleted,
  'staff_authorized_limits',p.staff_authorized_limits,
  'wordfilter_payload',encode(p.wordfilter_payload,'hex'),
  'saved_presentation',CASE WHEN to_jsonb(p)->>'saved_presentation' IS NOT NULL
      THEN substring(to_jsonb(p)->>'saved_presentation' FROM 3) END,
  'drawing_time_seconds',p.drawing_time_seconds,
  'drawing_source_post_id',p.drawing_source_post_id,
  'archived',t.archived_at IS NOT NULL,
  'relative_subject',CASE WHEN p.id<>p.thread_id THEN (
      SELECT op.subject FROM content.posts op WHERE op.id=p.thread_id AND op.board=p.board) END,
  'attachment',CASE WHEN a.post_id IS NOT NULL THEN jsonb_build_object(
      'post_id',a.post_id,'filename',a.filename,'bytes',a.bytes,'width',a.width,'height',a.height,
      'spoiler',a.spoiler,'tim',a.tim,'thumbnail_width',a.thumbnail_width,
      'thumbnail_height',a.thumbnail_height,'available',a.available,
      'md5',a.md5,'output_format',a.output_format) END
 ) AS payload
FROM content.posts p
JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
LEFT JOIN content.staff_post_media a ON a.post_id=p.id
WHERE NOT p.deleted AND NOT t.deleted
  AND (t.archived_at IS NULL OR t.archive_expires_at>clock_timestamp());
REVOKE ALL ON content.report_snapshot_source FROM PUBLIC,board_public,board_staff,board_auth;
GRANT SELECT ON content.report_snapshot_source TO board_report_snapshot_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_report_snapshot_owner;
GRANT REFERENCES(id) ON content.reports TO board_report_snapshot_owner;
SET ROLE board_report_snapshot_owner;

CREATE TABLE post_secrets.report_snapshot_capacity (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 byte_limit bigint NOT NULL CHECK(byte_limit BETWEEN 1 AND 1073741824),
 used_bytes bigint NOT NULL CHECK(used_bytes BETWEEN 0 AND byte_limit)
);
INSERT INTO post_secrets.report_snapshot_capacity VALUES(true,67108864,0);
CREATE TABLE post_secrets.report_post_snapshots (
 report_id bigint PRIMARY KEY REFERENCES content.reports(id) ON DELETE CASCADE,
 board text NOT NULL CHECK(board ~ '^[a-z0-9]{1,10}$'),
 post_id bigint NOT NULL CHECK(post_id>0),
 version smallint NOT NULL CHECK(version=1),
 captured_at timestamptz NOT NULL,
 payload jsonb NOT NULL CHECK(jsonb_typeof(payload)='object'),
 charged_bytes bigint NOT NULL CHECK(charged_bytes BETWEEN 128 AND 33554560),
 CHECK(payload->>'board'=board AND (payload->>'post_id')::bigint=post_id),
 CHECK(charged_bytes=octet_length(payload::text)+128)
);
REVOKE ALL ON post_secrets.report_post_snapshots,post_secrets.report_snapshot_capacity
 FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;

CREATE FUNCTION post_secrets.capture_report_post() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
DECLARE saved jsonb; charge bigint; invoker text;
BEGIN
 IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report capture is unavailable.' USING ERRCODE='42501';
 END IF;
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 IF invoker NOT IN('board_public','board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Report capture is unavailable.' USING ERRCODE='42501';
 END IF;
 IF current_setting('transaction_isolation')<>'read committed' THEN
  RAISE EXCEPTION 'Report capture requires Read Committed.' USING ERRCODE='22023';
 END IF;
 -- Ordinary admission already owns this parent. Raw privileged fixture inserts
 -- must not reverse tuple -> board order against a live writer.
 PERFORM b.slug FROM content.boards b WHERE b.slug=NEW.board FOR UPDATE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Report board is unavailable.' USING ERRCODE='23514'; END IF;
 SELECT s.payload INTO saved FROM content.report_snapshot_source s
  WHERE s.board=NEW.board AND s.post_id=NEW.post_id;
 -- An operator can insert retained historical reports for already unavailable
 -- targets. No current row is invented as a historical admission-time snapshot.
 IF saved IS NULL THEN
  IF invoker='board_migrator' THEN RETURN NEW; END IF;
  RAISE EXCEPTION 'Report target is unavailable.' USING ERRCODE='P0002';
 END IF;
 charge:=octet_length(saved::text)+128;
 IF charge>33554560 THEN
  RAISE EXCEPTION 'Report snapshot exceeds its bound.' USING ERRCODE='P0133';
 END IF;
 UPDATE post_secrets.report_snapshot_capacity SET used_bytes=used_bytes+charge
  WHERE singleton AND used_bytes<=byte_limit-charge;
 IF NOT FOUND THEN RAISE EXCEPTION 'Report snapshot capacity is unavailable.' USING ERRCODE='P0133'; END IF;
 INSERT INTO post_secrets.report_post_snapshots
  VALUES(NEW.id,NEW.board,NEW.post_id,1,clock_timestamp(),saved,charge);
 RETURN NEW;
END $$;

CREATE FUNCTION post_secrets.release_report_snapshot() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report capture is unavailable.' USING ERRCODE='42501';
 END IF;
 -- This is accounting only, under the same transaction as report FK deletion.
 UPDATE post_secrets.report_snapshot_capacity SET used_bytes=used_bytes-OLD.charged_bytes
  WHERE singleton AND used_bytes>=OLD.charged_bytes;
 IF NOT FOUND THEN RAISE EXCEPTION 'Report snapshot accounting is unavailable.' USING ERRCODE='P0133'; END IF;
 RETURN OLD;
END $$;
REVOKE ALL ON FUNCTION post_secrets.capture_report_post(),post_secrets.release_report_snapshot()
 FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION post_secrets.capture_report_post() TO board_migrator;
CREATE TRIGGER release_report_snapshot AFTER DELETE ON post_secrets.report_post_snapshots
 FOR EACH ROW EXECUTE FUNCTION post_secrets.release_report_snapshot();
RESET ROLE;
CREATE TRIGGER capture_report_post AFTER INSERT ON content.reports
 FOR EACH ROW EXECUTE FUNCTION post_secrets.capture_report_post();
SET ROLE board_report_snapshot_owner;
REVOKE EXECUTE ON FUNCTION post_secrets.capture_report_post() FROM board_migrator;
CREATE VIEW content.report_post_snapshots WITH (security_barrier=true) AS
 SELECT report_id,board,post_id,version,captured_at,payload
 FROM post_secrets.report_post_snapshots;
REVOKE ALL ON content.report_post_snapshots FROM PUBLIC,board_public,board_auth,board_migrator;
GRANT SELECT ON content.report_post_snapshots TO board_staff;
CREATE TABLE post_secrets.report_snapshot_catalog (
 singleton boolean PRIMARY KEY DEFAULT true CHECK(singleton),
 definition_hash bytea NOT NULL CHECK(octet_length(definition_hash)=32)
);
REVOKE ALL ON post_secrets.report_snapshot_catalog FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
CREATE FUNCTION post_secrets.report_snapshot_acl(permissions aclitem[]) RETURNS jsonb
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce(jsonb_agg(jsonb_build_array(
  pg_get_userbyid(a.grantor),CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,
  a.privilege_type,a.is_grantable) ORDER BY pg_get_userbyid(a.grantor),
  CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee) END,a.privilege_type),'[]'::jsonb)
 FROM aclexplode(permissions) a
$$;
CREATE FUNCTION post_secrets.report_snapshot_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
  FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE (n.nspname='post_secrets' AND c.relname IN('report_post_snapshots','report_snapshot_capacity','report_snapshot_catalog'))
   OR (n.nspname='content' AND c.relname IN('report_snapshot_source','report_post_snapshots'))
 ), funcs AS (
  SELECT p.* FROM pg_proc p WHERE p.proowner='board_report_snapshot_owner'::regrole
   OR p.oid='post_secrets.report_target(text,bigint,timestamp with time zone)'::regprocedure
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,r.relkind,r.relpersistence,
    pg_get_userbyid(r.relowner),r.relrowsecurity,r.relforcerowsecurity,r.reloptions,
    post_secrets.report_snapshot_acl(coalesce(r.relacl,acldefault('r',r.relowner))),
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,
       pg_get_expr(d.adbin,d.adrelid),post_secrets.report_snapshot_acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    CASE WHEN r.relkind='v' THEN pg_get_viewdef(r.oid,true) END) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(r.identity,c.conname,pg_get_constraintdef(c.oid,true))
    ORDER BY r.identity,c.conname) FROM pg_constraint c JOIN relations r ON r.oid=c.conrelid),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    p.prosecdef,p.proconfig,post_secrets.report_snapshot_acl(coalesce(p.proacl,acldefault('f',p.proowner))),
    pg_get_functiondef(p.oid)) ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgname,t.tgenabled,
    pg_get_triggerdef(t.oid)) ORDER BY t.tgrelid::regclass::text,t.tgname)
   FROM pg_trigger t WHERE NOT t.tgisinternal AND (
    (t.tgrelid='content.reports'::regclass AND t.tgname='capture_report_post')
    OR t.tgrelid='post_secrets.report_post_snapshots'::regclass)))::text,'UTF8'))
$$;
CREATE FUNCTION content.report_snapshots_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='board_report_snapshot_owner'
   AND NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole
   AND NOT rolreplication AND NOT rolbypassrls)
 AND NOT EXISTS(SELECT 1 FROM pg_auth_members
   WHERE member='board_report_snapshot_owner'::regrole)
 AND NOT has_schema_privilege('board_report_snapshot_owner','content','CREATE')
 AND NOT has_schema_privilege('board_report_snapshot_owner','post_secrets','CREATE')
 AND NOT EXISTS(SELECT 1 FROM (VALUES('board_public'),('board_staff'),('board_auth'),('board_media'),
   ('board_media_read'),('board_media_intake'),('board_monitor')) runtime(name)
   WHERE pg_has_role(runtime.name,'board_report_snapshot_owner','MEMBER'))
 AND coalesce((SELECT definition_hash=post_secrets.report_snapshot_catalog_hash()
   FROM post_secrets.report_snapshot_catalog WHERE singleton),false)
 AND EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid='content.reports'::regclass
   AND tgname='capture_report_post' AND tgenabled='O' AND NOT tgisinternal
   AND tgfoid='post_secrets.capture_report_post()'::regprocedure)
$$;
REVOKE ALL ON FUNCTION post_secrets.report_snapshot_acl(aclitem[]),post_secrets.report_snapshot_catalog_hash(),
 content.report_snapshots_ready() FROM PUBLIC,board_public,board_staff,board_auth,board_migrator;
GRANT EXECUTE ON FUNCTION content.report_snapshots_ready() TO board_staff,board_report_admission_owner;
RESET ROLE;
REVOKE REFERENCES(id) ON content.reports FROM board_report_snapshot_owner;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_report_snapshot_owner;

-- Every existing public/categorical admission uses this private target check.
-- Check capture readiness here so disabling its trigger cannot admit reports
-- without a snapshot. The existing visibility and eligibility rules follow.
GRANT CREATE ON SCHEMA post_secrets TO board_report_admission_owner;
SET ROLE board_report_admission_owner;
CREATE OR REPLACE FUNCTION post_secrets.report_target(p_board text,p_post bigint,p_now timestamptz) RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_invoker text; v_enabled boolean; v_thread bigint; v_sticky boolean; v_capcode text;
BEGIN
 IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report capture is unavailable.' USING ERRCODE='42501';
 END IF;
 v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 IF v_invoker NOT IN ('board_public','board_staff','board_migrator') THEN
  RAISE EXCEPTION 'Report admission is unavailable.' USING ERRCODE='42501';
 END IF;
 SELECT b.can_report_posts INTO v_enabled FROM content.boards b
  WHERE b.slug=p_board AND (NOT b.staff_only OR v_invoker IN ('board_staff','board_migrator'));
 IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
 IF NOT v_enabled THEN RAISE EXCEPTION 'You cannot report posts on this board.' USING ERRCODE='P0001'; END IF;
 SELECT p.thread_id,t.sticky,p.capcode INTO v_thread,v_sticky,v_capcode
  FROM content.posts p JOIN content.threads t ON t.board=p.board AND t.id=p.thread_id
  JOIN content.boards b ON b.slug=p.board
  WHERE p.board=p_board AND p.id=p_post AND NOT p.deleted AND NOT t.deleted
   AND (t.archived_at IS NULL OR (b.archive_retention_seconds>0 AND t.archive_expires_at>p_now));
 IF NOT FOUND THEN RAISE EXCEPTION 'Post not found.' USING ERRCODE='P0002'; END IF;
 IF p_post=v_thread AND v_sticky THEN RAISE EXCEPTION 'Error: You cannot report a sticky.' USING ERRCODE='P0001'; END IF;
 IF v_capcode IS NOT NULL THEN RAISE EXCEPTION 'Error: You cannot report this post.' USING ERRCODE='P0001'; END IF;
 RETURN v_thread;
END $$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_report_admission_owner;
DO $snapshot_transfer_reseal$
BEGIN
 IF pg_catalog.to_regclass('post_secrets.thread_transfer_catalog') IS NOT NULL THEN
  EXECUTE 'SET LOCAL ROLE board_thread_transfer_owner';
  UPDATE post_secrets.thread_transfer_catalog
   SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
  IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
   RAISE EXCEPTION 'Text transfer catalog is not ready after report capture.';
  END IF;
  EXECUTE 'RESET ROLE';
 END IF;
END $snapshot_transfer_reseal$;
SET ROLE board_report_snapshot_owner;
INSERT INTO post_secrets.report_snapshot_catalog VALUES(true,post_secrets.report_snapshot_catalog_hash());
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report capture catalog did not qualify'; END IF; END $$;
RESET ROLE;
