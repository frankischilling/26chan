-- The refusal guard needs a credential key-share lock, not credential mutation.
-- Exercise the allowlist and rolled-back negative controls on an owned database.
CREATE FUNCTION pg_temp.staff_refusal_credential_grants_valid() RETURNS boolean
LANGUAGE sql SET search_path=pg_catalog,pg_temp AS $$
 SELECT NOT has_any_column_privilege('board_staff_post_owner','staff_identity.credentials','INSERT')
 AND NOT has_table_privilege('board_staff_post_owner','staff_identity.credentials','DELETE,TRUNCATE,TRIGGER,REFERENCES')
 AND has_column_privilege('board_staff_post_owner','staff_identity.credentials','id','SELECT')
 AND has_column_privilege('board_staff_post_owner','staff_identity.credentials','id','UPDATE')
 AND has_column_privilege('board_staff_post_owner','staff_identity.credentials','account_id','SELECT')
 AND NOT EXISTS(SELECT 1 FROM information_schema.columns c
  WHERE c.table_schema='staff_identity' AND c.table_name='credentials'
  AND ((c.column_name<>'id' AND has_column_privilege('board_staff_post_owner','staff_identity.credentials',c.column_name,'UPDATE'))
   OR (c.column_name NOT IN('id','account_id') AND has_column_privilege('board_staff_post_owner','staff_identity.credentials',c.column_name,'SELECT'))))
 AND NOT has_any_column_privilege('board_staff','staff_identity.credentials','SELECT,INSERT,UPDATE,REFERENCES')
 AND NOT has_table_privilege('board_staff','staff_identity.credentials','DELETE,TRUNCATE,TRIGGER')
$$;
DO $$ DECLARE mutation text; BEGIN
 IF pg_temp.staff_refusal_credential_grants_valid() IS DISTINCT FROM true THEN
  RAISE EXCEPTION 'Staff refusal credential allowlist differs'; END IF;
 FOREACH mutation IN ARRAY ARRAY[
  'REVOKE UPDATE(id) ON staff_identity.credentials FROM board_staff_post_owner',
  'GRANT UPDATE(credential) ON staff_identity.credentials TO board_staff_post_owner',
  'GRANT UPDATE(account_id) ON staff_identity.credentials TO board_staff_post_owner',
  'GRANT INSERT(id) ON staff_identity.credentials TO board_staff_post_owner',
  'GRANT SELECT(credential) ON staff_identity.credentials TO board_staff_post_owner',
  'GRANT DELETE ON staff_identity.credentials TO board_staff_post_owner',
  'GRANT UPDATE(id) ON staff_identity.credentials TO board_staff',
  'GRANT SELECT(id) ON staff_identity.credentials TO board_staff'
 ] LOOP
  BEGIN
   EXECUTE mutation;
   IF pg_temp.staff_refusal_credential_grants_valid() IS DISTINCT FROM false THEN
    RAISE EXCEPTION 'Credential allowlist accepted mutation: %',mutation; END IF;
   -- Roll back only this fixture mutation, without retaining changed grants.
   RAISE SQLSTATE 'ZX190' USING MESSAGE='Restore owned credential grant fixture';
  EXCEPTION WHEN SQLSTATE 'ZX190' THEN NULL;
  END;
  IF pg_temp.staff_refusal_credential_grants_valid() IS DISTINCT FROM true THEN
   RAISE EXCEPTION 'Credential grant fixture was not restored'; END IF;
 END LOOP;
END $$;
DROP FUNCTION pg_temp.staff_refusal_credential_grants_valid();
