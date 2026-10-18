UPDATE staff_identity.accounts SET allow_boards=ARRAY['rqown','rqother','test'] WHERE username='owned-request-reviewer';
SET ROLE board_restriction_owner;
UPDATE restrictions.templates SET source_rule='global1' WHERE id=9002;
UPDATE restrictions.templates SET source_rule='hm6' WHERE id=9003;
UPDATE restrictions.templates SET request_save_policy='none' WHERE id=9001;
SET ROLE board_staff;
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207008,9002,false,false);
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207009,9003,false,false);
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207010,9002,false,false);
SELECT content.create_janitor_request(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,'rqown',207011,9001,false,false);
RESET ROLE;
-- This extra row is an owned reader fixture, not a claim of real admission.
-- It represents retained /test/ content without creating another live board.
INSERT INTO restrictions.requests(board,post_id,thread_id,actor_hash,automatic_identity,template_id,template_name,
 global,warning,save_policy,account_id,upload_observation,normalized_upload_md5,payload,charged_bytes)
 SELECT 'test',207208,207208,r.actor_hash,r.automatic_identity,r.template_id,r.template_name,
 r.global,r.warning,r.save_policy,r.account_id,r.upload_observation,r.normalized_upload_md5,p.saved,octet_length(p.saved::text)+1024
 FROM restrictions.requests r CROSS JOIN LATERAL (
 SELECT jsonb_set(jsonb_set(jsonb_set(r.payload,'{board}','"test"'),'{post_id}','207208'),'{thread_id}','207208') AS saved
 ) p WHERE r.post_id=207008;
UPDATE restrictions.request_capacity SET used_bytes=(SELECT sum(charged_bytes) FROM restrictions.requests);
SET ROLE board_auth;
DO $$ DECLARE page jsonb; ids bigint[]; stats jsonb; BEGIN
 BEGIN
  PERFORM staff_identity.janitor_request_queue(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,NULL,0);
  RAISE EXCEPTION 'Janitor read moderator queue';
 EXCEPTION WHEN SQLSTATE 'P134A' THEN NULL; END;
 page:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,NULL,0);
 SELECT array_agg((value->>'post_id')::bigint ORDER BY ordinal) INTO ids FROM jsonb_array_elements(page->'requests') WITH ORDINALITY item(value,ordinal);
 IF ids<>ARRAY[207010,207009,207008,207011]::bigint[] THEN
  RAISE EXCEPTION 'All-board source priority order differs: %',ids; END IF;
 IF page::text LIKE '%actor_hash%' OR page::text LIKE '%automatic_identity%' OR page::text LIKE '%normalized_upload_md5%' THEN
  RAISE EXCEPTION 'Request queue exposed private equality metadata'; END IF;
 page:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,'rqown',0);
 SELECT array_agg((value->>'post_id')::bigint ORDER BY ordinal) INTO ids FROM jsonb_array_elements(page->'requests') WITH ORDINALITY item(value,ordinal);
 IF ids<>ARRAY[207009,207010,207008,207011]::bigint[] THEN
  RAISE EXCEPTION 'Board-filtered source priority order differs: %',ids; END IF;
 page:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,'test',0);
 IF jsonb_array_length(page->'requests')<>1 OR page->'requests'->0->>'board'<>'test' THEN
  RAISE EXCEPTION 'Explicit test-board queue differs'; END IF;
 page:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,NULL,25);
 IF jsonb_array_length(page->'requests')<>0 OR page->>'previous_offset'<>'0' OR page->>'next_offset'<>'0' THEN
  RAISE EXCEPTION 'Empty queue pagination differs'; END IF;
 stats:=staff_identity.janitor_request_statistics(decode(repeat('22',32),'hex'),decode(repeat('33',32),'hex'),600,0);
 IF stats->'totals'<>'{"approved":2,"denied":1}'::jsonb OR stats->>'pending'<>'5'
  OR stats->'reviewed'<>'{"approved":0,"denied":0}'::jsonb OR jsonb_array_length(stats->'history')<>3 THEN
  RAISE EXCEPTION 'Own request statistics differ: %',stats; END IF;
 stats:=staff_identity.janitor_request_statistics(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,0);
 IF stats->'totals'<>'{"approved":0,"denied":0}'::jsonb OR stats->>'pending'<>'0'
  OR stats->'reviewed'<>'{"approved":2,"denied":1}'::jsonb THEN
  RAISE EXCEPTION 'Own review statistics differ'; END IF;
