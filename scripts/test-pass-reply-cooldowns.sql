-- Owned records and fixed posting clocks; all fixture data rolls back.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled,posting_reply_seconds,posting_image_seconds,posting_thread_seconds)
 VALUES('qpasscd','Owned Pass cooldown','',2000,100,100,100,10,false,5,9,11);
SET LOCAL ROLE board_public;
SELECT content.lock_posting_actor(sha256('cd-actor'),true);
SELECT set_config('board.posting_actor',encode(sha256('cd-actor'),'hex'),true);
DO $$ DECLARE op bigint:=nextval('content.post_number'); reply bigint:=nextval('content.post_number'); BEGIN
 INSERT INTO content.threads(id,board) VALUES(op,'qpasscd');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES
  (op,'qpasscd',op,'Anonymous','Owned cooldown','Owned OP',to_timestamp(1000)),
  (reply,'qpasscd',op,'Anonymous','','Owned reply',to_timestamp(1000));
 PERFORM set_config('board.fixture_op',op::text,true);
 PERFORM set_config('board.fixture_reply',reply::text,true);
END $$;
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET captcha_enabled=true WHERE slug='qpasscd';
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at)
 SELECT sha256('cd-anon'),sha256('cd-net'),sha256('cd-address'),sha256('cd-env'),n,n,n,n,n+3600
 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock;
SET LOCAL ROLE board_anonymous_owner;
WITH account AS (
 INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('cd-account'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year') RETURNING id
)
INSERT INTO verification_private.pass_sessions SELECT sha256('cd-member'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_write_receipt(sha256('cd-member'),sha256('cd-reply'),sha256('cd-peer'),sha256('cd-login'),sha256('cd-posting'),sha256('cd-anon'),sha256('cd-form'),'reply','qpasscd',current_setting('board.fixture_op')::bigint))<>'ready'
 OR (SELECT status FROM content.pass_write_receipt(sha256('cd-member'),sha256('cd-thread'),sha256('cd-peer'),sha256('cd-login'),sha256('cd-posting'),sha256('cd-anon'),sha256('cd-form'),'thread','qpasscd',0))<>'ready' THEN
 RAISE EXCEPTION 'Owned cooldown receipt issuance failed'; END IF;
END $$;
SET LOCAL ROLE board_public;
DO $$ DECLARE op bigint:=current_setting('board.fixture_op')::bigint; result record; BEGIN
 PERFORM content.lock_posting_actor(sha256('cd-actor'),false);
 -- No membership evidence uses the full reply delay while CAPTCHA is enabled.
 SELECT * INTO result FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',op,false,1002);
 IF result.kind<>'reply' OR result.remaining_seconds<>3 THEN RAISE EXCEPTION 'Ordinary reply delay changed'; END IF;
 PERFORM set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('forged-receipt'),'hex'),'peer',encode(sha256('cd-peer'),'hex'),'session',encode(sha256('cd-anon'),'hex'),'context',encode(sha256('cd-form'),'hex'))::text,true);
 SELECT * INTO result FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',op,false,1002);
 IF result.remaining_seconds<>3 THEN RAISE EXCEPTION 'Unissued receipt selected a discount'; END IF;
 PERFORM set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('cd-reply'),'hex'),'peer',encode(sha256('cd-peer'),'hex'),'session',encode(sha256('cd-anon'),'hex'),'context',encode(sha256('cd-form'),'hex'))::text,true);
 SELECT * INTO result FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',op,false,1002);
 IF result.kind<>'reply' OR result.remaining_seconds<>1 THEN RAISE EXCEPTION 'Odd reply delay did not round upward'; END IF;
 IF EXISTS(SELECT 1 FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',op,false,1003)) THEN
 RAISE EXCEPTION 'Exact membership reply boundary rejected'; END IF;
 SELECT * INTO result FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',op,true,1002);
 IF result.kind<>'image' OR result.remaining_seconds<>3 THEN RAISE EXCEPTION 'Odd image delay did not round upward'; END IF;
 IF EXISTS(SELECT 1 FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',op,true,1005)) THEN
 RAISE EXCEPTION 'Exact membership image boundary rejected'; END IF;
 BEGIN
  PERFORM * FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',0,false,1002);
  RAISE EXCEPTION 'Reply receipt authorized another target'; EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL;
 END;
 PERFORM set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('cd-thread'),'hex'),'peer',encode(sha256('cd-peer'),'hex'),'session',encode(sha256('cd-anon'),'hex'),'context',encode(sha256('cd-form'),'hex'))::text,true);
 SELECT * INTO result FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',0,false,1002);
 IF result.kind<>'thread' OR result.remaining_seconds<>9 THEN RAISE EXCEPTION 'Membership shortened thread delay'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF (SELECT count(*) FROM verification_private.pass_receipts WHERE pass_session=sha256('cd-member'))<>2 THEN
 RAISE EXCEPTION 'Cooldown preflight consumed a receipt'; END IF;
