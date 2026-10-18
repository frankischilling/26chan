DO $$ BEGIN
 IF content.report_profile_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Report profile authority is unavailable'; END IF;
 IF has_table_privilege('board_public','post_secrets.report_profile_catalog','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_table_privilege('board_staff','post_secrets.report_profile_catalog','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
 OR has_function_privilege('board_staff','content.admit_report_profiled(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean)','EXECUTE')
 OR has_function_privilege('board_auth','content.admit_categorical_report_profiled(text,bigint,bigint,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean)','EXECUTE')
 OR has_function_privilege('board_public','post_secrets.report_profile_catalog_hash()','EXECUTE') THEN
  RAISE EXCEPTION 'Report profile exposed private authority'; END IF;
END $$;
SELECT set_config('board.fixture_report_before',(SELECT value::text FROM public.owned_report_profile_before),false);
SET ROLE board_report_admission_owner;
DO $$ BEGIN
 IF current_setting('board.fixture_report_before')::jsonb IS DISTINCT FROM
 (SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]') FROM post_secrets.report_weight_evidence e) THEN
  RAISE EXCEPTION 'Report profile changed historical evidence'; END IF;
END $$;
RESET ROLE;
BEGIN;
SET LOCAL ROLE board_report_admission_owner;
GRANT EXECUTE ON FUNCTION content.admit_report_profiled(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint,boolean) TO board_staff;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Report profile ignored runtime grant drift'; END IF; END $$;
ROLLBACK;
