SET ROLE board_migrator;
BEGIN;
SELECT set_config('board.posting_actor',repeat('11',32),true);
INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page)
 VALUES('rqown','Owned requests','Synthetic',2000,100,100,100,10),('rqother','Other requests','Synthetic',2000,100,100,100,10);
INSERT INTO content.threads(id,board) VALUES(207001,'rqown'),(207101,'rqother');
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 SELECT 207000+i,'rqown',207001,'Owned request name','Owned subject','Owned request body '||i FROM generate_series(1,12) i;
INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
 VALUES(207101,'rqother',207101,'Other','','Owned other body');
COMMIT;
INSERT INTO staff_identity.accounts(username,role,allow_boards) VALUES('owned-request-janitor','janitor',ARRAY['rqown']);
INSERT INTO staff_identity.credentials(id,account_id,credential)
 SELECT decode(repeat('44',16),'hex'),id,'{}' FROM staff_identity.accounts WHERE username='owned-request-janitor';
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
 SELECT decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),id,decode(repeat('44',16),'hex')
 FROM staff_identity.accounts WHERE username='owned-request-janitor';
SET ROLE board_restriction_owner;
INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason,requires_file,request_save_policy)
 VALUES(9001,'Unconfigured','janitor',false,false,86400,'Owned','',false,NULL),
 (9002,'Ordinary request','janitor',false,false,86400,'Owned','',false,'none'),
 (9003,'Warning request','janitor',true,true,0,'Owned','',false,'json_only'),
 (9004,'Administrator request','admin',false,false,86400,'Owned','',false,'none'),
 (9005,'File request','janitor',false,false,86400,'Owned','',true,'none'),
 (9006,'Private media request','janitor',false,false,86400,'Owned','',false,'everything');
RESET ROLE;
SET ROLE board_staff;
DO $$ DECLARE template bigint; BEGIN
 FOREACH template IN ARRAY ARRAY[9001,9006] LOOP
  BEGIN
   PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207002,template,false,false);
   RAISE EXCEPTION 'Unavailable retention policy accepted';
  EXCEPTION WHEN SQLSTATE 'P0207' THEN NULL; END;
 END LOOP;
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207002,9002,true,false);
  RAISE EXCEPTION 'Disallowed warning request accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqother',207101,9002,false,false);
  RAISE EXCEPTION 'Cross-board request without proof accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207002,9004,false,false);
  RAISE EXCEPTION 'Template role ignored';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207002,9005,false,false);
  RAISE EXCEPTION 'File-required template accepted text';
 EXCEPTION WHEN check_violation THEN NULL; END;
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('99',32),'hex'),600,'rqown',207002,9002,false,false);
  RAISE EXCEPTION 'Wrong CSRF proof accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM restrictions.requests) OR (SELECT used_bytes FROM restrictions.request_capacity)<>0
  OR EXISTS(SELECT 1 FROM content.posts WHERE board='rqown' AND deleted) THEN
  RAISE EXCEPTION 'Denied request left a partial effect'; END IF;
END $$;
SET ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207002,9002,true,true);
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207003,9003,false,false);
DO $$ BEGIN
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207002,9002,false,false);
  RAISE EXCEPTION 'Repeated request accepted a deleted target';
 EXCEPTION WHEN SQLSTATE 'P134N' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF (SELECT count(*) FROM restrictions.requests)<>2
  OR (SELECT warning FROM restrictions.requests WHERE post_id=207002)
  OR NOT (SELECT warning FROM restrictions.requests WHERE post_id=207003)
  OR (SELECT payload->>'comment' FROM restrictions.requests WHERE post_id=207002)<>'Owned request body 2'
  OR EXISTS(SELECT 1 FROM content.posts WHERE id IN(207002,207003) AND (NOT deleted OR comment<>''))
  OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id IN(207002,207003))
  OR (SELECT deleted FROM content.threads WHERE id=207001)
  OR (SELECT used_bytes FROM restrictions.request_capacity)<>(SELECT sum(charged_bytes) FROM restrictions.requests) THEN
  RAISE EXCEPTION 'Atomic request capture/deletion or accounting differs'; END IF;
END $$;
BEGIN;
UPDATE restrictions.request_capacity SET byte_limit=used_bytes+1;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,false,false);
  RAISE EXCEPTION 'Exhausted request capacity accepted';
 EXCEPTION WHEN SQLSTATE 'P134C' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN IF EXISTS(SELECT 1 FROM content.posts WHERE id=207004 AND deleted)
 OR EXISTS(SELECT 1 FROM restrictions.requests WHERE post_id=207004) THEN
 RAISE EXCEPTION 'Capacity refusal left a partial request'; END IF; END $$;
ROLLBACK;
BEGIN;
UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes';
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,false,false);
  RAISE EXCEPTION 'Stale authentication accepted';
 EXCEPTION WHEN SQLSTATE 'P134Q' THEN NULL; END;
END $$;
ROLLBACK;
BEGIN;
GRANT SELECT(payload) ON restrictions.requests TO board_staff;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Private request grant drift accepted'; END IF; END $$;
ROLLBACK;
-- A caller rollback must restore the captured post, identity and quota as well.
BEGIN;
SET LOCAL ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,false,false);
ROLLBACK;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM restrictions.requests WHERE post_id=207004)
  OR (SELECT deleted FROM content.posts WHERE id=207004)
  OR NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=207004)
  OR (SELECT used_bytes FROM restrictions.request_capacity)<>(SELECT sum(charged_bytes) FROM restrictions.requests) THEN
  RAISE EXCEPTION 'Caller rollback retained a partial request'; END IF;
END $$;
BEGIN;
SET LOCAL ROLE board_migrator;
UPDATE content.threads SET sticky=true WHERE id=207001;
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207001,9002,false,false);
  RAISE EXCEPTION 'Janitor deleted a sticky thread through a request';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
ROLLBACK;
BEGIN;
UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE username='owned-request-janitor';
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,false,false);
  RAISE EXCEPTION 'Revoked request account accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
ROLLBACK;
BEGIN;
DELETE FROM staff_identity.credentials WHERE id=decode(repeat('44',16),'hex');
SET LOCAL ROLE board_staff;
DO $$ BEGIN
 BEGIN
  PERFORM content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,false,false);
  RAISE EXCEPTION 'Removed credential accepted';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
ROLLBACK;
-- Real retained exact weights authorize the otherwise denied board. No client
-- unlock field or board-access mutation is supplied to the request writer.
SET ROLE board_migrator;
-- Each real report still runs its capture/authority triggers. Bound individual
-- operator setup statements without changing runtime timeouts or report totals.
SELECT format($seed$INSERT INTO content.reports(board,post_id,reason)
 SELECT 'rqother',207101,'Owned request unlock' FROM generate_series(%s,%s)$seed$,
 n,least(n+255,2399)) FROM generate_series(1,2399,256) n
\gexec
SET ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,effective_weight,numeric_proof,evaluated_at)
 SELECT id,1,0.5,'BaseEqualsFallback',clock_timestamp() FROM content.reports WHERE board='rqother';
SET ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqother',207101,9002,false,false);
RESET ROLE;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM restrictions.requests WHERE post_id=207101)
  OR NOT (SELECT deleted FROM content.posts WHERE id=207101)
  OR NOT (SELECT deleted FROM content.threads WHERE id=207101) THEN
  RAISE EXCEPTION 'Qualified cross-board OP request was not atomic'; END IF;
END $$;
