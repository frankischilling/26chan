-- Owned synthetic hashes qualify SQL authority, not password hashing.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('qpassr1','Owned receipt test','',2000,100,100,100,10);
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at)
SELECT sha256('owned-pass-anon'),sha256('owned-pass-network'),sha256('owned-pass-address'),sha256('owned-pass-environment'),n,n,n,n,n+3600
FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock;
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
 OR has_function_privilege('board_staff','content.pass_session_state(bytea)','EXECUTE')
 OR has_function_privilege('board_public','content.pass_posting_state(bytea,bytea)','EXECUTE') THEN
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
SAVEPOINT posting_advice;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_posting_state(sha256('owned-session-7'),sha256('owned-scope24')))<>'ready'
 OR (SELECT status FROM content.pass_posting_state(sha256('owned-session-7'),sha256('different-posting-scope')))<>'in_use'
 OR (SELECT status FROM content.pass_posting_state(sha256('owned-session-7'),NULL))<>'in_use'
 OR (SELECT status FROM content.pass_posting_state(sha256('unknown-session'),NULL))<>'none' THEN
  RAISE EXCEPTION 'Posting advice did not preserve the narrower source network hold'; END IF;
 BEGIN
  PERFORM content.pass_posting_state(sha256('owned-session-7'),decode('00','hex'));
  RAISE EXCEPTION 'Malformed posting scope accepted'; EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT posting_scope FROM verification_private.pass_entitlements WHERE token_hash=sha256('owned-pass-7'))<>sha256('owned-scope24') THEN
  RAISE EXCEPTION 'Advisory read changed the membership network'; END IF;
END $$;
UPDATE verification_private.pass_entitlements SET last_used=date_trunc('second',clock_timestamp())-interval '1800 seconds'
 WHERE token_hash=sha256('owned-pass-7');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_posting_state(sha256('owned-session-7'),sha256('different-posting-scope')))<>'ready' THEN
  RAISE EXCEPTION 'Posting network hold did not release at the source boundary'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET last_used=clock_timestamp()+interval '1 hour',posting_scope=NULL
 WHERE token_hash=sha256('owned-pass-7');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_posting_state(sha256('owned-session-7'),sha256('different-posting-scope')))<>'ready' THEN
  RAISE EXCEPTION 'Unset previous posting scope imposed a hold'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET expires_at=date_trunc('second',clock_timestamp()),posting_scope=sha256('owned-scope24')
 WHERE token_hash=sha256('owned-pass-7');
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_posting_state(sha256('owned-session-7'),sha256('different-posting-scope')))<>'expired' THEN
  RAISE EXCEPTION 'Expiry did not precede posting network hold'; END IF;
END $$;
ROLLBACK TO posting_advice;
RESET ROLE;
SAVEPOINT receipt_tests;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_write_receipt(sha256('owned-session-7'),sha256('owned-pass-receipt'),sha256('owned-peer'),
 sha256('owned-scope16'),sha256('owned-scope24'),sha256('owned-pass-anon'),sha256('owned-form'),'thread','qpassr1',0))<>'ready' THEN
 RAISE EXCEPTION 'Valid membership did not issue a receipt'; END IF;
 BEGIN
 PERFORM verification_private.pass_take_receipt(sha256('owned-pass-receipt'),sha256('owned-peer'),sha256('owned-pass-anon'),sha256('owned-form'),'thread','qpassr1',0);
 RAISE EXCEPTION 'Verifier directly consumed a private receipt'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ DECLARE mismatch integer; result record; BEGIN
 FOR mismatch IN 1..5 LOOP
  BEGIN
   PERFORM verification_private.pass_take_receipt(sha256('owned-pass-receipt'),
    CASE WHEN mismatch=1 THEN sha256('wrong-peer') ELSE sha256('owned-peer') END,
    sha256('owned-pass-anon'),CASE WHEN mismatch=2 THEN sha256('wrong-form') ELSE sha256('owned-form') END,
    CASE WHEN mismatch=3 THEN 'report' ELSE 'thread' END,'qpassr1',CASE WHEN mismatch=4 THEN 1 ELSE 0 END);
   IF mismatch<5 THEN RAISE EXCEPTION 'Mismatched membership receipt accepted'; END IF;
   -- A failed enclosing write restores receipt consumption and account activity.
   RAISE EXCEPTION 'Owned rollback control' USING ERRCODE='P0217';
  EXCEPTION WHEN SQLSTATE 'P0166' THEN
   IF mismatch=5 THEN RAISE; END IF;
  WHEN SQLSTATE 'P0217' THEN
   IF mismatch<>5 THEN RAISE; END IF;
  END;
 END LOOP;
 IF NOT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('owned-pass-receipt')) THEN
 RAISE EXCEPTION 'Failed write consumed membership receipt'; END IF;
 SELECT * INTO result FROM verification_private.pass_take_receipt(sha256('owned-pass-receipt'),sha256('owned-peer'),sha256('owned-pass-anon'),sha256('owned-form'),'thread','qpassr1',0);
 IF result.entitlement_id IS NULL OR result.purchase_year IS NULL THEN RAISE EXCEPTION 'Receipt lost private membership identity'; END IF;
 BEGIN
 PERFORM verification_private.pass_take_receipt(sha256('owned-pass-receipt'),sha256('owned-peer'),sha256('owned-pass-anon'),sha256('owned-form'),'thread','qpassr1',0);
 RAISE EXCEPTION 'Membership receipt replay accepted'; EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ DECLARE n integer; result record; BEGIN
 FOR n IN 1..9 LOOP
  SELECT * INTO result FROM content.pass_write_receipt(sha256('owned-session-7'),sha256(convert_to('owned-receipt-cap-'||n,'UTF8')),
   sha256('owned-peer'),sha256('owned-scope16'),sha256('owned-scope24'),sha256('owned-pass-anon'),sha256('owned-form'),'thread','qpassr1',0);
  IF result.status<>(CASE WHEN n=9 THEN 'capacity' ELSE 'ready' END) THEN RAISE EXCEPTION 'Membership receipt inventory bound differs'; END IF;
 END LOOP;
END $$;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_receipts SET created_at=clock_timestamp()-interval '60 seconds',expires_at=clock_timestamp()-interval '30 seconds'
 WHERE token_hash=sha256('owned-receipt-cap-1');
DO $$ BEGIN
 BEGIN
  PERFORM verification_private.pass_take_receipt(sha256('owned-receipt-cap-1'),sha256('owned-peer'),sha256('owned-pass-anon'),sha256('owned-form'),'thread','qpassr1',0);
  RAISE EXCEPTION 'Expired membership receipt accepted'; EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
UPDATE verification_private.pass_entitlements SET pin_hash=pin_hash||'-receipt-reset' WHERE token_hash=sha256('owned-pass-7');
DO $$ BEGIN
 BEGIN
  PERFORM verification_private.pass_take_receipt(sha256('owned-receipt-cap-2'),sha256('owned-peer'),sha256('owned-pass-anon'),sha256('owned-form'),'thread','qpassr1',0);
  RAISE EXCEPTION 'Credential rotation left receipt authorized'; EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SET LOCAL ROLE board_verifier;
SELECT content.pass_signout(sha256('owned-session-7'));

SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE pass_session=sha256('owned-session-7')) THEN
 RAISE EXCEPTION 'Signout did not revoke outstanding receipts'; END IF;
END $$;
ROLLBACK TO receipt_tests;
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
