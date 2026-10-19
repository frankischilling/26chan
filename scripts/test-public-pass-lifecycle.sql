-- Owned synthetic hashes qualify SQL authority, not password hashing.
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 SELECT sha256(convert_to('owned-pass-'||n,'UTF8')),repeat('owned-phc-placeholder',3),n,
 statement_timestamp()-interval '2 years',CASE WHEN n=7 THEN NULL ELSE statement_timestamp()+interval '30 days' END
 FROM generate_series(0,7) n;
RESET ROLE;
DO $$ BEGIN
 IF has_schema_privilege('board_verifier','verification_private','USAGE')
 OR has_table_privilege('board_public','verification_private.pass_entitlements','SELECT,INSERT,UPDATE,DELETE')
 OR has_table_privilege('board_verifier','verification_private.pass_sessions','SELECT,INSERT,UPDATE,DELETE')
 OR has_function_privilege('board_public','content.pass_begin_login(bytea,bytea,bytea)','EXECUTE')
 OR has_function_privilege('board_staff','content.pass_session_state(bytea)','EXECUTE') THEN
 RAISE EXCEPTION 'Private membership authority leaked'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ DECLARE started record; result record; n integer; ticket bytea; session bytea; peer bytea:=sha256('owned-peer'); BEGIN
 FOR n IN 0..7 LOOP
  ticket:=sha256(convert_to('owned-ticket-'||n,'UTF8'));session:=sha256(convert_to('owned-session-'||n,'UTF8'));
  SELECT * INTO started FROM content.pass_begin_login(sha256(convert_to('owned-pass-'||n,'UTF8')),peer,ticket);
  IF started.status<>'verify' OR started.pin_hash IS NULL THEN RAISE EXCEPTION 'Owned login lookup failed'; END IF;
  SELECT * INTO result FROM content.pass_finish_login(ticket,peer,sha256('owned-scope16'),sha256('owned-scope24'),true,session,n=7);
  IF n BETWEEN 1 AND 5 THEN
   IF result.status<>'disabled_'||n THEN RAISE EXCEPTION 'Disabled status differs: %',result; END IF;
  ELSE
   IF result.status<>'ready' OR result.ttl_seconds<>(CASE WHEN n=7 THEN 31556900 ELSE 86400 END) THEN
    RAISE EXCEPTION 'Activation or session lifetime differs: %',result; END IF;
   IF (SELECT status FROM content.pass_session_state(session))<>'ready' THEN RAISE EXCEPTION 'Issued session unavailable'; END IF;
  END IF;
  IF (SELECT status FROM content.pass_finish_login(ticket,peer,sha256('owned-scope16'),sha256('owned-scope24'),true,session,false))<>'rejected' THEN
   RAISE EXCEPTION 'Login ticket replay accepted'; END IF;
 END LOOP;
 FOR n IN 1..5 LOOP
  ticket:=sha256(convert_to('owned-failed-ticket-'||n,'UTF8'));
  SELECT * INTO started FROM content.pass_begin_login(sha256('owned-missing'),sha256('owned-flood-peer'),ticket);
  IF started.status<>'verify' OR started.pin_hash IS NOT NULL THEN RAISE EXCEPTION 'Unknown lookup disclosed a hash'; END IF;
  IF (SELECT status FROM content.pass_finish_login(ticket,sha256('owned-flood-peer'),NULL,NULL,false,sha256('unused-session'),false))<>'rejected' THEN
   RAISE EXCEPTION 'Unknown credential succeeded'; END IF;
 END LOOP;
 IF (SELECT status FROM content.pass_begin_login(sha256('owned-pass-0'),sha256('owned-flood-peer'),sha256('owned-sixth-ticket')))<>'flood' THEN
  RAISE EXCEPTION 'Login failure budget exceeded'; END IF;
 PERFORM content.pass_signout(sha256('owned-session-0'));
 PERFORM content.pass_signout(sha256('owned-session-0'));
 IF (SELECT status FROM content.pass_session_state(sha256('owned-session-0')))<>'none' THEN RAISE EXCEPTION 'Signed-out session remained active'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ DECLARE started record; result record; BEGIN
 SELECT * INTO started FROM content.pass_begin_login(sha256('owned-pass-0'),sha256('owned-other-peer'),sha256('owned-network-ticket'));
 SELECT * INTO result FROM content.pass_finish_login(sha256('owned-network-ticket'),sha256('owned-other-peer'),sha256('other-scope16'),sha256('other-scope24'),true,sha256('owned-held-session'),false);
 IF result.status<>'in_use' OR result.remaining NOT BETWEEN 895 AND 900 THEN RAISE EXCEPTION 'Login network hold differs: %',result; END IF;
 SELECT * INTO started FROM content.pass_begin_login(sha256('owned-pass-0'),sha256('owned-other-peer'),sha256('owned-same-network-ticket'));
 SELECT * INTO result FROM content.pass_finish_login(sha256('owned-same-network-ticket'),sha256('owned-other-peer'),sha256('owned-scope16'),sha256('other-scope24'),true,sha256('owned-same-network-session'),false);
 IF result.status<>'ready' THEN RAISE EXCEPTION 'Same login scope was refused'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ DECLARE n integer; result record; BEGIN
 FOR n IN 1..7 LOOP
  PERFORM content.pass_begin_login(sha256('owned-pass-0'),sha256('owned-other-peer'),sha256(convert_to('owned-cap-ticket-'||n,'UTF8')));
  SELECT * INTO result FROM content.pass_finish_login(sha256(convert_to('owned-cap-ticket-'||n,'UTF8')),sha256('owned-other-peer'),sha256('owned-scope16'),sha256('other-scope24'),true,sha256(convert_to('owned-cap-session-'||n,'UTF8')),false);
  IF result.status<>'ready' THEN RAISE EXCEPTION 'Owned session inventory could not be filled'; END IF;
 END LOOP;
