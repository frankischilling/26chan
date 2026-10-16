-- Credit claims are advisory until the protected write atomically spends one.
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before credit consumption'; END IF;
END $$;
RESET ROLE;
ALTER TABLE verification_private.receipts ADD COLUMN credit_identity bytea
 CHECK(credit_identity IS NULL OR (from_credit AND octet_length(credit_identity)=32));
ALTER TABLE verification_private.post_handoffs ADD COLUMN credit_identity bytea
 CHECK(credit_identity IS NULL OR octet_length(credit_identity)=32);
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.take_write_receipt_data(
 p_receipt bytea,p_peer bytea,p_session bytea,p_purpose text,p_board text,p_target bigint,p_context bytea
) RETURNS verification_private.receipts
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE r verification_private.receipts%ROWTYPE; v_expiry bigint; v_now timestamptz;
BEGIN
 IF p_receipt IS NULL OR octet_length(p_receipt)<>32
    OR p_peer IS NULL OR octet_length(p_peer)<>32
    OR p_session IS NULL OR octet_length(p_session)<>32
    OR p_context IS NULL OR octet_length(p_context)<>32
    OR p_purpose IS NULL OR p_board IS NULL OR p_target IS NULL
    OR NOT ((p_purpose='thread' AND p_target=0)
      OR (p_purpose IN('reply','report') AND p_target>0)) THEN
  RAISE EXCEPTION 'Invalid protected challenge context.' USING ERRCODE='23514';
 END IF;
 -- Same order as issuer checks: board, session, verification policy, receipt.
 -- The surrounding write retains these locks through commit or rollback.
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 SELECT s.expires_at INTO v_expiry FROM post_secrets.anonymous_sessions s
 WHERE s.token_hash=p_session FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166'; END IF;
 PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
 SELECT a.* INTO r FROM verification_private.receipts a WHERE a.token_hash=p_receipt FOR UPDATE;
 IF NOT FOUND OR r.peer_hash<>p_peer OR r.session_hash<>p_session
    OR r.purpose<>p_purpose OR r.board IS DISTINCT FROM p_board
    OR r.target<>p_target OR r.context_hash<>p_context THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166';
 END IF;
 v_now:=clock_timestamp();
 IF r.expires_at<=v_now OR v_expiry<=extract(epoch FROM v_now)::bigint THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166';
 END IF;
 IF r.from_credit AND (r.credit_identity IS NULL OR p_purpose NOT IN('reply','report')) THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166'; END IF;
 DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;
 RETURN r;
END $$;
REVOKE ALL ON FUNCTION verification_private.take_write_receipt_data(bytea,bytea,bytea,text,text,bigint,bytea) FROM PUBLIC;

CREATE OR REPLACE FUNCTION verification_private.take_write_receipt(
 p_receipt bytea,p_peer bytea,p_session bytea,p_purpose text,p_board text,p_target bigint,p_context bytea
) RETURNS boolean LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE r verification_private.receipts%ROWTYPE;
BEGIN
 r:=verification_private.take_write_receipt_data(p_receipt,p_peer,p_session,p_purpose,p_board,p_target,p_context);
 IF r.from_credit THEN RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166'; END IF;
 RETURN r.may_grant_credit AND NOT r.suspicious AND r.unsolved_before<2;
