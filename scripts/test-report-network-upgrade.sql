SET ROLE board_report_admission_owner;
SELECT jsonb_build_object(
 'members',(SELECT jsonb_agg(to_jsonb(m) ORDER BY report_id) FROM post_secrets.report_membership m),
 'weights',(SELECT jsonb_agg(to_jsonb(w) ORDER BY report_id) FROM post_secrets.report_weight_evidence w),
 'groups',(SELECT jsonb_agg(to_jsonb(g) ORDER BY board,post_id) FROM post_secrets.report_group g))::text AS owned_private_after \gset
RESET ROLE;
SELECT snapshot=:'owned_private_after'::jsonb AS owned_private_equal FROM public.owned_report_private_before \gset
\if :owned_private_equal
\else
    \echo 'Report private history changed during upgrade'
    \quit 1
\endif
DO $$ DECLARE pair text[]; changed boolean; BEGIN
 FOREACH pair SLICE 1 IN ARRAY ARRAY[
  ['content.reports','public.owned_reports_before'],
  ['admission.network_rules','public.owned_network_before'],
  ['content.boards','public.owned_report_boards_before'],
  ['content.posts','public.owned_report_posts_before'],
  ['content.threads','public.owned_report_threads_before']
 ] LOOP
  EXECUTE format('SELECT EXISTS(TABLE %s EXCEPT TABLE %s) OR EXISTS(TABLE %s EXCEPT TABLE %s)',pair[1],pair[2],pair[2],pair[1]) INTO changed;
  IF changed THEN RAISE EXCEPTION 'Report network migration changed retained data'; END IF;
 END LOOP;
END $$;
SET ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report network restriction seal is invalid'; END IF; END $$;
RESET ROLE;
SET ROLE board_report_snapshot_owner;
DO $$ BEGIN IF content.report_snapshots_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report network migration changed snapshot authority'; END IF; END $$;
RESET ROLE;
SET ROLE board_thread_transfer_owner;
DO $$ BEGIN IF content.text_thread_transfer_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Report network migration changed transfer authority'; END IF; END $$;
RESET ROLE;
DO $$ DECLARE f regprocedure; BEGIN
 FOREACH f IN ARRAY ARRAY[
  'content.lock_report_network_policy()'::regprocedure,
  'content.report_network_rules(text,bigint,boolean)'::regprocedure,
  'post_secrets.report_network_activity(bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure,
  'post_secrets.assert_report_network(text,bigint,bytea,bytea,bytea,bytea,bytea,boolean,bigint)'::regprocedure
 ] LOOP
  IF NOT has_function_privilege('board_report_admission_owner',f,'EXECUTE')
  OR EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth','board_migrator','board_media','board_media_read','board_media_intake')
      AND has_function_privilege(r.oid,f,'EXECUTE')) THEN
   RAISE EXCEPTION 'Report network helper authority differs'; END IF;
 END LOOP;
END $$;
