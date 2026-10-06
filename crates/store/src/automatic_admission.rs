//! Catalog-only deployment checks for automatic-session admission authority.
//! This verifies the API and privilege boundary, not function-body behavior or
//! historical row provenance. Behavioral tests cover those separately.

pub const READINESS_SQL: &str = r#"WITH owners AS (
    SELECT r.oid,r.rolname FROM pg_catalog.pg_roles r
    WHERE r.rolname IN ('board_anonymous_owner','board_posting_cooldown_owner','board_report_admission_owner')
      AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls)
), runtime AS (
    SELECT r.oid,r.rolname FROM pg_catalog.pg_roles r
    WHERE r.rolname IN ('board_public','board_staff','board_auth')
), relations AS (
    SELECT c.oid,c.relname FROM pg_catalog.pg_class c
    JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname='post_secrets' AND c.relkind='r'
      AND c.relname IN ('anonymous_sessions','posting_history','report_membership')
), required_functions(schema_name,function_name,argument_types,owner_name,result_type,allowed_roles) AS (
    VALUES
    ('post_secrets','resolve_automatic_identity','17 16 20 16','board_anonymous_owner',
        'TABLE(automatic_identity uuid, source_new boolean)',
        ARRAY['board_posting_cooldown_owner','board_report_admission_owner']::text[]),
    ('post_secrets','lookup_automatic_identity','17 20','board_anonymous_owner',
        'TABLE(automatic_identity uuid, source_new boolean)',
        ARRAY['board_report_admission_owner']::text[]),
    ('content','register_anonymous_post','17 17 17 17 16 25 20 20','board_anonymous_owner',
        'void',ARRAY['board_public']::text[]),
    ('content','register_anonymous_report','17 17 17 17 16 25 20 20','board_anonymous_owner',
        'void',ARRAY['board_public']::text[]),
    ('content','check_user_thread_quota','17 25 20','board_posting_cooldown_owner',
        'TABLE(rejected boolean, user_thread_limit integer, user_thread_period_hours integer)',
        ARRAY['board_public','board_staff']::text[]),
    ('content','check_user_thread_quota','17 25 20 17 16 20','board_posting_cooldown_owner',
        'TABLE(rejected boolean, user_thread_limit integer, user_thread_period_hours integer)',
        ARRAY['board_public','board_staff']::text[])
)
SELECT (SELECT count(*) FROM owners)=3
AND (SELECT count(*) FROM runtime)=3
AND (SELECT count(*) FROM relations)=3
AND NOT EXISTS (
    SELECT 1 FROM runtime CROSS JOIN owners
    WHERE pg_has_role(runtime.oid,owners.oid,'MEMBER')
)
AND NOT EXISTS (
    SELECT 1 FROM required_functions required
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_proc p
        JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
        JOIN owners o ON o.oid=p.proowner AND o.rolname=required.owner_name
        -- Built-in OID vectors do not need USAGE on the private schema.
        WHERE n.nspname=required.schema_name AND p.proname=required.function_name
          AND p.proargtypes=required.argument_types::oidvector
          AND p.prokind='f' AND p.prosecdef AND p.provolatile='v'
          AND p.pronargdefaults=0 AND p.provariadic=0
          AND pg_catalog.pg_get_function_result(p.oid)=required.result_type
          AND cardinality(p.proconfig)=1
          AND replace(p.proconfig[1],' ','')='search_path=pg_catalog,pg_temp'
          AND has_function_privilege(o.oid,p.oid,'EXECUTE')
          AND NOT EXISTS (
              SELECT 1 FROM unnest(required.allowed_roles) allowed(role_name)
              WHERE NOT EXISTS (SELECT 1 FROM pg_catalog.pg_roles grantee
                  WHERE grantee.rolname=allowed.role_name
                    AND has_function_privilege(grantee.oid,p.oid,'EXECUTE'))
          )
          AND NOT EXISTS (
              SELECT 1 FROM runtime
              WHERE NOT (runtime.rolname=ANY(required.allowed_roles))
                AND has_function_privilege(runtime.oid,p.oid,'EXECUTE')
          )
          AND NOT EXISTS (
              SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
              WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner
                AND (a.is_grantable OR NOT EXISTS (
                    SELECT 1 FROM pg_catalog.pg_roles grantee
                    WHERE grantee.oid=a.grantee AND grantee.rolname=ANY(required.allowed_roles)))
          )
    )
)
AND NOT EXISTS (
    SELECT 1 FROM relations c
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_attribute a
        WHERE a.attrelid=c.oid AND a.attname='automatic_identity'
          AND a.attnum>0 AND NOT a.attisdropped AND NOT a.attnotnull
          AND a.atttypid='pg_catalog.uuid'::regtype
          AND a.attidentity='' AND a.attgenerated='' AND NOT a.atthasdef AND NOT a.atthasmissing
    )
)
AND EXISTS (
    SELECT 1 FROM relations c
    JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attname='automatic_identity'
    JOIN pg_catalog.pg_index i ON i.indrelid=c.oid
    WHERE c.relname='anonymous_sessions' AND a.attnum>0 AND NOT a.attisdropped
      AND i.indisunique AND i.indisvalid AND i.indisready AND i.indislive AND i.indimmediate
      AND i.indnkeyatts=1 AND i.indnatts=1 AND i.indkey[0]=a.attnum
      AND i.indpred IS NULL AND i.indexprs IS NULL
)
AND NOT EXISTS (
    SELECT 1 FROM relations c WHERE c.relname IN ('posting_history','report_membership')
    AND NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_attribute a
        JOIN pg_catalog.pg_attrdef d ON d.adrelid=a.attrelid AND d.adnum=a.attnum
        WHERE a.attrelid=c.oid AND a.attname='registration_xid'
          AND a.attnum>0 AND NOT a.attisdropped AND NOT a.attnotnull
          AND a.atttypid='pg_catalog.xid8'::regtype
          AND a.attidentity='' AND a.attgenerated='' AND NOT a.atthasmissing
          AND pg_catalog.pg_get_expr(d.adbin,d.adrelid) IN ('pg_current_xact_id()','pg_catalog.pg_current_xact_id()')
    )
)
-- Runtime roles may neither read nor choose equality or insert provenance.
-- Denying all direct private relation/column access also protects old evidence.
AND NOT EXISTS (
    SELECT 1 FROM relations c CROSS JOIN runtime r
    WHERE has_table_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
       OR EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
           WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
             AND has_column_privilege(r.oid,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES'))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('anonymous_sessions','automatic_identity','SELECT'),
        ('anonymous_sessions','automatic_identity','UPDATE'),
        ('posting_history','post_id','SELECT'),('posting_history','board','SELECT'),
        ('posting_history','thread_id','SELECT'),('posting_history','automatic_identity','SELECT'),
        ('posting_history','registration_xid','SELECT'),('posting_history','automatic_identity','UPDATE'),
        ('report_membership','report_id','SELECT'),('report_membership','board','SELECT'),
        ('report_membership','post_id','SELECT'),('report_membership','automatic_identity','SELECT'),
        ('report_membership','registration_xid','SELECT'),('report_membership','automatic_identity','UPDATE')
    ) required(table_name,column_name,privilege_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c
        JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
        CROSS JOIN owners o
        WHERE c.relname=required.table_name AND a.attname=required.column_name
          AND a.attnum>0 AND NOT a.attisdropped AND o.rolname='board_anonymous_owner'
          AND has_column_privilege(o.oid,c.oid,a.attnum,required.privilege_name)
    )
)
-- Registration can claim only equality; its owner cannot stamp provenance or
-- manufacture history/membership rows, including through table-wide grants.
AND NOT EXISTS (
    SELECT 1 FROM relations c CROSS JOIN owners o
    WHERE c.relname IN ('posting_history','report_membership') AND o.rolname='board_anonymous_owner'
      AND (has_table_privilege(o.oid,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        OR EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
            WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
              AND (has_column_privilege(o.oid,c.oid,a.attnum,'INSERT,REFERENCES')
                OR (a.attname<>'automatic_identity' AND has_column_privilege(o.oid,c.oid,a.attnum,'UPDATE'))
                OR (NOT (a.attname=ANY(CASE WHEN c.relname='posting_history'
                    THEN ARRAY['post_id','board','thread_id','automatic_identity','registration_xid']::text[]
                    ELSE ARRAY['report_id','board','post_id','automatic_identity','registration_xid']::text[] END))
                    AND has_column_privilege(o.oid,c.oid,a.attnum,'SELECT')))))
)
AND EXISTS (
    SELECT 1 FROM relations c JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
    CROSS JOIN owners o
    WHERE c.relname='posting_history' AND a.attname='automatic_identity'
      AND a.attnum>0 AND NOT a.attisdropped AND o.rolname='board_posting_cooldown_owner'
      AND has_column_privilege(o.oid,c.oid,a.attnum,'SELECT')
)"#;
