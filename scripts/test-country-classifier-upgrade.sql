DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM public.owned_classifier_before b JOIN post_secrets.anonymous_sessions s ON s.token_hash=b.token
 WHERE b.state IS DISTINCT FROM to_jsonb(s)
 OR b.history IS DISTINCT FROM (SELECT jsonb_agg(to_jsonb(h) ORDER BY kind) FROM post_secrets.classifier_history h WHERE h.token_hash=b.token)) THEN
  RAISE EXCEPTION 'Country bridge changed retained anonymous state or events'; END IF;
 IF NOT has_function_privilege('board_public','content.posting_classifier_history(text,bytea,boolean)','EXECUTE')
 OR NOT has_function_privilege('board_public','content.record_country_classifier_refusal(text,bytea,bytea,bytea,bytea,boolean,bigint)','EXECUTE')
 OR has_function_privilege('board_staff','content.posting_classifier_history(text,bytea,boolean)','EXECUTE')
 OR has_function_privilege('board_staff','content.record_country_classifier_refusal(text,bytea,bytea,bytea,bytea,boolean,bigint)','EXECUTE') THEN
  RAISE EXCEPTION 'Country bridge grants differ from the runtime contract'; END IF;
END $$;
\ir test-classifier-history-upgrade.sql
