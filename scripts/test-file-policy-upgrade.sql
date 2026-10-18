DO $$
DECLARE f oid:='content.file_admission_rules(text)'::regprocedure;
BEGIN
 IF EXISTS(SELECT 1 FROM admission.file_rules) THEN
  RAISE EXCEPTION 'File policy migration imported a hash list'; END IF;
 IF (SELECT snapshot FROM public.owned_file_policy_before) IS DISTINCT FROM
 jsonb_build_object(
  'rules',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.rules r),
  'network',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.network_rules r),
  'policy',(SELECT to_jsonb(p) FROM admission.policy p),
  'capacity',(SELECT to_jsonb(c) FROM admission.capacity c)) THEN
  RAISE EXCEPTION 'File policy upgrade changed existing admission state'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=f AND proowner='board_admission_owner'::regrole
     AND prosecdef AND proretset AND prorettype='record'::regtype
     AND replace(proconfig[1],' ','')='search_path=pg_catalog,pg_temp' AND cardinality(proconfig)=1
     AND pronargdefaults=0 AND provariadic=0 AND NOT proisstrict) THEN
  RAISE EXCEPTION 'File policy reader properties differ'; END IF;
 IF NOT has_function_privilege('board_public',f,'EXECUTE')
    OR NOT has_function_privilege('board_staff',f,'EXECUTE')
    OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(p.proacl) a WHERE p.oid=f
       AND (a.is_grantable OR (a.grantee<>p.proowner AND a.grantee NOT IN ('board_public'::regrole,'board_staff'::regrole)))) THEN
  RAISE EXCEPTION 'File policy execution grants differ'; END IF;
 IF EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN
    ('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor')
    AND (has_table_privilege(r.oid,'admission.file_rules','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
      OR has_any_column_privilege(r.oid,'admission.file_rules','SELECT,INSERT,UPDATE,REFERENCES')
      OR has_sequence_privilege(r.oid,'admission.file_rules_id_seq','USAGE,SELECT,UPDATE'))) THEN
  RAISE EXCEPTION 'Runtime file policy access broadened'; END IF;
 IF NOT has_table_privilege('board_admission_owner','admission.file_rules','SELECT')
    OR has_table_privilege('board_admission_owner','admission.file_rules','INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
    OR has_sequence_privilege('board_admission_owner','admission.file_rules_id_seq','USAGE,SELECT,UPDATE') THEN
  RAISE EXCEPTION 'File policy reader can mutate operator data'; END IF;
END $$;
