-- Private Pass identity for post-linked moderation; erased with deleted or archived posts.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer authority drifted before Pass post identity'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.report_snapshot_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Snapshot authority drifted before Pass post identity'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 PERFORM singleton FROM restrictions.catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Restriction authority drifted before Pass post identity'; END IF;
END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Posting authority drifted before Pass post identity'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report profile drifted before Pass post identity'; END IF; END $$;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid='content.check_public_post_challenge()'::regprocedure
 AND encode(sha256(convert_to(prosrc,'UTF8')),'hex')='67ef8e7ba616b2d44dc791903a83fc4b72ae7d24b7342fe5a02cd95be6487e7d') THEN
 RAISE EXCEPTION 'Challenge consumer drifted before private Pass capture'; END IF;
END $$;
GRANT REFERENCES(id) ON content.posts TO board_anonymous_owner;
GRANT CREATE ON SCHEMA verification_private,content TO board_anonymous_owner;
SET LOCAL ROLE board_anonymous_owner;
CREATE TABLE verification_private.pass_posts (
 post_id bigint PRIMARY KEY CHECK(post_id>0) REFERENCES content.posts(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
 identity_hash bytea NOT NULL CHECK(octet_length(identity_hash)=32)
);
REVOKE ALL ON verification_private.pass_posts FROM PUBLIC;
GRANT SELECT,INSERT ON verification_private.pass_posts TO board_thread_transfer_owner;
-- PostgreSQL row locks require UPDATE on at least one selected column.
GRANT UPDATE(post_id) ON verification_private.pass_posts TO board_thread_transfer_owner;
-- Retain only the private identifier HMAC, never the login token or PIN hash.
CREATE FUNCTION verification_private.pass_erase_post_identity() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_OP<>'UPDATE' OR TG_LEVEL<>'ROW' OR TG_WHEN<>'AFTER' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid membership erasure context' USING ERRCODE='23514'; END IF;
 IF TG_TABLE_NAME='posts' AND TG_NAME='pass_post_identity_erasure' THEN
  IF NEW.deleted OR NEW.content_erased THEN DELETE FROM verification_private.pass_posts WHERE post_id=NEW.id; END IF;
 ELSIF TG_TABLE_NAME='threads' AND TG_NAME='pass_archive_identity_erasure' THEN
  IF NEW.archived_at IS NOT NULL THEN
   DELETE FROM verification_private.pass_posts membership USING content.posts p
    WHERE membership.post_id=p.id AND p.board=NEW.board AND p.thread_id=NEW.id;
  END IF;
 ELSE RAISE EXCEPTION 'Invalid membership erasure target' USING ERRCODE='23514'; END IF;
 RETURN NULL;
END $$;
REVOKE ALL ON FUNCTION verification_private.pass_erase_post_identity() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION verification_private.pass_erase_post_identity() TO board_migrator;
GRANT EXECUTE ON FUNCTION content.public_pass_ready() TO board_thread_transfer_owner;
RESET ROLE;
CREATE TRIGGER pass_post_identity_erasure AFTER UPDATE ON content.posts
 FOR EACH ROW WHEN(NEW.deleted OR NEW.content_erased) EXECUTE FUNCTION verification_private.pass_erase_post_identity();
CREATE TRIGGER pass_archive_identity_erasure AFTER UPDATE ON content.threads
 FOR EACH ROW WHEN(OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL) EXECUTE FUNCTION verification_private.pass_erase_post_identity();
SET LOCAL ROLE board_anonymous_owner;
REVOKE EXECUTE ON FUNCTION verification_private.pass_erase_post_identity() FROM board_migrator;
CREATE OR REPLACE FUNCTION content.check_public_post_challenge() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE credit_request text; invoker text; enabled boolean; pass_only boolean; raw text; proof jsonb; key text; session_hash bytea;
 peer bytea; receipt verification_private.receipts%ROWTYPE; membership record; show_year boolean;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'posts' OR TG_OP<>'INSERT'
    OR TG_LEVEL<>'ROW' OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid post challenge trigger.' USING ERRCODE='23514'; END IF;
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 -- Staff writes retain their separate, independently authenticated authority.
 IF invoker IN('board_staff','board_migrator') THEN RETURN NEW; END IF;
 NEW.since4pass:=0;
 show_year:=coalesce(current_setting('board.pass_year_requested',true)='true',false);
 PERFORM set_config('board.pass_year_requested','false',true);
 IF invoker<>'board_public' THEN RAISE EXCEPTION 'Post challenge authority is unavailable.' USING ERRCODE='42501'; END IF;
 SELECT b.captcha_enabled,b.pass_post_only INTO enabled,pass_only FROM content.boards b WHERE b.slug=NEW.board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 raw:=current_setting('board.challenge_context',true);
 PERFORM set_config('board.challenge_context','',true);
 credit_request:=current_setting('board.challenge_credit_request',true);
 PERFORM set_config('board.challenge_credit_request','',true);
 IF NOT enabled AND (raw IS NULL OR raw='') THEN RETURN NEW; END IF;
 IF enabled AND pass_only AND (raw IS NULL OR raw='') THEN
  RAISE EXCEPTION 'Only Pass users can post on this board.' USING ERRCODE='P0201'; END IF;
 IF raw IS NULL OR octet_length(raw)>1024 OR raw='' THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166'; END IF;
 BEGIN proof:=raw::jsonb;
 EXCEPTION WHEN invalid_text_representation THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END;
 IF jsonb_typeof(proof)<>'object' THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 IF (SELECT count(*) FROM jsonb_object_keys(proof))<>4 THEN
  RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 FOREACH key IN ARRAY ARRAY['receipt','peer','session','context'] LOOP
  IF jsonb_typeof(proof->key) IS DISTINCT FROM 'string' OR octet_length(proof->>key)<>64
     OR (proof->>key) !~ '^[0-9a-f]{64}$' THEN
   RAISE EXCEPTION 'Invalid challenge proof context.' USING ERRCODE='P0166'; END IF;
 END LOOP;
 session_hash:=decode(proof->>'session','hex'); peer:=decode(proof->>'peer','hex');
 IF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=decode(proof->>'receipt','hex')) THEN
  SELECT * INTO STRICT membership FROM verification_private.pass_take_receipt(decode(proof->>'receipt','hex'),peer,session_hash,decode(proof->>'context','hex'),
   CASE WHEN NEW.id=NEW.thread_id THEN 'thread' ELSE 'reply' END,NEW.board,
   CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END);
  INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid,credit_identity)
   VALUES(NEW.id,session_hash,peer,false,pg_current_xact_id(),NULL);
  INSERT INTO verification_private.pass_post_handoffs VALUES(NEW.id,session_hash,pg_current_xact_id());
  IF show_year AND membership.purchase_year BETWEEN 1 AND 9999 THEN NEW.since4pass:=membership.purchase_year; END IF;
  INSERT INTO verification_private.pass_posts(post_id,identity_hash)
   SELECT NEW.id,e.token_hash FROM verification_private.pass_entitlements e WHERE e.id=membership.entitlement_id;
  IF NOT FOUND THEN RAISE EXCEPTION 'Membership capture unavailable' USING ERRCODE='55000'; END IF;
  RETURN NEW;
 END IF;

 receipt:=verification_private.take_write_receipt_data(decode(proof->>'receipt','hex'),peer,session_hash,
  CASE WHEN NEW.id=NEW.thread_id THEN 'thread' ELSE 'reply' END,NEW.board,
  CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END,decode(proof->>'context','hex'));
 -- Source CAPTCHA-disabled mode itself sets captcha_bypass, even without a Pass.
 IF enabled AND pass_only THEN
  RAISE EXCEPTION 'Only Pass users can post on this board.' USING ERRCODE='P0201'; END IF;
 INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid,credit_identity)
 VALUES(NEW.id,session_hash,peer,receipt.may_grant_credit AND NOT receipt.suspicious AND receipt.unsolved_before<2
  AND coalesce(credit_request='eligible',false),pg_current_xact_id(),CASE WHEN receipt.from_credit THEN receipt.credit_identity ELSE NULL END);
 RETURN NEW;
