-- Owned report identities; public receipt admission remains disconnected.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled)
 VALUES('qpassrpt','Owned report membership','',2000,100,100,100,10,false);
DO $$ DECLARE post bigint:=nextval('content.post_number'); BEGIN
 INSERT INTO content.threads(id,board) VALUES(post,'qpassrpt');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(post,'qpassrpt',post,'Anonymous','Owned report target','Owned report identity target');
 PERFORM set_config('board.fixture_report_post',post::text,true);
END $$;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,automatic_identity)
 SELECT sha256('rpt-anon'),sha256('network'),sha256('address'),sha256('environment'),n-100,n-100,n-100,n-100,n+3600,gen_random_uuid()
 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock;
SET LOCAL ROLE board_anonymous_owner;
WITH account AS (INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('rpt-account'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year') RETURNING id)
INSERT INTO verification_private.pass_sessions SELECT sha256('rpt-member'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('rpt-member'),sha256('rpt-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('rpt-anon'),sha256('form'),'report','qpassrpt',current_setting('board.fixture_report_post')::bigint))<>'ready' THEN
 RAISE EXCEPTION 'Owned report receipt unavailable'; END IF; END $$;
SET LOCAL ROLE board_migrator;
INSERT INTO content.reports(board,post_id,reason,created_at)
 SELECT 'qpassrpt',current_setting('board.fixture_report_post')::bigint,'Owned quota '||n,statement_timestamp()-interval '2 days' FROM generate_series(1,80) n;
SELECT set_config('board.fixture_quota_reports',(SELECT jsonb_agg(id)::text FROM content.reports WHERE board='qpassrpt'),true);
SET LOCAL ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity)
 SELECT id::bigint,sha256('old-address'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,current_setting('board.fixture_report_post')::bigint,statement_timestamp()-interval '2 days','11111111-1111-4111-8111-111111111111'::uuid
 FROM jsonb_array_elements_text(current_setting('board.fixture_quota_reports')::jsonb) ids(id);
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_reports SELECT id::bigint,sha256('rpt-account') FROM jsonb_array_elements_text(current_setting('board.fixture_quota_reports')::jsonb) ids(id);
SELECT verification_private.pass_consume_report_receipt(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-anon'),sha256('rpt-proof'),sha256('peer'),sha256('form'));
SET LOCAL ROLE board_report_admission_owner;
DO $$ BEGIN
 BEGIN
  PERFORM post_secrets.check_report_limits('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),NULL::uuid,statement_timestamp());
  RAISE EXCEPTION 'Pass identity evaded duplicate protection';
 EXCEPTION WHEN SQLSTATE 'P0001' THEN IF SQLERRM<>'You have already reported this post.' THEN RAISE; END IF; END;
 PERFORM post_secrets.check_report_limits('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('unrelated'),NULL::uuid,statement_timestamp());
END $$;
SAVEPOINT advisory;
SET LOCAL ROLE board_anonymous_owner;
DELETE FROM verification_private.pass_report_handoffs;
SET LOCAL ROLE board_report_admission_owner;
SELECT post_secrets.check_report_limits('qpassrpt',current_setting('board.fixture_report_post')::bigint,sha256('rpt-actor'),NULL::uuid,statement_timestamp());
ROLLBACK TO advisory;
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_report_handoffs SET post_id=post_id+1;
SELECT set_config('board.fixture_quota_target',(current_setting('board.fixture_report_post')::bigint+1)::text,true);
SET LOCAL ROLE board_report_admission_owner;
DO $$ DECLARE stamp timestamptz:=statement_timestamp(); offset_time interval; BEGIN
 FOREACH offset_time IN ARRAY ARRAY[interval '15 seconds',interval '14.999 seconds',interval '-1 second'] LOOP
  UPDATE post_secrets.report_membership SET reported_at=stamp-interval '2 days' WHERE board='qpassrpt';
  UPDATE post_secrets.report_membership SET reported_at=stamp-offset_time WHERE report_id=(SELECT min(report_id) FROM post_secrets.report_membership WHERE board='qpassrpt');
  BEGIN
   PERFORM post_secrets.check_report_limits('qpassrpt',current_setting('board.fixture_quota_target')::bigint,sha256('rpt-actor'),NULL::uuid,stamp);
   IF offset_time<>interval '15 seconds' THEN RAISE EXCEPTION 'Pass cooldown was skipped'; END IF;
  EXCEPTION WHEN SQLSTATE 'P0001' THEN
   IF offset_time=interval '15 seconds' OR SQLERRM<>'You have to wait a while before reporting another post.' THEN RAISE; END IF;
  END;
 END LOOP;
END $$;
DO $$ DECLARE stamp timestamptz:=statement_timestamp(); n integer; boundary interval; fresh interval; BEGIN
 FOR n IN 1..4 LOOP
  boundary:=CASE WHEN n<=2 THEN interval '1 hour' ELSE interval '24 hours' END;
  fresh:=boundary-interval '1 millisecond';
  UPDATE post_secrets.report_membership SET reported_at=stamp-interval '2 days' WHERE board='qpassrpt';
  UPDATE post_secrets.report_membership SET reported_at=stamp-boundary WHERE report_id=(SELECT max(report_id) FROM post_secrets.report_membership WHERE board='qpassrpt');
  UPDATE post_secrets.report_membership SET reported_at=stamp-fresh WHERE report_id IN(
   SELECT report_id FROM post_secrets.report_membership WHERE board='qpassrpt' ORDER BY report_id LIMIT CASE n WHEN 1 THEN 29 WHEN 2 THEN 30 WHEN 3 THEN 79 ELSE 80 END);
  BEGIN
   PERFORM post_secrets.check_report_limits('qpassrpt',current_setting('board.fixture_quota_target')::bigint,sha256('rpt-actor'),NULL::uuid,stamp);
   IF n IN(2,4) THEN RAISE EXCEPTION 'Pass quota was skipped'; END IF;
  EXCEPTION WHEN SQLSTATE 'P0001' THEN
   IF n IN(1,3) OR SQLERRM<>'You have to wait a while before reporting another post.' THEN RAISE; END IF;
  END;
 END LOOP;
END $$;
UPDATE post_secrets.report_membership SET actor_hash=sha256('rpt-actor'),reported_at=statement_timestamp()-interval '2 days' WHERE board='qpassrpt';
UPDATE post_secrets.report_membership SET reported_at=statement_timestamp()-interval '1 minute' WHERE report_id IN(
 SELECT report_id FROM post_secrets.report_membership WHERE board='qpassrpt' ORDER BY report_id LIMIT 29);
SELECT post_secrets.check_report_limits('qpassrpt',current_setting('board.fixture_quota_target')::bigint,sha256('rpt-actor'),'11111111-1111-4111-8111-111111111111'::uuid,statement_timestamp());
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_report_ids(sha256('rpt-actor'),'qpassrpt',current_setting('board.fixture_quota_target')::bigint);
  RAISE EXCEPTION 'Public role read report associations';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
ROLLBACK;
