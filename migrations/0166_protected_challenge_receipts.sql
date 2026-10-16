-- Private transaction-bound consumer for ordinary post/report proofs. Runtime
-- logins cannot call it directly or read the issuer's receipt rows.
GRANT CREATE ON SCHEMA verification_private TO board_anonymous_owner;
SET ROLE board_anonymous_owner;
CREATE FUNCTION verification_private.take_write_receipt(
 p_receipt bytea,p_peer bytea,p_session bytea,p_purpose text,p_board text,p_target bigint,p_context bytea
) RETURNS boolean
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
 IF r.expires_at<=v_now OR v_expiry<=extract(epoch FROM v_now)::bigint OR r.from_credit THEN
  RAISE EXCEPTION 'Challenge verification is required.' USING ERRCODE='P0166';
 END IF;
 DELETE FROM verification_private.receipts WHERE token_hash=p_receipt;
 RETURN r.may_grant_credit AND NOT r.suspicious AND r.unsolved_before<2;
END $$;
REVOKE ALL ON FUNCTION verification_private.take_write_receipt(bytea,bytea,bytea,text,text,bigint,bytea) FROM PUBLIC;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private FROM board_anonymous_owner;
