DO $$ BEGIN
 IF content.report_profile_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Automatic clear profile is unavailable'; END IF;
 IF EXISTS(SELECT value FROM public.owned_auto_clear_reports EXCEPT ALL SELECT to_jsonb(r)-'auto_cleared_at' FROM content.reports r)
 OR EXISTS(SELECT to_jsonb(r)-'auto_cleared_at' FROM content.reports r EXCEPT ALL SELECT value FROM public.owned_auto_clear_reports) THEN
 RAISE EXCEPTION 'Automatic clear migration changed retained reports'; END IF;
 IF NOT has_function_privilege('board_restriction_owner','post_secrets.auto_clear_post_reports(text,bigint)','EXECUTE')
 OR EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth','board_migrator')
 AND has_function_privilege(r.oid,'post_secrets.auto_clear_post_reports(text,bigint)','EXECUTE')) THEN
 RAISE EXCEPTION 'Automatic clear grants differ from their private boundary'; END IF;
END $$;
SELECT set_config('board.fixture_auto_clear_evidence',(SELECT value::text FROM public.owned_auto_clear_evidence),false);
SET ROLE board_report_admission_owner;
DO $$ BEGIN IF current_setting('board.fixture_auto_clear_evidence')::jsonb IS DISTINCT FROM
 (SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]') FROM post_secrets.report_weight_evidence e) THEN
 RAISE EXCEPTION 'Automatic clear migration changed retained weight evidence'; END IF; END $$;
RESET ROLE;
BEGIN;
SET LOCAL ROLE board_report_admission_owner;
GRANT EXECUTE ON FUNCTION post_secrets.auto_clear_post_reports(text,bigint) TO board_staff;
DO $$ BEGIN IF content.report_profile_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Automatic clear ignored runtime grant drift'; END IF; END $$;
ROLLBACK;
