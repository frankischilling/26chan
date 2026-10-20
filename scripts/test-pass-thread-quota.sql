-- Rollback-only accounts and posts. The source quota counts IP OR established
-- automatic session OR Pass identity, once per live OP.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled,user_thread_limit,user_thread_period_hours)
 VALUES('qpassop','Owned member OP quota','',2000,100,100,100,10,false,1,24);
SET LOCAL ROLE board_public;
SELECT content.lock_posting_actor(sha256('pass-op-old-peer'),true);
SELECT set_config('board.posting_actor',encode(sha256('pass-op-old-peer'),'hex'),true);
DO $$ DECLARE op bigint:=nextval('content.post_number'); BEGIN
 INSERT INTO content.threads(id,board) VALUES(op,'qpassop');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(op,'qpassop',op,'Anonymous','Owned quota','Owned membership OP');
 PERFORM set_config('board.fixture_pass_op',op::text,true);
END $$;
SET LOCAL ROLE board_migrator;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,automatic_identity)
 SELECT sha256('pass-op-new-anon'),sha256('network'),sha256('address'),sha256('environment'),n-100,n-100,n-100,n-100,n+3600,gen_random_uuid()
 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_posts(post_id,identity_hash) VALUES(current_setting('board.fixture_pass_op')::bigint,sha256('pass-op-account'));
WITH account AS (INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('pass-op-account'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year') RETURNING id)
INSERT INTO verification_private.pass_sessions SELECT sha256('pass-op-member'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_write_receipt(sha256('pass-op-member'),sha256('pass-op-receipt'),sha256('peer'),sha256('login'),sha256('posting'),sha256('pass-op-new-anon'),sha256('form'),'thread','qpassop',0))<>'ready' THEN
 RAISE EXCEPTION 'Owned quota proof issuance failed'; END IF;
END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('pass-op-receipt'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('pass-op-new-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ DECLARE stamp bigint:=floor(extract(epoch FROM clock_timestamp())); BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,stamp)) IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Pass-only active OP was not counted across addresses'; END IF;
END $$;
-- An OP matching all three predicates is counted once.
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET user_thread_limit=2 WHERE slug='qpassop';
UPDATE content.posts SET created_at=date_trunc('second',created_at) WHERE id=current_setting('board.fixture_pass_op')::bigint;
SELECT set_config('board.fixture_pass_quota_stamp',(SELECT extract(epoch FROM created_at)::bigint::text FROM content.posts WHERE id=current_setting('board.fixture_pass_op')::bigint),true);
-- Administrative setup only; runtime roles cannot rewrite captured identities.
RESET ROLE;
UPDATE post_secrets.posting_history SET automatic_identity=(SELECT automatic_identity FROM post_secrets.anonymous_sessions WHERE token_hash=sha256('pass-op-new-anon')) WHERE post_id=current_setting('board.fixture_pass_op')::bigint;
SET LOCAL ROLE board_public;
SELECT set_config('board.fixture_pass_quota_proof',current_setting('board.challenge_context'),true);
DO $$ DECLARE stamp bigint:=floor(extract(epoch FROM clock_timestamp())); BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-old-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,stamp)) IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Overlapping quota identities counted one OP more than once'; END IF;
END $$;
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET user_thread_limit=1 WHERE slug='qpassop';
SET LOCAL ROLE board_anonymous_owner;
UPDATE post_secrets.anonymous_sessions SET created_at=floor(extract(epoch FROM clock_timestamp()))::bigint WHERE token_hash=sha256('pass-op-new-anon');
SELECT set_config('board.fixture_new_session_clock',(SELECT created_at::text FROM post_secrets.anonymous_sessions WHERE token_hash=sha256('pass-op-new-anon')),true);
SET LOCAL ROLE board_public;
DO $$ DECLARE stamp bigint:=current_setting('board.fixture_new_session_clock')::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,stamp)) IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Source-new password state incorrectly excluded Pass identity'; END IF;
END $$;
-- A forged presentation flag and the same source-new automatic identity do not
-- substitute for a private membership receipt.
SELECT set_config('board.challenge_context','',true);
DO $$ DECLARE stamp bigint:=current_setting('board.fixture_new_session_clock')::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,stamp)) IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Ordinary source-new context inherited Pass or old password authority'; END IF;
END $$;
SELECT set_config('board.challenge_context',current_setting('board.fixture_pass_quota_proof'),true);
DO $$ DECLARE stamp bigint:=current_setting('board.fixture_pass_quota_stamp')::bigint; session_clock bigint:=current_setting('board.fixture_new_session_clock')::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp+86400,sha256('pass-op-new-anon'),false,session_clock)) IS DISTINCT FROM false
 OR (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp+86399,sha256('pass-op-new-anon'),false,session_clock)) IS DISTINCT FROM true
 OR (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp-1,sha256('pass-op-new-anon'),false,session_clock)) IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Pass quota changed strict cutoff or future-post semantics'; END IF;
