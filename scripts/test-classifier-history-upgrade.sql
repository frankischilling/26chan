SET ROLE board_anonymous_owner;
DO $$
DECLARE token bytea; original jsonb; current_state jsonb; h record;
BEGIN
 SELECT b.token,b.state INTO token,original FROM public.owned_classifier_before b;
 SELECT to_jsonb(s) INTO current_state FROM post_secrets.anonymous_sessions s WHERE token_hash=token;
 IF original IS DISTINCT FROM current_state THEN RAISE EXCEPTION 'Upgrade changed session state'; END IF;
 IF has_table_privilege('board_public','post_secrets.classifier_history','SELECT')
 OR has_function_privilege('board_public','post_secrets.read_classifier_history(bytea,boolean)','EXECUTE') THEN
  RAISE EXCEPTION 'Default privileges exposed private history'; END IF;
 IF post_secrets.classifier_history_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'History catalog unavailable'; END IF;
 SELECT * INTO h FROM post_secrets.read_classifier_history(token,false);
 IF NOT h.edge_blocked OR h.country_blocked THEN RAISE EXCEPTION 'History event was lost or widened'; END IF;
END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'History upgrade changed transfer authority'; END IF; END $$;
RESET ROLE;
SET ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'History upgrade changed restriction authority'; END IF; END $$;
RESET ROLE;
