SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before post challenge enforcement'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF (SELECT encode(sha256(convert_to(prosrc,'UTF8')),'hex') FROM pg_proc
  WHERE oid='content.register_anonymous_post(bytea,bytea,bytea,bytea,boolean,text,bigint,bigint)'::regprocedure)
  IS DISTINCT FROM 'e070415fb0618cf79861d1a41007f2ae56dba3ede0184f511a9dfc55ae439a04' THEN
  RAISE EXCEPTION 'Anonymous post registration changed before challenge enforcement'; END IF;
END $$;

CREATE TABLE verification_private.post_handoffs (
 post_id bigint PRIMARY KEY REFERENCES content.posts(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
 session_hash bytea NOT NULL REFERENCES post_secrets.anonymous_sessions(token_hash),
 peer_hash bytea NOT NULL CHECK(octet_length(peer_hash)=32),
 may_grant_credit boolean NOT NULL,
 registration_xid xid8 NOT NULL
);
REVOKE ALL ON verification_private.post_handoffs FROM PUBLIC;
GRANT SELECT,INSERT,DELETE ON verification_private.post_handoffs TO board_anonymous_owner;
GRANT UPDATE(post_id) ON verification_private.post_handoffs TO board_anonymous_owner;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
GRANT TRIGGER ON content.posts TO board_anonymous_owner;
SET ROLE board_anonymous_owner;

CREATE FUNCTION content.check_public_post_challenge() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE invoker text; enabled boolean; raw text; proof jsonb; key text; session_hash bytea;
 peer bytea; grant_credit boolean;
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
 grant_credit:=verification_private.take_write_receipt(decode(proof->>'receipt','hex'),peer,session_hash,
  CASE WHEN NEW.id=NEW.thread_id THEN 'thread' ELSE 'reply' END,NEW.board,
  CASE WHEN NEW.id=NEW.thread_id THEN 0 ELSE NEW.thread_id END,decode(proof->>'context','hex'));
 INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid)
 VALUES(NEW.id,session_hash,peer,grant_credit,pg_current_xact_id());
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.check_public_post_challenge() FROM PUBLIC;
CREATE TRIGGER a00_public_post_challenge BEFORE INSERT ON content.posts
 FOR EACH ROW EXECUTE FUNCTION content.check_public_post_challenge();

CREATE FUNCTION content.check_post_challenge_completion() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
BEGIN
 IF TG_TABLE_SCHEMA<>'content' OR TG_TABLE_NAME<>'posts' OR TG_OP<>'INSERT'
    OR TG_LEVEL<>'ROW' OR TG_WHEN<>'AFTER' OR TG_NARGS<>0 THEN
  RAISE EXCEPTION 'Invalid post challenge completion trigger.' USING ERRCODE='23514'; END IF;
 IF EXISTS(SELECT 1 FROM verification_private.post_handoffs WHERE post_id=NEW.id) THEN
  RAISE EXCEPTION 'Challenge activity registration is required.' USING ERRCODE='P0166'; END IF;
 RETURN NEW;
END $$;
REVOKE ALL ON FUNCTION content.check_post_challenge_completion() FROM PUBLIC;
CREATE CONSTRAINT TRIGGER post_challenge_completion AFTER INSERT ON content.posts
 DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION content.check_post_challenge_completion();

CREATE FUNCTION verification_private.finish_post_challenge(
 p_post bigint,p_session bytea,p_network bytea,p_address bytea,p_environment bytea,p_request bigint
) RETURNS void LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE h verification_private.post_handoffs%ROWTYPE; profile record; board text;
 allow_credits boolean; v_limit integer; v_now timestamptz;
BEGIN
 SELECT a.* INTO h FROM verification_private.post_handoffs a WHERE a.post_id=p_post FOR UPDATE;
 IF NOT FOUND THEN RETURN; END IF;
 IF h.registration_xid<>pg_current_xact_id() OR h.session_hash<>p_session THEN
  RAISE EXCEPTION 'Challenge activity registration changed.' USING ERRCODE='P0166'; END IF;
 SELECT p.board,b.captcha_enabled AND b.captcha_twister AND b.captcha_allow_bypass
 INTO board,allow_credits FROM content.posts p JOIN content.boards b ON b.slug=p.board
 WHERE p.id=p_post AND NOT p.deleted;
 IF NOT FOUND THEN RAISE EXCEPTION 'Challenge post is unavailable.' USING ERRCODE='P0166'; END IF;
 IF h.may_grant_credit AND allow_credits AND board NOT IN('pol','trash','bant')
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

