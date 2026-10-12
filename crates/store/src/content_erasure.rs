//! Catalog-only verification of fresh whole-content erasure. Never reads payload
//! or private author evidence and never invokes an erasure function.
//! A false result or query error means unavailable. PostgreSQL can reject a
//! revoked helper EXECUTE privilege before evaluating the CASE condition.
pub const READINESS_SQL: &str = r#"WITH relations AS (
    SELECT c.oid,n.nspname||'.'||c.relname AS table_name
    FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
    WHERE n.nspname IN ('content','post_secrets','staff_identity')
), owner_role AS (
    SELECT oid FROM pg_catalog.pg_roles WHERE rolname='board_content_erasure_owner'
      AND NOT (rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls)
), functions AS (
    SELECT p.* FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='post_secrets' AND p.proname IN (
        'guard_post_erasure','guard_thread_erasure','erase_thread_descendants','guard_erased_author_link',
        'guard_erasure_visibility_statement')
), triggers AS (
    SELECT t.*,n.nspname,c.relname FROM pg_catalog.pg_trigger t
    JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid
    JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
)
SELECT EXISTS(SELECT 1 FROM owner_role)
AND (SELECT count(*)=5 FROM functions)
AND NOT EXISTS(SELECT 1 FROM functions p WHERE
    p.proowner NOT IN(SELECT oid FROM owner_role) OR NOT p.prosecdef OR p.provolatile<>'v'
    OR p.pronargs<>0 OR p.prokind<>'f' OR p.prorettype<>'pg_catalog.trigger'::regtype
    OR NOT EXISTS(SELECT 1 FROM unnest(p.proconfig) config(value)
        WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
    OR EXISTS(SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
        WHERE a.privilege_type='EXECUTE' AND a.grantee<>p.proowner)
)
AND NOT EXISTS(SELECT 1 FROM (VALUES
    ('content','posts','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('content','threads','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('post_secrets','deletion','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('post_secrets','anonymous_posts','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('post_secrets','op_peers','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('post_secrets','op_replies','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('post_secrets','poster_contexts','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('post_secrets','posting_history','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('staff_identity','discussion_posts','guard_erasure_visibility','guard_erasure_visibility_statement',22,false),
    ('content','posts','z_guard_post_erasure','guard_post_erasure',23,false),
    ('content','threads','z_guard_thread_erasure','guard_thread_erasure',23,false),
    ('content','threads','erase_thread_descendants','erase_thread_descendants',17,true),
    ('post_secrets','deletion','z_guard_erased_author_link','guard_erased_author_link',23,false),
    ('post_secrets','anonymous_posts','z_guard_erased_author_link','guard_erased_author_link',23,false),
    ('post_secrets','op_peers','z_guard_erased_author_link','guard_erased_author_link',23,false),
    ('post_secrets','op_replies','z_guard_erased_author_link','guard_erased_author_link',23,false),
    ('post_secrets','poster_contexts','z_guard_erased_author_link','guard_erased_author_link',23,false),
    ('post_secrets','posting_history','z_guard_erased_author_link','guard_erased_author_link',23,false),
    ('staff_identity','discussion_posts','z_guard_erased_author_link','guard_erased_author_link',21,false)
) required(schema_name,table_name,trigger_name,function_name,trigger_type,transition)
WHERE NOT EXISTS(SELECT 1 FROM triggers t JOIN functions p ON p.oid=t.tgfoid
    WHERE t.nspname=required.schema_name AND t.relname=required.table_name
      AND t.tgname=required.trigger_name AND p.proname=required.function_name
      AND t.tgtype=required.trigger_type AND t.tgenabled='O' AND NOT t.tgisinternal
      AND t.tgnargs=0
      AND CASE WHEN required.table_name='discussion_posts'
          AND required.trigger_name='z_guard_erased_author_link' THEN
          t.tgconstraint<>0 AND t.tgdeferrable AND t.tginitdeferred
      ELSE t.tgconstraint=0 AND NOT t.tgdeferrable AND NOT t.tginitdeferred END
      AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
      AND CASE WHEN required.transition THEN
          t.tgattr::text=(SELECT a.attnum::text FROM pg_catalog.pg_attribute a
              WHERE a.attrelid=t.tgrelid AND a.attname='deleted' AND NOT a.attisdropped)
          AND translate(split_part(split_part(pg_catalog.pg_get_triggerdef(t.oid,false),' WHEN ',2),' EXECUTE ',1),' ()','')
              ='NOTold.content_erasedANDnew.content_erased'
      ELSE t.tgqual IS NULL AND t.tgattr::text='' END))
AND NOT EXISTS(SELECT 1 FROM (VALUES ('posts'),('threads')) required(table_name)
    WHERE NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a
        WHERE a.attrelid=to_regclass('content.'||required.table_name) AND a.attname='content_erased'
          AND a.attnum>0 AND NOT a.attisdropped AND a.attnotnull AND a.atttypid='boolean'::regtype
          AND has_column_privilege(current_user,a.attrelid,a.attnum,'SELECT'))
    OR NOT EXISTS(SELECT 1 FROM pg_catalog.pg_constraint c
        WHERE c.conrelid=to_regclass('content.'||required.table_name)
          AND c.conname=required.table_name||'_erased_payload' AND c.contype='c'
          AND c.convalidated AND NOT c.connoinherit
          AND pg_catalog.pg_get_constraintdef(c.oid) LIKE '%content_erased%'
          AND (required.table_name<>'posts' OR (
              pg_catalog.pg_get_constraintdef(c.oid) LIKE '%NOT file_deleted%'
              AND pg_catalog.pg_get_constraintdef(c.oid) LIKE '%drawing_time_seconds IS NULL%'
              AND pg_catalog.pg_get_constraintdef(c.oid) LIKE '%drawing_source_post_id IS NULL%'))))
AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_roles runtime
    WHERE runtime.rolname IN('board_public','board_staff','board_auth','board_media',
        'board_media_read','board_media_intake','board_monitor') AND (
        pg_has_role(runtime.oid,'board_content_erasure_owner','MEMBER')
        OR has_column_privilege(runtime.oid,(SELECT oid FROM relations WHERE table_name='content.posts'),'content_erased','INSERT,UPDATE')
        OR has_column_privilege(runtime.oid,(SELECT oid FROM relations WHERE table_name='content.threads'),'content_erased','INSERT,UPDATE')
        OR has_table_privilege(runtime.oid,(SELECT oid FROM relations WHERE table_name='content.posts'),'DELETE,TRUNCATE,TRIGGER')
        OR has_table_privilege(runtime.oid,(SELECT oid FROM relations WHERE table_name='content.threads'),'DELETE,TRUNCATE,TRIGGER')
        OR EXISTS(SELECT 1 FROM functions p WHERE has_function_privilege(runtime.oid,p.oid,'EXECUTE'))))
AND NOT has_schema_privilege('board_content_erasure_owner','content','CREATE')
AND NOT has_schema_privilege('board_content_erasure_owner','post_secrets','CREATE')
AND NOT has_schema_privilege('board_content_erasure_owner','staff_identity','CREATE')
AND NOT has_table_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='content.posts'),'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER')
AND NOT has_table_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='content.threads'),'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER')
AND NOT has_column_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='content.posts'),'comment','SELECT,UPDATE')
AND NOT has_column_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='post_secrets.deletion'),'password_hash','SELECT,UPDATE')
AND NOT has_column_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='post_secrets.anonymous_posts'),'password_proof','SELECT,UPDATE')
AND NOT has_column_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='staff_identity.discussion_posts'),'account_id','SELECT,UPDATE')
AND has_column_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='content.boards'),'slug','UPDATE')
AND NOT EXISTS(SELECT 1 FROM (VALUES
    ('content.boards','slug'),('content.boards','staff_only'),
    ('content.posts','id'),('content.posts','board'),('content.posts','thread_id'),
    ('content.posts','deleted'),('content.posts','content_erased'),
    ('content.threads','id'),('content.threads','board'),('content.threads','deleted'),
    ('content.threads','content_erased'),('post_secrets.deletion','post_id'),
    ('post_secrets.anonymous_posts','post_id'),('post_secrets.op_replies','post_id'),
    ('post_secrets.op_peers','thread_id'),('post_secrets.poster_contexts','post_id'),
    ('post_secrets.posting_history','post_id'),('staff_identity.discussion_posts','post_id')
) required(table_name,column_name)
WHERE NOT has_column_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name=required.table_name),required.column_name,'SELECT'))
AND NOT EXISTS(SELECT 1 FROM (VALUES
    ('content.boards',ARRAY['slug','staff_only']::text[],ARRAY['slug']::text[]),
    ('content.posts',ARRAY['id','board','thread_id','deleted','content_erased']::text[],ARRAY['deleted']::text[]),
    ('content.threads',ARRAY['id','board','deleted','content_erased']::text[],ARRAY[]::text[]),
    ('post_secrets.deletion',ARRAY['post_id']::text[],ARRAY[]::text[]),
    ('post_secrets.anonymous_posts',ARRAY['post_id']::text[],ARRAY[]::text[]),
    ('post_secrets.op_replies',ARRAY['post_id']::text[],ARRAY[]::text[]),
    ('post_secrets.op_peers',ARRAY['thread_id']::text[],ARRAY[]::text[]),
    ('post_secrets.poster_contexts',ARRAY['post_id']::text[],ARRAY[]::text[]),
    ('post_secrets.posting_history',ARRAY['post_id']::text[],ARRAY[]::text[]),
    ('staff_identity.discussion_posts',ARRAY['post_id']::text[],ARRAY[]::text[])
) allowed(table_name,read_columns,write_columns)
JOIN pg_catalog.pg_attribute a ON a.attrelid=(SELECT oid FROM relations WHERE table_name=allowed.table_name)
WHERE a.attnum>0 AND NOT a.attisdropped AND (
    (NOT a.attname=ANY(allowed.read_columns) AND has_column_privilege('board_content_erasure_owner',a.attrelid,a.attnum,'SELECT'))
    OR (NOT a.attname=ANY(allowed.write_columns) AND has_column_privilege('board_content_erasure_owner',a.attrelid,a.attnum,'UPDATE'))
    OR has_column_privilege('board_content_erasure_owner',a.attrelid,a.attnum,'INSERT')))
