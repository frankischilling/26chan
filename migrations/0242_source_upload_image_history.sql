-- Persist the source upload-board duplicate window independently of live posts.
-- Scope ordinary duplicate images to active posts and the source board limit.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before upload image history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before upload image history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before upload image history'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.pass_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before upload image history'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before upload image history'; END IF; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 PERFORM singleton FROM verification_private.vip_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.public_vip_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'VIP authority drifted before upload image history'; END IF;
END $$;
RESET ROLE;
SELECT singleton FROM post_secrets.classifier_history_catalog WHERE singleton FOR UPDATE;
SELECT singleton FROM post_secrets.report_profile_catalog WHERE singleton FOR UPDATE;
ALTER TABLE content.boards ADD COLUMN image_history_scope text
 CHECK(image_history_scope IS NULL OR image_history_scope ~ '^[a-z0-9_]{1,64}$');
-- Both supplied configurations name f_md5. Physical source DB placement was
-- external; deployments can assign separate scopes if their board map requires it.
UPDATE content.boards SET image_history_scope='f_md5' WHERE slug IN('f','test');
GRANT SELECT(image_history_scope) ON content.boards TO board_attachment_owner;
GRANT CREATE ON SCHEMA post_secrets,content TO board_attachment_owner;
SET LOCAL ROLE board_attachment_owner;
CREATE TABLE post_secrets.image_history_scopes(
 scope text PRIMARY KEY CHECK(scope ~ '^[a-z0-9_]{1,64}$'),
 row_limit integer NOT NULL DEFAULT 100000 CHECK(row_limit BETWEEN 1 AND 1000000)
);
CREATE TABLE post_secrets.image_upload_history(
 post_id bigint PRIMARY KEY CHECK(post_id>0),
 scope text NOT NULL REFERENCES post_secrets.image_history_scopes(scope),
 digest bytea NOT NULL CHECK(octet_length(digest)=16),
 accepted_at timestamptz NOT NULL
);
CREATE INDEX image_upload_history_digest ON post_secrets.image_upload_history(scope,digest,accepted_at DESC);
CREATE INDEX image_upload_history_expiry ON post_secrets.image_upload_history(accepted_at,post_id);
REVOKE ALL ON post_secrets.image_history_scopes,post_secrets.image_upload_history FROM PUBLIC;
INSERT INTO post_secrets.image_history_scopes(scope) VALUES('f_md5');
CREATE OR REPLACE FUNCTION content.image_duplicate_candidate(
 p_job text,p_capability text,p_board text,p_target bigint,p_authorized boolean,p_badged boolean
) RETURNS TABLE(post_id bigint,thread_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE metadata record; candidate record; maximum integer; hits bigint; v_scope text; observed timestamptz;
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
 SELECT b.image_history_scope INTO v_scope FROM content.boards b WHERE b.slug=p_board;
 IF v_scope IS NOT NULL THEN
  PERFORM s.scope FROM post_secrets.image_history_scopes s WHERE s.scope=v_scope FOR UPDATE;
  IF NOT FOUND THEN RAISE EXCEPTION 'Image history scope is unavailable' USING ERRCODE='P0242'; END IF;
  observed:=date_trunc('second',clock_timestamp());
  IF EXISTS(SELECT 1 FROM post_secrets.image_upload_history h WHERE h.scope=v_scope
   AND h.digest=decode(metadata.normalized_md5,'hex')
   AND h.accepted_at>observed-interval '24 hours') THEN
   post_id:=-1;thread_id:=-1;RETURN NEXT;
  END IF;
 END IF;
END $$;

CREATE FUNCTION content.record_image_upload_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_scope text; maximum integer; observed timestamptz;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'post_media' OR TG_LEVEL<>'ROW'
 OR TG_WHEN<>'AFTER' OR TG_OP<>'INSERT' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid image history trigger' USING ERRCODE='23514'; END IF;
 IF NEW.normalized_md5 IS NULL THEN RETURN NEW; END IF;
 SELECT b.image_history_scope INTO v_scope FROM content.posts p JOIN content.boards b ON b.slug=p.board
 WHERE p.id=NEW.post_id AND b.upload_board FOR UPDATE OF b;
 IF v_scope IS NULL THEN RETURN NEW; END IF;
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Image history authority unavailable' USING ERRCODE='P0242'; END IF;
 SELECT s.row_limit INTO STRICT maximum FROM post_secrets.image_history_scopes s WHERE s.scope=v_scope FOR UPDATE;
 observed:=date_trunc('second',clock_timestamp());
 -- Never evict recent history to admit a new file. Trim a bounded old batch,
 -- and fail closed if the deployment-owned capacity is still exhausted.
 DELETE FROM post_secrets.image_upload_history h WHERE h.post_id IN(
  SELECT expired_row.post_id FROM post_secrets.image_upload_history expired_row WHERE expired_row.scope=v_scope
   AND expired_row.accepted_at<observed-interval '48 hours' ORDER BY expired_row.accepted_at,expired_row.post_id LIMIT 256);
 IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.image_upload_history h
  WHERE h.scope=v_scope LIMIT maximum) bounded)>=maximum THEN
  RAISE EXCEPTION 'Image history capacity exhausted' USING ERRCODE='P0242'; END IF;
 INSERT INTO post_secrets.image_upload_history(post_id,scope,digest,accepted_at)
 VALUES(NEW.post_id,v_scope,NEW.normalized_md5,observed);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.record_image_upload_history() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.record_image_upload_history() TO board_migrator;
