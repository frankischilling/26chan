-- Rejected/quiet posts never reach the accepted-post proof issuer. Revalidate
-- their staff authority in the same transaction that commits filter effects.
SET LOCAL ROLE board_thread_transfer_owner;
DO $$ BEGIN
 PERFORM singleton FROM post_secrets.thread_transfer_catalog WHERE singleton FOR UPDATE;
 IF NOT FOUND OR content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer catalog drifted before staff refusal authority'; END IF;
END $$;
RESET ROLE;
GRANT UPDATE(id) ON staff_identity.credentials TO board_staff_post_owner;
GRANT CREATE ON SCHEMA content TO board_staff_post_owner;
SET LOCAL ROLE board_staff_post_owner;
CREATE FUNCTION content.revalidate_staff_refusal(p_session bytea,p_csrf bytea,p_idle integer,p_board text)
RETURNS void LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_account bigint; v_credential bytea;
BEGIN
 IF session_user<>'board_staff' OR current_setting('transaction_isolation')<>'read committed'
    OR p_session IS NULL OR octet_length(p_session)<>32 OR p_csrf IS NULL OR octet_length(p_csrf)<>32
    OR p_idle IS NULL OR p_idle NOT BETWEEN 60 AND 3600
    OR p_board IS NULL OR p_board !~ '^[a-z0-9]{1,10}$'
    OR NOT EXISTS(SELECT 1 FROM content.boards WHERE slug=p_board AND NOT staff_only) THEN
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
REVOKE ALL ON FUNCTION content.revalidate_staff_refusal(bytea,bytea,integer,text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.revalidate_staff_refusal(bytea,bytea,integer,text) TO board_staff;
RESET ROLE;
REVOKE CREATE ON SCHEMA content FROM board_staff_post_owner;
SET LOCAL ROLE board_thread_transfer_owner;
UPDATE post_secrets.thread_transfer_catalog SET definition_hash=post_secrets.transfer_catalog_hash() WHERE singleton;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Staff refusal authority changed transfer readiness'; END IF; END $$;
RESET ROLE;
