-- Owned synthetic identities and bans. Nothing survives this transaction.
BEGIN;
SET LOCAL ROLE board_migrator;
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled)
 VALUES('qpasswarn','Owned membership restrictions','',2000,100,100,100,10,false);
SET LOCAL ROLE board_public;
SELECT content.lock_posting_actor(sha256('pass-ban-actor'),true);
SELECT set_config('board.posting_actor',encode(sha256('pass-ban-actor'),'hex'),true);
DO $$ DECLARE op bigint:=nextval('content.post_number'); BEGIN
 INSERT INTO content.threads(id,board) VALUES(op,'qpasswarn');
 INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(op,'qpasswarn',op,'Anonymous','Owned source','Owned source post');
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
INSERT INTO staff_identity.accounts(username,role,allow_boards) VALUES('owned-pass-reviewer','moderator',ARRAY['qpasswarn']);
INSERT INTO staff_identity.credentials(id,account_id,credential) SELECT sha256('owned-pass-review-credential'),id,'{}' FROM staff_identity.accounts WHERE username='owned-pass-reviewer';
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) SELECT sha256('owned-pass-review-session'),sha256('owned-pass-review-csrf'),id,sha256('owned-pass-review-credential') FROM staff_identity.accounts WHERE username='owned-pass-reviewer';
SELECT set_config('board.fixture_warning_staff',(SELECT id::text FROM staff_identity.accounts WHERE username='owned-pass-reviewer'),true);
SET LOCAL ROLE board_restriction_owner;
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,pass_identity_hash)
 SELECT 'qpasswarn',current_setting('board.fixture_ban_post')::bigint,sha256('prior-address'),false,true,
 statement_timestamp(),statement_timestamp(),'Owned member warning','Never return this private reason',id,600,sha256('pass-ban-account')
 FROM staff_identity.accounts WHERE id=current_setting('board.fixture_warning_staff')::bigint;
SELECT set_config('board.fixture_warning',(SELECT id::text FROM restrictions.bans WHERE public_reason='Owned member warning'),true);
INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes,pass_identity_hash)
 SELECT 'qpasswarn',current_setting('board.fixture_ban_post')::bigint,sha256('another-address'),false,true,
 statement_timestamp(),statement_timestamp(),'Another member warning','Other private reason',id,600,sha256('other-pass-account')
 FROM staff_identity.accounts WHERE id=current_setting('board.fixture_warning_staff')::bigint;
