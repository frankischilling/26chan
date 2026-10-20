-- Owned synthetic identities and bans. Nothing survives this transaction.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled)
 VALUES('qpassban','Owned membership restrictions','',2000,100,100,100,10,false);
SET LOCAL ROLE board_public;
SELECT content.lock_posting_actor(sha256('pass-ban-actor'),true);
SELECT set_config('board.posting_actor',encode(sha256('pass-ban-actor'),'hex'),true);
DO $$ DECLARE op bigint:=nextval('content.post_number'); BEGIN
 INSERT INTO content.threads(id,board) VALUES(op,'qpassban');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(op,'qpassban',op,'Anonymous','Owned source','Owned source post');
 PERFORM set_config('board.fixture_ban_post',op::text,true);
END $$;
SET LOCAL ROLE board_migrator;
INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,automatic_identity)
 SELECT sha256('pass-ban-anon'),sha256('network'),sha256('address'),sha256('environment'),n-100,n-100,n-100,n-100,n+3600,gen_random_uuid()
 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock;
SET LOCAL ROLE board_anonymous_owner;
INSERT INTO verification_private.pass_posts(post_id,identity_hash) VALUES(current_setting('board.fixture_ban_post')::bigint,sha256('pass-ban-account'));
WITH account AS (INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at)
 VALUES(sha256('pass-ban-account'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year') RETURNING id)
INSERT INTO verification_private.pass_sessions SELECT sha256('pass-ban-member'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;
SET LOCAL ROLE board_migrator;
INSERT INTO staff_identity.accounts(username,role,allow_boards) VALUES('owned-pass-reviewer','moderator',ARRAY['qpassban']);
INSERT INTO staff_identity.credentials(id,account_id,credential) SELECT sha256('owned-pass-review-credential'),id,'{}' FROM staff_identity.accounts WHERE username='owned-pass-reviewer';
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) SELECT sha256('owned-pass-review-session'),sha256('owned-pass-review-csrf'),id,sha256('owned-pass-review-credential') FROM staff_identity.accounts WHERE username='owned-pass-reviewer';
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason,request_save_policy,request_blacklist_policy) VALUES(999891,'Owned Pass template','janitor',false,false,86400,'Owned member ban','','none','none');
SET LOCAL ROLE board_staff;
SELECT set_config('board.fixture_pass_request',content.create_janitor_request(sha256('owned-pass-review-session'),sha256('owned-pass-review-csrf'),600,'qpassban',current_setting('board.fixture_ban_post')::bigint,999891,false,false,NULL,NULL)::text,true);
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF (SELECT pass_identity_hash FROM restrictions.requests WHERE id=current_setting('board.fixture_pass_request')::bigint) IS DISTINCT FROM sha256('pass-ban-account') THEN
 RAISE EXCEPTION 'Request did not retain its private membership before deletion'; END IF;
 IF EXISTS(SELECT 1 FROM verification_private.pass_posts WHERE post_id=current_setting('board.fixture_ban_post')::bigint) THEN
 RAISE EXCEPTION 'Request creation did not erase the source association'; END IF;
END $$;
SAVEPOINT linked_history;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest,pass_identity_hash)
 VALUES('qpassban',NULL,sha256('other-history-actor'),false,false,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned Pass-linked history','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('pass-ban-account')),
 ('qpassban',NULL,sha256('pass-ban-actor'),false,false,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned same-address history','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('pass-ban-account')),
 ('g',NULL,sha256('hidden-history-actor'),false,false,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Hidden scope history','',NULL,536,'file',1,decode(repeat('09',16),'hex'),sha256('pass-ban-account'));
UPDATE restrictions.capacity SET used_bytes=used_bytes+1608 WHERE singleton;
SET LOCAL ROLE board_auth;
DO $$ DECLARE queue jsonb; detail jsonb; BEGIN
 queue:=staff_identity.janitor_request_queue(sha256('owned-pass-review-session'),sha256('owned-pass-review-csrf'),600,'qpassban',0);
 IF (queue->'requests'->0->'history'->>'total')::integer<>3 THEN
 RAISE EXCEPTION 'Request summary omitted membership history or double-counted address overlap'; END IF;
 detail:=staff_identity.janitor_request_history(sha256('owned-pass-review-session'),sha256('owned-pass-review-csrf'),600,current_setting('board.fixture_pass_request')::bigint,0);
 IF jsonb_array_length(detail->'rows')<>2 OR detail::text NOT LIKE '%Owned Pass-linked history%'
 OR detail::text LIKE '%Hidden scope history%'
 OR queue::text LIKE '%'||encode(sha256('pass-ban-account'),'hex')||'%'
 OR detail::text LIKE '%'||encode(sha256('pass-ban-account'),'hex')||'%' THEN
 RAISE EXCEPTION 'Request membership history was missing or exposed private identity'; END IF;
END $$;
ROLLBACK TO linked_history;
SET LOCAL ROLE board_staff;
SELECT content.review_janitor_request(sha256('owned-pass-review-session'),sha256('owned-pass-review-csrf'),600,current_setting('board.fixture_pass_request')::bigint,true,NULL,NULL,NULL,NULL,NULL);
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF (SELECT pass_identity_hash FROM restrictions.bans WHERE board='qpassban') IS DISTINCT FROM sha256('pass-ban-account') THEN
 RAISE EXCEPTION 'Restriction did not capture the post membership'; END IF;
 IF verification_private.pass_inserted_identity(current_setting('board.fixture_ban_post')::bigint,sha256('pass-ban-anon')) IS NOT NULL THEN
 RAISE EXCEPTION 'Historical post identity authorized a fresh insert'; END IF;
END $$;
SET LOCAL ROLE board_migrator;
UPDATE content.posts SET deleted=true WHERE id=current_setting('board.fixture_ban_post')::bigint;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_posts WHERE post_id=current_setting('board.fixture_ban_post')::bigint) THEN
 RAISE EXCEPTION 'Deleted source post retained private membership'; END IF; END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN
 IF (SELECT status FROM content.pass_write_receipt(sha256('pass-ban-member'),sha256('pass-ban-receipt'),sha256('peer'),sha256('login'),sha256('posting'),sha256('pass-ban-anon'),sha256('form'),'thread','qpassban',0))<>'ready' THEN
 RAISE EXCEPTION 'Owned membership proof issuance failed'; END IF;
