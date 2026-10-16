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

-- Observe the resumed activity before recording this post. Source credit
-- grants precede updatePostActivity; spending does not require the grant's
-- extra sixty-second address-age condition.
CREATE FUNCTION verification_private.credit_profile(
 p_session bytea,p_network bytea,p_address bytea,p_environment bytea,p_request bigint
) RETURNS TABLE(identity_key bytea,can_spend boolean,credit_count smallint,ttl_seconds integer)
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE s post_secrets.anonymous_sessions%ROWTYPE; v_last bigint; v_network_age bigint;
 v_password_age bigint; v_posts integer; v_reports integer; v_known boolean;
 v_email bytea; tier record;
BEGIN
 IF p_session IS NULL OR octet_length(p_session)<>32
    OR p_network IS NULL OR octet_length(p_network)<>32
    OR p_address IS NULL OR octet_length(p_address)<>32
    OR p_environment IS NULL OR octet_length(p_environment)<>32
    OR p_request IS NULL OR p_request<=0
    OR abs(p_request::numeric-extract(epoch FROM clock_timestamp())::bigint)>30 THEN
  RAISE EXCEPTION 'Invalid challenge activity context.' USING ERRCODE='23514';
 END IF;
 SELECT a.* INTO s FROM post_secrets.anonymous_sessions a WHERE a.token_hash=p_session FOR UPDATE;
 IF NOT FOUND OR s.expires_at<=p_request OR s.expires_at<=extract(epoch FROM clock_timestamp())::bigint THEN
  RAISE EXCEPTION 'Challenge session is unavailable.' USING ERRCODE='P0166';
 END IF;
 v_last:=CASE WHEN s.activity_at>0 THEN s.activity_at ELSE s.created_at END;
 IF greatest(0,p_request-v_last)>=604800 THEN
  s.created_at:=p_request; s.network_at:=p_request; s.address_at:=p_request; s.environment_at:=p_request;
  s.posts:=0; s.images:=0; s.threads:=0; s.reports:=0; s.pending:=0; s.change_score:=0; s.verified_level:=0;
 ELSE
  IF s.environment_hash<>p_environment THEN s.environment_at:=p_request; END IF;
  IF s.network_hash<>p_network THEN s.network_at:=p_request; s.address_at:=p_request;
  ELSIF s.address_hash<>p_address THEN s.address_at:=p_request; END IF;
 END IF;
 v_network_age:=greatest(0,p_request-s.network_at);
 v_password_age:=greatest(0,p_request-s.created_at);
 v_posts:=s.posts+CASE WHEN (s.pending & 1)<>0 THEN 1 ELSE 0 END;
 v_reports:=s.reports+CASE WHEN (s.pending & 8)<>0 THEN 1 ELSE 0 END;
 SELECT i.email_hash INTO v_email FROM verification_private.browser_identities b
 JOIN verification_private.identities i ON i.id=b.identity_id WHERE b.session_hash=p_session;
 identity_key:=CASE WHEN v_email IS NULL
  THEN sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||p_session)
  ELSE sha256(convert_to('26chan:captcha-credit:email:v1','UTF8')||v_email) END;
 can_spend:=false; credit_count:=0; ttl_seconds:=0;
 FOR tier IN SELECT * FROM (VALUES
   (7776000::bigint,100,3::smallint,10800),
   (1296000::bigint,20,2::smallint,7200),
   (259200::bigint,5,1::smallint,3600)
  ) AS tiers(age,posts,credits,seconds) ORDER BY age DESC LOOP
  v_known:=NOT(s.change_score>9 AND v_network_age<1800) AND
   (v_network_age>=tier.age OR (v_password_age>=tier.age
     AND (v_posts>=3 OR v_reports>=10)
     AND (v_network_age>=1800 OR v_posts>=9 OR v_reports>=20)));
  IF v_known AND v_posts>=tier.posts THEN
   can_spend:=true;
   IF greatest(0,p_request-s.address_at)>=60 THEN
    credit_count:=tier.credits; ttl_seconds:=tier.seconds;
   END IF;
   EXIT;
  END IF;
 END LOOP;
 RETURN NEXT;
END $$;
REVOKE ALL ON FUNCTION verification_private.credit_profile(bytea,bytea,bytea,bytea,bigint) FROM PUBLIC;
RESET ROLE;
REVOKE CREATE ON SCHEMA verification_private FROM board_anonymous_owner;