END $$;
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET user_thread_period_hours=0 WHERE slug='qpassop';
SET LOCAL ROLE board_public;
DO $$ DECLARE stamp bigint:=current_setting('board.fixture_pass_quota_stamp')::bigint; session_clock bigint:=current_setting('board.fixture_new_session_clock')::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,session_clock)) IS DISTINCT FROM false
 OR (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp-1,sha256('pass-op-new-anon'),false,session_clock)) IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Zero-period Pass quota changed source boundary'; END IF;
END $$;
SET LOCAL ROLE board_migrator;
UPDATE content.boards SET user_thread_limit=0 WHERE slug='qpassop';
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context','',true);
DO $$ DECLARE stamp bigint:=current_setting('board.fixture_new_session_clock')::bigint; BEGIN
 IF (SELECT rejected FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,stamp)) IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Zero maximum no longer rejects an empty quota'; END IF;
 BEGIN PERFORM count(*) FROM verification_private.pass_posts; RAISE EXCEPTION 'Public role read Pass identities'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM verification_private.pass_posting_identity('qpassop',0,sha256('pass-op-new-anon')); RAISE EXCEPTION 'Public role invoked private identity resolver'; EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
-- A malformed form binding or revoked membership cannot become quota advice.
SELECT set_config('board.challenge_context',jsonb_set(current_setting('board.fixture_pass_quota_proof')::jsonb,'{context}',to_jsonb(encode(sha256('wrong-form'),'hex')))::text,true);
DO $$ DECLARE stamp bigint:=current_setting('board.fixture_new_session_clock')::bigint; BEGIN
 BEGIN PERFORM * FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,stamp); RAISE EXCEPTION 'Wrong form binding accepted'; EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SELECT set_config('board.challenge_context',current_setting('board.fixture_pass_quota_proof'),true);
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET status=4 WHERE token_hash=sha256('pass-op-account');
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('pass-op-receipt')) THEN RAISE EXCEPTION 'Quota advice consumed its proof'; END IF; END $$;
SET LOCAL ROLE board_public;
DO $$ DECLARE stamp bigint:=current_setting('board.fixture_new_session_clock')::bigint; BEGIN
 BEGIN PERFORM * FROM content.check_user_thread_quota(sha256('pass-op-new-peer'),'qpassop',stamp,sha256('pass-op-new-anon'),false,stamp); RAISE EXCEPTION 'Revoked Pass accepted for quota advice'; EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
ROLLBACK;

-- Body drift must be detected even though the old structural readiness check
-- accepts the same owner, signature, result columns and ACL.
BEGIN;
CREATE OR REPLACE FUNCTION content.check_user_thread_quota(p_actor bytea,p_board text,p_request_at bigint,p_token bytea,p_minted boolean,p_session_request_at bigint)
RETURNS TABLE(rejected boolean,user_thread_limit integer,user_thread_period_hours integer)
LANGUAGE plpgsql SECURITY DEFINER SET search_path=pg_catalog,pg_temp AS $$ BEGIN RETURN QUERY SELECT false,1,24; END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Quota body drift escaped Pass attestation'; END IF; END $$;
ROLLBACK;
