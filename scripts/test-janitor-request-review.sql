SET ROLE board_migrator;
INSERT INTO staff_identity.accounts(username,role,allow_boards)
 VALUES('owned-request-reviewer','moderator',ARRAY['rqown','rqother']);
INSERT INTO staff_identity.credentials(id,account_id,credential)
 SELECT decode(repeat('66',16),'hex'),id,'{}' FROM staff_identity.accounts WHERE username='owned-request-reviewer';
INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id)
 SELECT decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),id,decode(repeat('66',16),'hex')
 FROM staff_identity.accounts WHERE username='owned-request-reviewer';
SET ROLE board_restriction_owner;
UPDATE restrictions.templates SET request_blacklist_policy='none' WHERE id BETWEEN 9001 AND 9006;
RESET ROLE;
CREATE TEMP TABLE owned_review_requests AS SELECT id,post_id FROM restrictions.requests;
GRANT SELECT ON owned_review_requests TO board_staff;
BEGIN;
UPDATE staff_identity.accounts SET allow_boards=ARRAY['rqown'] WHERE username='owned-request-reviewer';
SET LOCAL ROLE board_staff;
DO $$ DECLARE request bigint; BEGIN
 SELECT id INTO request FROM owned_review_requests WHERE post_id=207002;
 BEGIN
  PERFORM content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,true,NULL,NULL,NULL,NULL,NULL);
  RAISE EXCEPTION 'Related cross-board clearing ignored reviewer scope';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
END $$;
ROLLBACK;
SET ROLE board_staff;
DO $$ DECLARE request bigint; result jsonb; BEGIN
 SELECT id INTO request FROM owned_review_requests WHERE post_id=207003;
 BEGIN
  PERFORM content.review_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,request,false,NULL,NULL,NULL,NULL,NULL);
  RAISE EXCEPTION 'Janitor reviewed a request';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 result:=content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,false,NULL,NULL,NULL,NULL,NULL);
 IF result->>'changed'<>'true' OR result->>'cleared'<>'1' OR result->>'ban_id' IS NOT NULL THEN
  RAISE EXCEPTION 'Denial outcome differs'; END IF;
 result:=content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,false,NULL,NULL,NULL,NULL,NULL);
 IF result->>'changed'<>'false' THEN RAISE EXCEPTION 'Repeated denial counted again'; END IF;
 SELECT id INTO request FROM owned_review_requests WHERE post_id=207002;
 result:=content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,true,NULL,NULL,NULL,NULL,NULL);
 IF result->>'changed'<>'true' OR result->>'cleared'<>'2' OR result->>'ban_id' IS NULL THEN
  RAISE EXCEPTION 'Approval or related clearing differs: %',result; END IF;
 BEGIN
  PERFORM content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,true,NULL,NULL,NULL,NULL,NULL);
  RAISE EXCEPTION 'Repeated approval succeeded';
 EXCEPTION WHEN SQLSTATE 'P134N' THEN NULL; END;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM restrictions.requests)
  OR (SELECT used_bytes FROM restrictions.request_capacity)<>0
  OR (SELECT count(*) FROM restrictions.bans)<>1
  OR (SELECT count(*) FROM restrictions.request_ban_evidence)<>1
  OR (SELECT count(*) FROM restrictions.request_reviews)<>2
  OR NOT EXISTS(SELECT 1 FROM restrictions.request_totals WHERE approved=1 AND denied=1)
  OR (SELECT used_bytes FROM restrictions.capacity)<>(SELECT sum(charged_bytes) FROM restrictions.bans)
  OR NOT EXISTS(SELECT 1 FROM restrictions.request_ban_evidence WHERE payload->>'comment'='Owned request body 2') THEN
  RAISE EXCEPTION 'Review counters, evidence, quotas or duplicate outcomes differ'; END IF;
END $$;