END $$;
CREATE OR REPLACE FUNCTION verification_private.pass_catalog_hash() RETURNS bytea LANGUAGE sql STABLE
SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 WITH relations AS (
  SELECT c.*,n.nspname||'.'||c.relname AS identity FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='verification_private' AND c.relname LIKE 'pass\_%' ESCAPE '\'
 ), funcs AS (
  SELECT p.* FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
  WHERE (n.nspname IN('verification_private','content') AND p.proname LIKE 'pass\_%' ESCAPE '\')
   OR (n.nspname='content' AND p.proname='public_pass_ready')
   OR p.oid='content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)'::regprocedure
 )
 SELECT sha256(convert_to(jsonb_build_array(
  (SELECT jsonb_agg(jsonb_build_array(r.identity,pg_get_userbyid(r.relowner),r.relkind,r.relpersistence,
    r.relrowsecurity,r.relforcerowsecurity,r.reloptions,
    verification_private.pass_acl(coalesce(r.relacl,acldefault(CASE WHEN r.relkind='S' THEN 's'::"char" ELSE 'r'::"char" END,r.relowner))),
    CASE WHEN r.relkind='i' THEN pg_get_indexdef(r.oid) END,
    (SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod),a.attnotnull,a.attidentity,
      pg_get_expr(d.adbin,d.adrelid),verification_private.pass_acl(a.attacl)) ORDER BY a.attnum)
     FROM pg_attribute a LEFT JOIN pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
     WHERE a.attrelid=r.oid AND a.attnum>0 AND NOT a.attisdropped),
    (SELECT jsonb_agg(jsonb_build_array(k.conname,pg_get_constraintdef(k.oid,true),k.convalidated) ORDER BY k.conname)
     FROM pg_constraint k WHERE k.conrelid=r.oid)) ORDER BY r.identity) FROM relations r),
  (SELECT jsonb_agg(jsonb_build_array(p.oid::regprocedure::text,pg_get_userbyid(p.proowner),
    verification_private.pass_acl(coalesce(p.proacl,acldefault('f',p.proowner))),pg_get_functiondef(p.oid))
    ORDER BY p.oid::regprocedure::text) FROM funcs p),
  (SELECT jsonb_agg(jsonb_build_array(r.identity,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid)) ORDER BY r.identity,t.tgname)
    FROM pg_trigger t JOIN relations r ON r.oid=t.tgrelid WHERE NOT t.tgisinternal),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgname,t.tgenabled,pg_get_triggerdef(t.oid))
   ORDER BY t.tgrelid::regclass::text,t.tgname) FROM pg_trigger t WHERE t.tgfoid='verification_private.pass_erase_post_identity()'::regprocedure),
  (SELECT jsonb_agg(jsonb_build_array(t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgenabled,t.tgtype,
   t.tgdeferrable,t.tginitdeferred,t.tgconstrrelid::regclass::text)
   ORDER BY t.tgrelid::regclass::text,t.tgfoid::regprocedure::text,t.tgtype,t.tgconstrrelid::regclass::text)
   FROM pg_trigger t WHERE t.tgisinternal AND t.tgconstraint IN(
    SELECT k.oid FROM pg_constraint k JOIN relations r ON r.oid=k.conrelid WHERE k.contype='f')),
  (SELECT jsonb_build_array(pg_get_userbyid(n.nspowner),verification_private.pass_acl(coalesce(n.nspacl,acldefault('n',n.nspowner))))
    FROM pg_namespace n WHERE n.nspname='verification_private'))::text,'UTF8'))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