AND NOT EXISTS(SELECT 1 FROM (VALUES ('content'),('post_secrets'),('staff_identity')) required(schema_name)
    WHERE NOT has_schema_privilege('board_content_erasure_owner',required.schema_name,'USAGE'))
-- Require the catalog helper's contract before accepting its visibility result.
-- CASE does not suppress executor-initialization errors after EXECUTE is revoked;
-- both false and query errors keep readiness unavailable. The helper grants no
-- access to private schemas, rows or policy text.
AND CASE WHEN EXISTS(SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='content' AND p.proname='erasure_visibility_ready'
      AND p.proowner IN(SELECT oid FROM owner_role) AND NOT p.prosecdef AND p.provolatile='s'
      AND p.pronargs=0 AND p.prokind='f' AND p.prorettype='boolean'::regtype
      AND NOT p.proretset AND p.proargmodes IS NULL
      AND p.prolang=(SELECT oid FROM pg_catalog.pg_language WHERE lanname='sql')
      AND EXISTS(SELECT 1 FROM unnest(p.proconfig) config(value)
          WHERE replace(config.value,' ','')='search_path=pg_catalog,pg_temp')
      AND has_function_privilege(p.proowner,p.oid,'EXECUTE')
      AND has_function_privilege('board_public',p.oid,'EXECUTE')
      AND has_function_privilege('board_staff',p.oid,'EXECUTE')
      AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(coalesce(p.proacl,pg_catalog.acldefault('f',p.proowner))) a
          WHERE a.privilege_type<>'EXECUTE' OR a.is_grantable
            OR a.grantee NOT IN(p.proowner,'board_public'::regrole,'board_staff'::regrole,
                'board_thread_transfer_owner'::regrole)))
    THEN content.erasure_visibility_ready() ELSE false END
AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_auth_members m WHERE m.member IN(SELECT oid FROM owner_role))
AND EXISTS(SELECT 1 FROM pg_catalog.pg_auth_members m WHERE m.roleid IN(SELECT oid FROM owner_role)
    AND m.member='board_migrator'::regrole AND NOT m.inherit_option AND m.set_option AND NOT m.admin_option)
AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_auth_members m WHERE m.roleid IN(SELECT oid FROM owner_role)
    AND m.member<>'board_migrator'::regrole)
AND has_column_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name='content.posts'),'deleted','UPDATE')
AND EXISTS(SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='content' AND p.proname='board_flag_label' AND p.proargtypes::text='25 25'
      AND p.prorettype='text'::regtype AND p.provolatile='i' AND NOT p.prosecdef
      AND has_function_privilege('board_content_erasure_owner',p.oid,'EXECUTE'))
AND NOT EXISTS(SELECT 1 FROM (VALUES
    ('post_secrets.deletion'),('post_secrets.anonymous_posts'),('post_secrets.op_replies'),
    ('post_secrets.op_peers'),('post_secrets.poster_contexts'),('post_secrets.posting_history'),
    ('staff_identity.discussion_posts')) required(table_name)
    WHERE NOT has_table_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name=required.table_name),'DELETE')
       OR has_table_privilege('board_content_erasure_owner',(SELECT oid FROM relations WHERE table_name=required.table_name),'SELECT,INSERT,UPDATE,TRUNCATE,TRIGGER'))
