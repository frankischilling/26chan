-- Keep provider selection, enablement, credit entitlement, and upload timing
-- independent. The audited source exceptions are in captcha-board-policy.json.
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before challenge policy migration';
 END IF;
END $$;
RESET ROLE;
ALTER TABLE content.boards
 ADD COLUMN captcha_enabled boolean NOT NULL DEFAULT true,
 ADD COLUMN captcha_twister boolean NOT NULL DEFAULT true,
 ADD COLUMN captcha_allow_bypass boolean NOT NULL DEFAULT true,
 ADD COLUMN preupload_captcha boolean NOT NULL DEFAULT false,
 ADD COLUMN noscript_captcha_only boolean NOT NULL DEFAULT false;
UPDATE content.boards SET captcha_enabled=false WHERE slug='j';
UPDATE content.boards SET captcha_twister=false WHERE slug='f';
UPDATE content.boards SET captcha_allow_bypass=false WHERE slug IN('pol','trash','bant');
UPDATE content.boards SET preupload_captcha=true WHERE slug='hr';
GRANT SELECT(captcha_enabled,captcha_twister,captcha_allow_bypass,preupload_captcha,noscript_captcha_only)
 ON content.boards TO board_anonymous_owner;

GRANT CREATE ON SCHEMA content TO board_anonymous_owner;
SET ROLE board_anonymous_owner;

CREATE FUNCTION content.verification_board_policy(p_board text)
RETURNS TABLE(enabled boolean,twister boolean,allow_bypass boolean,preupload boolean,noscript_only boolean)
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT b.captcha_enabled,b.captcha_twister,b.captcha_allow_bypass,b.preupload_captcha,b.noscript_captcha_only
 FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only
$$;
REVOKE ALL ON FUNCTION content.verification_board_policy(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_board_policy(text) TO board_verifier;

-- Inspect an existing wait ticket before deciding whether a new ticket needs
-- an escalated challenge. Missing/expired trusted risk input requires that
-- challenge; only a current score of at least 99 skips it.
CREATE FUNCTION content.verification_ticket_advice(p_ticket bytea,p_peer bytea,p_board text)
RETURNS TABLE(valid boolean,issued_at bigint,current_at bigint,escalation_required boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_ticket verification_private.tickets%ROWTYPE; v_valid boolean;
BEGIN
 IF (p_ticket IS NOT NULL AND octet_length(p_ticket)<>32)
    OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_board IS NULL THEN
  RAISE EXCEPTION 'Invalid challenge ticket context.' USING ERRCODE='23514';
 END IF;
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 v_now:=date_trunc('second',clock_timestamp());
 SELECT t.* INTO v_ticket FROM verification_private.tickets t
 WHERE t.token_hash=p_ticket AND t.peer_hash=p_peer AND t.board=p_board;
 v_valid:=FOUND AND v_ticket.created_at<=v_now AND v_ticket.expires_at>v_now;
 RETURN QUERY SELECT v_valid,
  CASE WHEN v_valid THEN extract(epoch FROM v_ticket.created_at)::bigint ELSE 0::bigint END,
  extract(epoch FROM v_now)::bigint,
  NOT EXISTS(SELECT 1 FROM verification_private.peer_risk r
   WHERE r.peer_hash=p_peer AND r.expires_at>v_now AND r.score>=99);
END $$;
REVOKE ALL ON FUNCTION content.verification_ticket_advice(bytea,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_ticket_advice(bytea,bytea,text) TO board_verifier;

-- Tickets attest only elapsed waiting time, never a CAPTCHA solution or a
-- posting entitlement. The issuer supplies random candidates, not timestamps.
-- A valid ticket remains reusable on its board and peer until the one-hour
-- boundary, matching the source's refresh/reload behavior.
CREATE FUNCTION content.verification_prepare_ticket(
 p_presented bytea,p_candidate bytea,p_peer bytea,p_board text
) RETURNS TABLE(available boolean,issued boolean,issued_at bigint,current_at bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_ticket verification_private.tickets%ROWTYPE; v_limit integer;
BEGIN
 IF (p_presented IS NOT NULL AND octet_length(p_presented)<>32)
    OR p_candidate IS NULL OR octet_length(p_candidate)<>32
    OR p_peer IS NULL OR octet_length(p_peer)<>32
    OR p_board IS NULL THEN
  RAISE EXCEPTION 'Invalid challenge ticket context.' USING ERRCODE='23514';
 END IF;
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 v_now:=date_trunc('second',clock_timestamp());
 SELECT t.* INTO v_ticket FROM verification_private.tickets t
 WHERE t.token_hash=p_presented AND t.peer_hash=p_peer AND t.board=p_board;
 IF FOUND AND v_ticket.created_at<=v_now AND v_ticket.expires_at>v_now THEN
  RETURN QUERY SELECT true,false,extract(epoch FROM v_ticket.created_at)::bigint,extract(epoch FROM v_now)::bigint;
  RETURN;
 END IF;
 SELECT challenge_limit INTO v_limit FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
 v_now:=date_trunc('second',clock_timestamp());
 DELETE FROM verification_private.tickets t WHERE t.token_hash IN (
  SELECT old.token_hash FROM verification_private.tickets old WHERE old.expires_at<=v_now
  ORDER BY old.expires_at LIMIT 64 FOR UPDATE SKIP LOCKED);
 IF (SELECT count(*) FROM (SELECT 1 FROM verification_private.tickets LIMIT v_limit) bounded)>=v_limit THEN
  -- Return exhaustion so bounded cleanup commits even after a lowered cap.
  -- Raising here would roll it back and could prevent all future progress.
  RETURN QUERY SELECT false,false,0::bigint,extract(epoch FROM v_now)::bigint;
  RETURN;
 END IF;
 INSERT INTO verification_private.tickets(token_hash,peer_hash,board,created_at,expires_at)
 VALUES(p_candidate,p_peer,p_board,v_now,v_now+interval '1 hour');
 RETURN QUERY SELECT true,true,extract(epoch FROM v_now)::bigint,extract(epoch FROM v_now)::bigint;
END $$;
REVOKE ALL ON FUNCTION content.verification_prepare_ticket(bytea,bytea,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_prepare_ticket(bytea,bytea,bytea,text) TO board_verifier;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_anonymous_owner;
-- Board columns and their narrow issuer grants are covered by transfer's
-- catalog seal. Qualify the old seal before DDL, then record this reviewed DDL.
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog
 SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Challenge board policy did not preserve transfer authority';
 END IF;
END $$;
RESET ROLE;