CREATE OR REPLACE FUNCTION content.register_anonymous_post(
    p_token bytea,p_network bytea,p_address bytea,p_environment bytea,
    p_minted boolean,p_board text,p_post bigint,p_now bigint
) RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_kind smallint; v_proof bytea; v_identity uuid;
BEGIN
    PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
    SELECT (1 | CASE WHEN p.id=p.thread_id THEN 4 ELSE 0 END
        | CASE WHEN EXISTS(SELECT 1 FROM content.visible_post_media m WHERE m.post_id=p.id AND NOT m.file_deleted) THEN 2 ELSE 0 END)::smallint,
        sha256(convert_to(d.password_hash,'UTF8')) INTO v_kind,v_proof
    FROM content.posts p JOIN post_secrets.deletion d ON d.post_id=p.id
    JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board
    WHERE p.id=p_post AND p.board=p_board AND NOT p.deleted AND NOT t.deleted;
    IF NOT FOUND OR EXISTS(SELECT 1 FROM post_secrets.anonymous_posts a WHERE a.post_id=p_post) THEN
        RAISE EXCEPTION 'Anonymous post registration is unavailable.' USING ERRCODE='23514';
    END IF;
    -- Registration may claim only the row inserted by this transaction's
    -- private posting-history trigger. Old unregistered posts cannot acquire
    -- either admission equality or anonymous deletion authority by adoption.
    IF NOT EXISTS(SELECT 1 FROM post_secrets.posting_history h
        JOIN content.posts p ON p.id=h.post_id AND p.board=h.board AND p.thread_id=h.thread_id
        WHERE h.post_id=p_post AND h.board=p_board
            AND h.registration_xid=pg_catalog.pg_current_xact_id()
            AND h.automatic_identity IS NULL) THEN
        RAISE EXCEPTION 'Anonymous post registration is unavailable.' USING ERRCODE='23514';
    END IF;
    PERFORM post_secrets.resolve_automatic_identity(p_token,p_minted,p_now,true);
    PERFORM verification_private.finish_post_challenge(p_post,p_token,p_network,p_address,p_environment,p_now);
    PERFORM post_secrets.advance_anonymous_session(p_token,p_network,p_address,p_environment,p_minted,v_kind,p_now);
    -- Valid activity owns the session lock. Allocate only now; COALESCE keeps
    -- the same private identity across activity/idle resets and cookie refresh.
    UPDATE post_secrets.anonymous_sessions a
        SET automatic_identity=coalesce(a.automatic_identity,pg_catalog.gen_random_uuid())
        WHERE a.token_hash=p_token RETURNING a.automatic_identity INTO v_identity;
    IF NOT FOUND THEN RAISE EXCEPTION 'Anonymous authorization changed.' USING ERRCODE='28000'; END IF;
    v_identity:=coalesce((SELECT b.identity_id FROM verification_private.browser_identities b
        WHERE b.session_hash=p_token),v_identity);
    INSERT INTO post_secrets.anonymous_posts(post_id,token_hash,password_proof) VALUES(p_post,p_token,v_proof);
    INSERT INTO verification_private.post_ownership(post_id,identity_id,password_proof)
        SELECT p_post,b.identity_id,v_proof FROM verification_private.browser_identities b WHERE b.session_hash=p_token;
    UPDATE post_secrets.posting_history h SET automatic_identity=v_identity
        WHERE h.post_id=p_post AND h.board=p_board
            AND h.registration_xid=pg_catalog.pg_current_xact_id()
            AND h.automatic_identity IS NULL;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'Anonymous post registration is unavailable.' USING ERRCODE='23514';
    END IF;
END $$;
CREATE FUNCTION content.public_challenge_ready() RETURNS boolean
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT coalesce((SELECT count(*)=5 AND bool_and(
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
   to_regprocedure('verification_private.credit_profile(bytea,bytea,bytea,bytea,bigint)'))),false)
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
REVOKE TRIGGER ON content.posts FROM board_anonymous_owner;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Post challenge boundary did not preserve transfer authority'; END IF;
END $$;
RESET ROLE;