AND EXISTS(SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
    WHERE n.nspname='post_secrets' AND p.proname='guard_post_erasure'
      AND position('NEW.file_deleted:=false' in p.prosrc)>0)
AND EXISTS(SELECT 1 FROM pg_catalog.pg_proc p
    WHERE p.oid=to_regprocedure('content.require_attachment_for_empty_post()')
      AND p.proowner='board_attachment_owner'::regrole AND p.prosecdef
      AND position('NOT p.content_erased' in p.prosrc)>0)
"#;

// Additional 0143 catalog contract; use alongside READINESS_SQL.
pub const MEDIA_READINESS_SQL: &str = r#"SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_class WHERE oid='content.post_media'::regclass
 AND NOT relrowsecurity AND NOT relforcerowsecurity AND relkind='r' AND relpersistence='p')
AND EXISTS(SELECT 1 FROM pg_catalog.pg_attribute WHERE attrelid='content.post_media'::regclass
 AND attname='metadata_erased' AND attnotnull AND NOT attisdropped AND atttypid='boolean'::regtype)
AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_roles r WHERE r.rolname IN('board_public','board_staff','board_auth','board_media','board_media_read','board_media_intake','board_monitor')
 AND has_column_privilege(r.oid,'content.post_media','metadata_erased','INSERT,UPDATE'))
AND NOT has_table_privilege('board_content_erasure_owner','content.post_media','SELECT,INSERT,UPDATE,DELETE,TRUNCATE,TRIGGER')
AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a WHERE a.attrelid='content.post_media'::regclass AND a.attnum>0 AND NOT a.attisdropped AND (
 (has_column_privilege('board_content_erasure_owner',a.attrelid,a.attnum,'SELECT')<> (a.attname=ANY(ARRAY['post_id','metadata_erased'])))
 OR (has_column_privilege('board_content_erasure_owner',a.attrelid,a.attnum,'UPDATE')<> (a.attname=ANY(ARRAY['filename','bytes','width','height','spoiler','file_deleted','created_at','metadata_erased'])))
 OR has_column_privilege('board_content_erasure_owner',a.attrelid,a.attnum,'INSERT')))
AND (SELECT count(*)=2 AND bool_and(p.proowner='board_content_erasure_owner'::regrole
 AND p.prosecdef AND p.provolatile='v' AND p.pronargs=0 AND p.prorettype='trigger'::regtype
 AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp']::text[]
 AND NOT EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.grantee<>p.proowner))
 FROM pg_proc p WHERE p.pronamespace=(SELECT oid FROM pg_namespace WHERE nspname='post_secrets') AND p.proname IN('guard_media_erasure','erase_deleted_media'))
AND NOT EXISTS(SELECT 1 FROM (VALUES
 ('content.post_media','z_guard_media_erasure','guard_media_erasure',23,false),
 ('content.posts','erase_deleted_media','erase_deleted_media',17,true)
) v(relation,name,func,kind,conditional) WHERE NOT EXISTS(SELECT 1 FROM pg_trigger t
 WHERE t.tgrelid=to_regclass(v.relation) AND t.tgname=v.name AND t.tgfoid=(SELECT oid FROM pg_proc WHERE pronamespace=(SELECT oid FROM pg_namespace WHERE nspname='post_secrets') AND proname=v.func AND pronargs=0)
 AND t.tgtype=v.kind AND t.tgenabled='O' AND NOT t.tgisinternal AND t.tgnargs=0
 AND t.tgconstraint=0 AND NOT t.tgdeferrable AND NOT t.tginitdeferred AND t.tgattr::text=''
 AND t.tgoldtable IS NULL AND t.tgnewtable IS NULL
 AND CASE WHEN v.conditional THEN translate(split_part(split_part(pg_get_triggerdef(t.oid,false),' WHEN ',2),' EXECUTE ',1),' ()','')='NOTold.content_erasedANDnew.content_erased'
 ELSE t.tgqual IS NULL END))
"#;