END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('pass-ban-receipt'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('pass-ban-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ DECLARE now_at bigint:=floor(extract(epoch FROM clock_timestamp())); BEGIN
 IF content.evaluate_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',now_at)<>'allowed'
 OR content.evaluate_member_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',now_at,0)<>'banned' THEN
 RAISE EXCEPTION 'Membership ban did not follow its valid account across address change'; END IF;
END $$;
-- Direct inserts must repeat the membership ban decision after consuming
-- their proof. Refusal restores the proof and leaves no private post identity.
SELECT content.lock_posting_actor(sha256('changed-actor'),true);
SELECT set_config('board.posting_actor',encode(sha256('changed-actor'),'hex'),true);
SELECT set_config('board.restriction_token',encode(sha256('pass-ban-anon'),'hex'),true);
DO $$ DECLARE op bigint:=nextval('content.post_number'); BEGIN
 PERFORM set_config('board.fixture_refused_member',op::text,true);
 BEGIN
  INSERT INTO content.threads(id,board) VALUES(op,'qpassban');
  INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(op,'qpassban',op,'Anonymous','Owned ban rejection','Owned banned member insert');
  RAISE EXCEPTION 'Direct member insert bypassed its membership ban';
 EXCEPTION WHEN SQLSTATE 'P134B' THEN NULL;
 END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('pass-ban-receipt'))
 OR EXISTS(SELECT 1 FROM verification_private.pass_posts WHERE post_id=current_setting('board.fixture_refused_member')::bigint) THEN
 RAISE EXCEPTION 'Banned insert did not restore proof and private identity'; END IF;
END $$;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 BEGIN
  UPDATE restrictions.bans SET pass_identity_hash=sha256('forged-membership') WHERE board='qpassban';
  RAISE EXCEPTION 'Restriction membership was mutable';
 EXCEPTION WHEN insufficient_privilege THEN NULL;
 END;
END $$;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.bans SET template_id=124,upload_observation='absent' WHERE board='qpassban';
SET LOCAL ROLE board_public;
DO $$ BEGIN
 IF content.evaluate_member_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',floor(extract(epoch FROM clock_timestamp()))::bigint,0)<>'allowed' THEN
 RAISE EXCEPTION 'Source Pass template 124 exception differs'; END IF;
END $$;
SAVEPOINT password_match;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF (SELECT automatic_identity FROM post_secrets.lookup_automatic_identity(sha256('pass-ban-anon'),floor(extract(epoch FROM clock_timestamp()))::bigint)) IS NULL THEN RAISE EXCEPTION 'Owned password identity is missing'; END IF; END $$;
UPDATE restrictions.bans SET automatic_identity=(SELECT automatic_identity FROM post_secrets.lookup_automatic_identity(sha256('pass-ban-anon'),floor(extract(epoch FROM clock_timestamp()))::bigint)) WHERE board='qpassban';
SET LOCAL ROLE board_public;
DO $$ BEGIN
 IF content.evaluate_member_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',floor(extract(epoch FROM clock_timestamp()))::bigint,0)<>'banned' THEN
 RAISE EXCEPTION 'Membership improperly skipped the earlier password match'; END IF;
END $$;
ROLLBACK TO password_match;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.bans SET warning=true,ends_at=starts_at WHERE board='qpassban';
SET LOCAL ROLE board_public;
DO $$ BEGIN
 IF content.evaluate_member_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',floor(extract(epoch FROM clock_timestamp()))::bigint,0)<>'warned' THEN
 RAISE EXCEPTION 'Membership warning did not precede template exception'; END IF;
END $$;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.bans SET warning=false,starts_at=statement_timestamp()-interval '1 hour',ends_at=statement_timestamp()-interval '1 second' WHERE board='qpassban';
SET LOCAL ROLE board_public;
DO $$ BEGIN
 IF content.evaluate_member_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',floor(extract(epoch FROM clock_timestamp()))::bigint,0)<>'allowed' THEN
 RAISE EXCEPTION 'Expired membership ban remained active'; END IF;
END $$;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM restrictions.bans WHERE board='qpassban' AND active)
 OR NOT EXISTS(SELECT 1 FROM restrictions.audit a JOIN restrictions.bans b ON b.id=a.ban_id WHERE b.board='qpassban' AND a.action='expiration') THEN
 RAISE EXCEPTION 'Visited expiry was not retired and audited'; END IF;
