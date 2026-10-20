-- Scope ordinary duplicate images to active posts and the source board limit.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before image duplicate admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before image duplicate admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before image duplicate admission'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before image duplicate admission'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before image duplicate admission'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before image duplicate admission'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
ALTER TABLE content.boards ADD COLUMN image_repost_limit integer NOT NULL DEFAULT 0
 CHECK(image_repost_limit BETWEEN 0 AND 100000);
UPDATE content.boards SET image_repost_limit=3 WHERE slug='wsg';
CREATE INDEX post_media_live_digest ON content.post_media(normalized_md5,post_id)
 WHERE normalized_md5 IS NOT NULL AND NOT file_deleted;
GRANT SELECT(image_repost_limit) ON content.boards TO board_attachment_owner;
GRANT CREATE ON SCHEMA content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE FUNCTION content.image_duplicate_candidate(
 p_job text,p_capability text,p_board text,p_target bigint,p_authorized boolean,p_badged boolean
) RETURNS TABLE(post_id bigint,thread_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE metadata record; candidate record; maximum integer; hits bigint;
BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Duplicate image authority is unavailable' USING ERRCODE='P0241'; END IF;
 -- This holds the board, target and one-use upload locks and checks any VIP
 -- badge against its private receipt. Caller flags alone grant no exemption.
 SELECT * INTO STRICT metadata FROM content.attachment_upload_filter_metadata(
  p_job,p_capability,p_board,p_target,p_authorized,p_badged);
 IF p_badged THEN RETURN; END IF;
 IF metadata.normalized_md5 IS NULL OR metadata.normalized_md5 !~ '^[0-9a-f]{32}$' THEN
  RAISE EXCEPTION 'Duplicate image identity is unavailable' USING ERRCODE='P0241'; END IF;
 SELECT p.id,p.thread_id INTO candidate FROM content.post_media m
 JOIN content.posts p ON p.id=m.post_id JOIN content.threads t ON t.board=p.board AND t.id=p.thread_id
 WHERE p.board=p_board AND NOT p.deleted AND NOT p.file_deleted AND NOT m.file_deleted
  AND NOT t.deleted AND t.archived_at IS NULL AND m.normalized_md5=decode(metadata.normalized_md5,'hex')
  AND CASE WHEN p_target=0 THEN p.id=p.thread_id ELSE p.thread_id=p_target END
 ORDER BY p.id LIMIT 1;
 IF FOUND THEN post_id:=candidate.id;thread_id:=candidate.thread_id;RETURN NEXT;RETURN;END IF;
 SELECT b.image_repost_limit INTO STRICT maximum FROM content.boards b WHERE b.slug=p_board;
 IF p_target>0 AND maximum>0 THEN
  SELECT count(*) INTO hits FROM (
   SELECT 1 FROM content.post_media m JOIN content.posts p ON p.id=m.post_id
   JOIN content.threads t ON t.board=p.board AND t.id=p.thread_id
   WHERE p.board=p_board AND p.id<>p.thread_id AND NOT p.deleted AND NOT p.file_deleted
    AND NOT m.file_deleted AND NOT t.deleted AND t.archived_at IS NULL
    AND m.normalized_md5=decode(metadata.normalized_md5,'hex') LIMIT maximum
  ) matching;
  IF hits>=maximum THEN post_id:=0;thread_id:=0;RETURN NEXT;END IF;
 END IF;
END $$;
REVOKE ALL ON FUNCTION content.image_duplicate_candidate(text,text,text,bigint,boolean,boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.image_duplicate_candidate(text,text,text,bigint,boolean,boolean) TO board_public,board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_attachment_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION post_secrets.transfer_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
    SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE c.relkind='r' AND (
      (n.nspname='post_secrets' AND (c.relname LIKE 'thread_transfer_%'
        OR c.relname IN('deletion','op_peers','op_replies','posting_history','anonymous_posts','anonymous_sessions','poster_contexts')))
      OR (n.nspname='content' AND c.relname IN('boards','threads','posts','post_media','moderation_audit'))
      OR (n.nspname='verification_private' AND c.relname IN('post_ownership','pass_posts'))
      OR (n.nspname='staff_identity' AND c.relname IN('accounts','sessions','credentials')))
 ), triggers AS (
    SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
 ), funcs AS (
    SELECT p.* FROM pg_proc p WHERE p.proowner='board_thread_transfer_owner'::regrole
      OR p.oid IN(SELECT t.tgfoid FROM triggers t)
      OR p.oid IN('content.image_duplicate_candidate(text,text,text,bigint,boolean,boolean)'::regprocedure,
        'content.erasure_visibility_ready()'::regprocedure,
        'staff_identity.has_board_access(bigint,text)'::regprocedure,
        'content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean,timestamptz)'::regprocedure,
        'content.insert_post_attachment(bigint,text,bigint,text,text,text,text,text,boolean)'::regprocedure,
        'content.consume_staff_attachment_receipt(bigint,text,bigint,text,bytea,boolean,boolean)'::regprocedure)
 )
 SELECT sha256(convert_to(jsonb_build_array(
    (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner)::text,
       r.relkind,r.relpersistence,r.relrowsecurity,r.relforcerowsecurity,
       post_secrets.transfer_acl(coalesce(r.relacl,acldefault('r',r.relowner))),
       (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,
          a.attidentity,a.attgenerated,pg_get_expr(d.adbin,d.adrelid),post_secrets.transfer_acl(a.attacl)) ORDER BY a.attnum)
        FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
        WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
       (SELECT jsonb_agg(jsonb_build_array(k.conname,k.contype,k.convalidated,k.condeferrable,k.condeferred,
           pg_get_constraintdef(k.oid,true)) ORDER BY k.conname) FROM pg_constraint k WHERE k.conrelid=r.oid),
       (SELECT jsonb_agg(jsonb_build_array(p.polname,p.polcmd,p.polpermissive,
          (SELECT jsonb_agg(CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END
             ORDER BY CASE WHEN x=0 THEN 'PUBLIC' ELSE pg_get_userbyid(x)::text END) FROM unnest(p.polroles) x),
          pg_get_expr(p.polqual,p.polrelid),pg_get_expr(p.polwithcheck,p.polrelid)) ORDER BY p.polname)
        FROM pg_policy p WHERE p.polrelid=r.oid)) ORDER BY r.identity) FROM relations r),
    (SELECT jsonb_agg(jsonb_build_array(t.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
       ORDER BY t.identity,t.tgname) FROM triggers t),
    (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner)::text,
       post_secrets.transfer_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
       ORDER BY p.oid::regprocedure::text) FROM funcs p))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets FROM board_thread_transfer_owner;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
SELECT set_config('board.migration_image_classifier_hash',encode(post_secrets.classifier_history_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.classifier_history_catalog SET definition_hash=decode(current_setting('board.migration_image_classifier_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_report_admission_owner;
SELECT set_config('board.migration_image_report_hash',encode(post_secrets.report_profile_catalog_hash(),'hex'),true);
RESET ROLE;
UPDATE post_secrets.report_profile_catalog SET definition_hash=decode(current_setting('board.migration_image_report_hash'),'hex') WHERE singleton;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Image duplicate admission changed verification_private.pass_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.vip_catalog SET definition_hash=verification_private.vip_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_vip_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Image duplicate admission changed verification_private.vip_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Image duplicate admission changed post_secrets.report_snapshot_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Image duplicate admission changed post_secrets.thread_transfer_catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Image duplicate admission changed restrictions.catalog authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_challenge_ready() IS DISTINCT FROM true OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Image duplicate admission changed challenge authority'; END IF; END $$;
RESET ROLE;
