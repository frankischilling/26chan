INSERT INTO content.reports(board,post_id,reason,state)
 SELECT board,id,'Owned pre-auto-clear report','open' FROM content.posts ORDER BY id LIMIT 1;
SET ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,evaluated_at)
 SELECT max(id),1,'2020-01-01 UTC' FROM content.reports;
SELECT set_config('board.fixture_auto_clear_evidence',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]')::text FROM post_secrets.report_weight_evidence e),false);
RESET ROLE;
CREATE TABLE public.owned_auto_clear_evidence AS SELECT current_setting('board.fixture_auto_clear_evidence')::jsonb AS value;
UPDATE content.reports SET group_cleared_at=to_timestamp(1600000000),group_cleared_by=7123,group_clear_inherited=false
 WHERE reason='Owned pre-auto-clear report';
CREATE TABLE public.owned_auto_clear_reports AS SELECT to_jsonb(r) AS value FROM content.reports r;
CREATE TABLE public.owned_auto_clear_catalog AS SELECT definition_hash FROM post_secrets.report_profile_catalog WHERE singleton;
SET ROLE board_report_admission_owner;
ALTER DEFAULT PRIVILEGES IN SCHEMA post_secrets GRANT EXECUTE ON FUNCTIONS TO board_staff,board_auth;
RESET ROLE;
