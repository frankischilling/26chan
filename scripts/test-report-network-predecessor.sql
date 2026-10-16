-- Synthetic history in the disposable predecessor database only.
BEGIN;
INSERT INTO admission.network_rules(boards,range_start,range_end,reports_only,created_at,updated_at)
 VALUES('demo',3221225984,3221226239,true,1700000000,1700000100);
INSERT INTO content.reports(board,post_id,reason,created_at)
 VALUES('demo',1000001,'Owned historical report',to_timestamp(1700000000)) RETURNING id AS owned_report \gset
SET LOCAL ROLE board_report_admission_owner;
INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at)
 VALUES(:owned_report,decode(repeat('11',32),'hex'),'demo',1000001,1000001,to_timestamp(1700000000));
INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,known_or_verified,evaluated_at)
 VALUES(:owned_report,1,false,to_timestamp(1700000000));
SELECT jsonb_build_object(
 'members',(SELECT jsonb_agg(to_jsonb(m) ORDER BY report_id) FROM post_secrets.report_membership m),
 'weights',(SELECT jsonb_agg(to_jsonb(w) ORDER BY report_id) FROM post_secrets.report_weight_evidence w),
 'groups',(SELECT jsonb_agg(to_jsonb(g) ORDER BY board,post_id) FROM post_secrets.report_group g))::text AS owned_private_before \gset
RESET ROLE;
CREATE TABLE public.owned_report_private_before AS SELECT :'owned_private_before'::jsonb AS snapshot;
CREATE TABLE public.owned_reports_before AS SELECT * FROM content.reports;
CREATE TABLE public.owned_network_before AS SELECT * FROM admission.network_rules;
CREATE TABLE public.owned_report_boards_before AS SELECT * FROM content.boards;
CREATE TABLE public.owned_report_posts_before AS SELECT * FROM content.posts;
CREATE TABLE public.owned_report_threads_before AS SELECT * FROM content.threads;
COMMIT;