END $$;
-- A host-only automatic restriction is skipped by the consumed membership
-- proof, but not by an ordinary insert or a historical association.
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,origin,file_rule_id,file_digest)
 VALUES('qpassban',NULL,sha256('changed-actor'),false,false,statement_timestamp()-interval '1 hour',statement_timestamp()+interval '1 day','Owned address ban','',NULL,529,'file',1,decode(repeat('09',16),'hex'));
SET LOCAL ROLE board_public;
SELECT content.lock_posting_actor(sha256('changed-actor'),true);
SELECT set_config('board.posting_actor',encode(sha256('changed-actor'),'hex'),true);
SELECT set_config('board.restriction_token',encode(sha256('pass-ban-anon'),'hex'),true);
DO $$ DECLARE op bigint:=nextval('content.post_number'); now_at bigint:=floor(extract(epoch FROM clock_timestamp())); BEGIN
 IF content.evaluate_member_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',now_at,0)<>'allowed'
 OR content.evaluate_posting_restrictions(sha256('changed-actor'),sha256('pass-ban-anon'),'qpassban',now_at)<>'banned' THEN
 RAISE EXCEPTION 'Host-only ban bypass did not require current membership'; END IF;
 INSERT INTO content.threads(id,board) VALUES(op,'qpassban');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(op,'qpassban',op,'Anonymous','Owned member acceptance','Owned member post under address ban');
 PERFORM set_config('board.fixture_member_insert',op::text,true);
 BEGIN
  op:=nextval('content.post_number');
  INSERT INTO content.threads(id,board) VALUES(op,'qpassban');
  INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(op,'qpassban',op,'Anonymous','Owned nonmember rejection','Owned ordinary post under address ban');
  RAISE EXCEPTION 'Ordinary post inherited consumed membership';
 EXCEPTION WHEN SQLSTATE 'P134B' THEN NULL;
 END;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('pass-ban-receipt'))
 OR NOT EXISTS(SELECT 1 FROM verification_private.pass_posts WHERE post_id=current_setting('board.fixture_member_insert')::bigint AND identity_hash=sha256('pass-ban-account')) THEN
 RAISE EXCEPTION 'Accepted restricted post lacked consumed membership evidence'; END IF;
END $$;
ROLLBACK;

BEGIN;
SET LOCAL ROLE board_restriction_owner;
ALTER TABLE restrictions.bans DISABLE TRIGGER capture_pass_identity;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Disabled membership capture escaped restriction attestation'; END IF; END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
GRANT EXECUTE ON FUNCTION verification_private.pass_posting_identity(text,bigint,bytea) TO board_public;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Private membership reader grant escaped attestation'; END IF; END $$;
ROLLBACK;
DO $$ BEGIN
 IF has_function_privilege('board_public','restrictions.posting_decision_qualified(bytea,bytea,text,bigint,bytea)','EXECUTE')
 OR has_function_privilege('board_public','verification_private.pass_inserted_identity(bigint,bytea)','EXECUTE')
 OR has_function_privilege('board_public','restrictions.record_file_ban_with_membership(bytea,bytea,text,bigint,bytea,integer,boolean,text,text,text,bytea)','EXECUTE')
 OR has_function_privilege('board_staff','restrictions.record_file_ban_with_membership(bytea,bytea,text,bigint,bytea,integer,boolean,text,text,text,bytea)','EXECUTE')
 OR has_any_column_privilege('board_public','restrictions.bans','SELECT') THEN
 RAISE EXCEPTION 'Runtime acquired private membership or ban authority'; END IF;
END $$;