END $$;
SAVEPOINT late_insert_failure;
-- A constraint failure after the BEFORE INSERT consumer must restore its receipt
-- and leave no private membership association. Retry the same proof successfully.
SET LOCAL ROLE board_public;
DO $$ DECLARE failed bigint:=nextval('content.post_number'); actual_constraint text; BEGIN
 PERFORM set_config('board.fixture_failed_post',failed::text,true);
 BEGIN
  INSERT INTO content.threads(id,board) VALUES(failed,'qpasscd');
  INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
   VALUES(failed,'qpasscd',failed,repeat('x',256),'Owned rollback','Owned failed post');
  RAISE EXCEPTION 'Oversized name unexpectedly succeeded';
 EXCEPTION WHEN check_violation THEN
  GET STACKED DIAGNOSTICS actual_constraint=CONSTRAINT_NAME;
  IF actual_constraint<>'posts_name_check' THEN RAISE EXCEPTION 'Unexpected constraint: %',actual_constraint; END IF;
 END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('cd-thread'))
 OR EXISTS(SELECT 1 FROM verification_private.pass_posts WHERE post_id=current_setting('board.fixture_failed_post')::bigint) THEN
 RAISE EXCEPTION 'Failed insert consumed a receipt or retained private identity'; END IF;
END $$;
SET LOCAL ROLE board_public;
DO $$ DECLARE op bigint:=nextval('content.post_number'); BEGIN
 INSERT INTO content.threads(id,board) VALUES(op,'qpasscd');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
  VALUES(op,'qpasscd',op,'Anonymous','Owned retry','Owned successful retry');
 PERFORM set_config('board.fixture_retry_post',op::text,true);
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('cd-thread'))
 OR NOT EXISTS(SELECT 1 FROM verification_private.pass_posts WHERE post_id=current_setting('board.fixture_retry_post')::bigint AND identity_hash=sha256('cd-account')) THEN
 RAISE EXCEPTION 'Retry did not consume the original proof and capture private identity'; END IF;
END $$;
ROLLBACK TO late_insert_failure;
SAVEPOINT revoke;
UPDATE verification_private.pass_entitlements SET status=4 WHERE token_hash=sha256('cd-account');
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM * FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',0,false,1002);
  RAISE EXCEPTION 'Revoked membership retained a cooldown bypass'; EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL;
 END;
END $$;
ROLLBACK TO revoke;
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET captcha_enabled=false WHERE slug='qpasscd';
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context','',true);
DO $$ DECLARE result record; BEGIN
 SELECT * INTO result FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',current_setting('board.fixture_op')::bigint,false,1002);
 IF result.kind<>'reply' OR result.remaining_seconds<>1 THEN RAISE EXCEPTION 'CAPTCHA-disabled source bypass differs'; END IF;
 IF has_function_privilege('board_public','content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)','EXECUTE') THEN
 RAISE EXCEPTION 'Public runtime can choose its own discount'; END IF;
END $$;
SAVEPOINT core_drift;
SET LOCAL ROLE board_posting_cooldown_owner;
GRANT EXECUTE ON FUNCTION content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean) TO board_public;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Untrusted direct cooldown grant escaped membership attestation'; END IF; END $$;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM * FROM content.pass_check_posting_cooldown(sha256('cd-actor'),'qpasscd',0,false,1002);
  RAISE EXCEPTION 'Drifted membership cooldown remained available'; EXCEPTION WHEN SQLSTATE '55000' THEN NULL;
 END;
END $$;
ROLLBACK TO core_drift;
ROLLBACK;
