\ir test-country-classifier-upgrade.sql
DO $$ BEGIN
 IF has_table_privilege('board_public','post_secrets.classifier_events','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_table_privilege('board_staff','post_secrets.classifier_events','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_table_privilege('board_moderation_retention','post_secrets.classifier_events','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_sequence_privilege('board_staff','post_secrets.classifier_events_id_seq','USAGE,SELECT,UPDATE')
 OR has_function_privilege('board_staff','content.record_posting_classifier_events(text,bigint,text[],text,double precision)','EXECUTE')
 OR has_function_privilege('board_staff','content.expire_classifier_events()','EXECUTE')
 OR has_function_privilege('board_public','post_secrets.prune_classifier_events()','EXECUTE')
 OR has_function_privilege('board_public','content.expire_classifier_events()','EXECUTE')
 OR NOT has_function_privilege('board_public','content.record_posting_classifier_events(text,bigint,text[],text,double precision)','EXECUTE')
 OR NOT has_function_privilege('board_moderation_retention','content.expire_classifier_events()','EXECUTE') THEN
  RAISE EXCEPTION 'Classifier diagnostic grants differ from their narrow contract'; END IF;
 IF (SELECT rows FROM public.owned_diagnostics_before) IS DISTINCT FROM
  (SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM post_secrets.classifier_events e) THEN
  RAISE EXCEPTION 'Retained diagnostic rows changed'; END IF;
END $$;
