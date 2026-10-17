DO $$ BEGIN
 IF (SELECT snapshot FROM public.owned_range_cookie_before) IS DISTINCT FROM jsonb_build_object(
 'sessions',(SELECT jsonb_agg(to_jsonb(s) ORDER BY token_hash) FROM post_secrets.anonymous_sessions s),
 'policy',(SELECT to_jsonb(p) FROM post_secrets.anonymous_policy p),
 'table_acl',(SELECT relacl::text FROM pg_class WHERE oid='post_secrets.anonymous_sessions'::regclass)) THEN
  RAISE EXCEPTION 'Range-cookie upgrade changed existing state or table grants'; END IF;
 IF EXISTS(SELECT 1 FROM (VALUES
  ('post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint)','board_anonymous_owner','void',ARRAY['board_restriction_owner']),
  ('content.refresh_automatic_range_cookie(bigint,text,bigint,bytea,text,bytea,bytea,bytea,bytea,boolean,bigint)','board_restriction_owner','boolean',ARRAY['board_public','board_staff'])
 ) q(signature,owner_name,result_type,allowed) WHERE NOT EXISTS (
  SELECT 1 FROM pg_proc p WHERE p.oid=to_regprocedure(q.signature) AND p.proowner=q.owner_name::regrole
   AND p.prosecdef AND NOT p.proretset AND p.prorettype=to_regtype(q.result_type)
   AND p.provolatile='v' AND p.proparallel='u' AND NOT p.proisstrict
   AND p.pronargdefaults=0 AND p.provariadic=0
   AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
   AND NOT EXISTS(SELECT 1 FROM unnest(q.allowed) a(name) WHERE NOT has_function_privilege(a.name,p.oid,'EXECUTE'))
   AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.is_grantable
    OR (a.grantee<>p.proowner AND NOT EXISTS(SELECT 1 FROM pg_roles r WHERE r.oid=a.grantee AND r.rolname=ANY(q.allowed))))
 )) THEN RAISE EXCEPTION 'Range-cookie function authority differs'; END IF;
END $$;
BEGIN;
SET LOCAL ROLE board_anonymous_owner;
GRANT EXECUTE ON FUNCTION post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint) TO board_public;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM false THEN
 RAISE EXCEPTION 'Range-cookie seal accepted an unscoped runtime resume grant'; END IF; END $$;
ROLLBACK;
BEGIN;
SET LOCAL ROLE board_restriction_owner;
DO $$ BEGIN IF content.restrictions_ready() IS DISTINCT FROM true THEN
 RAISE EXCEPTION 'Range-cookie rollback did not restore readiness'; END IF; END $$;
ROLLBACK;
