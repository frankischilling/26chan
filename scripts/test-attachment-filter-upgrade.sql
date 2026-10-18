DO $$
DECLARE f oid:='content.attachment_upload_filter_metadata(text,text,text,bigint,boolean,boolean)'::regprocedure;
BEGIN
 IF NOT EXISTS(SELECT 1 FROM pg_proc WHERE oid=f AND proowner='board_attachment_owner'::regrole
     AND prosecdef AND proretset AND prorettype='record'::regtype
     AND replace(proconfig[1],' ','')='search_path=pg_catalog,pg_temp' AND cardinality(proconfig)=1
     AND pronargdefaults=0 AND provariadic=0 AND NOT proisstrict) THEN
  RAISE EXCEPTION 'Attachment preflight function properties differ.';
 END IF;
 IF NOT has_function_privilege('board_public',f,'EXECUTE')
    OR NOT has_function_privilege('board_staff',f,'EXECUTE')
    OR EXISTS(SELECT 1 FROM pg_proc p CROSS JOIN LATERAL aclexplode(p.proacl) a WHERE p.oid=f
       AND (a.is_grantable OR (a.grantee<>p.proowner AND a.grantee NOT IN ('board_public'::regrole,'board_staff'::regrole)))) THEN
  RAISE EXCEPTION 'Attachment preflight execution grants differ.';
 END IF;
 IF has_table_privilege('board_attachment_owner','media.input_receipts','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
    OR NOT has_column_privilege('board_attachment_owner','media.input_receipts','asset_id','SELECT')
    OR NOT has_column_privilege('board_attachment_owner','media.input_receipts','component','SELECT')
    OR NOT has_column_privilege('board_attachment_owner','media.input_receipts','md5','SELECT')
    OR has_column_privilege('board_attachment_owner','media.input_receipts','sha256','SELECT')
    OR has_column_privilege('board_attachment_owner','media.input_receipts','bytes','SELECT')
    OR has_any_column_privilege('board_attachment_owner','media.input_receipts','INSERT,UPDATE,REFERENCES') THEN
  RAISE EXCEPTION 'Attachment receipt reader grants differ.';
 END IF;
END $$;
