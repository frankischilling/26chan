\set ON_ERROR_STOP on
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('qcticket1','Ticket test','',2000,100,100,100,10),('qcticket2','Ticket test','',2000,100,100,100,10);
DO $tests$
DECLARE first_ticket record; reused record; replacement record; policy record;
 a bytea:=decode(repeat('a1',32),'hex'); b bytea:=decode(repeat('b2',32),'hex');
 peer bytea:=decode(repeat('c3',32),'hex');
BEGIN
 EXECUTE 'SET LOCAL ROLE board_verifier';
 SELECT * INTO first_ticket FROM content.verification_prepare_ticket(NULL,a,peer,'qcticket1');
 IF NOT first_ticket.issued OR first_ticket.issued_at<>first_ticket.current_at THEN
  RAISE EXCEPTION 'New ticket did not start its wait at database time'; END IF;
 SELECT * INTO reused FROM content.verification_prepare_ticket(a,b,peer,'qcticket1');
 IF reused.issued OR reused.issued_at<>first_ticket.issued_at THEN
  RAISE EXCEPTION 'Reload reset a valid ticket'; END IF;
 SELECT * INTO replacement FROM content.verification_prepare_ticket(a,b,peer,'qcticket2');
 IF NOT replacement.issued THEN RAISE EXCEPTION 'Ticket crossed boards'; END IF;
 SELECT * INTO replacement FROM content.verification_prepare_ticket(a,decode(repeat('d4',32),'hex'),b,'qcticket1');
 IF NOT replacement.issued THEN RAISE EXCEPTION 'Ticket crossed peers'; END IF;
 SELECT * INTO policy FROM content.verification_board_policy('f');
 IF policy.enabled IS DISTINCT FROM true OR policy.twister IS DISTINCT FROM false THEN
  RAISE EXCEPTION 'Classic provider was mistaken for disabled CAPTCHA'; END IF;
 SELECT * INTO policy FROM content.verification_board_policy('hr');
 IF policy.preupload IS DISTINCT FROM true THEN RAISE EXCEPTION 'Preupload switch lost'; END IF;
 IF EXISTS(SELECT 1 FROM content.verification_board_policy('j')) THEN
  RAISE EXCEPTION 'Staff-only policy was exposed'; END IF;
 SELECT * INTO policy FROM content.verification_board_policy('pol');
 IF policy.allow_bypass IS DISTINCT FROM false THEN RAISE EXCEPTION 'Bypass restriction lost'; END IF;
 IF EXISTS(SELECT 1 FROM content.verification_board_policy('qcmissing')) THEN
  RAISE EXCEPTION 'Unknown board received a policy'; END IF;
 EXECUTE 'RESET ROLE';
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE verification_private.tickets SET created_at=date_trunc('second',clock_timestamp())-interval '1 hour',
  expires_at=date_trunc('second',clock_timestamp()) WHERE token_hash=a;
 EXECUTE 'SET LOCAL ROLE board_verifier';
 SELECT * INTO replacement FROM content.verification_prepare_ticket(a,decode(repeat('e5',32),'hex'),peer,'qcticket1');
 IF NOT replacement.issued THEN RAISE EXCEPTION 'Expired ticket was reused'; END IF;
 EXECUTE 'RESET ROLE';
 EXECUTE 'SET LOCAL ROLE board_migrator';
 UPDATE verification_private.policy SET challenge_limit=(SELECT count(*) FROM verification_private.tickets);
 EXECUTE 'SET LOCAL ROLE board_verifier';
 SELECT * INTO replacement FROM content.verification_prepare_ticket(NULL,decode(repeat('f6',32),'hex'),peer,'qcticket1');
 IF replacement.available THEN RAISE EXCEPTION 'Ticket capacity was ignored'; END IF;
 -- A full store still permits reuse of an unexpired authenticated ticket.
 SELECT * INTO reused FROM content.verification_prepare_ticket(b,a,peer,'qcticket2');
 IF NOT reused.available OR reused.issued THEN RAISE EXCEPTION 'Full capacity discarded a valid ticket'; END IF;
 BEGIN
  PERFORM * FROM content.verification_prepare_ticket(NULL,a,peer,'qcmissing');
  RAISE EXCEPTION 'Missing board unexpectedly admitted';
 EXCEPTION WHEN SQLSTATE 'P0002' THEN NULL; END;
 BEGIN
  PERFORM * FROM content.verification_prepare_ticket(NULL,decode('ab','hex'),peer,'qcticket1');
  RAISE EXCEPTION 'Short ticket hash was admitted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 EXECUTE 'RESET ROLE';
 EXECUTE 'SET LOCAL ROLE board_public';
 BEGIN
  PERFORM * FROM content.verification_prepare_ticket(NULL,a,peer,'qcticket1');
  RAISE EXCEPTION 'Public role minted a ticket';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM token_hash FROM verification_private.tickets;
  RAISE EXCEPTION 'Public role read ticket authority';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN
  PERFORM * FROM content.verification_board_policy('g');
  RAISE EXCEPTION 'Public role called issuer policy API';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $tests$;
RESET ROLE;
SET LOCAL ROLE board_migrator;
-- More than one cleanup batch must make progress after a lowered capacity.
UPDATE verification_private.policy SET challenge_limit=(SELECT count(*)+1 FROM verification_private.tickets);
INSERT INTO verification_private.tickets(token_hash,peer_hash,board,created_at,expires_at)
SELECT sha256(convert_to('owned-expired-ticket-'||n,'UTF8')),decode(repeat('c3',32),'hex'),'qcticket1',
 statement_timestamp()-interval '2 hours',statement_timestamp()-interval '1 hour'
FROM generate_series(1,130) n;
DO $cleanup_progress$
DECLARE result record; remaining integer; expected integer; attempt integer;
BEGIN
 FOR attempt IN 1..3 LOOP
  EXECUTE 'SET LOCAL ROLE board_verifier';
  SELECT * INTO result FROM content.verification_prepare_ticket(NULL,
   decode(repeat('f6',32),'hex'),decode(repeat('c3',32),'hex'),'qcticket1');
  IF result.available IS DISTINCT FROM (attempt=3) OR result.issued IS DISTINCT FROM (attempt=3) THEN
   RAISE EXCEPTION 'Bounded ticket cleanup returned the wrong capacity state'; END IF;
  EXECUTE 'RESET ROLE';
  EXECUTE 'SET LOCAL ROLE board_migrator';
  SELECT count(*) INTO remaining FROM verification_private.tickets WHERE expires_at<=clock_timestamp();
  expected:=greatest(130-attempt*64,0);
  IF remaining<>expected THEN RAISE EXCEPTION 'Expired ticket cleanup lost progress'; END IF;
 END LOOP;
END $cleanup_progress$;
RESET ROLE;
ROLLBACK;
