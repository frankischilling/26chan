DO $$
DECLARE f oid:='admission.automatic_ban_exempt(inet)'::regprocedure;
BEGIN
 IF EXISTS(SELECT 1 FROM admission.automatic_ban_exemptions) THEN
  RAISE EXCEPTION 'Automatic-ban migration imported exemptions'; END IF;
 IF (SELECT snapshot FROM public.owned_ban_exemptions_before) IS DISTINCT FROM
 jsonb_build_object(
  'rules',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.rules r),
  'files',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM admission.file_rules r),
  'bans',(SELECT jsonb_agg(to_jsonb(b) ORDER BY peer) FROM admission.bans b),
  'hits',(SELECT jsonb_agg(to_jsonb(h) ORDER BY rule_id,peer) FROM admission.hits h),
  'policy',(SELECT to_jsonb(p) FROM admission.policy p),
  'capacity',(SELECT to_jsonb(c) FROM admission.capacity c)) THEN
  RAISE EXCEPTION 'Automatic-ban upgrade changed existing policy or effects'; END IF;
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=f AND proowner='board_admission_owner'::regrole
     AND NOT prosecdef AND NOT proretset AND prorettype='boolean'::regtype
     AND replace(proconfig[1],' ','')='search_path=pg_catalog,pg_temp' AND cardinality(proconfig)=1
     AND pronargdefaults=0 AND provariadic=0 AND NOT proisstrict) THEN
  RAISE EXCEPTION 'Automatic-ban helper properties differ'; END IF;
 IF EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(p.proacl) a WHERE p.oid=f
     AND (a.is_grantable OR a.grantee<>p.proowner)) THEN
  RAISE EXCEPTION 'Automatic-ban helper execution broadened'; END IF;
 IF EXISTS(SELECT 1 FROM pg_roles r WHERE r.rolname IN
    ('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor')
    AND (has_table_privilege(r.oid,'admission.automatic_ban_exemptions','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
      OR has_any_column_privilege(r.oid,'admission.automatic_ban_exemptions','SELECT,INSERT,UPDATE,REFERENCES')
      OR has_function_privilege(r.oid,f,'EXECUTE'))) THEN
  RAISE EXCEPTION 'Runtime automatic-ban exemption access broadened'; END IF;
 IF NOT has_table_privilege('board_admission_owner','admission.automatic_ban_exemptions','SELECT')
    OR has_table_privilege('board_admission_owner','admission.automatic_ban_exemptions','INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') THEN
  RAISE EXCEPTION 'Automatic-ban helper can mutate operator data'; END IF;
END $$;
