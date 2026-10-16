\set ON_ERROR_STOP on
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
VALUES('qreceipt1','Receipt test','',2000,100,100,100,10);
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,
 created_at,network_at,address_at,environment_at,expires_at)
SELECT decode(repeat('4a',32),'hex'),decode(repeat('4b',32),'hex'),decode(repeat('4c',32),'hex'),decode(repeat('4d',32),'hex'),
 n,n,n,n,n+3600 FROM (SELECT extract(epoch FROM statement_timestamp())::bigint n) t;
INSERT INTO verification_private.receipts(token_hash,peer_hash,session_hash,purpose,board,target,context_hash,
 from_credit,suspicious,unsolved_before,may_grant_credit,created_at,expires_at)
VALUES(decode(repeat('5a',32),'hex'),decode(repeat('5b',32),'hex'),decode(repeat('4a',32),'hex'),
 'thread','qreceipt1',0,decode(repeat('5c',32),'hex'),false,false,1,true,statement_timestamp(),statement_timestamp()+interval '30 seconds'),
 (decode(repeat('6a',32),'hex'),decode(repeat('5b',32),'hex'),decode(repeat('4a',32),'hex'),
 'thread','qreceipt1',0,decode(repeat('5c',32),'hex'),false,false,1,true,statement_timestamp()-interval '60 seconds',statement_timestamp()-interval '30 seconds'),
 (decode(repeat('6b',32),'hex'),decode(repeat('5b',32),'hex'),decode(repeat('4a',32),'hex'),
 'reply','qreceipt1',1,decode(repeat('5c',32),'hex'),true,false,1,false,statement_timestamp(),statement_timestamp()+interval '30 seconds'),
 (decode(repeat('6c',32),'hex'),decode(repeat('5b',32),'hex'),decode(repeat('4a',32),'hex'),
 'thread','qreceipt1',0,decode(repeat('5c',32),'hex'),false,false,1,true,statement_timestamp(),statement_timestamp()+interval '30 seconds');
DO $tests$
DECLARE receipt bytea:=decode(repeat('5a',32),'hex'); peer bytea:=decode(repeat('5b',32),'hex');
 session_hash bytea:=decode(repeat('4a',32),'hex'); context_hash bytea:=decode(repeat('5c',32),'hex');
 role_name text; grant_credit boolean; mismatch integer;
BEGIN
 FOREACH role_name IN ARRAY ARRAY['board_public','board_verifier','board_mailer'] LOOP
  EXECUTE format('SET LOCAL ROLE %I',role_name);
  BEGIN
   PERFORM verification_private.take_write_receipt(receipt,peer,session_hash,'thread','qreceipt1',0,context_hash);
   RAISE EXCEPTION 'Runtime login consumed a private write receipt directly';
  EXCEPTION WHEN insufficient_privilege THEN NULL; END;
  EXECUTE 'RESET ROLE';
 END LOOP;
 EXECUTE 'SET LOCAL ROLE board_anonymous_owner';
 FOR mismatch IN 1..5 LOOP
  BEGIN
   PERFORM verification_private.take_write_receipt(receipt,
    CASE WHEN mismatch=1 THEN receipt ELSE peer END,
    CASE WHEN mismatch=2 THEN receipt ELSE session_hash END,
    CASE WHEN mismatch=3 THEN 'reply' WHEN mismatch=4 THEN 'report' ELSE 'thread' END,
    CASE WHEN mismatch=5 THEN 'g' ELSE 'qreceipt1' END,
    CASE WHEN mismatch IN(3,4) THEN 1 ELSE 0 END,context_hash);
   RAISE EXCEPTION 'Receipt accepted mismatched scope';
  EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 END LOOP;
 BEGIN
  PERFORM verification_private.take_write_receipt(receipt,peer,session_hash,'thread','qreceipt1',0,peer);
  RAISE EXCEPTION 'Receipt accepted the wrong form context';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 IF NOT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=receipt) THEN
  RAISE EXCEPTION 'Mismatched form consumed another context receipt'; END IF;
 BEGIN
  grant_credit:=verification_private.take_write_receipt(receipt,peer,session_hash,'thread','qreceipt1',0,context_hash);
  IF NOT grant_credit THEN RAISE EXCEPTION 'Eligible proof lost its grant metadata'; END IF;
  RAISE EXCEPTION 'Synthetic protected write failure' USING ERRCODE='P0167';
 EXCEPTION WHEN SQLSTATE 'P0167' THEN NULL; END;
 IF NOT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=receipt) THEN
  RAISE EXCEPTION 'Failed write did not roll back proof consumption'; END IF;
 PERFORM verification_private.take_write_receipt(receipt,peer,session_hash,'thread','qreceipt1',0,context_hash);
 BEGIN
  PERFORM verification_private.take_write_receipt(receipt,peer,session_hash,'thread','qreceipt1',0,context_hash);
  RAISE EXCEPTION 'Consumed proof was replayed';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 BEGIN
  PERFORM verification_private.take_write_receipt(decode(repeat('6a',32),'hex'),peer,session_hash,'thread','qreceipt1',0,context_hash);
  RAISE EXCEPTION 'Expired proof was consumed';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 BEGIN
  PERFORM verification_private.take_write_receipt(decode(repeat('6b',32),'hex'),peer,session_hash,'reply','qreceipt1',1,context_hash);
  RAISE EXCEPTION 'Credit-backed receipt skipped the spending boundary';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 UPDATE post_secrets.anonymous_sessions SET expires_at=extract(epoch FROM clock_timestamp())::bigint-1
 WHERE token_hash=session_hash;
 BEGIN
  PERFORM verification_private.take_write_receipt(decode(repeat('6c',32),'hex'),peer,session_hash,'thread','qreceipt1',0,context_hash);
  RAISE EXCEPTION 'Expired session consumed an unexpired receipt';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $tests$;
RESET ROLE;
ROLLBACK;