UPDATE restrictions.capacity SET used_bytes=used_bytes+1200 WHERE singleton;
SET LOCAL ROLE board_public;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM content.restriction_status(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',floor(extract(epoch FROM clock_timestamp()))::bigint)) THEN
 RAISE EXCEPTION 'Ordinary status exposed membership-only warnings'; END IF;
 IF content.acknowledge_warning(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',floor(extract(epoch FROM clock_timestamp()))::bigint,current_setting('board.fixture_warning')::bigint) THEN
 RAISE EXCEPTION 'Ordinary acknowledgment accepted a different member'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('pass-ban-member'),sha256('status-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('pass-ban-anon'),sha256('form'),'restriction_status','qpasswarn',0))<>'ready' THEN
 RAISE EXCEPTION 'Owned warning proof issuance failed'; END IF; END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('status-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('pass-ban-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ DECLARE rows jsonb; n bigint:=floor(extract(epoch FROM clock_timestamp())); BEGIN
 BEGIN
  PERFORM content.pass_acknowledge_warning(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',n,current_setting('board.fixture_warning')::bigint);
  RAISE EXCEPTION 'Status proof authorized warning acknowledgment';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 SELECT jsonb_agg(to_jsonb(r)) INTO rows FROM content.pass_restriction_status(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',n) r;
 IF jsonb_array_length(rows)<>1 OR rows->0->>'public_reason'<>'Owned member warning'
 OR rows::text LIKE '%private reason%' OR rows::text LIKE '%'||encode(sha256('pass-ban-account'),'hex')||'%' THEN
 RAISE EXCEPTION 'Member status omitted own warning or exposed another identity'; END IF;
 BEGIN
  PERFORM content.pass_restriction_status(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',n);
  RAISE EXCEPTION 'Status proof replay succeeded';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('pass-ban-member'),sha256('posting-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('pass-ban-anon'),sha256('form'),'thread','qpasswarn',0))<>'ready' THEN
 RAISE EXCEPTION 'Owned warning proof issuance failed'; END IF; END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('posting-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('pass-ban-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_restriction_status(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',floor(extract(epoch FROM clock_timestamp()))::bigint);
  RAISE EXCEPTION 'Posting receipt authorized membership status';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('pass-ban-member'),sha256('warning-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('pass-ban-anon'),sha256('form'),'warning_view','qpasswarn',current_setting('board.fixture_warning')::bigint))<>'ready' THEN
 RAISE EXCEPTION 'Owned warning proof issuance failed'; END IF; END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('warning-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('pass-ban-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ DECLARE n bigint:=floor(extract(epoch FROM clock_timestamp())); BEGIN
 BEGIN
  PERFORM content.pass_acknowledge_warning(sha256('changed-address'),sha256('pass-ban-anon'),'g',n,current_setting('board.fixture_warning')::bigint);
  RAISE EXCEPTION 'Warning proof accepted another board';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 BEGIN
  PERFORM content.pass_acknowledge_warning(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',n,current_setting('board.fixture_warning')::bigint+1);
  RAISE EXCEPTION 'Warning proof accepted another warning ID';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 BEGIN
  PERFORM content.pass_acknowledge_warning(sha256('changed-address'),sha256('other-anon'),'qpasswarn',n,current_setting('board.fixture_warning')::bigint);
  RAISE EXCEPTION 'Warning proof accepted another session';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
 IF NOT content.pass_acknowledge_warning(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',n,current_setting('board.fixture_warning')::bigint) THEN
 RAISE EXCEPTION 'Own warning was not acknowledged after address change'; END IF;
 BEGIN
  PERFORM content.pass_acknowledge_warning(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',n,current_setting('board.fixture_warning')::bigint);
  RAISE EXCEPTION 'Warning proof replay succeeded';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE id=current_setting('board.fixture_warning')::bigint AND NOT active AND retirement='warning-viewed')
 OR (SELECT count(*) FROM restrictions.audit WHERE ban_id=current_setting('board.fixture_warning')::bigint AND action='warning-viewed')<>1
 OR NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE public_reason='Another member warning' AND active) THEN
 RAISE EXCEPTION 'Member acknowledgment changed the wrong warning or audit'; END IF;
END $$;
SET LOCAL ROLE board_anonymous_owner;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM verification_private.pass_entitlements WHERE token_hash=sha256('pass-ban-account')
 AND (last_used IS NOT NULL OR posting_scope IS NOT NULL)) THEN
 RAISE EXCEPTION 'Status or warning viewing changed the posting-network hold'; END IF; END $$;
SET LOCAL ROLE board_restriction_owner;
SELECT set_config('board.fixture_other_warning',(SELECT id::text FROM restrictions.bans WHERE public_reason='Another member warning'),true);
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('pass-ban-member'),sha256('other-warning-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('pass-ban-anon'),sha256('form'),'warning_view','qpasswarn',current_setting('board.fixture_other_warning')::bigint))<>'ready' THEN
 RAISE EXCEPTION 'Owned negative proof issuance failed'; END IF; END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('other-warning-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('pass-ban-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
DO $$ BEGIN
 IF content.pass_acknowledge_warning(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',floor(extract(epoch FROM clock_timestamp()))::bigint,current_setting('board.fixture_other_warning')::bigint) THEN
 RAISE EXCEPTION 'Valid receipt acknowledged another membership warning'; END IF;
END $$;
SET LOCAL ROLE board_verifier;
DO $$ BEGIN IF (SELECT status FROM content.pass_write_receipt(sha256('pass-ban-member'),sha256('revoked-proof'),sha256('peer'),sha256('login'),sha256('posting'),sha256('pass-ban-anon'),sha256('form'),'restriction_status','qpasswarn',0))<>'ready' THEN
 RAISE EXCEPTION 'Owned warning proof issuance failed'; END IF; END $$;
SET LOCAL ROLE board_public;
SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('revoked-proof'),'hex'),'peer',encode(sha256('peer'),'hex'),'session',encode(sha256('pass-ban-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true);
SET LOCAL ROLE board_anonymous_owner;
UPDATE verification_private.pass_entitlements SET pin_hash=repeat('owned-rotated-placeholder',3) WHERE token_hash=sha256('pass-ban-account');
SET LOCAL ROLE board_public;
DO $$ BEGIN
 BEGIN
  PERFORM content.pass_restriction_status(sha256('changed-address'),sha256('pass-ban-anon'),'qpasswarn',floor(extract(epoch FROM clock_timestamp()))::bigint);
  RAISE EXCEPTION 'Status proof survived credential revocation';
 EXCEPTION WHEN SQLSTATE 'P0166' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF has_function_privilege('board_public','verification_private.pass_restriction_identity(bytea,text,bigint,text)','EXECUTE')
 OR has_function_privilege('board_staff','verification_private.pass_restriction_identity(bytea,text,bigint,text)','EXECUTE')
 OR has_function_privilege('board_verifier','verification_private.pass_restriction_identity(bytea,text,bigint,text)','EXECUTE') THEN
 RAISE EXCEPTION 'Private membership identity reader leaked'; END IF;
END $$;
SAVEPOINT warning_privilege_drift;
SET LOCAL ROLE board_anonymous_owner;
GRANT EXECUTE ON FUNCTION verification_private.pass_restriction_identity(bytea,text,bigint,text) TO board_public;
DO $$ BEGIN IF content.public_pass_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Warning identity grant drift was accepted'; END IF; END $$;
ROLLBACK TO warning_privilege_drift;
ROLLBACK;