CREATE FUNCTION content.expire_image_upload_history() RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE removed bigint; cutoff timestamptz;
BEGIN
 IF session_user<>'board_moderation_retention' OR current_setting('transaction_isolation')<>'read committed'
 OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Image history expiry unavailable' USING ERRCODE='42501'; END IF;
 cutoff:=date_trunc('second',clock_timestamp())-interval '48 hours';
 WITH candidates AS (SELECT post_id FROM post_secrets.image_upload_history
  WHERE accepted_at<cutoff
  ORDER BY accepted_at,post_id LIMIT 1000 FOR UPDATE SKIP LOCKED),
 deleted AS (DELETE FROM post_secrets.image_upload_history h USING candidates c
  WHERE h.post_id=c.post_id RETURNING 1)
 SELECT count(*) INTO removed FROM deleted;
 RETURN removed;
END $$;
REVOKE ALL ON FUNCTION content.expire_image_upload_history() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.expire_image_upload_history() TO board_moderation_retention;
RESET ROLE;
CREATE TRIGGER record_image_upload_history AFTER INSERT ON content.post_media
 FOR EACH ROW EXECUTE FUNCTION content.record_image_upload_history();
SET LOCAL ROLE board_attachment_owner;
REVOKE EXECUTE ON FUNCTION content.record_image_upload_history() FROM board_migrator;
RESET ROLE;
REVOKE CREATE ON SCHEMA post_secrets,content FROM board_attachment_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION post_secrets.transfer_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
    SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE c.relkind='r' AND (
      (n.nspname='post_secrets' AND (c.relname LIKE 'thread_transfer_%'
        OR c.relname IN('deletion','op_peers','op_replies','posting_history','anonymous_posts','anonymous_sessions','poster_contexts','image_upload_history','image_history_scopes')))
      OR (n.nspname='content' AND c.relname IN('boards','threads','posts','post_media','moderation_audit'))
      OR (n.nspname='verification_private' AND c.relname IN('post_ownership','pass_posts'))
      OR (n.nspname='staff_identity' AND c.relname IN('accounts','sessions','credentials')))
 ), triggers AS (
    SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
 ), funcs AS (
    SELECT p.* FROM pg_proc p WHERE p.proowner='board_thread_transfer_owner'::regrole
      OR p.oid IN(SELECT t.tgfoid FROM triggers t)
      OR p.oid IN('content.image_duplicate_candidate(text,text,text,bigint,boolean,boolean)'::regprocedure,
        'content.expire_image_upload_history()'::regprocedure,
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