-- Warning approval clears only warnings for the accepted template. Related
-- non-warning requests survive and do not change another counted outcome.
BEGIN;
SET LOCAL ROLE board_restriction_owner;
UPDATE restrictions.templates SET request_can_warn=true WHERE id=9002;
SET LOCAL ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,true,false);
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207005,9002,true,false);
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207006,9002,false,false);
RESET ROLE;
INSERT INTO owned_review_requests SELECT id,post_id FROM restrictions.requests WHERE post_id=207004;
SET LOCAL ROLE board_staff;
DO $$ DECLARE request bigint; result jsonb; BEGIN
 SELECT id INTO request FROM owned_review_requests WHERE post_id=207004;
 result:=content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,true,NULL,NULL,NULL,NULL,NULL);
 IF result->>'cleared'<>'2' THEN RAISE EXCEPTION 'Warning duplicate scope differs'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF (SELECT count(*) FROM restrictions.requests)<>1
  OR NOT EXISTS(SELECT 1 FROM restrictions.requests WHERE post_id=207006 AND NOT warning)
  OR NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE post_id=207004 AND warning AND ends_at=starts_at)
  OR NOT EXISTS(SELECT 1 FROM restrictions.request_totals WHERE approved=2 AND denied=1) THEN
  RAISE EXCEPTION 'Warning review effects or counted outcomes differ'; END IF;
END $$;
ROLLBACK;
-- Amending to a zero-day template without a length is a one-day ban, not a
-- warning; JSON-only policy retains related requests.
BEGIN;
SET LOCAL ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,false,false);
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207005,9003,false,false);
RESET ROLE;
INSERT INTO owned_review_requests SELECT id,post_id FROM restrictions.requests WHERE post_id=207004;
SET LOCAL ROLE board_staff;
DO $$ DECLARE request bigint; result jsonb; BEGIN
 SELECT id INTO request FROM owned_review_requests WHERE post_id=207004;
 BEGIN
  PERFORM content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,true,9003,1025,NULL,NULL,NULL);
  RAISE EXCEPTION 'Invalid amended duration accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
 result:=content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,true,9003,NULL,false,'Owned amended reason','Owned private detail');
 IF result->>'cleared'<>'1' THEN RAISE EXCEPTION 'JSON-only duplicate scope differs'; END IF;
END $$;
RESET ROLE;
DO $$ BEGIN
 IF NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE post_id=207004 AND NOT warning AND NOT global
  AND ends_at-starts_at=interval '1 day' AND public_reason='Owned amended reason'
  AND private_reason LIKE '%. Owned private detail')
  OR NOT EXISTS(SELECT 1 FROM restrictions.requests WHERE post_id=207005)
  OR NOT EXISTS(SELECT 1 FROM restrictions.request_reviews WHERE post_id=207004
   AND requested_template=9002 AND accepted_template=9003 AND NOT warning_action) THEN
  RAISE EXCEPTION 'Amended zero-day request effects differ'; END IF;
END $$;
ROLLBACK;
-- A template changed to zero after creation takes the non-warning source
-- action branch while producing an immediate warning-shaped restriction.
BEGIN;
SET LOCAL ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207004,9002,false,false);
RESET ROLE;
INSERT INTO owned_review_requests SELECT id,post_id FROM restrictions.requests WHERE post_id=207004;
UPDATE restrictions.templates SET warning=true,seconds=0 WHERE id=9002;
SET LOCAL ROLE board_staff;
DO $$ DECLARE request bigint; BEGIN
 SELECT id INTO request FROM owned_review_requests WHERE post_id=207004;
 PERFORM content.review_janitor_request(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,request,true,NULL,NULL,NULL,NULL,NULL);
END $$;
RESET ROLE;
DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM restrictions.bans WHERE post_id=207004 AND warning AND ends_at=starts_at)
 OR NOT EXISTS(SELECT 1 FROM restrictions.request_reviews WHERE post_id=207004 AND NOT warning_action) THEN
 RAISE EXCEPTION 'Warning duration and action branch were collapsed'; END IF; END $$;
ROLLBACK;
