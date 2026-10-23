\set ON_ERROR_STOP on
BEGIN;
DO $$
DECLARE who text; bad boolean;
BEGIN
 FOREACH who IN ARRAY ARRAY['board_public','board_staff','board_auth','board_verifier','board_media','board_media_intake'] LOOP
  IF has_schema_privilege(who,'submission_private','USAGE,CREATE') OR has_schema_privilege(who,'submission_api','CREATE') THEN
   RAISE EXCEPTION 'Unqualified role can reach submissions: %',who;
  END IF;
  SELECT EXISTS(SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
   WHERE n.nspname='submission_private' AND
    (has_table_privilege(who,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
     OR has_any_column_privilege(who,c.oid,'SELECT,INSERT,UPDATE,REFERENCES'))
  ) INTO bad;
  IF bad THEN RAISE EXCEPTION 'Unqualified role can access submission rows: %',who; END IF;
 END LOOP;
 IF NOT submission_private.valid_email('Owned+alias@example.test',254)
  OR submission_private.valid_email('x@y@example.test',254)
  OR submission_private.valid_email('x@-example.test',254)
  OR submission_private.valid_email('x@local',254)
  OR submission_private.valid_email('a..b@example.test',254)
  OR NOT submission_private.valid_text('',150,false,false)
  OR submission_private.valid_text(U&'\00A0',150,true,false)
  OR submission_private.valid_boards(ARRAY['polls'])
  OR submission_private.valid_boards(ARRAY['abcdefghijk'])
  OR submission_private.valid_text('😀',1,true,false) OR NOT submission_private.valid_text('😀',2,true,false)
  OR submission_private.valid_text(E'\n',20,true,true)
  OR submission_private.valid_text(E'x\t',20,true,false)
  OR NOT submission_private.valid_text(E'x\t',20,true,true)
  OR submission_private.valid_boards(ARRAY['g','g'])
  OR submission_private.valid_boards(ARRAY[NULL::text])
  OR submission_private.valid_boards('{}'::text[])
  OR submission_private.valid_text_array(ARRAY['a','b','c'],4,100,true)
 THEN RAISE EXCEPTION 'Submission validation differs'; END IF;
END $$;
INSERT INTO submission_private.application_campaigns(id,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity)
VALUES(1,100,200,ARRAY['g'],ARRAY['Owned one','Owned two','Owned three','Owned four'],150,2000,100);
INSERT INTO submission_private.applications(campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at)
VALUES(1,25,-4,3,'16:00-19:00','g',ARRAY['one','two','three','four'],100,100);
INSERT INTO submission_private.application_identity(application_id,campaign_id,ownership_hash,actor_hash,firstname,handle,email,email_identity)
SELECT id,1,decode(repeat('a1',32),'hex'),decode(repeat('b2',32),'hex'),'Owned','Owned handle','owned@example.test','owned@example.test'
FROM submission_private.applications WHERE campaign_id=1;
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_attribute WHERE attrelid='submission_private.applications'::regclass
  AND attname IN('firstname','handle','email','email_identity','ownership_hash','actor_hash')) THEN
  RAISE EXCEPTION 'Blind review relation contains identity fields';
 END IF;
 IF EXISTS(SELECT 1 FROM submission_private.application_campaigns WHERE enabled) THEN
  RAISE EXCEPTION 'Migration unexpectedly opened applications';
 END IF;
END $$;
SET LOCAL ROLE board_submission_owner;
SELECT submission_api.ready();
DO $$ BEGIN
 BEGIN PERFORM message FROM submission_private.feedback LIMIT 0;
  RAISE EXCEPTION 'Submission owner can read private feedback messages';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
 BEGIN PERFORM email FROM submission_private.application_identity LIMIT 0;
  RAISE EXCEPTION 'Submission owner can read application contact identity';
 EXCEPTION WHEN insufficient_privilege THEN NULL; END;
END $$;
DO $$ BEGIN
 IF submission_api.ready() IS DISTINCT FROM true
  OR submission_api.feedback_profile() IS NOT NULL
  OR submission_api.application_profile() IS NOT NULL
  OR submission_api.contest_profile(1::smallint) IS NOT NULL THEN
  RAISE EXCEPTION 'Default submission APIs are not closed and ready';
 END IF;
END $$;
RESET ROLE;
SAVEPOINT authority_drift;
GRANT SELECT(firstname) ON submission_private.application_identity TO board_submission_owner;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Submission owner column drift was accepted'; END IF; END $$;
RESET ROLE;
ROLLBACK TO authority_drift;
SET LOCAL ROLE board_submission_owner;
DO $$ BEGIN IF submission_api.ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Submission owner recovery failed'; END IF; END $$;
RESET ROLE;
ROLLBACK;