END $$;
RESET ROLE;
BEGIN;
UPDATE restrictions.requests r SET payload=p.saved,charged_bytes=octet_length(p.saved::text)+1024
 FROM (SELECT id,jsonb_set(payload,'{comment}',to_jsonb(repeat('x',9*1024*1024))) AS saved
  FROM restrictions.requests WHERE post_id IN(207008,207010)) p WHERE r.id=p.id;
UPDATE restrictions.request_capacity SET used_bytes=(SELECT sum(charged_bytes) FROM restrictions.requests);
SET LOCAL ROLE board_auth;
DO $$ BEGIN
 BEGIN
  PERFORM staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,NULL,0);
  RAISE EXCEPTION 'Oversized request page was returned';
 EXCEPTION WHEN SQLSTATE 'P134C' THEN NULL; END;
END $$;
ROLLBACK;
BEGIN;
GRANT EXECUTE ON FUNCTION staff_identity.janitor_request_queue(bytea,bytea,integer,text,integer) TO board_staff;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Queue reader grant drift accepted'; END IF; END $$;
ROLLBACK;
-- Twenty-six eligible rows exercise the real page sentinel and disjoint pages.
BEGIN;
INSERT INTO restrictions.requests(board,post_id,thread_id,actor_hash,automatic_identity,template_id,template_name,
 global,warning,save_policy,account_id,upload_observation,normalized_upload_md5,payload,charged_bytes)
 SELECT r.board,300000+i,r.thread_id,r.actor_hash,r.automatic_identity,r.template_id,r.template_name,
 r.global,r.warning,r.save_policy,r.account_id,r.upload_observation,r.normalized_upload_md5,p.saved,octet_length(p.saved::text)+1024
 FROM restrictions.requests r CROSS JOIN generate_series(1,22) i CROSS JOIN LATERAL (
 SELECT jsonb_set(r.payload,'{post_id}',to_jsonb(300000+i)) AS saved
 ) p WHERE r.post_id=207008;
UPDATE restrictions.request_capacity SET used_bytes=(SELECT sum(charged_bytes) FROM restrictions.requests);
SET LOCAL ROLE board_auth;
DO $$ DECLARE first_page jsonb; second_page jsonb; BEGIN
 first_page:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,NULL,0);
 second_page:=staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,NULL,25);
 IF jsonb_array_length(first_page->'requests')<>25 OR first_page->>'next_offset'<>'25'
  OR jsonb_array_length(second_page->'requests')<>1 OR second_page->>'next_offset'<>'0'
  OR EXISTS(SELECT 1 FROM jsonb_array_elements(first_page->'requests') a,
    jsonb_array_elements(second_page->'requests') b WHERE a->>'id'=b->>'id') THEN
  RAISE EXCEPTION 'Request queue sentinel or page boundaries differ'; END IF;
 IF NOT EXISTS(SELECT 1 FROM jsonb_array_elements(first_page->'counts') c WHERE c->>'board'='test' AND c->>'count'='1') THEN
  RAISE EXCEPTION 'Board count omitted the explicitly selectable test board'; END IF;
 BEGIN
  PERFORM staff_identity.janitor_request_queue(decode(repeat('55',32),'hex'),decode(repeat('77',32),'hex'),600,NULL,100001);
  RAISE EXCEPTION 'Unbounded queue offset accepted';
 EXCEPTION WHEN check_violation THEN NULL; END;
END $$;
ROLLBACK;
