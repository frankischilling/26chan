DO $$ BEGIN
 IF content.post_notices_ready() IS DISTINCT FROM true OR content.report_profile_ready() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Post notice readiness failed'; END IF;
 IF EXISTS(SELECT value FROM public.owned_notice_posts EXCEPT ALL SELECT to_jsonb(p)-'staff_notices' FROM content.posts p)
 OR EXISTS(SELECT to_jsonb(p)-'staff_notices' FROM content.posts p EXCEPT ALL SELECT value FROM public.owned_notice_posts)
 OR EXISTS(SELECT 1 FROM content.posts WHERE staff_notices<>'{}'::text[]) THEN
  RAISE EXCEPTION 'Post notice upgrade changed retained content'; END IF;
 IF EXISTS(SELECT value FROM public.owned_notice_reports EXCEPT ALL SELECT to_jsonb(r) FROM content.reports r)
 OR EXISTS(SELECT to_jsonb(r) FROM content.reports r EXCEPT ALL SELECT value FROM public.owned_notice_reports) THEN
  RAISE EXCEPTION 'Post notice upgrade changed retained reports'; END IF;
 IF NOT has_function_privilege('board_staff','content.issue_post_restriction_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text)','EXECUTE')
 OR EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_auth') AND
 has_function_privilege(r.oid,'content.issue_post_restriction_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text)','EXECUTE'))
 OR EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth') AND
 has_function_privilege(r.oid,'restrictions.issue_post_restriction_with_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text)','EXECUTE')) THEN
  RAISE EXCEPTION 'Post notice issuer grants are not private'; END IF;
END $$;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Transfer seal drift'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN RAISE EXCEPTION 'Report snapshot seal drift'; END IF; END $$;
RESET ROLE;
BEGIN;
SET LOCAL ROLE board_restriction_owner;
GRANT EXECUTE ON FUNCTION restrictions.issue_post_restriction_with_notice(bytea,bytea,integer,text,bigint,boolean,boolean,bigint,text,text,boolean,text) TO board_staff;
DO $$ BEGIN IF content.post_notices_ready() IS DISTINCT FROM false THEN RAISE EXCEPTION 'Notice issuer ignored grant drift'; END IF; END $$;
ROLLBACK;
