-- This file runs only in the owned upgrade database created by the shell test.
DO $$ BEGIN
 IF EXISTS(SELECT * FROM content.posts EXCEPT SELECT * FROM public.owned_posts_before)
 OR EXISTS(SELECT * FROM public.owned_posts_before EXCEPT SELECT * FROM content.posts)
 OR EXISTS(SELECT * FROM content.threads EXCEPT SELECT * FROM public.owned_threads_before)
 OR EXISTS(SELECT * FROM public.owned_threads_before EXCEPT SELECT * FROM content.threads)
 OR EXISTS(SELECT * FROM content.boards EXCEPT SELECT * FROM public.owned_boards_before)
 OR EXISTS(SELECT * FROM public.owned_boards_before EXCEPT SELECT * FROM content.boards)
 OR EXISTS(SELECT * FROM content.moderation_audit EXCEPT SELECT * FROM public.owned_audits_before)
 OR EXISTS(SELECT * FROM public.owned_audits_before EXCEPT SELECT * FROM content.moderation_audit)
 THEN RAISE EXCEPTION 'Historical content, profiles, policy, clocks or audit changed'; END IF;
 IF NOT EXISTS(SELECT 1 FROM content.posts WHERE substring(wordfilter_payload FROM 1 FOR 4)=decode('57463031','hex'))
 OR NOT EXISTS(SELECT 1 FROM content.posts WHERE substring(wordfilter_payload FROM 1 FOR 4)=decode('57463032','hex'))
 THEN RAISE EXCEPTION 'Historical typed payload fixtures are missing'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN
 IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Transfer readiness did not survive generated formatting upgrade'; END IF;
END $$;
RESET ROLE;

-- New policy storage is empty until an operator deliberately configures it.
DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM admission.network_rules) THEN
  RAISE EXCEPTION 'Migration installed unexpected network policy'; END IF;
 IF has_table_privilege('board_public','admission.network_rules','SELECT')
 OR has_table_privilege('board_staff','admission.network_rules','UPDATE') THEN
  RAISE EXCEPTION 'Runtime gained direct network policy authority'; END IF;
 IF NOT has_function_privilege('board_public','content.network_admission_rules(text,bigint)','EXECUTE')
 OR NOT has_function_privilege('board_staff','content.network_admission_rules(text,bigint)','EXECUTE')
 OR has_function_privilege('board_media','content.network_admission_rules(text,bigint)','EXECUTE') THEN
  RAISE EXCEPTION 'Network policy reader grant mismatch'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE p.oid='content.network_admission_rules(text,bigint)'::regprocedure
    AND p.prosecdef AND p.proowner='board_admission_owner'::regrole
    AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'])
 OR has_schema_privilege('board_admission_owner','content','CREATE') THEN
  RAISE EXCEPTION 'Network reader ownership or search path differs'; END IF;
END $$;

BEGIN;
UPDATE content.boards SET worksafe=true WHERE slug='demo';
CREATE TEMP TABLE owned_network_revision AS SELECT revision FROM admission.policy WHERE singleton;
INSERT INTO admission.network_rules(boards,range_start,range_end,expires_at,active) VALUES
 ('',0,4294967295,0,true),('demo',0,4294967295,0,true),('_ws_',0,4294967295,0,true),
 ('other',0,4294967295,0,true),('demo',0,4294967295,1000,true),('demo',0,4294967295,0,false);
DO $$ BEGIN
 IF (SELECT revision FROM admission.policy WHERE singleton)<>(SELECT revision+1 FROM owned_network_revision) THEN
  RAISE EXCEPTION 'Network changes did not advance shared generation'; END IF;
END $$;
COMMIT;