END $$;
REVOKE ALL ON FUNCTION verification_private.take_write_receipt(bytea,bytea,bytea,text,text,bigint,bytea) FROM PUBLIC;
CREATE OR REPLACE FUNCTION content.check_public_post_challenge() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE invoker text; enabled boolean; raw text; proof jsonb; key text; session_hash bytea;
 peer bytea; receipt verification_private.receipts%ROWTYPE;
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'posts' OR TG_OP<>'INSERT'
    OR TG_LEVEL<>'ROW' OR TG_WHEN<>'BEFORE' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid post challenge trigger.' USING ERRCODE='23514'; END IF;
 invoker:=CASE WHEN current_setting('role')='none' THEN session_user::text ELSE current_setting('role') END;
 -- Staff writes retain their separate, independently authenticated authority.
 IF invoker IN('board_staff','board_migrator') THEN RETURN NEW; END IF;
 IF invoker<>'board_public' THEN RAISE EXCEPTION 'Post challenge authority is unavailable.' USING ERRCODE='42501'; END IF;
 SELECT b.captcha_enabled INTO enabled FROM content.boards b WHERE b.slug=NEW.board AND NOT b.staff_only FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 raw:=current_setting('board.challenge_context',true);
 PERFORM set_config('board.challenge_context','',true);
 IF NOT enabled THEN RETURN NEW; END IF;
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
 receipt:=verification_private.take_write_receipt_data(decode(proof->>'receipt','hex'),peer,session_hash,
  CASE WHEN NEW.id=NEW.thread_id THEN 'thread' ELSE 'reply' END,NEW.board,
  CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END,decode(proof->>'context','hex'));
 INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid,credit_identity)
 VALUES(NEW.id,session_hash,peer,receipt.may_grant_credit AND NOT receipt.suspicious AND receipt.unsolved_before<2,pg_current_xact_id(),CASE WHEN receipt.from_credit THEN receipt.credit_identity ELSE NULL END);
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.check_public_post_challenge() FROM PUBLIC;
CREATE OR REPLACE FUNCTION verification_private.finish_post_challenge(
 p_post bigint,p_session bytea,p_network bytea,p_address bytea,p_environment bytea,p_request bigint
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE h verification_private.post_handoffs%ROWTYPE; profile record; board text;
 allow_credits boolean; is_reply boolean; v_limit integer; v_now timestamptz;
BEGIN
 SELECT a.* INTO h FROM verification_private.post_handoffs a WHERE a.post_id=p_post FOR UPDATE;
 IF NOT FOUND THEN RETURN; END IF;
 IF h.registration_xid<>pg_current_xact_id() OR h.session_hash<>p_session THEN
  RAISE EXCEPTION 'Challenge activity registration changed.' USING ERRCODE='P0166'; END IF;
 SELECT p.board,b.captcha_enabled AND b.captcha_twister AND b.captcha_allow_bypass
 ,p.id<>p.thread_id INTO board,allow_credits,is_reply FROM content.posts p JOIN content.boards b ON b.slug=p.board
 WHERE p.id=p_post AND NOT p.deleted;
 IF NOT FOUND THEN RAISE EXCEPTION 'Challenge post is unavailable.' USING ERRCODE='P0166'; END IF;
 IF h.credit_identity IS NOT NULL THEN
  IF NOT is_reply OR NOT allow_credits OR board IN('pol','trash','bant')
     OR NOT (SELECT current_score>=80 FROM content.verification_risk(h.peer_hash,NULL)) THEN
   RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
  SELECT * INTO profile FROM verification_private.credit_profile(p_session,p_network,p_address,p_environment,p_request);
  IF NOT profile.can_spend OR profile.identity_key<>h.credit_identity THEN
   RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
  -- The receipt consumer retains the policy lock. The conditional update also
  -- protects the last credit against concurrent claims by verified browsers.
  UPDATE verification_private.credits SET remaining=remaining-1
   WHERE identity_key=profile.identity_key AND remaining>0 AND expires_at>clock_timestamp();
  IF NOT FOUND THEN RAISE EXCEPTION 'Challenge credit is unavailable.' USING ERRCODE='P0166'; END IF;
 ELSIF h.may_grant_credit AND allow_credits AND board NOT IN('pol','trash','bant')
    AND (SELECT current_score>=80 FROM content.verification_risk(h.peer_hash,NULL)) THEN
  SELECT * INTO profile FROM verification_private.credit_profile(p_session,p_network,p_address,p_environment,p_request);
  IF profile.credit_count>0 THEN
   SELECT challenge_limit INTO v_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
   IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
   v_now:=clock_timestamp();
   DELETE FROM verification_private.credits c WHERE c.identity_key IN(
    SELECT old.identity_key FROM verification_private.credits old WHERE old.expires_at<=v_now
    ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
   -- A full courtesy-credit store does not reject an otherwise valid post.
   IF EXISTS(SELECT 1 FROM verification_private.credits WHERE identity_key=profile.identity_key)
      OR (SELECT count(*) FROM (SELECT 1 FROM verification_private.credits LIMIT v_limit) bounded)<v_limit THEN
    INSERT INTO verification_private.credits(identity_key,remaining,expires_at)
    VALUES(profile.identity_key,profile.credit_count,to_timestamp(p_request)+make_interval(secs=>profile.ttl_seconds))
    ON CONFLICT(identity_key) DO UPDATE SET remaining=EXCLUDED.remaining,expires_at=EXCLUDED.expires_at;
   END IF;
  END IF;
 END IF;
 DELETE FROM verification_private.post_handoffs WHERE post_id=p_post;
END $$;
REVOKE ALL ON FUNCTION verification_private.finish_post_challenge(bigint,bytea,bytea,bytea,bytea,bigint) FROM PUBLIC;

CREATE FUNCTION content.verification_credit_receipt(
 p_receipt bytea,p_peer bytea,p_session bytea,p_network bytea,p_address bytea,p_environment bytea,
 p_board text,p_target bigint,p_purpose text,p_context bytea,p_request bigint
) RETURNS TABLE(status text,remaining smallint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE profile record; credit verification_private.credits%ROWTYPE; allowed boolean;
 v_limit integer; now_at timestamptz;
BEGIN
 IF p_receipt IS NULL OR octet_length(p_receipt)<>32 OR p_peer IS NULL OR octet_length(p_peer)<>32
    OR p_context IS NULL OR octet_length(p_context)<>32 OR p_board IS NULL
    OR p_target IS NULL OR p_target<=0 OR p_purpose IS NULL OR p_purpose NOT IN('reply','report') THEN
  RAISE EXCEPTION 'Invalid challenge credit context.' USING ERRCODE='23514'; END IF;
 SELECT b.captcha_enabled AND b.captcha_twister AND b.captcha_allow_bypass AND b.slug NOT IN('pol','trash','bant')
 INTO allowed FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 IF NOT allowed THEN RETURN QUERY SELECT 'ineligible'::text,0::smallint; RETURN; END IF;
 IF p_purpose='reply' AND NOT EXISTS(SELECT 1 FROM content.visible_threads t WHERE t.board=p_board AND t.id=p_target)
    OR p_purpose='report' AND NOT EXISTS(SELECT 1 FROM content.posts p JOIN content.visible_threads t
      ON t.board=p.board AND t.id=p.thread_id WHERE p.board=p_board AND p.id=p_target AND NOT p.deleted) THEN
  RAISE EXCEPTION 'Public target not found.' USING ERRCODE='P0002'; END IF;
 SELECT * INTO profile FROM verification_private.credit_profile(p_session,p_network,p_address,p_environment,p_request);
 IF NOT profile.can_spend THEN RETURN QUERY SELECT 'ineligible'::text,0::smallint; RETURN; END IF;
 SELECT receipt_limit INTO v_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
 IF NOT (SELECT current_score>=80 FROM content.verification_risk(p_peer,NULL)) THEN
  RETURN QUERY SELECT 'ineligible'::text,0::smallint; RETURN; END IF;
 SELECT c.* INTO credit FROM verification_private.credits c WHERE c.identity_key=profile.identity_key FOR UPDATE;
 now_at:=clock_timestamp();
 IF NOT FOUND OR credit.remaining<=0 OR credit.expires_at<=now_at THEN
  RETURN QUERY SELECT 'ineligible'::text,0::smallint; RETURN; END IF;
 DELETE FROM verification_private.receipts r WHERE r.token_hash IN(
  SELECT old.token_hash FROM verification_private.receipts old WHERE old.expires_at<=now_at
  ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
 IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.receipts LIMIT v_limit) bounded)>=v_limit THEN
  RETURN QUERY SELECT 'unavailable'::text,0::smallint; RETURN; END IF;
 INSERT INTO verification_private.receipts(token_hash,peer_hash,session_hash,purpose,board,target,context_hash,
  from_credit,suspicious,unsolved_before,may_grant_credit,created_at,expires_at,credit_identity)
 VALUES(p_receipt,p_peer,p_session,p_purpose,p_board,p_target,p_context,true,false,0,false,
  now_at,least(now_at+interval '30 seconds',credit.expires_at),profile.identity_key);
 -- Multiple outstanding claims may name the same last credit. Only a protected
 -- write can spend it, and failed writes restore both claim and credit.
 RETURN QUERY SELECT 'ready'::text,credit.remaining;
END $$;
REVOKE ALL ON FUNCTION content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint) TO board_verifier;

CREATE OR REPLACE FUNCTION content.public_challenge_ready() RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT count(*)=6 AND bool_and(
  p.proowner='board_anonymous_owner'::regrole
  AND p.prosecdef=(p.proname IN('check_public_post_challenge','check_post_challenge_completion'))
  AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']
  AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner))
  FROM pg_proc p WHERE p.oid IN(
   to_regprocedure('content.check_public_post_challenge()'),
   to_regprocedure('content.check_post_challenge_completion()'),
   to_regprocedure('verification_private.finish_post_challenge(bigint,bytea,bytea,bytea,bytea,bigint)'),
   to_regprocedure('verification_private.take_write_receipt(bytea,bytea,bytea,text,text,bigint,bytea)'),
   to_regprocedure('verification_private.credit_profile(bytea,bytea,bytea,bytea,bigint)'),
   to_regprocedure('verification_private.take_write_receipt_data(bytea,bytea,bytea,text,text,bigint,bytea)'))),false)
 AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
  AND t.tgname='a00_public_post_challenge' AND t.tgfoid='content.check_public_post_challenge()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=7 AND NOT t.tgdeferrable AND NOT t.tgisinternal
  AND t.tgnargs=0 AND t.tgqual IS NULL)
 AND EXISTS(SELECT 1 FROM pg_trigger t WHERE t.tgrelid='content.posts'::regclass
  AND t.tgname='post_challenge_completion' AND t.tgfoid='content.check_post_challenge_completion()'::regprocedure
  AND t.tgenabled='O' AND t.tgtype=5 AND t.tgdeferrable AND t.tginitdeferred AND NOT t.tgisinternal
  AND t.tgnargs=0 AND t.tgqual IS NULL)
 AND NOT EXISTS(SELECT 1 FROM unnest(ARRAY['board_public','board_staff','board_auth','board_verifier','board_mailer']) runtime(role_name)
  WHERE pg_has_role(runtime.role_name,'board_anonymous_owner','MEMBER')
   OR has_schema_privilege(runtime.role_name,'verification_private','USAGE')
   OR has_table_privilege(runtime.role_name,'verification_private.post_handoffs','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER'))
$$;
REVOKE ALL ON FUNCTION content.public_challenge_ready() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.public_challenge_ready() TO board_public,board_verifier;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Credit consumption did not preserve transfer authority'; END IF;
END $$;
RESET ROLE;
