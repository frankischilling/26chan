-- A solved ticket challenge authorizes one waiting-ticket issuance. Its receipt
-- never authorizes a post, report, email action, or early end to the wait.
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog has drifted before ticket challenge completion'; END IF;
END $$;
RESET ROLE;
GRANT CREATE ON SCHEMA content,verification_private TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE OR REPLACE FUNCTION content.verification_ticket_advice(p_ticket bytea,p_peer bytea,p_board text)
RETURNS TABLE(valid boolean,issued_at bigint,current_at bigint,escalation_required boolean)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_precise timestamptz; v_ticket verification_private.tickets%ROWTYPE; v_valid boolean;
BEGIN
 IF (p_ticket IS NOT NULL AND octet_length(p_ticket)<>32)
    OR p_peer IS NULL OR octet_length(p_peer)<>32 OR p_board IS NULL THEN
  RAISE EXCEPTION 'Invalid challenge ticket context.' USING ERRCODE='23514';
 END IF;
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 v_precise:=clock_timestamp(); v_now:=date_trunc('second',v_precise);
 SELECT t.* INTO v_ticket FROM verification_private.tickets t
 WHERE t.token_hash=p_ticket AND t.peer_hash=p_peer AND t.board=p_board;
 v_valid:=FOUND AND v_ticket.created_at<=v_now AND v_ticket.expires_at>v_now;
 RETURN QUERY SELECT v_valid,
  CASE WHEN v_valid THEN extract(epoch FROM v_ticket.created_at)::bigint ELSE 0::bigint END,
  extract(epoch FROM v_now)::bigint,
  NOT EXISTS(SELECT 1 FROM verification_private.peer_risk r
   WHERE r.peer_hash=p_peer AND r.expires_at>v_precise AND r.score>=99);
END $$;
REVOKE ALL ON FUNCTION content.verification_ticket_advice(bytea,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_ticket_advice(bytea,bytea,text) TO board_verifier;

CREATE FUNCTION verification_private.prepare_ticket(
 p_presented bytea,p_candidate bytea,p_peer bytea,p_board text,p_escalated boolean
) RETURNS TABLE(available boolean,issued boolean,issued_at bigint,current_at bigint)
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_now timestamptz; v_ticket verification_private.tickets%ROWTYPE; v_limit integer;
BEGIN
 IF p_escalated IS NULL OR (p_presented IS NOT NULL AND octet_length(p_presented)<>32)
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
 -- Only the private completion wrapper supplies true after consuming a scoped
 -- solved receipt. Otherwise recheck the trusted score under its policy lock.
 IF NOT p_escalated AND NOT EXISTS(SELECT 1 FROM verification_private.peer_risk r
    WHERE r.peer_hash=p_peer AND r.score>=99 AND r.expires_at>clock_timestamp()) THEN
  RAISE EXCEPTION 'Ticket challenge is required.' USING ERRCODE='P0166'; END IF;
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
REVOKE ALL ON FUNCTION verification_private.prepare_ticket(bytea,bytea,bytea,text,boolean) FROM PUBLIC;
CREATE OR REPLACE FUNCTION content.verification_prepare_ticket(
 p_presented bytea,p_candidate bytea,p_peer bytea,p_board text
) RETURNS TABLE(available boolean,issued boolean,issued_at bigint,current_at bigint)
LANGUAGE sql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
 SELECT * FROM verification_private.prepare_ticket(p_presented,p_candidate,p_peer,p_board,false)
$$;
REVOKE ALL ON FUNCTION content.verification_prepare_ticket(bytea,bytea,bytea,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_prepare_ticket(bytea,bytea,bytea,text) TO board_verifier;

CREATE FUNCTION content.verification_finish_ticket(
 p_presented bytea,p_candidate bytea,p_peer bytea,p_session bytea,p_board text,
 p_target bigint,p_context bytea,p_receipt bytea
) RETURNS TABLE(available boolean,issued boolean,issued_at bigint,current_at bigint)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE r verification_private.receipts%ROWTYPE; session_expiry bigint; now_at timestamptz;
BEGIN
 IF p_candidate IS NULL OR octet_length(p_candidate)<>32
    OR (p_presented IS NOT NULL AND octet_length(p_presented)<>32)
    OR p_peer IS NULL OR octet_length(p_peer)<>32
    OR p_session IS NULL OR octet_length(p_session)<>32
    OR p_context IS NULL OR octet_length(p_context)<>32
    OR p_receipt IS NULL OR octet_length(p_receipt)<>32
    OR p_board IS NULL OR p_target IS NULL OR p_target<0 THEN
  RAISE EXCEPTION 'Invalid ticket challenge context.' USING ERRCODE='23514'; END IF;
 PERFORM b.slug FROM content.boards b WHERE b.slug=p_board AND NOT b.staff_only FOR KEY SHARE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Public board not found.' USING ERRCODE='P0002'; END IF;
 SELECT s.expires_at INTO session_expiry FROM post_secrets.anonymous_sessions s WHERE s.token_hash=p_session FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Ticket challenge is required.' USING ERRCODE='P0166'; END IF;
 PERFORM singleton FROM verification_private.policy WHERE singleton FOR UPDATE;
 IF NOT FOUND THEN RAISE EXCEPTION 'Verification policy is unavailable.' USING ERRCODE='55000'; END IF;
 SELECT a.* INTO r FROM verification_private.receipts a WHERE a.token_hash=p_receipt FOR UPDATE;
 IF NOT FOUND OR r.purpose<>'ticket' OR r.board IS DISTINCT FROM p_board OR r.target<>p_target
    OR r.peer_hash<>p_peer OR r.session_hash<>p_session OR r.context_hash<>p_context OR r.from_credit THEN
  RAISE EXCEPTION 'Ticket challenge is required.' USING ERRCODE='P0166'; END IF;
 now_at:=clock_timestamp();
 IF r.expires_at<=now_at OR session_expiry<=extract(epoch FROM now_at)::bigint THEN
  RAISE EXCEPTION 'Ticket challenge is required.' USING ERRCODE='P0166'; END IF;
 DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;
 -- A capacity response commits bounded pruning, just as direct ticket issuance
 -- does. It never returns an unstored token or a successful challenge response.
 RETURN QUERY SELECT * FROM verification_private.prepare_ticket(p_presented,p_candidate,p_peer,p_board,true);
END $$;
REVOKE ALL ON FUNCTION content.verification_finish_ticket(bytea,bytea,bytea,bytea,text,bigint,bytea,bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.verification_finish_ticket(bytea,bytea,bytea,bytea,text,bigint,bytea,bytea) TO board_verifier;
RESET ROLE;
REVOKE CREATE ON SCHEMA content,verification_private FROM board_anonymous_owner;
SET ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Ticket completion did not preserve transfer authority'; END IF;
END $$;
RESET ROLE;
