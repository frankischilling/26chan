DO $$
DECLARE f oid:='admission.record_dmca_warning(bytea,timestamptz)'::regprocedure;
BEGIN
 IF EXISTS(SELECT 1 FROM admission.dmca_warnings)
    OR (SELECT warning_limit FROM admission.file_effect_capacity WHERE singleton) IS DISTINCT FROM 100000 THEN
  RAISE EXCEPTION 'File-warning migration imported state or changed its default capacity'; END IF;
 IF (SELECT snapshot FROM public.owned_dmca_before) IS DISTINCT FROM
 jsonb_build_object(
  'rules',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.rules r),
  'files',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.file_rules r),
  'bans',(SELECT jsonb_agg(to_jsonb(b) ORDER BY peer) FROM admission.bans b),
  'hits',(SELECT jsonb_agg(to_jsonb(h) ORDER BY rule_id,peer) FROM admission.hits h),
  'policy',(SELECT to_jsonb(p) FROM admission.policy p),
  'capacity',(SELECT to_jsonb(c) FROM admission.capacity c)) THEN
  RAISE EXCEPTION 'File-warning upgrade changed existing policy or effects'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc p WHERE oid=f AND proowner='board_admission_owner'::regrole
     AND NOT prosecdef AND NOT proretset AND prorettype='boolean'::regtype AND provolatile='v'
     AND cardinality(proconfig)=2
     AND EXISTS(SELECT 1 FROM unnest(p.proconfig) c WHERE replace(c,' ','')='search_path=pg_catalog,pg_temp')
     AND EXISTS(SELECT 1 FROM unnest(p.proconfig) c WHERE lower(c)='timezone=utc')
     AND pronargdefaults=0 AND provariadic=0 AND NOT proisstrict) THEN
  RAISE EXCEPTION 'File-warning helper properties differ'; END IF;
 IF EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(p.proacl) a WHERE p.oid=f
     AND (a.is_grantable OR a.grantee<>p.proowner)) THEN
  RAISE EXCEPTION 'File-warning helper execution broadened'; END IF;
 IF EXISTS(SELECT 1 FROM pg_roles r CROSS JOIN (VALUES('admission.dmca_warnings'::regclass),('admission.file_effect_capacity'::regclass)) t(id)
    WHERE r.rolname LIKE 'board\_%' ESCAPE '\'
    AND r.rolname NOT IN ('board_admission_owner','board_migrator')
    AND (has_table_privilege(r.oid,t.id,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
      OR has_any_column_privilege(r.oid,t.id,'SELECT,INSERT,UPDATE,REFERENCES')
      OR has_function_privilege(r.oid,f,'EXECUTE'))) THEN
  RAISE EXCEPTION 'Runtime file-warning access broadened'; END IF;
 IF has_column_privilege('board_admission_owner','admission.dmca_warnings','actor_hash','UPDATE')
    OR has_column_privilege('board_admission_owner','admission.file_effect_capacity','warning_limit','UPDATE')
    OR NOT has_column_privilege('board_admission_owner','admission.dmca_warnings','last_warning_at','UPDATE') THEN
  RAISE EXCEPTION 'File-warning owner can change identity or operator capacity'; END IF;
END $$;