END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET pin_hash=pin_hash||'-rotated' WHERE token_hash=sha256('owned-pass-0');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 PERFORM content.pass_begin_login(sha256('owned-pass-0'),sha256('owned-other-peer'),sha256('owned-after-reset-ticket'));
 IF (SELECT status FROM content.pass_finish_login(sha256('owned-after-reset-ticket'),sha256('owned-other-peer'),sha256('owned-scope16'),sha256('other-scope24'),true,sha256('owned-after-reset-session'),false))<>'ready' THEN
 RAISE EXCEPTION 'Rotated credentials were stranded by stale sessions'; END IF;
END $$;
RESET ROLE;
SAVEPOINT drift;
SET LOCAL ROLE board_anonymous_owner;
GRANT SELECT ON verification_private.pass_entitlements TO board_public;
DO $$ BEGIN
 IF content.public_pass_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Credential read grant drift was accepted'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_session_state(sha256('owned-session-7'));
  RAISE EXCEPTION 'Drifted issuer continued'; EXCEPTION WHEN SQLSTATE '55000' THEN NULL; END;
END $$;
ROLLBACK TO drift;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT status FROM verification_private.pass_entitlements WHERE token_hash=sha256('owned-pass-6'))<>0
 OR (SELECT status FROM verification_private.pass_entitlements WHERE token_hash=sha256('owned-pass-7'))<>0
 OR (SELECT expires_at FROM verification_private.pass_entitlements WHERE token_hash=sha256('owned-pass-7'))<statement_timestamp()+interval '364 days' THEN
 RAISE EXCEPTION 'Activation state was not saved'; END IF;
END $$;
UPDATE verification_private.pass_entitlements SET status=4 WHERE token_hash=sha256('owned-pass-6');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_session_state(sha256('owned-session-6')))<>'none' THEN RAISE EXCEPTION 'Revoked credential version retained access'; END IF;
END $$;
ROLLBACK;