GRANT CREATE ON SCHEMA content,post_secrets TO board_thread_transfer_owner;
SET LOCAL ROLE board_thread_transfer_owner;
CREATE OR REPLACE FUNCTION post_secrets.transfer_post_hash(p_id bigint) RETURNS bytea
LANGUAGE sql STABLE SET search_path=pg_catalog,pg_temp SET TimeZone='UTC' SET DateStyle='ISO, YMD' AS $$
 SELECT sha256(convert_to(jsonb_build_array(p.id,p.board,p.thread_id,p.name,p.trip,p.subject,p.comment,p.staff_notices,p.since4pass,
    (SELECT encode(m.identity_hash,'hex') FROM verification_private.pass_posts m WHERE m.post_id=p.id),
    p.created_at,p.deleted,p.content_erased,p.capcode,p.poster_id,p.json_op_poster_id,p.country,p.country_name,
    p.board_flag,p.board_flag_type,p.flag_name,p.image_spoiler,p.file_deleted,p.comment_format,
    p.staff_authorized_limits,encode(p.wordfilter_payload,'hex'),p.wordfilter_search,p.drawing_time_seconds,
    p.drawing_source_post_id,p.dice_result,p.fortune_text,p.fortune_color,
    encode(p.saved_presentation,'hex'),p.saved_presentation_search,
    (SELECT jsonb_build_array(h.automatic_browser,encode(h.automatic_prefix,'hex'))
       FROM post_secrets.posting_history h WHERE h.post_id=p.id AND h.board=p.board))::text,'UTF8'))
 FROM content.posts p WHERE p.id=p_id
