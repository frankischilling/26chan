//! Schema qualification for the sticky reply window's private-state retirement.

/// Reads PostgreSQL catalogs only. Both writer services require this contract
/// before readiness; no private post, password, or anonymous token is selected.
pub const READINESS_SQL: &str = r#"WITH owner_role AS (
    SELECT oid FROM pg_catalog.pg_roles
    WHERE rolname='board_posting_cooldown_owner'
      AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)
), runtime AS (
    SELECT oid FROM pg_catalog.pg_roles
    WHERE rolname IN ('board_public','board_staff','board_auth','board_media',
        'board_media_read','board_media_intake','board_monitor')
), relations AS (
    SELECT c.oid,n.nspname,c.relname
    FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE (n.nspname='post_secrets' AND c.relname IN ('deletion','anonymous_posts'))
       OR (n.nspname='content' AND c.relname IN ('boards','threads','posts'))
)
SELECT (SELECT count(*) FROM runtime)=7
AND NOT EXISTS (
    SELECT 1 FROM runtime CROSS JOIN owner_role
    WHERE pg_has_role(runtime.oid,owner_role.oid,'MEMBER')
)
AND EXISTS (
    SELECT 1 FROM pg_catalog.pg_trigger t
    JOIN relations c ON c.oid=t.tgrelid
    JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid
    JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    JOIN owner_role r ON r.oid=p.proowner
    WHERE c.nspname='content' AND c.relname='posts'
      AND t.tgname='retire_pruned_reply_credentials'
      AND t.tgtype=17 AND t.tgenabled='O' AND NOT t.tgisinternal
      AND t.tgnargs=0 AND t.tgconstraint=0 AND NOT t.tgdeferrable AND NOT t.tginitdeferred
      AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
      AND n.nspname='post_secrets' AND p.proname='retire_pruned_reply_credentials'
      AND p.pronargs=0 AND p.prokind='f' AND p.prorettype='pg_catalog.trigger'::regtype
      AND p.prosecdef AND p.provolatile='v'
      AND has_function_privilege(r.oid,p.oid,'EXECUTE')
      AND EXISTS (SELECT 1 FROM unnest(p.proconfig) AS config(value)
          WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
      AND NOT EXISTS (
          SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
          WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner)
      AND t.tgqual IS NOT NULL
      AND t.tgattr::text=(SELECT a.attnum::text FROM pg_catalog.pg_attribute a
          WHERE a.attrelid=c.oid AND a.attname='deleted' AND NOT a.attisdropped)
      AND translate(split_part(split_part(pg_catalog.pg_get_triggerdef(t.oid,false),' WHEN ',2),' EXECUTE ',1),' ()','')
          ='NOTold.deletedANDnew.deletedANDnew.id<>new.thread_id'
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES
        ('content','boards','slug','SELECT'),
        ('content','boards','slug','UPDATE'),
        ('content','boards','staff_only','SELECT'),
        ('content','boards','reply_limit','SELECT'),
        ('content','threads','id','SELECT'),
        ('content','threads','board','SELECT'),
        ('content','threads','sticky','SELECT'),
        ('content','threads','undead','SELECT'),
        ('content','threads','deleted','SELECT'),
        ('content','threads','archived_at','SELECT'),
        ('content','posts','id','SELECT'),
        ('content','posts','board','SELECT'),
        ('content','posts','thread_id','SELECT'),
        ('content','posts','deleted','SELECT'),
        ('post_secrets','deletion','post_id','SELECT'),
        ('post_secrets','anonymous_posts','post_id','SELECT')
    ) AS required(schema_name,table_name,column_name,privilege_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c CROSS JOIN owner_role r
        JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid
        WHERE c.nspname=required.schema_name AND c.relname=required.table_name
          AND a.attname=required.column_name AND a.attnum>0 AND NOT a.attisdropped
          AND has_column_privilege(r.oid,c.oid,a.attnum,required.privilege_name)
    )
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('deletion'),('anonymous_posts')) AS required(table_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM relations c CROSS JOIN owner_role r
        WHERE c.nspname='post_secrets' AND c.relname=required.table_name
          AND has_table_privilege(r.oid,c.oid,'DELETE')
          AND NOT has_table_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,TRUNCATE,REFERENCES,TRIGGER')
          AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
              WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
                AND ((a.attname<>'post_id' AND has_column_privilege(r.oid,c.oid,a.attnum,'SELECT'))
                    OR has_column_privilege(r.oid,c.oid,a.attnum,'INSERT,UPDATE,REFERENCES')))
          AND NOT EXISTS (SELECT 1 FROM runtime
              WHERE has_table_privilege(runtime.oid,c.oid,'DELETE'))
    )
)
-- Anonymous ownership stays behind its existing proof-bound functions. Even
-- column-only grants must not expose or let runtimes manufacture that proof.
AND NOT EXISTS (
    SELECT 1 FROM relations c CROSS JOIN runtime r
    WHERE c.nspname='post_secrets' AND c.relname='anonymous_posts'
      AND (has_table_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
        OR EXISTS (SELECT 1 FROM pg_catalog.pg_attribute a
            WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped
              AND has_column_privilege(r.oid,c.oid,a.attnum,'SELECT,INSERT,UPDATE,REFERENCES')))
)
AND NOT EXISTS (
    SELECT 1 FROM (VALUES ('content'),('post_secrets')) AS required(schema_name)
    WHERE NOT EXISTS (
        SELECT 1 FROM pg_catalog.pg_namespace n CROSS JOIN owner_role r
        WHERE n.nspname=required.schema_name AND has_schema_privilege(r.oid,n.oid,'USAGE')
          AND NOT has_schema_privilege(r.oid,n.oid,'CREATE')
    )
)"#;
