//! Catalog checks complement the existing definition hash seals.
pub const READINESS_SQL: &str = r#"
WITH expected(signature, grantees, owner_name) AS (VALUES
 ('content.comment_duplicate_remaining(bytea,text,bytea,bigint)', ARRAY['board_public','board_staff']::text[], 'board_posting_cooldown_owner'),
 ('content.record_comment_projection(bigint,bytea,bytea)', ARRAY['board_public','board_staff']::text[], 'board_posting_cooldown_owner'),
 ('content.expire_comment_history()', ARRAY['board_moderation_retention']::text[], 'board_posting_cooldown_owner'),
 ('content.remove_comment_history()', ARRAY[]::text[], 'board_posting_cooldown_owner'),
 ('content.remove_thread_comment_history()', ARRAY[]::text[], 'board_posting_cooldown_owner'),
 ('content.revalidate_staff_quota_refusal(bytea,bytea,integer,text)', ARRAY['board_staff']::text[], 'board_staff_post_owner')
)
SELECT
 NOT EXISTS (SELECT 1 FROM expected e LEFT JOIN pg_proc p ON p.oid=to_regprocedure(e.signature)
 WHERE p.oid IS NULL OR NOT p.prosecdef OR p.proowner<>e.owner_name::regrole
 OR p.provolatile<>'v' OR p.prolang<>(SELECT oid FROM pg_language WHERE lanname='plpgsql')
 OR p.proconfig IS DISTINCT FROM ARRAY['search_path=pg_catalog, pg_temp']::text[]
 OR EXISTS (SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a
   WHERE a.privilege_type='EXECUTE' AND (a.grantee=0 OR
    (pg_get_userbyid(a.grantee)::text<>ALL(e.grantees || ARRAY[e.owner_name,'board_migrator']))))
 OR EXISTS (SELECT 1 FROM unnest(e.grantees) g WHERE NOT has_function_privilege(g,p.oid,'EXECUTE')))
 AND EXISTS (SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
  WHERE n.nspname='post_secrets' AND c.relname='comment_history'
  AND c.relowner='board_migrator'::regrole AND c.relkind='r' AND c.relpersistence='p'
  AND NOT EXISTS (SELECT 1 FROM pg_roles r WHERE r.rolname NOT IN('board_migrator','board_posting_cooldown_owner')
    AND left(r.rolname,3)<>'pg_' AND NOT r.rolsuper AND (has_table_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')
     OR has_any_column_privilege(r.oid,c.oid,'SELECT,INSERT,UPDATE,REFERENCES'))))
"#;