$$;
CREATE OR REPLACE FUNCTION content.consume_text_thread_transfer(p_ticket bytea)
RETURNS TABLE(request_id text,source_board text,source_thread bigint,destination_board text,
    destination_thread bigint,post_count bigint,audit_id bigint,source_post_id bigint,destination_post_id bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE i post_secrets.thread_transfer_intents%ROWTYPE; v_count integer; v_audit bigint; v_now timestamptz;
    v_inserted integer; v_anonymous_at bigint;
BEGIN
    IF post_secrets.transfer_invoker()<>'board_staff' THEN RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM post_secrets.transfer_gate();
    SELECT * INTO i FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    IF NOT FOUND THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    IF post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,true,i.credential_id)<>i.account_id THEN
        RAISE EXCEPTION 'Transfer authorization is unavailable.' USING ERRCODE='P131A'; END IF;
    PERFORM ticket_hash FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket FOR UPDATE NOWAIT;
    IF NOT i.sealed THEN RAISE EXCEPTION 'Transfer is not sealed.' USING ERRCODE='P131I'; END IF;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    v_count:=post_secrets.transfer_lock_source(i.source_board,i.source_thread,i.destination_board);
    IF v_count<>i.post_count OR post_secrets.transfer_source_hash(i.source_board,i.source_thread,i.destination_board)
        IS DISTINCT FROM i.source_fingerprint OR EXISTS(
            SELECT 1 FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket
            AND post_secrets.transfer_post_hash(m.source_post_id) IS DISTINCT FROM m.source_hash) THEN
        RAISE EXCEPTION 'Transfer source changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM post_secrets.thread_transfer_map WHERE ticket_hash=p_ticket)<>v_count
        OR (SELECT count(*) FROM post_secrets.thread_transfer_sealed_rows WHERE ticket_hash=p_ticket)<>v_count
        OR EXISTS(SELECT 1 FROM post_secrets.thread_transfer_map m JOIN content.posts p ON p.id=m.destination_post_id WHERE m.ticket_hash=p_ticket)
        OR EXISTS(SELECT 1 FROM content.threads t WHERE t.id=i.destination_thread) THEN
        RAISE EXCEPTION 'Transfer destination changed.' USING ERRCODE='P131C'; END IF;
    IF (SELECT count(*) FROM (SELECT 1 FROM content.threads t WHERE t.board=i.destination_board AND NOT t.deleted
        AND t.archived_at IS NULL AND NOT t.sticky AND NOT t.undead LIMIT 1001) occupied)>=
        (SELECT b.thread_limit FROM content.boards b WHERE b.slug=i.destination_board)
        OR (SELECT count(*) FROM (SELECT 1 FROM post_secrets.posting_history h WHERE h.board=i.destination_board LIMIT 100001) occupied)
          +(SELECT count(*) FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m
            ON m.source_post_id=h.post_id WHERE m.ticket_hash=p_ticket)>100000 THEN
        RAISE EXCEPTION 'Transfer destination capacity is exhausted.' USING ERRCODE='P131L'; END IF;
    -- Session before membership; NOWAIT avoids reverse-order expiry cleanup.
    PERFORM s.token_hash FROM post_secrets.anonymous_sessions s WHERE EXISTS(
        SELECT 1 FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket AND a.token_hash=s.token_hash) ORDER BY s.token_hash FOR UPDATE NOWAIT;
    PERFORM d.post_id FROM post_secrets.deletion d JOIN post_secrets.thread_transfer_map m ON m.source_post_id=d.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY d.post_id FOR SHARE OF d NOWAIT;
    PERFORM o.thread_id FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread FOR SHARE NOWAIT;
    PERFORM r.post_id FROM post_secrets.op_replies r JOIN post_secrets.thread_transfer_map m ON m.source_post_id=r.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY r.post_id FOR SHARE OF r NOWAIT;
    PERFORM h.post_id FROM post_secrets.posting_history h JOIN post_secrets.thread_transfer_map m ON m.source_post_id=h.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY h.post_id FOR SHARE OF h NOWAIT;
    PERFORM a.post_id FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.source_post_id=a.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY a.post_id FOR SHARE OF a NOWAIT;
    PERFORM o.post_id FROM verification_private.post_ownership o JOIN post_secrets.thread_transfer_map m ON m.source_post_id=o.post_id
        WHERE m.ticket_hash=p_ticket ORDER BY o.post_id FOR SHARE OF o NOWAIT;
    PERFORM membership.post_id FROM verification_private.pass_posts membership
      JOIN post_secrets.thread_transfer_map m ON m.source_post_id=membership.post_id
      WHERE m.ticket_hash=p_ticket ORDER BY membership.post_id FOR SHARE OF membership NOWAIT;
    INSERT INTO post_secrets.thread_transfer_active
        SELECT pg_current_xact_id(),m.destination_post_id,i.destination_board,i.destination_thread,p_ticket
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    v_now:=clock_timestamp();
    INSERT INTO content.threads(id,board,created_at,bumped_at,modified_at,http_modified_at,reply_count,
        sticky,sticky_rank,closed,permasage,permaage,undead,archived_at,archive_expires_at,deleted,content_erased)
    SELECT i.destination_thread,i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
        false,0,false,false,false,false,NULL,NULL,false,false
    FROM content.posts p WHERE p.id=i.source_thread AND p.board=i.source_board;
    INSERT INTO content.posts(id,board,thread_id,name,trip,subject,comment,created_at,capcode,poster_id,json_op_poster_id,
        country,country_name,board_flag,board_flag_type,flag_name,image_spoiler,file_deleted,comment_format,
        staff_authorized_limits,saved_presentation,saved_presentation_search,staff_notices,since4pass)
    SELECT m.destination_post_id,i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,p.created_at,
        p.capcode,NULL,NULL,CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
        CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
        NULL,'pol',NULL,false,p.file_deleted,m.comment_format,m.authorized,s.saved_presentation,s.saved_presentation_search,p.staff_notices,p.since4pass
    FROM post_secrets.thread_transfer_map m JOIN post_secrets.thread_transfer_sealed_rows s
        ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal JOIN content.posts p ON p.id=m.source_post_id
    JOIN content.boards b ON b.slug=i.source_board WHERE m.ticket_hash=p_ticket ORDER BY m.ordinal;
    GET DIAGNOSTICS v_inserted=ROW_COUNT;
    IF v_inserted<>v_count OR EXISTS(
        SELECT 1 FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.thread_transfer_sealed_rows s ON s.ticket_hash=m.ticket_hash AND s.ordinal=m.ordinal
        JOIN content.posts p ON p.id=m.source_post_id JOIN content.boards b ON b.slug=i.source_board
        LEFT JOIN content.posts d ON d.id=m.destination_post_id WHERE m.ticket_hash=p_ticket AND (
            d.id IS NULL OR ROW(d.board,d.thread_id,d.name,d.trip,d.subject,d.comment,d.created_at,d.capcode,
                d.poster_id,d.json_op_poster_id,d.country,d.country_name,d.board_flag,d.board_flag_type,d.flag_name,
                d.image_spoiler,d.file_deleted,d.comment_format,d.staff_authorized_limits,d.saved_presentation,
                d.saved_presentation_search,d.staff_notices,d.since4pass,d.wordfilter_payload,d.wordfilter_search,d.drawing_time_seconds,
                d.drawing_source_post_id,d.dice_result,d.fortune_text,d.fortune_color,d.deleted,d.content_erased)
              IS DISTINCT FROM ROW(i.destination_board,i.destination_thread,p.name,p.trip,p.subject,s.comment,
                p.created_at,p.capcode,NULL::text,NULL::text,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country ELSE 'XX' END,
                CASE WHEN b.country_flags AND p.board_flag IS NULL THEN p.country_name ELSE 'Unknown' END,
                NULL::text,'pol'::text,NULL::text,false,p.file_deleted,m.comment_format,m.authorized,
                s.saved_presentation,s.saved_presentation_search,p.staff_notices,p.since4pass,NULL::bytea,NULL::text,NULL::integer,
                NULL::bigint,NULL::text,NULL::text,NULL::text,false,false))) THEN
        RAISE EXCEPTION 'Transfer payload verification failed.' USING ERRCODE='P131R';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM content.threads t JOIN content.posts p ON p.id=i.source_thread
        WHERE t.id=i.destination_thread AND ROW(t.board,t.created_at,t.bumped_at,t.modified_at,t.http_modified_at,
            t.reply_count,t.sticky,t.sticky_rank,t.closed,t.permasage,t.permaage,t.undead,t.archived_at,
            t.archive_expires_at,t.deleted,t.content_erased)
          IS NOT DISTINCT FROM ROW(i.destination_board,p.created_at,v_now,p.created_at,v_now,v_count-1,
            false,0::smallint,false,false,false,false,NULL::timestamptz,NULL::timestamptz,false,false)) THEN
        RAISE EXCEPTION 'Transfer thread verification failed.' USING ERRCODE='P131R';
    END IF;
    INSERT INTO post_secrets.deletion(post_id,password_hash)
        SELECT m.destination_post_id,d.password_hash FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.op_peers(thread_id,peer)
        SELECT i.destination_thread,o.peer FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread;
    INSERT INTO post_secrets.op_replies(post_id,thread_id)
        SELECT m.destination_post_id,i.destination_thread FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id AND r.thread_id=i.source_thread
        WHERE m.ticket_hash=p_ticket;
    INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at,automatic_identity,registration_xid,automatic_browser,automatic_prefix)
        SELECT m.destination_post_id,i.destination_board,i.destination_thread,h.actor_hash,h.request_at,h.automatic_identity,
            CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid END,h.automatic_browser,h.automatic_prefix
        FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
            AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket;
    v_anonymous_at:=extract(epoch FROM clock_timestamp())::bigint;
    INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof)
        SELECT m.destination_post_id,a.token_hash,a.password_proof FROM post_secrets.thread_transfer_map m
        JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
        JOIN post_secrets.anonymous_sessions s ON s.token_hash=a.token_hash AND s.expires_at>v_anonymous_at
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
        WHERE m.ticket_hash=p_ticket;
    INSERT INTO verification_private.post_ownership(post_id,identity_id,password_proof)
        SELECT m.destination_post_id,o.identity_id,o.password_proof FROM post_secrets.thread_transfer_map m
        JOIN verification_private.post_ownership o ON o.post_id=m.source_post_id
        JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND o.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
        WHERE m.ticket_hash=p_ticket;
    -- Compare complete private row sets, not only counts. These sets and all
    -- hashes/peers/UUIDs remain inside SQL; errors contain no evidence values.
    INSERT INTO verification_private.pass_posts(post_id,identity_hash)
      SELECT m.destination_post_id,membership.identity_hash FROM post_secrets.thread_transfer_map m
      JOIN verification_private.pass_posts membership ON membership.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket;
    IF EXISTS(
        WITH expected(kind,id,payload) AS (
            SELECT 'deletion',m.destination_post_id,jsonb_build_array(d.password_hash)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.deletion d ON d.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',i.destination_thread,jsonb_build_array(o.peer)
            FROM post_secrets.op_peers o WHERE o.thread_id=i.source_thread
            UNION ALL SELECT 'reply',m.destination_post_id,jsonb_build_array(i.destination_thread)
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.op_replies r ON r.post_id=m.source_post_id
                AND r.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',m.destination_post_id,jsonb_build_array(i.destination_board,i.destination_thread,
                encode(h.actor_hash,'hex'),h.request_at,h.automatic_identity,
                CASE WHEN h.registration_xid=pg_current_xact_id() THEN NULL ELSE h.registration_xid::text END,
                h.automatic_browser,encode(h.automatic_prefix,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.posting_history h ON h.post_id=m.source_post_id
                AND h.board=i.source_board AND h.thread_id=i.source_thread WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',m.destination_post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN post_secrets.anonymous_posts a ON a.post_id=m.source_post_id
            JOIN post_secrets.anonymous_sessions session ON session.token_hash=a.token_hash AND session.expires_at>v_anonymous_at
            JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND a.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
            WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'pass',m.destination_post_id,jsonb_build_array(encode(membership.identity_hash,'hex'))
                FROM post_secrets.thread_transfer_map m JOIN verification_private.pass_posts membership ON membership.post_id=m.source_post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'email-ownership',m.destination_post_id,jsonb_build_array(o.identity_id,encode(o.password_proof,'hex'))
            FROM post_secrets.thread_transfer_map m JOIN verification_private.post_ownership o ON o.post_id=m.source_post_id
            JOIN post_secrets.deletion d ON d.post_id=m.source_post_id AND o.password_proof=sha256(convert_to(d.password_hash,'UTF8'))
            WHERE m.ticket_hash=p_ticket
        ), actual(kind,id,payload) AS (
            SELECT 'deletion',d.post_id,jsonb_build_array(d.password_hash) FROM post_secrets.deletion d
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=d.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'peer',o.thread_id,jsonb_build_array(o.peer) FROM post_secrets.op_peers o WHERE o.thread_id=i.destination_thread
            UNION ALL SELECT 'reply',r.post_id,jsonb_build_array(r.thread_id) FROM post_secrets.op_replies r
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=r.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'history',h.post_id,jsonb_build_array(h.board,h.thread_id,encode(h.actor_hash,'hex'),
                h.request_at,h.automatic_identity,h.registration_xid::text,h.automatic_browser,encode(h.automatic_prefix,'hex')) FROM post_secrets.posting_history h
                JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=h.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'anonymous',a.post_id,jsonb_build_array(encode(a.token_hash,'hex'),encode(a.password_proof,'hex'))
                FROM post_secrets.anonymous_posts a JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=a.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'pass',membership.post_id,jsonb_build_array(encode(membership.identity_hash,'hex'))
                FROM verification_private.pass_posts membership JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=membership.post_id WHERE m.ticket_hash=p_ticket
            UNION ALL SELECT 'email-ownership',o.post_id,jsonb_build_array(o.identity_id,encode(o.password_proof,'hex'))
                FROM verification_private.post_ownership o JOIN post_secrets.thread_transfer_map m ON m.destination_post_id=o.post_id WHERE m.ticket_hash=p_ticket
        ) SELECT 1 FROM ((SELECT * FROM expected EXCEPT ALL SELECT * FROM actual)
            UNION ALL (SELECT * FROM actual EXCEPT ALL SELECT * FROM expected)) different
    ) OR EXISTS(SELECT 1 FROM post_secrets.poster_contexts c JOIN post_secrets.thread_transfer_map m
        ON m.destination_post_id=c.post_id WHERE m.ticket_hash=p_ticket) THEN
        RAISE EXCEPTION 'Transfer evidence verification failed.' USING ERRCODE='P131R';
    END IF;
    UPDATE content.threads SET deleted=true WHERE board=i.source_board AND id=i.source_thread;
    INSERT INTO content.moderation_audit(account_id,board,target_id,action)
        VALUES(i.account_id,i.source_board,i.source_thread,'text-thread-transfer') RETURNING id INTO v_audit;
    INSERT INTO post_secrets.thread_transfer_results(request_id,account_id,source_board,source_thread,
        destination_board,destination_thread,post_count,audit_id)
        VALUES(i.request_id,i.account_id,i.source_board,i.source_thread,i.destination_board,i.destination_thread,v_count,v_audit);
    INSERT INTO post_secrets.thread_transfer_result_posts
        SELECT i.request_id,m.ordinal,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_map m WHERE m.ticket_hash=p_ticket;
    IF i.expires_at<=clock_timestamp() THEN RAISE EXCEPTION 'Transfer has expired.' USING ERRCODE='P131E'; END IF;
    PERFORM post_secrets.transfer_authority(i.session_hash,i.csrf_hash,i.idle_seconds,
        i.source_board,i.destination_board,true,false,i.credential_id);
    DELETE FROM post_secrets.thread_transfer_active WHERE ticket_hash=p_ticket AND transaction_id=pg_current_xact_id();
    DELETE FROM post_secrets.thread_transfer_intents WHERE ticket_hash=p_ticket;
    RETURN QUERY SELECT r.request_id::text,r.source_board,r.source_thread,r.destination_board,r.destination_thread,
        r.post_count::bigint,r.audit_id,m.source_post_id,m.destination_post_id
        FROM post_secrets.thread_transfer_results r JOIN post_secrets.thread_transfer_result_posts m ON m.request_id=r.request_id
        WHERE r.request_id=i.request_id ORDER BY m.ordinal;
END $$;
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
      OR p.oid IN('content.erasure_visibility_ready()'::regprocedure,
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
CREATE OR REPLACE FUNCTION content.text_thread_transfer_ready() RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 WITH owner_role AS (
    SELECT oid FROM pg_roles WHERE rolname='board_thread_transfer_owner'
        AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)
 ), required(schema_name,table_name,rls,policy_name) AS (VALUES
    ('content','boards',true,'board_visibility'),('content','posts',true,'post_visibility'),
    ('content','threads',true,'thread_visibility'),('content','post_media',false,''),
    ('post_secrets','deletion',true,'deletion_visibility'),('post_secrets','op_peers',true,'op_peer_visibility'),
    ('post_secrets','op_replies',true,'op_reply_visibility'),('post_secrets','posting_history',false,''),
    ('post_secrets','anonymous_posts',false,''),('post_secrets','anonymous_sessions',false,''),
    ('post_secrets','poster_contexts',false,''),('verification_private','post_ownership',false,''),('verification_private','pass_posts',false,''),
    ('staff_identity','accounts',false,''),('staff_identity','sessions',false,''),('staff_identity','credentials',false,'')
 ), relations AS (
    SELECT c.*,r.rls,r.policy_name FROM required r JOIN pg_namespace n ON n.nspname=r.schema_name
       JOIN pg_class c ON c.relnamespace=n.oid AND c.relname=r.table_name
 )
 SELECT EXISTS(SELECT 1 FROM owner_role)
    AND content.erasure_visibility_ready()
    AND content.public_pass_ready()
    AND (SELECT count(*) FROM relations)=16
    AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.relrowsecurity<>r.rls OR r.relforcerowsecurity
        OR r.relkind<>'r' OR r.relpersistence<>'p')
    AND NOT EXISTS(SELECT 1 FROM relations r WHERE r.rls AND NOT EXISTS(
        SELECT 1 FROM pg_policy p WHERE p.polrelid=r.oid AND p.polname=r.policy_name
          AND p.polroles=ARRAY[0]::oid[] AND p.polcmd='*' AND p.polpermissive AND p.polwithcheck IS NULL))
    -- In particular no transfer-owner restrictive policy may hide replies or
    -- private evidence that canonical erasure would subsequently remove.
    AND NOT EXISTS(SELECT 1 FROM relations r JOIN pg_policy p ON p.polrelid=r.oid
        WHERE (p.polname<>r.policy_name OR NOT p.polpermissive) AND EXISTS(
          SELECT 1 FROM unnest(p.polroles) role_id WHERE role_id=0
             OR pg_has_role((SELECT oid FROM owner_role),role_id,'USAGE')))
    AND EXISTS(SELECT 1 FROM pg_policy p WHERE p.polrelid='content.boards'::regclass AND p.polname='board_visibility'
        AND lower(regexp_replace(pg_get_expr(p.polqual,p.polrelid),'[[:space:]()]','','g'))=
            'notstaff_onlyorcurrent_user=anyarray[''board_staff''::name,''board_migrator''::name]')
    AND NOT EXISTS(SELECT 1 FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.member
        WHERE m.roleid=(SELECT oid FROM owner_role) AND r.rolname<>'board_migrator')
    AND NOT EXISTS(SELECT 1 FROM pg_auth_members m WHERE m.member=(SELECT oid FROM owner_role))
    AND has_schema_privilege('board_thread_transfer_owner','verification_private','USAGE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','verification_private','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','content','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','post_secrets','CREATE')
    AND NOT has_schema_privilege('board_thread_transfer_owner','staff_identity','CREATE')
    AND (SELECT definition_hash=post_secrets.transfer_catalog_hash()
         FROM post_secrets.thread_transfer_catalog WHERE singleton)
    AND NOT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
        JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
        CROSS JOIN (VALUES('board_public'),('board_staff'),('board_auth'),('board_media'),
            ('board_media_read'),('board_media_intake'),('board_monitor'),('board_verifier'),('board_mailer')) runtime(name)
        WHERE n.nspname='post_secrets' AND c.relname LIKE 'thread_transfer_%'
            AND (has_column_privilege(runtime.name,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES')
                OR has_table_privilege(runtime.name,c.oid,'DELETE,TRUNCATE,TRIGGER')))
    AND NOT EXISTS(SELECT 1 FROM pg_constraint c JOIN pg_class r ON r.oid=c.conrelid
        JOIN pg_namespace n ON n.oid=r.relnamespace JOIN pg_class target ON target.oid=c.confrelid
        JOIN pg_namespace tn ON tn.oid=target.relnamespace
        WHERE n.nspname='post_secrets' AND r.relname LIKE 'thread_transfer_%' AND c.contype='f'
            AND (tn.nspname<>'post_secrets' OR target.relname NOT IN('thread_transfer_intents','thread_transfer_map')))
$$;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,post_secrets FROM board_thread_transfer_owner;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_catalog SET definition_hash=verification_private.pass_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Private Pass capture authority differs'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_report_snapshot_owner;
UPDATE post_secrets.report_snapshot_catalog SET definition_hash=post_secrets.report_snapshot_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass post identity changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass post identity changed transfer authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.catalog SET definition_hash=restrictions.catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass post identity changed restriction authority'; END IF; END $$;
RESET ROLE;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM true OR content.public_challenge_ready() IS DISTINCT FROM true
 OR post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass post identity changed posting authority'; END IF; END $$;
RESET ROLE;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Pass post identity changed report profile'; END IF; END $$;
