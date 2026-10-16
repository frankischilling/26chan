-- Expose credit advice without reserving a receipt or spending a credit.
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before credit advice'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
DROP FUNCTION content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint);
CREATE FUNCTION content.verification_credit_receipt(
 p_receipt bytea,p_peer bytea,p_session bytea,p_network bytea,p_address bytea,p_environment bytea,
 p_board text,p_target bigint,p_purpose text,p_context bytea,p_request bigint
) RETURNS TABLE(status text,remaining smallint,ttl_seconds integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE profile record; credit verification_private.credits%ROWTYPE; allowed boolean;
 v_limit integer; now_at timestamptz;
BEGIN
 IF (p_receipt IS NOT NULL AND octet_length(p_receipt)<>32) OR p_peer IS NULL OR octet_length(p_peer)<>32
    OR p_context IS NULL OR octet_length(p_context)<>32 OR p_board IS NULL
    OR p_target IS NULL OR p_target<=0 OR p_purpose IS NULL OR p_purpose NOT IN('reply','report') THEN
  RAISE EXCEPTION 'Invalid challenge credit context.' USING ERRCODE='23514'; END IF;
 SELECT b.captcha_enabled AND b.captcha_twister AND b.captcha_allow_bypass AND b.slug NOT IN('pol','trash','bant')
 INTO allowed FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 IF NOT allowed THEN RETURN QUERY SELECT 'ineligible'::text,0::smallint,0; RETURN; END IF;
 IF p_purpose='reply' AND NOT EXISTS(SELECT 1 FROM content.visible_threads t WHERE t.board=p_board AND t.id=p_target)
    OR p_purpose='report' AND NOT EXISTS(SELECT 1 FROM content.posts p JOIN content.visible_threads t
      ON t.board=p.board AND t.id=p.thread_id WHERE p.board=p_board AND p.id=p_target AND NOT p.deleted) THEN
  RAISE EXCEPTION 'Public target not found.' USING ERRCODE='P0002'; END IF;
 SELECT * INTO profile FROM verification_private.credit_profile(p_session,p_network,p_address,p_environment,p_request);
 IF NOT profile.can_spend THEN RETURN QUERY SELECT 'ineligible'::text,0::smallint,0; RETURN; END IF;
 SELECT receipt_limit INTO v_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
 IF NOT (SELECT current_score>=80 FROM content.verification_risk(p_peer,NULL)) THEN
  RETURN QUERY SELECT 'ineligible'::text,0::smallint,0; RETURN; END IF;
 SELECT c.* INTO credit FROM verification_private.credits c WHERE c.identity_key=profile.identity_key FOR UPDATE;
 now_at:=clock_timestamp();
 IF NOT FOUND OR credit.remaining<=0 OR credit.expires_at<=now_at THEN
  RETURN QUERY SELECT 'ineligible'::text,0::smallint,0; RETURN; END IF;
 -- A NULL candidate requests read-only advice. It does not consume a credit,
 -- create a receipt, prune rows, or reserve future posting authority.
 IF p_receipt IS NULL THEN RETURN QUERY SELECT 'ready'::text,credit.remaining,greatest(1,least(120,ceil(extract(epoch FROM credit.expires_at-now_at))::integer)); RETURN; END IF;
 DELETE FROM verification_private.receipts r WHERE r.token_hash IN(
  SELECT old.token_hash FROM verification_private.receipts old WHERE old.expires_at<=now_at
  ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
 IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.receipts LIMIT v_limit) bounded)>=v_limit THEN
  RETURN QUERY SELECT 'unavailable'::text,0::smallint,0; RETURN; END IF;
 INSERT INTO verification_private.receipts(token_hash,peer_hash,session_hash,purpose,board,target,context_hash,
  from_credit,suspicious,unsolved_before,may_grant_credit,created_at,expires_at,credit_identity)
 VALUES(p_receipt,p_peer,p_session,p_purpose,p_board,p_target,p_context,true,false,0,false,
  now_at,least(now_at+interval '30 seconds',credit.expires_at),profile.identity_key);
 -- Multiple outstanding claims may name the same last credit. Only a protected
 -- write can spend it, and failed writes restore both claim and credit.
 RETURN QUERY SELECT 'ready'::text,credit.remaining,greatest(1,least(120,ceil(extract(epoch FROM credit.expires_at-now_at))::integer));
END $$;
REVOKE ALL ON FUNCTION content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_credit_receipt(bytea,bytea,bytea,bytea,bytea,bytea,text,bigint,text,bytea,bigint) TO board_verifier;

RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Credit consumption did not preserve transfer authority'; END IF;
END $$;
RESET ROLE;
