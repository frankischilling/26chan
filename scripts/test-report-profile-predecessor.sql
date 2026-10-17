-- Historical v1 evidence must stay byte-for-byte unchanged through the upgrade.
CREATE TABLE public.owned_report_profile_ids(id bigint PRIMARY KEY);
WITH inserted AS (
 INSERT INTO content.reports(board,post_id,reason,state)
 SELECT board,id,'Owned pre-profile report','open' FROM content.posts ORDER BY id LIMIT 1 RETURNING id
) INSERT INTO public.owned_report_profile_ids SELECT id FROM inserted;
SET ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,evaluated_at)
 SELECT max(id),1,'2020-01-01 UTC' FROM content.reports;
RESET ROLE;
SET ROLE board_report_admission_owner;
SELECT set_config('board.fixture_report_before',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]')::text FROM post_secrets.report_weight_evidence e),false);
RESET ROLE;
CREATE TABLE public.owned_report_profile_before AS SELECT current_setting('board.fixture_report_before')::jsonb AS value;
ALTER DEFAULT PRIVILEGES FOR ROLE board_migrator IN SCHEMA post_secrets GRANT SELECT,INSERT ON TABLES TO board_staff;
SET ROLE board_report_admission_owner;
ALTER DEFAULT PRIVILEGES FOR ROLE board_report_admission_owner IN SCHEMA content GRANT EXECUTE ON FUNCTIONS TO board_staff,board_auth;
ALTER DEFAULT PRIVILEGES FOR ROLE board_report_admission_owner IN SCHEMA post_secrets GRANT EXECUTE ON FUNCTIONS TO board_staff,board_auth;
RESET ROLE;
