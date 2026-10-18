//! Catalog-only startup contract for the private automatic-range boundary.
pub const READINESS_SQL: &str = r#"
WITH required(schema_name,function_name,owner_name,arguments,result_type,returns_set,definer,allowed) AS (VALUES
 ('content','lock_automatic_range_policy','board_admission_owner','16'::oidvector,'void',false,true,ARRAY['board_restriction_owner']::text[]),
 ('restrictions','record_automatic_range','board_restriction_owner','25 20 20'::oidvector,'void',false,false,ARRAY[]::text[]),
 ('content','automatic_range_events','board_restriction_owner','25 20 17 25'::oidvector,'record',true,true,ARRAY['board_public','board_staff']::text[]),
 ('content','record_automatic_range_hit','board_restriction_owner','20 25 20 17 25'::oidvector,'void',false,true,ARRAY['board_public','board_staff']::text[]),
 ('content','refresh_automatic_range_cookie','board_restriction_owner','20 25 20 17 25 17 17 17 17 16 20'::oidvector,'boolean',false,true,ARRAY['board_public','board_staff']::text[]),
 ('post_secrets','resume_anonymous_refusal','board_anonymous_owner','17 17 17 17 16 20'::oidvector,'void',false,true,ARRAY['board_restriction_owner']::text[])
), runtime AS (
 SELECT oid,rolname FROM pg_catalog.pg_roles WHERE rolname IN
 ('board_public','board_staff','board_auth','board_migrator','board_media','board_media_read','board_media_intake','board_monitor')
)
SELECT current_user IN ('board_public','board_staff') AND content.restrictions_ready()
AND NOT EXISTS (
 SELECT 1 FROM required q WHERE NOT EXISTS (
  SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
  JOIN pg_catalog.pg_roles o ON o.oid=p.proowner JOIN pg_catalog.pg_language l ON l.oid=p.prolang
  WHERE n.nspname=q.schema_name AND p.proname=q.function_name AND o.rolname=q.owner_name
   AND p.proargtypes=q.arguments AND p.prorettype=pg_catalog.to_regtype(q.result_type)
   AND p.proretset=q.returns_set AND p.prosecdef=q.definer AND p.prokind='f' AND p.provolatile='v'
   AND p.pronargdefaults=0 AND p.provariadic=0 AND NOT p.proisstrict AND NOT p.proleakproof
   AND p.proparallel='u' AND l.lanname='plpgsql'
   AND NOT(o.rolcanlogin OR o.rolsuper OR o.rolcreatedb OR o.rolcreaterole OR o.rolreplication OR o.rolbypassrls)
   AND cardinality(p.proconfig)=1 AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
   AND NOT EXISTS(SELECT 1 FROM unnest(q.allowed) a(name)
      WHERE NOT pg_catalog.has_function_privilege(a.name,p.oid,'EXECUTE'))
   AND NOT EXISTS(SELECT 1 FROM runtime r WHERE NOT r.rolname=ANY(q.allowed)
      AND pg_catalog.has_function_privilege(r.oid,p.oid,'EXECUTE'))
   AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
      WHERE a.is_grantable OR (a.grantee<>p.proowner AND NOT EXISTS(
         SELECT 1 FROM pg_catalog.pg_roles g WHERE g.oid=a.grantee AND g.rolname=ANY(q.allowed))))
 ))
AND EXISTS (
 SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
 JOIN pg_catalog.pg_roles o ON o.oid=c.relowner
 WHERE n.nspname='restrictions' AND c.relname='automatic_ranges' AND c.relkind='r'
  AND c.relpersistence='p' AND NOT c.relrowsecurity AND NOT c.relforcerowsecurity
  AND o.rolname='board_restriction_owner'
  AND NOT EXISTS(SELECT 1 FROM runtime r WHERE pg_catalog.has_table_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
   OR EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
      AND pg_catalog.has_column_privilege(r.oid,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES')))
)
AND NOT EXISTS (
 SELECT 1 FROM (VALUES('automatic_browser','text'),('automatic_prefix','bytea')) q(name,kind)
 WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid
  JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='post_secrets' AND c.relname='posting_history' AND a.attname=q.name
   AND a.attnum>0 AND NOT a.attisdropped AND NOT a.attnotnull AND a.atttypid=pg_catalog.to_regtype(q.kind)
   AND a.attidentity='' AND a.attgenerated='' AND NOT a.atthasdef AND NOT a.atthasmissing))
"#;
