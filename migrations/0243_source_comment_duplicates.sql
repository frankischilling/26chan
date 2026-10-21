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
-- Comparison history is forward-only. Existing rendered comments cannot be
-- reconstructed into the original pre-insert comparison representation.
CREATE TABLE post_secrets.comment_history (
 post_id bigint PRIMARY KEY REFERENCES content.posts(id) ON DELETE CASCADE,
 board text NOT NULL REFERENCES content.boards(slug) ON DELETE CASCADE,
 actor_hash bytea NOT NULL CHECK(octet_length(actor_hash)=32),
 projection_hash bytea NOT NULL CHECK(octet_length(projection_hash)=32),
 projection_version smallint NOT NULL CHECK(projection_version=1),
 request_at bigint NOT NULL CHECK(request_at BETWEEN 0 AND 9223372036854775507),
 captured_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX comment_history_lookup ON post_secrets.comment_history(board,actor_hash,projection_hash);
CREATE INDEX comment_history_expiry ON post_secrets.comment_history(request_at,captured_at,post_id);
REVOKE ALL ON post_secrets.comment_history FROM PUBLIC;
GRANT SELECT,INSERT,DELETE,UPDATE(post_id) ON post_secrets.comment_history TO board_posting_cooldown_owner;

CREATE FUNCTION content.comment_duplicate_remaining(p_actor bytea,p_board text,p_hash bytea,p_request bigint)
RETURNS bigint LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_last bigint; v_private boolean; v_invoker text;
BEGIN
 v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 IF v_invoker NOT IN ('board_public','board_staff','board_migrator') OR current_setting('transaction_isolation')<>'read committed'
 OR p_actor IS NULL OR octet_length(p_actor)<>32 OR p_hash IS NULL OR octet_length(p_hash)<>32
 OR p_request IS NULL OR p_request NOT BETWEEN 0 AND 9223372036854775507 THEN
  RAISE EXCEPTION 'Invalid comment admission context' USING ERRCODE='23514'; END IF;
 SELECT staff_only INTO v_private FROM content.boards WHERE slug=p_board FOR UPDATE;
 IF NOT FOUND OR (v_private AND v_invoker='board_public') THEN
  RAISE EXCEPTION 'Board not found' USING ERRCODE='P0002'; END IF;
 SELECT max(floor(extract(epoch FROM p.created_at))::bigint) INTO v_last
 FROM post_secrets.comment_history h JOIN content.posts p ON p.id=h.post_id AND p.board=h.board
 JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
 WHERE h.board=p_board AND h.actor_hash=p_actor AND h.projection_hash=p_hash AND h.projection_version=1
 AND NOT p.deleted AND NOT t.deleted AND floor(extract(epoch FROM p.created_at))::bigint>p_request-300;
 IF v_last IS NOT NULL AND v_last<>0 THEN RETURN v_last+300-p_request; END IF;
 RETURN NULL;
END $$;

CREATE FUNCTION content.record_comment_projection(p_post bigint,p_actor bytea,p_hash bytea)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_board text; v_request bigint; v_private boolean; v_invoker text;
BEGIN
 v_invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 IF v_invoker NOT IN ('board_public','board_staff','board_migrator') OR current_setting('transaction_isolation')<>'read committed'
 OR p_actor IS NULL OR octet_length(p_actor)<>32 OR p_hash IS NULL OR octet_length(p_hash)<>32 THEN
  RAISE EXCEPTION 'Invalid comment projection' USING ERRCODE='23514'; END IF;
 SELECT h.board INTO v_board FROM post_secrets.posting_history h WHERE h.post_id=p_post AND h.actor_hash=p_actor AND h.xmin::text=pg_current_xact_id()::text;
 IF NOT FOUND THEN RAISE EXCEPTION 'Posting identity unavailable' USING ERRCODE='P0243'; END IF;
 SELECT staff_only INTO v_private FROM content.boards WHERE slug=v_board FOR UPDATE;
 IF NOT FOUND OR (v_private AND v_invoker='board_public') THEN
  RAISE EXCEPTION 'Board not found' USING ERRCODE='P0002'; END IF;
 -- Recheck under a fresh READ COMMITTED snapshot after the board lock.
 SELECT h.request_at INTO v_request FROM post_secrets.posting_history h
 JOIN content.posts p ON p.id=h.post_id AND p.board=h.board
 JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board
 WHERE h.post_id=p_post AND h.board=v_board AND h.actor_hash=p_actor
 AND h.xmin::text=pg_current_xact_id()::text AND NOT p.deleted AND NOT t.deleted;
 IF NOT FOUND THEN RAISE EXCEPTION 'Posting identity changed' USING ERRCODE='P0243'; END IF;
 IF (SELECT count(*) FROM (SELECT 1 FROM post_secrets.comment_history WHERE board=v_board LIMIT 100000) bounded)>=100000 THEN
  RAISE EXCEPTION 'Comment history capacity exhausted' USING ERRCODE='P0243'; END IF;
 INSERT INTO post_secrets.comment_history(post_id,board,actor_hash,projection_hash,projection_version,request_at)
 VALUES(p_post,v_board,p_actor,p_hash,1,v_request);
END $$;

CREATE FUNCTION content.remove_comment_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF NEW.deleted THEN DELETE FROM post_secrets.comment_history WHERE post_id=NEW.id; END IF;
 RETURN NEW;
END $$;
CREATE FUNCTION content.remove_thread_comment_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF NEW.deleted THEN DELETE FROM post_secrets.comment_history h USING content.posts p
 WHERE p.id=h.post_id AND p.thread_id=NEW.id AND p.board=NEW.board; END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER remove_comment_history AFTER UPDATE OF deleted ON content.posts
 FOR EACH ROW EXECUTE FUNCTION content.remove_comment_history();
CREATE TRIGGER remove_thread_comment_history AFTER UPDATE OF deleted ON content.threads
 FOR EACH ROW EXECUTE FUNCTION content.remove_thread_comment_history();

CREATE FUNCTION content.expire_comment_history() RETURNS bigint
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_count bigint; v_cutoff timestamptz:=clock_timestamp()-interval '1 hour';
BEGIN
 IF (CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END)<>'board_moderation_retention' THEN
  RAISE EXCEPTION 'Retention role required' USING ERRCODE='42501'; END IF;
 WITH expired AS (SELECT h.post_id FROM post_secrets.comment_history h
 WHERE request_at<floor(extract(epoch FROM v_cutoff))::bigint AND captured_at<v_cutoff
 AND EXISTS(SELECT 1 FROM content.posts p WHERE p.id=h.post_id AND p.created_at<v_cutoff)
 ORDER BY request_at,captured_at,post_id LIMIT 1000 FOR UPDATE SKIP LOCKED),
 removed AS (DELETE FROM post_secrets.comment_history h USING expired e WHERE h.post_id=e.post_id RETURNING h.post_id)
 SELECT count(*) INTO v_count FROM removed;
 RETURN v_count;
END $$;
REVOKE ALL ON FUNCTION content.comment_duplicate_remaining(bytea,text,bytea,bigint),
 content.record_comment_projection(bigint,bytea,bytea),content.remove_comment_history(),
 content.remove_thread_comment_history(),content.expire_comment_history() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.comment_duplicate_remaining(bytea,text,bytea,bigint),
 content.record_comment_projection(bigint,bytea,bytea) TO board_public,board_staff;
GRANT EXECUTE ON FUNCTION content.expire_comment_history() TO board_moderation_retention;
GRANT CREATE ON SCHEMA content TO board_posting_cooldown_owner;
ALTER FUNCTION content.comment_duplicate_remaining(bytea,text,bytea,bigint) OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.record_comment_projection(bigint,bytea,bytea) OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.remove_comment_history() OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.remove_thread_comment_history() OWNER TO board_posting_cooldown_owner;
ALTER FUNCTION content.expire_comment_history() OWNER TO board_posting_cooldown_owner;
REVOKE CREATE ON SCHEMA content FROM board_posting_cooldown_owner;
GRANT CREATE ON SCHEMA content TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
CREATE FUNCTION content.revalidate_staff_quota_refusal(p_session bytea,p_csrf bytea,p_idle integer,p_board text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_account bigint; v_credential bytea;
BEGIN
 IF session_user<>'board_staff' OR current_setting('transaction_isolation')<>'read committed'
    OR p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
    OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
    OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug=p_board) THEN
  RAISE EXCEPTION 'Staff refusal authorization is unavailable.' USING ERRCODE='28000'; END IF;
 SELECT account_id,credential_id INTO v_account,v_credential FROM staff_identity.sessions WHERE token_hash=p_session;
 IF NOT FOUND THEN RAISE EXCEPTION 'Staff refusal authorization is unavailable.' USING ERRCODE='28000'; END IF;
 -- These locks are only taken on terminal refusal paths. No independent
 -- accepted-post issuer will run afterward. NOWAIT avoids an inverse wait
 -- against revocation while this transaction already owns content locks.
 PERFORM id FROM staff_identity.accounts WHERE id=v_account FOR SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Staff refusal authorization is unavailable.' USING ERRCODE='28000'; END IF;
 PERFORM token_hash FROM staff_identity.sessions WHERE token_hash=p_session FOR SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Staff refusal authorization is unavailable.' USING ERRCODE='28000'; END IF;
 PERFORM id FROM staff_identity.credentials WHERE id=v_credential FOR KEY SHARE NOWAIT;
 IF NOT FOUND THEN RAISE EXCEPTION 'Staff refusal authorization is unavailable.' USING ERRCODE='28000'; END IF;
 IF NOT EXISTS(
  SELECT 1 FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id
  JOIN staff_identity.credentials c ON c.id=s.credential_id AND c.account_id=a.id
  WHERE s.token_hash=p_session AND s.account_id=v_account AND s.credential_id=v_credential
    AND s.csrf_hash=p_csrf AND s.expires_at>clock_timestamp()
    AND s.last_activity_at>clock_timestamp()-make_interval(secs=>p_idle)
    AND s.authenticated_at>clock_timestamp()-interval '10 minutes'
    AND a.revoked_at IS NULL AND a.role IN('janitor','moderator','manager','admin')
    AND staff_identity.has_board_access(a.id,p_board)
    AND (p_board<>'test' OR (a.role IN('moderator','manager','admin') AND 'developer'=ANY(a.flags)))
 ) THEN RAISE EXCEPTION 'Staff refusal authorization is unavailable.' USING ERRCODE='28000'; END IF;
EXCEPTION WHEN lock_not_available THEN
 RAISE EXCEPTION 'Staff refusal authorization is unavailable.' USING ERRCODE='28000';
END $$;
REVOKE ALL ON FUNCTION content.revalidate_staff_quota_refusal(bytea,bytea,integer,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.revalidate_staff_quota_refusal(bytea,bytea,integer,text) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_staff_post_owner;
GRANT CREATE ON SCHEMA post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION post_secrets.transfer_catalog_hash() RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
    SELECT c.*,n.nspname,n.nspname||'.'||c.relname AS identity
    FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
    WHERE c.relkind='r' AND (
      (n.nspname='post_secrets' AND (c.relname LIKE 'thread_transfer_%'
        OR c.relname IN('deletion','op_peers','op_replies','posting_history','anonymous_posts','anonymous_sessions','poster_contexts','image_upload_history','image_history_scopes','comment_history')))
      OR (n.nspname='content' AND c.relname IN('boards','threads','posts','post_media','moderation_audit'))
      OR (n.nspname='verification_private' AND c.relname IN('post_ownership','pass_posts'))
      OR (n.nspname='staff_identity' AND c.relname IN('accounts','sessions','credentials')))
 ), triggers AS (
    SELECT t.*,r.identity FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal
 ), funcs AS (
    SELECT p.* FROM pg_proc p WHERE p.proowner='board_thread_transfer_owner'::regrole
      OR p.oid IN(SELECT t.tgfoid FROM triggers t)
      OR p.oid IN('content.revalidate_staff_quota_refusal(bytea,bytea,integer,text)'::regprocedure,
        'content.comment_duplicate_remaining(bytea,text,bytea,bigint)'::regprocedure,
        'content.record_comment_projection(bigint,bytea,bytea)'::regprocedure,
        'content.expire_comment_history()'::regprocedure,
        'content.image_duplicate_candidate(text,text,text,bigint,boolean,boolean)'::regprocedure,
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
